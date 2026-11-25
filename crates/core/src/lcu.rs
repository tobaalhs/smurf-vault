//! API local del cliente de League (LCU). Solo funciona con el cliente abierto. Igual que src/lcu.js:
//! la conexión (puerto + token) se guarda en memoria y solo se vuelve a buscar cuando falla; primero
//! en el lockfile y solo de vez en cuando en el proceso (eso lanza PowerShell).
//!
//! Todo es bloqueante: se llama desde hilos de fondo, nunca desde la interfaz.
use crate::matches::{from_lol_game, from_tft_game, KEEP};
use crate::model::Rank;
use crate::snapshot::{ban_from_session, mastery_from, num, RankUpdates, Snapshot};
use base64::Engine;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const PROCESS_LOOKUP_EVERY: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq)]
pub struct Conn {
    pub port: String,
    pub password: String,
}

pub struct Lcu {
    conn: Mutex<Option<Conn>>,
    /// Carpetas donde buscar el lockfile; se aprende la del proceso si está en otra parte.
    dirs: Mutex<Vec<PathBuf>>,
    last_lookup: Mutex<Option<Instant>>,
    agent: ureq::Agent,
}

/// La conexión al cliente, compartida por toda la app.
pub fn lcu() -> &'static Lcu {
    static LCU: OnceLock<Lcu> = OnceLock::new();
    LCU.get_or_init(Lcu::new)
}

/// Cliente HTTP para 127.0.0.1: el cliente usa un certificado autofirmado, así que solo acá se acepta.
pub fn local_agent() -> ureq::Agent {
    let tls = native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        .build()
        .expect("TLS");
    ureq::AgentBuilder::new().tls_connector(Arc::new(tls)).timeout(Duration::from_secs(5)).build()
}

fn read_lockfile(dir: &std::path::Path) -> Option<Conn> {
    let text = std::fs::read_to_string(dir.join("lockfile")).ok()?;
    let parts: Vec<&str> = text.split(':').collect();
    let (port, password) = (parts.get(2)?, parts.get(3)?);
    (!port.is_empty() && !password.is_empty()).then(|| Conn { port: port.to_string(), password: password.to_string() })
}

/// Corre PowerShell sin abrir ventana y devuelve lo que imprimió.
pub fn powershell(script: &str) -> Option<String> {
    let mut cmd = std::process::Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", script]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn capture<'a>(re: &str, text: &'a str) -> Option<&'a str> {
    regex::Regex::new(re).ok()?.captures(text)?.get(1).map(|m| m.as_str())
}

/// Carpetas de los clientes de juego con API local (LoL y TFT), de RiotClientInstalls.json.
fn installed_game_dirs() -> Vec<PathBuf> {
    let base = std::env::var("PROGRAMDATA").unwrap_or_else(|_| r"C:\ProgramData".into());
    let path = PathBuf::from(base).join("Riot Games").join("RiotClientInstalls.json");
    let Some(installs) = std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()) else {
        return Vec::new();
    };
    let re = regex::Regex::new(r"(?i)league of legends|teamfight tactics").unwrap();
    installs["associated_client"]
        .as_object()
        .map(|m| {
            m.keys()
                .filter(|p| re.is_match(p))
                .map(|p| PathBuf::from(p.replace('/', "\\").trim_end_matches(['\\', '/'])))
                .collect()
        })
        .unwrap_or_default()
}

fn rank_from(q: &Value) -> Option<Rank> {
    let tier = q["tier"].as_str().unwrap_or("");
    if tier.is_empty() || tier == "NONE" {
        return None;
    }
    let division = q["division"].as_str().filter(|d| *d != "NA").unwrap_or("").to_string();
    Some(Rank {
        tier: tier.to_string(),
        division,
        lp: num(&q["leaguePoints"]).unwrap_or(0),
        wins: num(&q["wins"]).unwrap_or(0),
        losses: num(&q["losses"]).unwrap_or(0),
        ..Default::default()
    })
}

/// Marco del ícono elegido: { type: 'level', theme } o { type: 'ranked' }; None si no respondió.
fn crest_from(regalia: &Value) -> Option<Value> {
    let kind = regalia["crestType"].as_str().filter(|s| !s.is_empty())?;
    let theme = num(&regalia["selectedPrestigeCrest"]).unwrap_or(0);
    if kind == "prestige" && (1..=21).contains(&theme) {
        return Some(json!({ "type": "level", "theme": theme }));
    }
    Some(json!({ "type": "ranked" }))
}

impl Lcu {
    fn new() -> Self {
        Self {
            conn: Mutex::new(None),
            dirs: Mutex::new(vec![r"C:\Riot Games\League of Legends".into(), r"D:\Riot Games\League of Legends".into()]),
            last_lookup: Mutex::new(None),
            agent: local_agent(),
        }
    }

    fn from_lockfile(&self) -> Option<Conn> {
        self.dirs.lock().unwrap().iter().find_map(|d| read_lockfile(d))
    }

    fn from_process(&self) -> Option<Conn> {
        let out = powershell(
            "Get-CimInstance Win32_Process -Filter \"Name='LeagueClientUx.exe'\" | Select-Object -ExpandProperty CommandLine",
        )?;
        if let Some(dir) = capture(r#"--install-directory=([^"]+?)(?:"|\s--|$)"#, &out) {
            let dir = PathBuf::from(dir.trim().trim_end_matches(['\\', '/']));
            let mut dirs = self.dirs.lock().unwrap();
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
        let port = capture(r"--app-port=(\d+)", &out)?;
        let password = capture(r"--remoting-auth-token=([\w-]+)", &out)?;
        Some(Conn { port: port.into(), password: password.into() })
    }

    /// La conexión guardada, o la busca. `force` busca el proceso aunque se haya buscado hace poco.
    pub fn connection(&self, force: bool) -> Option<Conn> {
        if let Some(c) = self.conn.lock().unwrap().clone() {
            return Some(c);
        }
        let found = self.from_lockfile().or_else(|| {
            let mut last = self.last_lookup.lock().unwrap();
            if !force && last.is_some_and(|t| t.elapsed() < PROCESS_LOOKUP_EVERY) {
                return None;
            }
            *last = Some(Instant::now());
            drop(last);
            self.from_process()
        });
        *self.conn.lock().unwrap() = found.clone();
        found
    }

    pub fn is_connected(&self) -> bool {
        self.conn.lock().unwrap().is_some()
    }

    /// Petición a un cliente concreto: (status, json). Error si no responde.
    pub fn raw_request(&self, c: &Conn, method: &str, path: &str, body: Option<&Value>) -> Result<(u16, Value), String> {
        let auth = base64::engine::general_purpose::STANDARD.encode(format!("riot:{}", c.password));
        let req = self
            .agent
            .request(method, &format!("https://127.0.0.1:{}{}", c.port, path))
            .set("Authorization", &format!("Basic {auth}"))
            .set("Accept", "application/json");
        let res = match body {
            Some(b) => req.send_json(b),
            None => req.call(),
        };
        let resp = match res {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => return Err(e.to_string()),
        };
        let status = resp.status();
        let json = resp.into_string().ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
        Ok((status, json))
    }

    /// Petición al cliente: (status, json), o None si no está abierto. Si la conexión guardada dejó de
    /// servir (cliente cerrado o reiniciado), se descarta.
    pub fn request(&self, method: &str, path: &str, force: bool, body: Option<&Value>) -> Option<(u16, Value)> {
        let c = self.connection(force)?;
        match self.raw_request(&c, method, path, body) {
            Ok((401, _)) | Err(_) => {
                *self.conn.lock().unwrap() = None;
                None
            }
            Ok(r) => Some(r),
        }
    }

    pub fn get(&self, path: &str) -> Option<Value> {
        self.get_with(path, false)
    }

    fn get_with(&self, path: &str, force: bool) -> Option<Value> {
        self.request("GET", path, force, None).filter(|(s, _)| *s < 400).map(|(_, j)| j)
    }

    /// Fase del cliente (Lobby, ReadyCheck, ChampSelect, InProgress...), si está abierto.
    pub fn gameflow_phase(&self) -> Option<String> {
        self.get("/lol-gameflow/v1/gameflow-phase")?.as_str().map(str::to_string)
    }

    /// Datos de la cuenta abierta en el cliente, o None si no hay cliente o sesión.
    pub fn current_account(&self, force: bool) -> Option<Snapshot> {
        let summoner = self.get_with("/lol-summoner/v1/current-summoner", force)?;
        let puuid = summoner["puuid"].as_str().filter(|p| !p.is_empty())?.to_string();

        // Las consultas van en paralelo, como el Promise.all de Electron.
        let paths = [
            "/lol-login/v1/session".to_string(),
            "/lol-ranked/v1/current-ranked-stats".to_string(),
            "/riotclient/region-locale".to_string(),
            format!("/lol-match-history/v1/products/lol/current-summoner/matches?begIndex=0&endIndex={KEEP}"),
            format!("/lol-match-history/v1/products/tft/{puuid}/matches?begin=0&count={KEEP}"),
            "/lol-summoner/v1/current-summoner/summoner-profile".to_string(),
            "/lol-regalia/v2/current-summoner/regalia".to_string(),
            "/lol-champion-mastery/v1/local-player/champion-mastery".to_string(),
            "/lol-champion-mastery/v1/local-player/champion-mastery-score".to_string(),
        ];
        let results: Vec<Option<Value>> = std::thread::scope(|s| {
            let handles: Vec<_> = paths.iter().map(|p| s.spawn(move || self.get(p))).collect();
            handles.into_iter().map(|h| h.join().ok().flatten()).collect()
        });
        let [session, ranked, region, history, tft_history, profile, regalia, mastery, mastery_score]: [Option<Value>; 9] =
            results.try_into().ok()?;
        let null = Value::Null;
        let or_null = |v: &Option<Value>| v.clone().unwrap_or(Value::Null);

        let mut matches: Vec<_> = history
            .as_ref()
            .and_then(|h| h["games"]["games"].as_array())
            .map(|g| g.iter().filter_map(from_lol_game).collect())
            .unwrap_or_default();
        if let Some(games) = tft_history.as_ref().and_then(|h| h["games"].as_array()) {
            matches.extend(games.iter().filter_map(|g| from_tft_game(g, &puuid)));
        }
        let last_played_at = matches.iter().map(|m| m.at.clone()).max();
        let queues = ranked.as_ref().map(|r| &r["queueMap"]).unwrap_or(&null);
        let rank = |q: &str| ranked.as_ref().map(|_| rank_from(&queues[q]));

        Some(Snapshot {
            username: session.as_ref().and_then(|s| s["username"].as_str()).unwrap_or("").to_string(),
            puuid: puuid.clone(),
            api_puuid: None,
            game_name: summoner["gameName"].as_str().or(summoner["displayName"].as_str()).unwrap_or("").to_string(),
            tag_line: summoner["tagLine"].as_str().unwrap_or("").to_string(),
            level: num(&summoner["summonerLevel"]),
            icon_id: num(&summoner["profileIconId"]),
            server: crate::riot::server_from_client(region.as_ref().and_then(|r| r["region"].as_str()).unwrap_or("")),
            last_played_at,
            background_skin_id: profile.as_ref().and_then(|p| num(&p["backgroundSkinId"])).filter(|id| *id != 0),
            crest: regalia.as_ref().and_then(crest_from),
            mastery: mastery_from(&or_null(&mastery), &or_null(&mastery_score)),
            // El cliente siempre responde la sesión, así que "sin baneo" es confiable.
            ban: Some(ban_from_session(&or_null(&session), chrono::Utc::now().timestamp_millis())),
            ranks: RankUpdates {
                solo: rank("RANKED_SOLO_5x5"),
                flex: rank("RANKED_FLEX_SR"),
                tft: rank("RANKED_TFT"),
                double_up: rank("RANKED_TFT_DOUBLE_UP"),
            },
            matches: (history.is_some() || tft_history.is_some()).then_some(matches),
        })
    }

    /// Conexión de cada cliente de juego abierto (LoL y TFT), para el autoaceptar.
    pub fn game_clients(&self) -> Vec<Conn> {
        let mut dirs = self.dirs.lock().unwrap().clone();
        for d in installed_game_dirs() {
            if !dirs.contains(&d) {
                dirs.push(d);
            }
        }
        let mut out: Vec<Conn> = Vec::new();
        for c in dirs.iter().filter_map(|d| read_lockfile(d)) {
            if !out.contains(&c) {
                out.push(c);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_and_crest_from_client() {
        let r = rank_from(&json!({ "tier": "GOLD", "division": "II", "leaguePoints": 40, "wins": 3, "losses": 1 })).unwrap();
        assert_eq!((r.tier.as_str(), r.division.as_str(), r.lp), ("GOLD", "II", 40));
        assert_eq!(rank_from(&json!({ "tier": "MASTER", "division": "NA" })).unwrap().division, "");
        assert!(rank_from(&json!({ "tier": "NONE" })).is_none());
        assert_eq!(crest_from(&json!({ "crestType": "prestige", "selectedPrestigeCrest": 5 })).unwrap()["theme"], 5);
        assert_eq!(crest_from(&json!({ "crestType": "ranked" })).unwrap()["type"], "ranked");
        assert!(crest_from(&Value::Null).is_none());
    }

    #[test]
    fn lockfile_format() {
        let dir = std::env::temp_dir().join(format!("sv-lock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("lockfile"), "LeagueClient:1234:54321:secret:https").unwrap();
        assert_eq!(read_lockfile(&dir), Some(Conn { port: "54321".into(), password: "secret".into() }));
        std::fs::remove_dir_all(&dir).ok();
    }
}
