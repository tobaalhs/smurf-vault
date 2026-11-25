//! Smurf Vault en Tauri: el backend en Rust reemplaza a main.js de Electron. La interfaz es la misma
//! (renderer/), que habla con estos comandos a través de renderer/tauri-api.js (el equivalente de
//! preload.js). Los nombres de los comandos siguen los canales de Electron: "vault:unlock" -> vault_unlock.
mod vault;

use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use vault::{Envelope, Key};

// ---------- archivos ----------

/// Carpeta de datos: la misma que la versión Electron (%APPDATA%\Smurf Vault), así se abren la
/// bóveda, la configuración, el historial y las sesiones que ya existen.
fn data_dir() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")).join("Smurf Vault")
}

fn local_vault() -> PathBuf {
    data_dir().join("vault.dat")
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

fn read_envelope(path: &Path) -> Option<Envelope> {
    serde_json::from_value(read_json(path)?).ok()
}

/// Escribe la bóveda local guardando antes la anterior en vault.prev.dat (igual que Electron).
fn write_local(env: &Envelope) -> Result<(), String> {
    let path = local_vault();
    fs::create_dir_all(data_dir()).map_err(|e| e.to_string())?;
    if path.exists() {
        fs::copy(&path, data_dir().join("vault.prev.dat")).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("dat.tmp");
    fs::write(&tmp, serde_json::to_string(env).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

/// Preferencias no secretas (config.json), con los mismos valores por defecto que Electron.
fn read_config() -> Map<String, Value> {
    let mut config = json!({
        "autoAccept": false, "autoAcceptDelay": 0, "closeToTray": true, "trayHintShown": false,
        "appearOffline": false, "autoType": true, "theme": {}, "remoteEnabled": false, "remoteToken": null,
    });
    if let (Some(Value::Object(saved)), Value::Object(defaults)) = (read_json(&data_dir().join("config.json")), &mut config) {
        defaults.extend(saved);
    }
    match config {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

/// { idCuenta: fecha en que se guardó su sesión del Riot Client }
fn session_index() -> Value {
    let mut out = Map::new();
    if let Ok(entries) = fs::read_dir(data_dir().join("sessions")) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(id) = name.strip_suffix(".dat") else { continue };
            if let Ok(modified) = entry.metadata().and_then(|m| m.modified()) {
                let at = chrono::DateTime::<chrono::Utc>::from(modified).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                out.insert(id.to_string(), Value::String(at));
            }
        }
    }
    Value::Object(out)
}

// ---------- estado ----------

/// La bóveda desbloqueada, más el historial de partidas y el LP (cifrados con la misma clave).
#[allow(dead_code)] // key y salt se usan al guardar cambios (próximos comandos)
struct Session {
    key: Key,
    salt: Vec<u8>,
    data: Value,
    history: Value,
    lp: Value,
}

#[derive(Default)]
struct AppState {
    session: Mutex<Option<Session>>,
}

/// Historial y LP: archivos locales cifrados con la clave de la bóveda; si no se pueden leer, vacíos.
fn load_with_key(key: &Key, file: &str) -> Value {
    read_envelope(&data_dir().join(file))
        .and_then(|env| vault::decrypt_with_key(key, &env).ok())
        .unwrap_or_else(|| json!({}))
}

/// Lo que ve la interfaz: la bóveda más el historial y el LP de cada cuenta y las sesiones guardadas.
fn public_data(s: &Session) -> Value {
    let mut data = s.data.clone();
    if let Some(accounts) = data.get_mut("accounts").and_then(Value::as_array_mut) {
        for acc in accounts.iter_mut() {
            let id = acc.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
            if let Some(obj) = acc.as_object_mut() {
                if let Some(m) = s.history.get(&id) {
                    obj.insert("matches".into(), m.clone());
                }
                if let Some(lp) = s.lp.get(&id) {
                    obj.insert("lp".into(), lp.clone());
                }
            }
        }
    }
    if let Some(obj) = data.as_object_mut() {
        obj.insert("sessions".into(), session_index());
    }
    data
}

fn open_session(key: Key, salt: Vec<u8>, data: Value) -> Session {
    // { ...emptyVault(), ...data }: una bóveda vieja sin algún campo queda completa.
    let mut full = vault::empty_vault();
    if let (Value::Object(base), Value::Object(saved)) = (&mut full, data) {
        base.extend(saved);
    }
    let history = load_with_key(&key, "history.dat");
    let lp = load_with_key(&key, "lp.dat");
    Session { key, salt, data: full, history, lp }
}

// ---------- comandos: app ----------

#[tauri::command]
fn app_status(app: AppHandle, state: State<AppState>) -> Value {
    let config = read_config();
    let credentials = data_dir().join("credentials.json");
    json!({
        "hasLocal": local_vault().exists(),
        "unlocked": state.session.lock().unwrap().is_some(),
        // Google Drive todavía no está portado: se muestra sin vincular.
        "google": { "hasCredentials": credentials.exists(), "linked": false, "email": null },
        "credentialsPath": credentials.to_string_lossy(),
        "version": app.package_info().version.to_string(),
        "updateReady": null,
        "lockSkins": config.get("lockSkins").cloned().unwrap_or_else(|| json!([])),
        "theme": config.get("theme").cloned().unwrap_or_else(|| json!({})),
    })
}

/// Solo abrimos en el navegador links conocidos, nunca URLs arbitrarias.
#[tauri::command]
fn app_open_external(url: String) -> Result<(), String> {
    const ALLOWED: [&str; 2] = ["https://developer.riotgames.com/", "https://github.com/tobaalhs/smurf-vault"];
    if !ALLOWED.iter().any(|u| url.starts_with(u)) {
        return Err("Link no permitido".into());
    }
    std::process::Command::new("explorer").arg(&url).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command]
fn tools_get() -> Value {
    let mut config = read_config();
    config.remove("remoteToken"); // el código del celular no se manda a la interfaz salvo en el QR
    config.insert("openAtLogin".into(), json!(false));
    config.insert("clientConnected".into(), json!(false));
    Value::Object(config)
}

// ---------- comandos: bóveda ----------

#[tauri::command]
async fn vault_create(password: String, state: State<'_, AppState>) -> Result<Value, String> {
    if password.chars().count() < 8 {
        return Err("Usa al menos 8 caracteres".into());
    }
    if local_vault().exists() {
        return Err("Ya existe una bóveda en este PC".into());
    }
    let (key, salt) = vault::create_key(&password)?;
    let data = vault::empty_vault();
    write_local(&vault::encrypt_with_key(&key, &salt, &data)?)?;
    let session = open_session(key, salt, data);
    let out = public_data(&session);
    *state.session.lock().unwrap() = Some(session);
    Ok(out)
}

/// Abre la bóveda local. No escribe nada (Drive todavía no está portado).
#[tauri::command]
async fn vault_unlock(password: String, state: State<'_, AppState>) -> Result<Value, String> {
    let env = read_envelope(&local_vault()).ok_or_else(|| "No hay bóveda para desbloquear".to_string())?;
    let (key, salt, data) = vault::decrypt_envelope(&env, &password)?;
    let session = open_session(key, salt, data);
    let out = public_data(&session);
    *state.session.lock().unwrap() = Some(session);
    Ok(out)
}

#[tauri::command]
fn vault_lock(state: State<AppState>) -> bool {
    *state.session.lock().unwrap() = None;
    true
}

#[tauri::command]
fn vault_get(state: State<AppState>) -> Value {
    state.session.lock().unwrap().as_ref().map(public_data).unwrap_or(Value::Null)
}

// ---------- comandos: ventana (la app no usa la barra de título de Windows) ----------

#[tauri::command]
fn win_minimize(window: WebviewWindow) -> Result<(), String> {
    window.minimize().map_err(|e| e.to_string())
}

#[tauri::command]
fn win_toggle_maximize(window: WebviewWindow) -> Result<(), String> {
    let result = if window.is_maximized().unwrap_or(false) { window.unmaximize() } else { window.maximize() };
    result.map_err(|e| e.to_string())
}

#[tauri::command]
fn win_close(window: WebviewWindow) -> Result<(), String> {
    window.close().map_err(|e| e.to_string())
}

#[tauri::command]
fn win_is_maximized(window: WebviewWindow) -> bool {
    window.is_maximized().unwrap_or(false)
}

pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .setup(|app| {
            let window = app.get_webview_window("main").expect("ventana principal");
            // Avisamos a la interfaz al maximizar o restaurar, para cambiar el ícono del botón.
            let w = window.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::Resized(_) = event {
                    let _ = w.emit("win:state", json!({ "maximized": w.is_maximized().unwrap_or(false) }));
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_status,
            app_open_external,
            tools_get,
            vault_create,
            vault_unlock,
            vault_lock,
            vault_get,
            win_minimize,
            win_toggle_maximize,
            win_close,
            win_is_maximized,
        ])
        .run(tauri::generate_context!())
        .expect("no se pudo iniciar Smurf Vault");
}
