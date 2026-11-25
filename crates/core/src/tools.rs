//! Herramientas que corren solas en segundo plano, con la bóveda bloqueada o no: autoaceptar
//! partidas (LoL y TFT) y aparecer desconectado. Igual que src/autoaccept.js y src/offline.js.
//!
//! Cada una tiene su propio hilo; la interfaz solo cambia los interruptores y lee el estado.
use crate::lcu::{lcu, Conn};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

const READY_CHECK: &str = "/lol-matchmaking/v1/ready-check";
const ACCEPT_POLL: Duration = Duration::from_secs(1);
/// El ready check dura unos 10 s.
pub const MAX_DELAY_S: u32 = 8;
const OFFLINE_EVERY: Duration = Duration::from_secs(8);

/// ¿Se encontró un cliente de juego? (para "Cliente de LoL conectado · esperando partida").
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClientStatus {
    Searching,
    Connected,
    NotFound,
}

pub struct Tools {
    auto_accept: AtomicBool,
    delay: AtomicU32,
    offline: AtomicBool,
    status: AtomicU8,
    /// Cuántas partidas se aceptaron (la interfaz avisa cuando sube).
    accepted: AtomicU64,
}

/// Las herramientas de la app. Los hilos arrancan la primera vez que se piden.
pub fn tools() -> &'static Tools {
    static TOOLS: OnceLock<Tools> = OnceLock::new();
    TOOLS.get_or_init(|| {
        std::thread::spawn(auto_accept_loop);
        std::thread::spawn(offline_loop);
        Tools {
            auto_accept: AtomicBool::new(false),
            delay: AtomicU32::new(0),
            offline: AtomicBool::new(false),
            status: AtomicU8::new(0),
            accepted: AtomicU64::new(0),
        }
    })
}

impl Tools {
    pub fn configure_auto_accept(&self, enabled: bool, delay_s: u32) {
        self.delay.store(delay_s.min(MAX_DELAY_S), Ordering::SeqCst);
        if !enabled {
            self.status.store(0, Ordering::SeqCst);
        }
        self.auto_accept.store(enabled, Ordering::SeqCst);
    }

    /// Al apagarlo vuelves a quedar en línea.
    pub fn configure_offline(&self, enabled: bool) {
        let was = self.offline.swap(enabled, Ordering::SeqCst);
        if was && !enabled {
            std::thread::spawn(|| set_availability("chat"));
        }
    }

    pub fn client_status(&self) -> ClientStatus {
        match self.status.load(Ordering::SeqCst) {
            1 => ClientStatus::Connected,
            2 => ClientStatus::NotFound,
            _ => ClientStatus::Searching,
        }
    }

    pub fn accepted_count(&self) -> u64 {
        self.accepted.load(Ordering::SeqCst)
    }
}

fn is_waiting(json: &Value) -> bool {
    json["state"] == "InProgress" && json["playerResponse"] == "None"
}

fn ready_check_on(conn: &Conn) -> Option<Value> {
    match lcu().raw_request(conn, "GET", READY_CHECK, None) {
        Ok((200, j)) => Some(j),
        _ => None,
    }
}

/// Un ready check pendiente en cualquier cliente abierto (LoL o TFT), y si alguno respondió.
fn pending_client() -> (Option<Conn>, bool) {
    let mut connected = false;
    for conn in lcu().game_clients() {
        let Ok((status, json)) = lcu().raw_request(&conn, "GET", READY_CHECK, None) else { continue };
        connected = true;
        if status == 200 && is_waiting(&json) {
            return (Some(conn), true);
        }
    }
    (None, connected)
}

fn auto_accept_loop() {
    loop {
        std::thread::sleep(ACCEPT_POLL);
        let t = tools();
        if !t.auto_accept.load(Ordering::SeqCst) {
            continue;
        }
        let (conn, connected) = pending_client();
        t.status.store(if connected { 1 } else { 2 }, Ordering::SeqCst);
        let Some(conn) = conn else { continue };
        std::thread::sleep(Duration::from_secs(t.delay.load(Ordering::SeqCst) as u64));
        // Se revisa de nuevo en ese mismo cliente: puede que la hayas rechazado o aceptado a mano.
        if t.auto_accept.load(Ordering::SeqCst) && ready_check_on(&conn).is_some_and(|j| is_waiting(&j)) {
            if let Ok((s, _)) = lcu().raw_request(&conn, "POST", &format!("{READY_CHECK}/accept"), None) {
                if s < 400 {
                    t.accepted.fetch_add(1, Ordering::SeqCst);
                }
            }
        }
    }
}

fn set_availability(availability: &str) {
    lcu().request("PUT", "/lol-chat/v1/me", false, Some(&json!({ "availability": availability })));
}

/// El cliente vuelve a ponerte en línea al reiniciarse o cambiar de cuenta: se corrige cada 8 s.
fn offline_loop() {
    loop {
        std::thread::sleep(OFFLINE_EVERY);
        if !tools().offline.load(Ordering::SeqCst) {
            continue;
        }
        if let Some(me) = lcu().get("/lol-chat/v1/me") {
            if me["availability"] != "offline" {
                set_availability("offline");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waiting_ready_check() {
        assert!(is_waiting(&json!({ "state": "InProgress", "playerResponse": "None" })));
        assert!(!is_waiting(&json!({ "state": "InProgress", "playerResponse": "Accepted" })));
        assert!(!is_waiting(&Value::Null));
    }
}
