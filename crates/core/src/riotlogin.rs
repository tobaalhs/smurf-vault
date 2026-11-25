//! Inicio de sesión automático en el Riot Client, sin escribir la contraseña a mano. Port de
//! src/riotlogin.js, que a su vez viene de LeagueAccounts (src/riot_client.rs + riot_renderer.rs):
//! 1) Cierra el Riot Client y lo arranca en modo headless para llegar a la pantalla de login.
//! 2) Lanza el render del cliente (Riot Client.exe) con depuración remota (CDP) en un puerto local.
//! 3) Se conecta por WebSocket al CDP e inyecta riot_renderer.js, que rellena usuario y contraseña en
//!    el formulario nativo de Riot y lo envía.
//! 4) Espera a que la API local del cliente confirme la sesión iniciada con esa cuenta.
//!
//! La contraseña solo viaja hacia el propio Riot Client (127.0.0.1): ni al portapapeles ni al
//! teclado. Bloqueante: se llama desde un hilo de fondo.
use crate::lcu::{lcu, Conn};
use crate::switcher::{kill_and_wait, riot_client_lockfile, riot_client_path, riot_data_root, running_processes};
use serde_json::{json, Value};
use std::net::TcpStream;
use std::time::{Duration, Instant};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

const RIOT_PROCESSES: [&str; 5] =
    ["RiotClientServices.exe", "RiotClientUx.exe", "RiotClientUxRender.exe", "Riot Client.exe", "RiotClientCrashHandler.exe"];

const LOGIN_TIMEOUT: Duration = Duration::from_secs(120);
const SIGNED_OUT_TIMEOUT: Duration = Duration::from_secs(45);
const RENDERER_TIMEOUT: Duration = Duration::from_secs(30);
/// El jugador tiene este tiempo para resolver MFA/captcha.
const CONFIRM_TIMEOUT: Duration = Duration::from_secs(300);
const SIGN_OUT_TIMEOUT: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(500);

/// El JS que se inyecta en el render del Riot Client.
const INJECTION: &str = include_str!("riot_renderer.js");

/// Pasos del inicio de sesión, para la guía de la interfaz.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    WaitAuth,
    SignOut,
    Connect,
    FindWindow,
    Type,
    Verification,
    Confirm,
    Done,
}

impl Step {
    pub fn text(self) -> &'static str {
        match self {
            Step::WaitAuth => "Abriendo el Riot Client…",
            Step::SignOut => "Cerrando la sesión anterior…",
            Step::Connect => "Preparando el inicio de sesión…",
            Step::FindWindow => "Abriendo la pantalla de login…",
            Step::Type => "Escribiendo tus credenciales…",
            Step::Verification => "Riot pide verificación: complétala en la ventana del Riot Client.",
            Step::Confirm => "Confirmando el inicio de sesión…",
            Step::Done => "Sesión iniciada",
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum LoginOutcome {
    AlreadySignedIn,
    Success,
}

fn err(msg: &str) -> String {
    msg.to_string()
}

/// Estado de /player-session-lifecycle/v1/session: con sesión, en el login, pidiendo una acción
/// (MFA/captcha) o desconocido (el cliente aún arranca).
#[derive(PartialEq, Debug)]
enum State {
    SignedIn,
    SignedOut,
    Action,
    Unknown,
}

fn session_state(res: Option<(u16, Value)>) -> State {
    let Some((200, body)) = res else { return State::Unknown };
    if body["puuid"].as_str().is_some_and(|p| !p.is_empty()) {
        return State::SignedIn;
    }
    if body["loginState"] == "PendingLoginStrategy" {
        return State::SignedOut;
    }
    if !body["loginState"].is_null() && body["actionRequired"] == true {
        return State::Action;
    }
    State::Unknown
}

fn api_get(conn: &Conn, path: &str) -> Option<(u16, Value)> {
    lcu().raw_request(conn, "GET", path, None).ok()
}

fn session(conn: &Conn) -> State {
    session_state(api_get(conn, "/player-session-lifecycle/v1/session"))
}

/// Usuario de login de la cuenta con sesión iniciada (userInfo viene como JSON dentro de un texto).
fn signed_in_user(conn: &Conn) -> Option<String> {
    let (200, body) = api_get(conn, "/rso-auth/v1/authorization/userinfo")? else { return None };
    let info = match &body["userInfo"] {
        Value::String(s) => serde_json::from_str::<Value>(s).ok()?,
        Value::Null => body.clone(),
        v => v.clone(),
    };
    info["username"].as_str().filter(|n| !n.is_empty()).map(str::to_string)
}

fn sign_out(conn: &Conn) -> Result<(), String> {
    let help = api_get(conn, "/help").map(|(_, j)| j).unwrap_or(Value::Null);
    if help["functions"].get("DeletePlayerSessionLifecycleV1Session").is_none() {
        return Err(err("El cliente no permite cerrar sesión"));
    }
    match lcu().raw_request(conn, "DELETE", "/player-session-lifecycle/v1/session", None) {
        Ok((s, _)) if s < 400 => Ok(()),
        _ => Err(err("No se pudo cerrar la sesión actual")),
    }
}

fn same_user(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

fn client_running() -> bool {
    let r = running_processes();
    r.contains("riotclientservices.exe") && r.contains("riot client.exe")
}

fn start_service(exe: &std::path::Path, headless: bool) -> Result<(), String> {
    crate::switcher::spawn_detached(exe, if headless { &["--headless"] } else { &[] })
}

/// Solo se acepta la URL de depuración del puerto que se abrió, en loopback, para una página.
pub fn valid_debugger_url(value: &str, port: u16) -> bool {
    let prefix = format!("ws://127.0.0.1:{port}/devtools/page/");
    value.strip_prefix(&prefix).is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
}

fn free_port() -> Result<u16, String> {
    let l = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    l.local_addr().map(|a| a.port()).map_err(|e| e.to_string())
}

/// Cliente CDP mínimo: evalúa expresiones en el render del Riot Client.
struct Cdp {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    seq: u64,
}

impl Cdp {
    fn connect(url: &str, port: u16) -> Result<Self, String> {
        if !valid_debugger_url(url, port) {
            return Err(err("URL de depuración no válida"));
        }
        let (socket, _) = tungstenite::connect(url).map_err(|_| err("No se pudo conectar al render del cliente"))?;
        if let MaybeTlsStream::Plain(s) = socket.get_ref() {
            let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
        }
        Ok(Self { socket, seq: 0 })
    }

    /// Las expresiones pueden llevar credenciales: no se registran ni se reintentan.
    fn eval(&mut self, expression: &str, timeout: Duration) -> Result<Value, String> {
        self.seq += 1;
        let id = self.seq;
        let msg = json!({ "id": id, "method": "Runtime.evaluate", "params": { "expression": expression, "returnByValue": true, "awaitPromise": true } });
        self.socket.send(Message::text(msg.to_string())).map_err(|_| err("No se pudo enviar la inyección"))?;
        let until = Instant::now() + timeout;
        loop {
            if Instant::now() > until {
                return Err(err("El render del cliente no respondió"));
            }
            let text = match self.socket.read() {
                Ok(Message::Text(t)) => t.to_string(),
                Ok(_) => continue,
                Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
                Err(_) => return Err(err("El render del cliente se cerró")),
            };
            let Ok(reply) = serde_json::from_str::<Value>(&text) else { continue };
            if reply["id"].as_u64() != Some(id) {
                continue; // evento u otra respuesta
            }
            if !reply["error"].is_null() || !reply["result"]["exceptionDetails"].is_null() {
                return Err(err("El render del cliente rechazó la inyección"));
            }
            return Ok(reply["result"]["result"]["value"].clone());
        }
    }

    fn close(&mut self) {
        let _ = self.socket.send(Message::text(json!({ "id": 0, "method": "Browser.close" }).to_string()));
        let _ = self.socket.close(None);
    }
}

/// Mata el render de depuración y toda su descendencia.
fn kill_tree(child: &mut std::process::Child) {
    if let Ok(None) = child.try_wait() {
        let mut cmd = std::process::Command::new("taskkill");
        cmd.args(["/F", "/T", "/PID", &child.id().to_string()]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let _ = cmd.output();
    }
}

/// Inicia sesión con `username`/`password` en el Riot Client inyectando las credenciales. Avisa cada
/// paso con `on_step`. Error con un mensaje para el usuario si falla.
pub fn auto_login(username: &str, password: &str, on_step: &dyn Fn(Step)) -> Result<LoginOutcome, String> {
    if password.is_empty() {
        return Err(err("La cuenta no tiene contraseña guardada"));
    }
    let exe = riot_client_path().ok_or("No encontré el Riot Client instalado")?;
    let root = exe.parent().ok_or("Ruta del Riot Client inválida")?.to_path_buf();
    let mut child: Option<std::process::Child> = None;
    let mut cdp: Option<Cdp> = None;
    let result = (|| -> Result<LoginOutcome, String> {
        // 1) Que el servicio del cliente esté corriendo para poder consultarlo.
        on_step(Step::WaitAuth);
        if !client_running() {
            start_service(&exe, false)?;
        }
        // 2) Esperar a conocer el estado: ya con sesión (misma u otra cuenta) o sin sesión.
        let until = Instant::now() + LOGIN_TIMEOUT;
        let mut sign_out_sent: Option<Instant> = None;
        loop {
            if Instant::now() > until {
                return Err(err("El cliente de Riot no respondió a tiempo"));
            }
            if let Some((_, conn)) = riot_client_lockfile() {
                match session(&conn) {
                    State::SignedIn => {
                        if let Some(user) = signed_in_user(&conn) {
                            if same_user(&user, username) {
                                on_step(Step::Done);
                                return Ok(LoginOutcome::AlreadySignedIn);
                            }
                            match sign_out_sent {
                                None => {
                                    on_step(Step::SignOut);
                                    sign_out(&conn)?;
                                    sign_out_sent = Some(Instant::now());
                                }
                                Some(t) if t.elapsed() > SIGN_OUT_TIMEOUT => {
                                    return Err(err("No se pudo cerrar la sesión de la otra cuenta"))
                                }
                                Some(_) => {}
                            }
                        }
                    }
                    State::SignedOut => break,
                    _ => {}
                }
            }
            std::thread::sleep(POLL);
        }

        // 3) Reiniciar el cliente en headless para tener un render de login limpio bajo nuestro control.
        on_step(Step::Connect);
        kill_and_wait(&RIOT_PROCESSES)?;
        start_service(&exe, true)?;
        let until = Instant::now() + SIGNED_OUT_TIMEOUT;
        let (pid, conn) = loop {
            if Instant::now() > until {
                return Err(err("El cliente no llegó a la pantalla de login"));
            }
            if let Some((pid, conn)) = riot_client_lockfile() {
                match session(&conn) {
                    State::SignedOut => break (pid, conn),
                    State::SignedIn => {
                        if signed_in_user(&conn).is_some_and(|u| same_user(&u, username)) {
                            on_step(Step::Done);
                            return Ok(LoginOutcome::AlreadySignedIn);
                        }
                        return Err(err("El cliente quedó con otra cuenta"));
                    }
                    _ => {}
                }
            }
            std::thread::sleep(Duration::from_millis(300));
        };

        // 4) Lanzar el render del cliente con depuración remota en un puerto local.
        let machine = api_get(&conn, "/riotclient/machine-id").and_then(|(_, j)| j.as_str().map(str::to_string));
        let host_session = api_get(&conn, "/product-session/v1/host-session/id").and_then(|(_, j)| j.as_str().map(str::to_string));
        let (Some(machine), Some(host_session)) = (machine, host_session) else {
            return Err(err("No pude leer los datos del cliente"));
        };
        let ux = root.join("RiotClientElectron").join("Riot Client.exe");
        let data_root = riot_data_root().join("Riot Client");
        let port = free_port()?;
        let args = vec![
            format!("--app-port={}", conn.port),
            format!("--remoting-auth-token={}", conn.password),
            format!("--app-pid={pid}"),
            format!("--machine-id={machine}"),
            format!("--session-id={host_session}"),
            format!("--app-root={}", root.display()),
            format!("--user-data-root={}", data_root.display()),
            format!("--log-dir={}", data_root.join("Logs").join("Riot Client UX Logs").display()),
            "--crashpad-environment=prod".into(),
            "--enable-hardware-acceleration".into(),
            "--allow-chrome-dev-tools".into(),
            "--remote-debugging-address=127.0.0.1".into(),
            format!("--remote-debugging-port={port}"),
        ];
        let mut cmd = std::process::Command::new(&ux);
        cmd.args(&args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        child = Some(cmd.spawn().map_err(|_| err("No pude abrir el render del Riot Client"))?);

        // 5) Buscar el objetivo de depuración de la ventana "Riot Client" y conectarse.
        on_step(Step::FindWindow);
        let http = ureq::AgentBuilder::new().timeout(Duration::from_secs(2)).build();
        let until = Instant::now() + RENDERER_TIMEOUT;
        let ws_url = loop {
            if Instant::now() > until {
                return Err(err("No apareció la ventana de login"));
            }
            if child.as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(Some(_)))) {
                return Err(err("El render del cliente se cerró"));
            }
            let targets: Option<Value> = http.get(&format!("http://127.0.0.1:{port}/json/list")).call().ok().and_then(|r| r.into_json().ok());
            let page = targets.as_ref().and_then(|t| t.as_array()).and_then(|list| {
                list.iter().find(|t| {
                    t["type"] == "page" && t["title"] == "Riot Client" && t["webSocketDebuggerUrl"].as_str().is_some_and(|u| valid_debugger_url(u, port))
                })
            });
            if let Some(p) = page {
                break p["webSocketDebuggerUrl"].as_str().unwrap_or("").to_string();
            }
            std::thread::sleep(Duration::from_millis(200));
        };
        let c = cdp.insert(Cdp::connect(&ws_url, port)?);

        // 6) Inyectar el script y esperar a que el formulario esté listo.
        c.eval(INJECTION, Duration::from_secs(15))?;
        let until = Instant::now() + RENDERER_TIMEOUT;
        while c.eval("globalThis.__leagueAccountsLogin.ready()", Duration::from_secs(15))? != Value::Bool(true) {
            if Instant::now() > until {
                return Err(err("El formulario de login no estuvo listo"));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        // Dejar que la inicialización nativa de autenticación se asiente antes de rellenar.
        std::thread::sleep(Duration::from_millis(1500));

        // 7) Rellenar usuario y contraseña, y enviar el formulario.
        on_step(Step::Type);
        let fill = format!("globalThis.__leagueAccountsLogin.fill(...{})", json!([username, password]));
        if c.eval(&fill, Duration::from_secs(15))? != Value::Bool(true) {
            return Err(err("No se pudieron rellenar las credenciales"));
        }
        if c.eval("globalThis.__leagueAccountsLogin.submit()", Duration::from_secs(15))? != Value::Bool(true) {
            return Err(err("No se pudo enviar el formulario"));
        }

        // 8) Esperar la confirmación. Si Riot pide MFA/captcha, se muestra la ventana.
        on_step(Step::Confirm);
        let until = Instant::now() + CONFIRM_TIMEOUT;
        let mut verification = false;
        loop {
            if Instant::now() > until {
                return Err(err("No se confirmó el inicio de sesión a tiempo"));
            }
            let (_, current) = riot_client_lockfile().ok_or("Se perdió la conexión con el cliente")?;
            if session(&current) == State::SignedIn {
                if let Some(user) = signed_in_user(&current) {
                    if same_user(&user, username) {
                        on_step(Step::Done);
                        return Ok(LoginOutcome::Success);
                    }
                    return Err(err("Se inició sesión con otra cuenta"));
                }
            }
            let state = c.eval("globalThis.__leagueAccountsLogin.status()", Duration::from_secs(15))?;
            match state.as_str().unwrap_or("") {
                "rejected" => return Err(err("Usuario o contraseña incorrectos")),
                "captcha_rejected" => return Err(err("Riot pidió un captcha y lo rechazó")),
                "rate_limited" => return Err(err("Demasiados intentos: espera un momento")),
                "changed" => return Err(err("Cambió la cuenta durante el login")),
                "verification" if !verification => {
                    c.eval("globalThis.__leagueAccountsLogin.showVerification()", Duration::from_secs(15))?;
                    on_step(Step::Verification);
                    verification = true;
                }
                _ => {}
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    })();
    // Cerrar el render de depuración: el inicio de sesión ya quedó en el servicio del cliente.
    if let Some(c) = cdp.as_mut() {
        c.close();
    }
    std::thread::sleep(Duration::from_millis(300));
    if let Some(ch) = child.as_mut() {
        kill_tree(ch);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_states_like_the_original() {
        assert_eq!(session_state(Some((200, json!({ "puuid": "x" })))), State::SignedIn);
        assert_eq!(session_state(Some((200, json!({ "loginState": "PendingLoginStrategy" })))), State::SignedOut);
        assert_eq!(session_state(Some((200, json!({ "loginState": "X", "actionRequired": true })))), State::Action);
        assert_eq!(session_state(Some((404, Value::Null))), State::Unknown);
        assert_eq!(session_state(None), State::Unknown);
    }

    #[test]
    fn only_local_page_debugger_urls() {
        assert!(valid_debugger_url("ws://127.0.0.1:9222/devtools/page/ABC-123", 9222));
        assert!(!valid_debugger_url("ws://127.0.0.1:9223/devtools/page/ABC", 9222));
        assert!(!valid_debugger_url("ws://evil.com:9222/devtools/page/ABC", 9222));
        assert!(!valid_debugger_url("ws://127.0.0.1:9222/devtools/browser/ABC", 9222));
        assert!(!valid_debugger_url("ws://127.0.0.1:9222/devtools/page/ABC?x=1", 9222));
    }
}
