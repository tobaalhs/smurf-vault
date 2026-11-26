//! Manejo del Riot Client para cambiar de cuenta: cerrarlo, cerrar su sesión (para que muestre el
//! login), abrir el LoL y esperar sus ventanas.
//!
//! Bloqueante: se llama desde hilos de fondo.
use crate::lcu::{lcu, powershell, Conn};
use serde_json::Value;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

const SESSION_FILE: &str = "RiotGamesPrivateSettings.yaml";
/// Carpetas de %LOCALAPPDATA%\Riot Games con la sesión (versiones viejas y nuevas del cliente).
const SESSION_DIRS: [&str; 2] = ["Riot Client", "League of Legends"];
pub const RIOT_PROCESSES: [&str; 8] = [
    "RiotClientServices.exe",
    "RiotClientUx.exe",
    "RiotClientUxRender.exe",
    "Riot Client.exe",
    "RiotClientCrashHandler.exe",
    "LeagueClient.exe",
    "LeagueClientUx.exe",
    "LeagueClientUxRender.exe",
];
const IN_GAME_PROCESS: &str = "League of Legends.exe";

/// Fases del cliente en las que cerrarlo te saca de la cola o de la partida.
pub fn busy_phase(phase: &str) -> Option<&'static str> {
    Some(match phase {
        "ReadyCheck" => "aceptando partida",
        "ChampSelect" => "en selección de campeones",
        "GameStart" => "entrando a la partida",
        "InProgress" | "Reconnect" => "en partida",
        _ => return None,
    })
}

pub fn riot_data_root() -> PathBuf {
    PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default()).join("Riot Games")
}

/// Archivos donde el Riot Client recuerda la sesión (versiones viejas y nuevas del cliente).
fn session_files() -> Vec<PathBuf> {
    let root = riot_data_root();
    SESSION_DIRS.iter().map(|d| root.join(d).join("Data").join(SESSION_FILE)).collect()
}

/// Cierra la sesión del Riot Client (borra lo que recuerda), para que al abrirlo muestre el login.
pub fn sign_out() {
    for f in session_files() {
        let _ = std::fs::remove_file(f);
    }
}

fn hidden(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

/// Nombres (en minúsculas) de los procesos que están corriendo.
pub fn running_processes() -> HashSet<String> {
    let Ok(out) = hidden(std::process::Command::new("tasklist").args(["/FO", "CSV", "/NH"])).output() else {
        return HashSet::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.strip_prefix('"')?.split('"').next().map(|n| n.to_lowercase()))
        .collect()
}

pub fn is_in_game() -> bool {
    running_processes().contains(&IN_GAME_PROCESS.to_lowercase())
}

/// Cierra los procesos indicados (los que estén corriendo) y espera a que terminen de verdad: si no,
/// pueden volver a escribir la sesión al cerrarse.
pub fn kill_and_wait(names: &[&str]) -> Result<(), String> {
    let running = running_processes();
    let targets: Vec<&str> = names.iter().copied().filter(|p| running.contains(&p.to_lowercase())).collect();
    if targets.is_empty() {
        return Ok(());
    }
    let mut cmd = std::process::Command::new("taskkill");
    cmd.arg("/F");
    for t in &targets {
        cmd.args(["/IM", t]);
    }
    let _ = hidden(&mut cmd).output();
    for _ in 0..40 {
        let now = running_processes();
        if !names.iter().any(|p| now.contains(&p.to_lowercase())) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err("No se pudo cerrar el cliente de Riot".into())
}

pub fn close_riot() -> Result<(), String> {
    kill_and_wait(&RIOT_PROCESSES)
}

/// Ruta de RiotClientServices.exe según RiotClientInstalls.json, o la de por defecto.
pub fn riot_client_path() -> Option<PathBuf> {
    let base = std::env::var("PROGRAMDATA").unwrap_or_else(|_| r"C:\ProgramData".into());
    let installs: Value = std::fs::read_to_string(PathBuf::from(base).join("Riot Games").join("RiotClientInstalls.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null);
    let found = [installs["rc_default"].as_str(), installs["rc_live"].as_str(), Some(r"C:\Riot Games\Riot Client\RiotClientServices.exe")]
        .into_iter()
        .flatten()
        .map(PathBuf::from)
        .find(|p| p.exists());
    found
}

/// Arranca un programa suelto (sigue corriendo aunque se cierre Smurf Vault).
pub fn spawn_detached(exe: &std::path::Path, args: &[&str]) -> Result<(), String> {
    let mut cmd = std::process::Command::new(exe);
    cmd.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0000_0008 | 0x0000_0200); // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
    }
    cmd.spawn().map(|_| ()).map_err(|e| format!("No pude abrir el Riot Client: {e}"))
}

pub fn launch_league() -> Result<(), String> {
    let exe = riot_client_path().ok_or("No encontré el Riot Client instalado")?;
    spawn_detached(&exe, &["--launch-product=league_of_legends", "--launch-patchline=live"])
}

/// Puerto y token de la API local del Riot Client (lockfile: nombre:pid:puerto:token:protocolo).
pub fn riot_client_lockfile() -> Option<(String, Conn)> {
    let raw = std::fs::read_to_string(riot_data_root().join("Riot Client").join("Config").join("lockfile")).ok()?;
    let parts: Vec<&str> = raw.trim().split(':').collect();
    let (pid, port, password) = (parts.get(1)?, parts.get(2)?, parts.get(3)?);
    (!port.is_empty() && !password.is_empty()).then(|| (pid.to_string(), Conn { port: port.to_string(), password: password.to_string() }))
}

/// Cada cambio de cuenta nuevo cancela la espera del anterior.
static LAUNCH_RUN: AtomicU64 = AtomicU64::new(0);

/// El Riot Client ignora --launch-product si la sesión todavía no está lista y se queda en su inicio:
/// se espera a que haya sesión y se le pide el LoL por su API local, como el botón "Jugar". Sin
/// sesión guardada sigue esperando a que inicies sesión a mano (hasta `timeout`).
pub fn ensure_league_starts(timeout: Duration) -> bool {
    let run = LAUNCH_RUN.fetch_add(1, Ordering::SeqCst) + 1;
    let until = Instant::now() + timeout;
    while Instant::now() < until && LAUNCH_RUN.load(Ordering::SeqCst) == run {
        std::thread::sleep(Duration::from_secs(2));
        let running = running_processes();
        if running.contains("leagueclient.exe") || running.contains("leagueclientux.exe") {
            minimize_riot_client();
            return true;
        }
        let Some((_, conn)) = riot_client_lockfile() else { continue };
        let Ok((status, _)) = lcu().raw_request(&conn, "GET", "/entitlements/v1/token", None) else { continue };
        if status != 200 {
            continue; // todavía en el login
        }
        let _ = lcu().raw_request(&conn, "POST", "/product-launcher/v1/products/league_of_legends/patchlines/live", None);
        std::thread::sleep(Duration::from_secs(4)); // tiempo para que aparezca el proceso
    }
    false
}

/// Con el LoL ya abierto, minimiza el Riot Client (varias veces: a veces vuelve al frente al cargar).
/// No lo cierra: el LoL lo necesita, y sigue en la barra de tareas.
fn minimize_riot_client() {
    const SCRIPT: &str = r#"
Add-Type -Namespace SV -Name Win -MemberDefinition '[DllImport("user32.dll")] public static extern bool ShowWindowAsync(IntPtr h, int n);'
Get-Process -Name 'Riot Client' -ErrorAction SilentlyContinue |
  Where-Object { $_.MainWindowHandle -ne 0 } |
  ForEach-Object { [SV.Win]::ShowWindowAsync($_.MainWindowHandle, 6) | Out-Null }
"#;
    for wait in [1500, 3000, 5000] {
        std::thread::sleep(Duration::from_millis(wait));
        let _ = powershell(SCRIPT);
    }
}

/// Antes de cambiar de cuenta: falla si el cliente está en cola, selección o partida.
pub fn check_not_busy() -> Result<(), String> {
    if let Some(what) = lcu().gameflow_phase().as_deref().and_then(busy_phase) {
        return Err(format!("No se puede cambiar de cuenta {what}"));
    }
    if is_in_game() {
        return Err("No se puede cambiar de cuenta en partida".into());
    }
    Ok(())
}

// ---------- autoescrito (respaldo si la inyección falla) ----------

const LOGIN_WINDOW: &str = "Riot Client";

/// Espera a que exista la ventana del Riot Client (su login). false si no aparece a tiempo.
pub fn wait_for_login_window(timeout: Duration) -> bool {
    let check = "if (Get-Process -Name 'Riot Client' -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 -and $_.MainWindowTitle }) { 'yes' }";
    let until = Instant::now() + timeout;
    while Instant::now() < until {
        if powershell(check).is_some_and(|o| o.contains("yes")) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(1500));
    }
    false
}

/// Enfoca la ventana del Riot Client y manda teclas fijas de SendKeys (^a, ^v, {TAB}, {ENTER}). Nunca
/// datos: la contraseña va por el portapapeles. false si no pudo enfocarla.
pub fn send_keys(steps: &[&str]) -> bool {
    let valid = regex::Regex::new(r"^\^?[a-z]$|^\{(TAB|ENTER)\}$").unwrap();
    if !steps.iter().all(|k| valid.is_match(k)) {
        return false;
    }
    let keys: String = steps.iter().map(|k| format!("$sh.SendKeys('{k}'); Start-Sleep -Milliseconds 150\n")).collect();
    let script = format!(
        "$ErrorActionPreference = 'Stop'\n$sh = New-Object -ComObject WScript.Shell\nif (-not $sh.AppActivate('{LOGIN_WINDOW}')) {{ exit 2 }}\nStart-Sleep -Milliseconds 400\n{keys}'ok'"
    );
    powershell(&script).is_some_and(|o| o.contains("ok"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_phases() {
        assert_eq!(busy_phase("ChampSelect"), Some("en selección de campeones"));
        assert_eq!(busy_phase("Lobby"), None);
    }
}
