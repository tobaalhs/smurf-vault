//! Control desde el celular: un servidor HTTP chico en la red local que sirve la página del celular
//! (carpeta remote/) y una API para ver la cola o la selección de campeones y dar órdenes. Igual que
//! src/remote.js: solo responde a quien tenga el código secreto (va en el QR) y solo consulta el
//! cliente de LoL mientras hay un celular conectado.
//!
//! HTTP hecho a mano sobre TcpListener: son cuatro rutas y un stream de eventos (SSE).
use crate::champselect::ChampSelect;
use crate::gamedata::GameData;
use crate::paths::Paths;
use base64::Engine;
use rand::RngCore;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const PORTS: [u16; 5] = [43210, 43211, 43212, 43213, 43214];
const CSP: &str = "default-src 'self'; img-src 'self' data: https://raw.communitydragon.org https://cdn.communitydragon.org; style-src 'self'; script-src 'self'; connect-src 'self'";
const MAX_BODY: usize = 10_000;
const MAX_HEADER: usize = 16_384;

const INDEX_HTML: &str = include_str!("../remote/index.html");
const REMOTE_CSS: &str = include_str!("../remote/remote.css");
const REMOTE_JS: &str = include_str!("../remote/remote.js");
const ICON_PNG: &[u8] = include_bytes!("../remote/icon.png");

/// Código secreto nuevo para el QR.
pub fn new_token() -> String {
    let mut bytes = [0u8; 18];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// IPv4 de la red local, primero las de adaptadores reales (no VPN ni máquinas virtuales).
pub fn lan_addresses() -> Vec<String> {
    let virtual_re = regex::Regex::new(r"(?i)vethernet|virtualbox|vmware|wsl|hyper-v|loopback|bluetooth|tailscale|zerotier|hamachi").unwrap();
    let mut out: Vec<(String, u8)> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .filter_map(|i| match i.ip() {
            std::net::IpAddr::V4(ip) => {
                let o = ip.octets();
                let private = o[0] == 10 || (o[0] == 192 && o[1] == 168) || (o[0] == 172 && (16..=31).contains(&o[1]));
                let name = format!("{} {}", i.name, i.adapter_name);
                Some((ip.to_string(), if virtual_re.is_match(&name) { 0 } else { 2 } + u8::from(private)))
            }
            _ => None,
        })
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1));
    out.into_iter().map(|(ip, _)| ip).collect()
}

/// Lo que muestra Herramientas.
#[derive(Clone, Debug)]
pub struct RemoteStatus {
    pub url: Option<String>,
    pub address: Option<String>,
    pub clients: usize,
}

struct Inner {
    token: Mutex<String>,
    clients: Mutex<Vec<TcpStream>>,
    last_state: Mutex<String>,
    force: AtomicBool,
    running: AtomicBool,
    champ: Mutex<ChampSelect>,
    paths: Paths,
}

pub struct RemoteServer {
    inner: Arc<Inner>,
    port: u16,
}

impl RemoteServer {
    /// Abre el servidor en el primer puerto libre.
    pub fn start(token: String, paths: Paths) -> Result<Self, String> {
        let (listener, port) = PORTS
            .iter()
            .find_map(|p| TcpListener::bind(("0.0.0.0", *p)).ok().map(|l| (l, *p)))
            .ok_or("No hay un puerto libre para el control desde el celular")?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let inner = Arc::new(Inner {
            token: Mutex::new(token),
            clients: Mutex::new(Vec::new()),
            last_state: Mutex::new(String::new()),
            force: AtomicBool::new(false),
            running: AtomicBool::new(true),
            champ: Mutex::new(ChampSelect::default()),
            paths,
        });
        let accept = inner.clone();
        std::thread::spawn(move || {
            while accept.running.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let inner = accept.clone();
                        std::thread::spawn(move || {
                            let _ = stream.set_nonblocking(false);
                            handle(&inner, stream);
                        });
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(150)),
                }
            }
        });
        let poll = inner.clone();
        std::thread::spawn(move || poll_loop(&poll));
        Ok(Self { inner, port })
    }

    pub fn stop(&self) {
        self.inner.running.store(false, Ordering::SeqCst);
        drop_clients(&self.inner);
    }

    /// Cambia el código: los celulares conectados con el anterior quedan afuera.
    pub fn set_token(&self, token: String) {
        *self.inner.token.lock().unwrap() = token;
        drop_clients(&self.inner);
    }

    pub fn status(&self) -> RemoteStatus {
        let ip = lan_addresses().into_iter().next();
        let token = self.inner.token.lock().unwrap().clone();
        RemoteStatus {
            url: ip.as_ref().map(|ip| format!("http://{ip}:{}/?t={token}", self.port)),
            address: ip.map(|ip| format!("{ip}:{}", self.port)),
            clients: self.inner.clients.lock().unwrap().len(),
        }
    }
}

impl Drop for RemoteServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn drop_clients(inner: &Inner) {
    for c in inner.clients.lock().unwrap().drain(..) {
        let _ = c.shutdown(std::net::Shutdown::Both);
    }
}

/// Matriz del QR (true = módulo oscuro) y su ancho.
pub fn qr_matrix(text: &str) -> Option<(Vec<bool>, usize)> {
    let code = qrcode::QrCode::new(text.as_bytes()).ok()?;
    let width = code.width();
    Some((code.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect(), width))
}

struct Request {
    method: String,
    path: String,
    query: Vec<(String, String)>,
    token_header: Option<String>,
    body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    let header_end = loop {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        if buf.len() > MAX_HEADER {
            return None;
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let (method, target) = (first.next()?.to_string(), first.next()?.to_string());
    let mut len = 0usize;
    let mut token_header = None;
    for l in lines {
        let Some((k, v)) = l.split_once(':') else { continue };
        match k.trim().to_ascii_lowercase().as_str() {
            "content-length" => len = v.trim().parse().unwrap_or(0),
            "x-token" => token_header = Some(v.trim().to_string()),
            _ => {}
        }
    }
    if len > MAX_BODY {
        return None;
    }
    let mut body = buf[header_end + 4..].to_vec();
    while body.len() < len {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(len);
    let (path, qs) = target.split_once('?').unwrap_or((&target, ""));
    let query = qs
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), percent_decode(v)))
        .collect();
    Some(Request { method, path: path.to_string(), query, token_header, body })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b'+', _) => {
                out.push(b' ');
                i += 1;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn respond(stream: &mut TcpStream, status: &str, kind: &str, extra: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

fn reply(stream: &mut TcpStream, status: &str, body: Value) {
    respond(stream, status, "application/json; charset=utf-8", "Cache-Control: no-store\r\n", body.to_string().as_bytes());
}

/// Compara el código sin filtrar por tiempos cuánto coincide.
fn same_token(given: &str, token: &str) -> bool {
    given.len() == token.len() && !token.is_empty() && given.bytes().zip(token.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

fn handle(inner: &Arc<Inner>, mut stream: TcpStream) {
    let Some(req) = read_request(&mut stream) else { return };
    let static_file: Option<(&[u8], &str)> = match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") => Some((INDEX_HTML.as_bytes(), "text/html; charset=utf-8")),
        ("GET", "/remote.css") => Some((REMOTE_CSS.as_bytes(), "text/css; charset=utf-8")),
        ("GET", "/remote.js") => Some((REMOTE_JS.as_bytes(), "text/javascript; charset=utf-8")),
        ("GET", "/icon.png") => Some((ICON_PNG, "image/png")),
        _ => None,
    };
    if let Some((body, kind)) = static_file {
        return respond(&mut stream, "200 OK", kind, &format!("Content-Security-Policy: {CSP}\r\nCache-Control: no-cache\r\n"), body);
    }
    if !req.path.starts_with("/api/") {
        return reply(&mut stream, "404 Not Found", json!({ "error": "No existe" }));
    }
    let given = req.token_header.clone().or_else(|| req.query.iter().find(|(k, _)| k == "t").map(|(_, v)| v.clone())).unwrap_or_default();
    if !same_token(&given, &inner.token.lock().unwrap()) {
        return reply(&mut stream, "401 Unauthorized", json!({ "error": "Código inválido: escanea el QR de nuevo" }));
    }
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/api/static") => {
            let data = GameData::load_cache(&inner.paths);
            reply(&mut stream, "200 OK", json!({ "champions": data.champions, "spells": if data.spells.is_array() { data.spells } else { json!([]) } }));
        }
        ("GET", "/api/events") => subscribe(inner, stream),
        ("POST", "/api/command") => {
            let cmd: Value = serde_json::from_slice(&req.body).unwrap_or(json!({}));
            let res = inner.champ.lock().unwrap().run(&cmd);
            match res {
                Ok(()) => reply(&mut stream, "200 OK", json!({ "ok": true })),
                Err(e) => reply(&mut stream, "400 Bad Request", json!({ "error": e })),
            }
            // Que el celular vea el resultado al tiro.
            inner.force.store(true, Ordering::SeqCst);
        }
        _ => reply(&mut stream, "404 Not Found", json!({ "error": "No existe" })),
    }
}

/// Evento SSE con el estado y la hora del PC (el celular calcula los tiempos con su reloj).
fn message(state: &str) -> String {
    format!("data: {{\"now\":{},\"state\":{state}}}\n\n", chrono::Utc::now().timestamp_millis())
}

/// Estado en vivo (Server-Sent Events): la conexión queda abierta y el hilo de consultas escribe.
fn subscribe(inner: &Arc<Inner>, mut stream: TcpStream) {
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: keep-alive\r\n\r\nretry: 2000\n\n";
    if stream.write_all(head.as_bytes()).is_err() {
        return;
    }
    let last = inner.last_state.lock().unwrap().clone();
    if !last.is_empty() && stream.write_all(message(&last).as_bytes()).is_err() {
        return;
    }
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    inner.clients.lock().unwrap().push(stream);
    inner.force.store(true, Ordering::SeqCst);
}

fn broadcast(inner: &Inner, text: &str) {
    inner.clients.lock().unwrap().retain_mut(|c| c.write_all(text.as_bytes()).is_ok());
}

/// Consulta el cliente mientras hay celulares conectados y les avisa si algo cambió. Más seguido en
/// cola y selección.
fn poll_loop(inner: &Arc<Inner>) {
    let mut last_ping = Instant::now();
    while inner.running.load(Ordering::SeqCst) {
        if inner.clients.lock().unwrap().is_empty() {
            std::thread::sleep(Duration::from_millis(300));
            continue;
        }
        let state = inner.champ.lock().unwrap().state();
        let json = state.to_string();
        let force = inner.force.swap(false, Ordering::SeqCst);
        let changed = *inner.last_state.lock().unwrap() != json;
        if changed || force {
            *inner.last_state.lock().unwrap() = json.clone();
            broadcast(inner, &message(&json));
        }
        if last_ping.elapsed() > Duration::from_secs(15) {
            broadcast(inner, ": ping\n\n");
            last_ping = Instant::now();
        }
        let fast = matches!(state["phase"].as_str(), Some("ReadyCheck" | "ChampSelect" | "Matchmaking"));
        let wait = if fast { 1000 } else { 3000 };
        let until = Instant::now() + Duration::from_millis(wait);
        while Instant::now() < until && !inner.force.load(Ordering::SeqCst) && inner.running.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_and_qr() {
        let t = new_token();
        assert_eq!(t.len(), 24);
        assert!(same_token(&t, &t));
        assert!(!same_token("x", &t));
        assert!(!same_token("", ""));
        let (m, w) = qr_matrix("http://192.168.1.2:43210/?t=abc").unwrap();
        assert_eq!(m.len(), w * w);
        assert_eq!(percent_decode("a%2Fb+c"), "a/b c");
    }

    #[test]
    fn serves_the_page_and_checks_the_code() {
        let dir = std::env::temp_dir().join(format!("sv-remote-{}", std::process::id()));
        let server = RemoteServer::start("secreto".into(), Paths::at(&dir)).unwrap();
        let port = server.status().address.map(|a| a.rsplit(':').next().unwrap().parse::<u16>().unwrap());
        let port = port.unwrap_or(43210);
        let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(3)).build();
        let page = agent.get(&format!("http://127.0.0.1:{port}/")).call().unwrap().into_string().unwrap();
        assert!(page.contains("<html") || page.contains("<!doctype") || page.contains("<!DOCTYPE"));
        let denied = agent.get(&format!("http://127.0.0.1:{port}/api/static?t=malo")).call();
        assert!(matches!(denied, Err(ureq::Error::Status(401, _))));
        let ok = agent.get(&format!("http://127.0.0.1:{port}/api/static")).set("x-token", "secreto").call().unwrap();
        assert_eq!(ok.status(), 200);
        server.stop();
    }
}
