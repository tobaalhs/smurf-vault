//! Vinculación con Google (OAuth 2.0 para apps de escritorio, loopback + PKCE) y la bóveda en la
//! carpeta oculta "appDataFolder" de Drive. Igual que src/drive.js: mismo archivo vault.dat en Drive
//! y mismo token local (google-token.bin, cifrado como safeStorage de Electron).
//!
//! Bloqueante: se llama desde hilos de fondo. Las subidas van por una cola propia.
use crate::paths::Paths;
use crate::safestore::SafeStore;
use crate::vault::Envelope;
use base64::Engine;
use rand::RngCore;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SCOPES: &str = "https://www.googleapis.com/auth/drive.appdata openid email";
const VAULT_NAME: &str = "vault.dat";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const MISSING_SCOPE: &str =
    "Falta el permiso de Google Drive. Vuelve a vincular y marca la casilla \"Ver, crear y borrar sus propios datos de configuración en Google Drive\".";

/// Estado de la sincronización, para el chip de la barra de arriba.
#[derive(Clone, Debug, PartialEq)]
pub enum SyncState {
    /// Sin Google vinculado: la bóveda queda solo en este PC.
    Local,
    Syncing,
    Ok(String),
    Error(String),
}

#[derive(Clone, Debug)]
pub struct DriveStatus {
    pub has_credentials: bool,
    pub linked: bool,
    pub email: Option<String>,
}

struct Token {
    refresh_token: String,
    email: Option<String>,
}

struct State {
    token: Option<Token>,
    access: Option<(String, Instant)>,
    file_id: Option<String>,
}

pub struct Drive {
    paths: Paths,
    safe: SafeStore,
    state: Mutex<State>,
    sync: Mutex<SyncState>,
    uploads: Mutex<Option<Sender<Envelope>>>,
    agent: ureq::Agent,
}

fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn sha256(data: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    sha2::Sha256::digest(data).into()
}

impl Drive {
    pub fn new(paths: Paths) -> Arc<Self> {
        let safe = SafeStore::new(&paths.dir);
        let token = std::fs::read(paths.google_token())
            .ok()
            .and_then(|b| safe.decrypt(&b))
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| {
                Some(Token { refresh_token: v["refresh_token"].as_str()?.to_string(), email: v["email"].as_str().map(str::to_string) })
            });
        let linked = token.is_some();
        let drive = Arc::new(Self {
            paths,
            safe,
            state: Mutex::new(State { token, access: None, file_id: None }),
            sync: Mutex::new(if linked { SyncState::Ok(String::new()) } else { SyncState::Local }),
            uploads: Mutex::new(None),
            agent: ureq::AgentBuilder::new().timeout(Duration::from_secs(30)).build(),
        });
        // Cola de subidas: un hilo que sube solo la última versión pendiente.
        let (tx, rx) = channel::<Envelope>();
        *drive.uploads.lock().unwrap() = Some(tx);
        let weak = Arc::downgrade(&drive);
        std::thread::spawn(move || {
            while let Ok(mut env) = rx.recv() {
                while let Ok(newer) = rx.try_recv() {
                    env = newer;
                }
                let Some(d) = weak.upgrade() else { return };
                if !d.status().linked {
                    continue;
                }
                d.set_sync(SyncState::Syncing);
                match d.upload(&env) {
                    Ok(()) => d.set_sync(SyncState::Ok(crate::store::now_iso())),
                    Err(e) => d.set_sync(SyncState::Error(e)),
                }
            }
        });
        drive
    }

    // ---------- credenciales / token ----------

    /// credentials.json: el que cargó el usuario (carpeta de datos), el que viene junto al programa o,
    /// en desarrollo, el de la raíz del repo (como app.getAppPath() en Electron).
    fn credentials_path(&self) -> Option<std::path::PathBuf> {
        let mut candidates = vec![self.paths.credentials()];
        if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) {
            candidates.push(dir.join("credentials.json"));
        }
        #[cfg(debug_assertions)]
        candidates.push(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../credentials.json"));
        candidates.into_iter().find(|p| p.exists())
    }

    pub fn has_credentials(&self) -> bool {
        self.credentials_path().is_some()
    }

    fn client(&self) -> Result<(String, String), String> {
        let path = self.credentials_path().ok_or("Falta credentials.json de Google")?;
        let text = std::fs::read_to_string(path).map_err(|_| "Falta credentials.json de Google".to_string())?;
        let raw: Value = serde_json::from_str(&text).map_err(|_| "credentials.json no es un JSON válido".to_string())?;
        let c = if raw["installed"].is_object() { &raw["installed"] } else if raw["web"].is_object() { &raw["web"] } else { &raw };
        let id = c["client_id"].as_str().ok_or("credentials.json no tiene client_id")?;
        Ok((id.to_string(), c["client_secret"].as_str().unwrap_or("").to_string()))
    }

    /// Copia el credentials.json elegido a la carpeta de datos (el instalador no trae uno).
    pub fn import_credentials(&self, from: &std::path::Path) -> Result<(), String> {
        let text = std::fs::read_to_string(from).map_err(|_| "No pude leer ese archivo".to_string())?;
        let raw: Value = serde_json::from_str(&text).map_err(|_| "Ese archivo no es un JSON válido".to_string())?;
        let c = if raw["installed"].is_object() { &raw["installed"] } else { &raw };
        if c["client_id"].as_str().is_none() {
            return Err("Ese JSON no parece un credentials.json de \"App de escritorio\"".into());
        }
        crate::paths::write_atomic(&self.paths.credentials(), text.as_bytes()).map_err(|e| e.to_string())
    }

    fn save_token(&self, token: Option<Token>) {
        let path = self.paths.google_token();
        match &token {
            Some(t) => {
                let json = json!({ "refresh_token": t.refresh_token, "email": t.email }).to_string();
                if let Ok(blob) = self.safe.encrypt(&json) {
                    let _ = crate::paths::write_atomic(&path, &blob);
                }
            }
            None => {
                let _ = std::fs::remove_file(&path);
            }
        }
        let linked = token.is_some();
        self.state.lock().unwrap().token = token;
        if !linked {
            self.set_sync(SyncState::Local);
        }
    }

    pub fn status(&self) -> DriveStatus {
        let s = self.state.lock().unwrap();
        DriveStatus {
            has_credentials: self.has_credentials(),
            linked: s.token.is_some(),
            email: s.token.as_ref().and_then(|t| t.email.clone()),
        }
    }

    pub fn sync_state(&self) -> SyncState {
        self.sync.lock().unwrap().clone()
    }

    pub fn set_sync(&self, s: SyncState) {
        *self.sync.lock().unwrap() = s;
    }

    // ---------- login ----------

    /// Abre el login de Google en el navegador (`open_url`) y espera la respuesta en un puerto local.
    pub fn link(&self, open_url: &dyn Fn(&str)) -> Result<DriveStatus, String> {
        let (client_id, secret) = self.client()?;
        let mut rnd = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut rnd);
        let verifier = b64url(&rnd);
        let challenge = b64url(&sha256(verifier.as_bytes()));
        let mut st = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut st);
        let state: String = st.iter().map(|b| format!("{b:02x}")).collect();

        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let redirect = format!("http://127.0.0.1:{}", listener.local_addr().map_err(|e| e.to_string())?.port());
        let auth = format!(
            "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={challenge}&code_challenge_method=S256&state={state}&access_type=offline&prompt=consent",
            urlencode(&client_id),
            urlencode(&redirect),
            urlencode(SCOPES)
        );
        open_url(&auth);

        let until = Instant::now() + LOGIN_TIMEOUT;
        let code = loop {
            if Instant::now() > until {
                return Err("Se acabó el tiempo para iniciar sesión".into());
            }
            let Ok((mut stream, _)) = listener.accept() else {
                std::thread::sleep(Duration::from_millis(200));
                continue;
            };
            let _ = stream.set_nonblocking(false);
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).to_string();
            let target = head.split_whitespace().nth(1).unwrap_or("").to_string();
            let (path, qs) = target.split_once('?').unwrap_or((&target, ""));
            if path != "/" {
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                continue;
            }
            let param = |k: &str| qs.split('&').filter_map(|kv| kv.split_once('=')).find(|(a, _)| *a == k).map(|(_, v)| v.to_string());
            let error = param("error");
            let code = param("code").filter(|_| error.is_none() && param("state").as_deref() == Some(state.as_str()));
            let page = format!(
                "<html><body style=\"font-family:sans-serif;background:#0b1015;color:#e8e2d0;text-align:center;padding-top:80px\"><h2>{}</h2><p>Ya puedes cerrar esta pestaña y volver a Smurf Vault.</p></body></html>",
                if code.is_some() { "✅ Cuenta de Google vinculada" } else { "❌ No se pudo vincular" }
            );
            let _ = stream.write_all(
                format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}", page.len())
                    .as_bytes(),
            );
            match code {
                Some(c) => break c,
                None => return Err(error.map(|e| format!("Google respondió: {e}")).unwrap_or_else(|| "Respuesta inválida de Google".into())),
            }
        };

        let tok = self.token_request(&[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", &redirect),
            ("code_verifier", &verifier),
            ("client_id", &client_id),
            ("client_secret", &secret),
        ])?;
        let refresh = tok["refresh_token"].as_str().ok_or("Google no entregó refresh_token")?.to_string();
        // Google muestra los permisos como casillas: sin la de Drive el token no sirve.
        if !tok["scope"].as_str().unwrap_or("").contains("drive.appdata") {
            let _ = self.agent.post(&format!("https://oauth2.googleapis.com/revoke?token={}", urlencode(&refresh))).call();
            return Err(MISSING_SCOPE.into());
        }
        let email = tok["id_token"]
            .as_str()
            .and_then(|t| t.split('.').nth(1))
            .and_then(|p| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(p.trim_end_matches('=')).ok())
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| v["email"].as_str().map(str::to_string));
        self.use_access(&tok);
        self.state.lock().unwrap().file_id = None;
        self.save_token(Some(Token { refresh_token: refresh, email }));
        self.set_sync(SyncState::Ok(String::new()));
        Ok(self.status())
    }

    pub fn unlink(&self) {
        let refresh = self.state.lock().unwrap().token.as_ref().map(|t| t.refresh_token.clone());
        self.save_token(None);
        {
            let mut s = self.state.lock().unwrap();
            s.access = None;
            s.file_id = None;
        }
        if let Some(r) = refresh {
            let _ = self.agent.post(&format!("https://oauth2.googleapis.com/revoke?token={}", urlencode(&r))).call();
        }
    }

    fn token_request(&self, params: &[(&str, &str)]) -> Result<Value, String> {
        let res = self.agent.post("https://oauth2.googleapis.com/token").send_form(params);
        let (ok, json) = match res {
            Ok(r) => (true, r.into_json::<Value>().unwrap_or(Value::Null)),
            Err(ureq::Error::Status(_, r)) => (false, r.into_json::<Value>().unwrap_or(Value::Null)),
            Err(e) => return Err(format!("No pude conectar con Google: {e}")),
        };
        if !ok {
            if json["error"] == "invalid_grant" {
                // Token revocado o caducado (típico si la app sigue en modo "Testing": dura 7 días).
                self.save_token(None);
                return Err("La vinculación con Google caducó, vuelve a vincular la cuenta".into());
            }
            let msg = json["error_description"].as_str().or(json["error"].as_str()).unwrap_or("desconocido");
            return Err(format!("Error de Google: {msg}"));
        }
        Ok(json)
    }

    fn use_access(&self, tok: &Value) {
        let secs = tok["expires_in"].as_u64().unwrap_or(3600).saturating_sub(60);
        if let Some(a) = tok["access_token"].as_str() {
            self.state.lock().unwrap().access = Some((a.to_string(), Instant::now() + Duration::from_secs(secs)));
        }
    }

    fn access(&self) -> Result<String, String> {
        let refresh = {
            let s = self.state.lock().unwrap();
            if let Some((a, until)) = &s.access {
                if Instant::now() < *until {
                    return Ok(a.clone());
                }
            }
            s.token.as_ref().map(|t| t.refresh_token.clone()).ok_or("Google no está vinculado")?
        };
        let (id, secret) = self.client()?;
        let tok = self.token_request(&[("grant_type", "refresh_token"), ("refresh_token", &refresh), ("client_id", &id), ("client_secret", &secret)])?;
        self.use_access(&tok);
        Ok(tok["access_token"].as_str().unwrap_or("").to_string())
    }

    /// Petición autenticada a Drive: devuelve el cuerpo como texto.
    fn api(&self, method: &str, url: &str, content_type: Option<&str>, body: Option<&[u8]>) -> Result<String, String> {
        let token = self.access()?;
        let mut req = self.agent.request(method, url).set("Authorization", &format!("Bearer {token}"));
        if let Some(ct) = content_type {
            req = req.set("Content-Type", ct);
        }
        let res = match body {
            Some(b) => req.send_bytes(b),
            None => req.call(),
        };
        match res {
            Ok(r) => r.into_string().map_err(|e| e.to_string()),
            Err(ureq::Error::Status(status, r)) => {
                let text = r.into_string().unwrap_or_default();
                if status == 403 && text.contains("insufficient authentication scopes") {
                    // Vinculado sin el permiso de Drive: se desvincula para hacerlo bien.
                    self.save_token(None);
                    self.state.lock().unwrap().access = None;
                    return Err(MISSING_SCOPE.into());
                }
                let msg = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
                    .unwrap_or_else(|| text.chars().take(200).collect());
                Err(format!("Drive {status}: {msg}"))
            }
            Err(e) => Err(format!("No pude conectar con Google Drive: {e}")),
        }
    }

    // ---------- archivo de la bóveda ----------

    fn find_file(&self) -> Result<Option<String>, String> {
        if let Some(id) = self.state.lock().unwrap().file_id.clone() {
            return Ok(Some(id));
        }
        let q = urlencode(&format!("name='{VAULT_NAME}'"));
        let text = self.api(
            "GET",
            &format!("https://www.googleapis.com/drive/v3/files?spaces=appDataFolder&q={q}&fields=files(id,modifiedTime)&orderBy=modifiedTime%20desc"),
            None,
            None,
        )?;
        let id = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["files"][0]["id"].as_str().map(str::to_string));
        self.state.lock().unwrap().file_id = id.clone();
        Ok(id)
    }

    /// La bóveda guardada en Drive, o None si no hay.
    pub fn download(&self) -> Result<Option<Envelope>, String> {
        let Some(id) = self.find_file()? else { return Ok(None) };
        let text = self.api("GET", &format!("https://www.googleapis.com/drive/v3/files/{id}?alt=media"), None, None)?;
        serde_json::from_str(&text).map(Some).map_err(|_| "La bóveda de Drive está dañada".to_string())
    }

    pub fn upload(&self, env: &Envelope) -> Result<(), String> {
        let body = serde_json::to_vec(env).map_err(|e| e.to_string())?;
        if let Some(id) = self.find_file()? {
            self.api("PATCH", &format!("https://www.googleapis.com/upload/drive/v3/files/{id}?uploadType=media"), Some("application/json"), Some(&body))?;
            return Ok(());
        }
        let mut rnd = [0u8; 8];
        rand::thread_rng().fill_bytes(&mut rnd);
        let boundary = format!("smurf-vault{}", rnd.iter().map(|b| format!("{b:02x}")).collect::<String>());
        let meta = json!({ "name": VAULT_NAME, "parents": ["appDataFolder"] }).to_string();
        let mut multipart = format!("--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{meta}\r\n--{boundary}\r\nContent-Type: application/json\r\n\r\n").into_bytes();
        multipart.extend_from_slice(&body);
        multipart.extend_from_slice(format!("\r\n--{boundary}--").as_bytes());
        let text = self.api(
            "POST",
            "https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart&fields=id",
            Some(&format!("multipart/related; boundary={boundary}")),
            Some(&multipart),
        )?;
        self.state.lock().unwrap().file_id = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["id"].as_str().map(str::to_string));
        Ok(())
    }

    /// Deja la bóveda para subirla en segundo plano (si Google está vinculado).
    pub fn queue_upload(&self, env: Envelope) {
        if !self.status().linked {
            return;
        }
        self.set_sync(SyncState::Syncing);
        if let Some(tx) = self.uploads.lock().unwrap().as_ref() {
            let _ = tx.send(env);
        }
    }
}

/// La versión más nueva entre la local y la de Drive (por fecha de guardado).
pub fn newest(local: Option<Envelope>, remote: Option<Envelope>) -> Option<(Envelope, bool)> {
    match (local, remote) {
        (None, None) => None,
        (Some(l), None) => Some((l, false)),
        (None, Some(r)) => Some((r, true)),
        (Some(l), Some(r)) => Some(if r.updated_at > l.updated_at { (r, true) } else { (l, false) }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_and_encoding() {
        // Ejemplo del RFC 7636.
        let challenge = b64url(&sha256(b"dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"));
        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        assert_eq!(urlencode("a b/c"), "a%20b%2Fc");
    }
}
