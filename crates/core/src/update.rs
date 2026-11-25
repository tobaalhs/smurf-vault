//! Actualizaciones desde GitHub Releases, como electron-updater en la versión Electron: se busca la
//! última versión, se baja su instalador, se verifica con el sha512 de latest.yml y queda listo para
//! instalarse (al tocar "Reiniciar" o al salir). Solo en la versión instalada: la portable y el
//! desarrollo no se actualizan solas. Bloqueante.
use base64::Engine;
use serde_json::Value;
use sha2::Digest;
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

const REPO: &str = "tobaalhs/smurf-vault";
/// Argumento que el instalador entiende como "es una actualización" (igual que electron-updater).
pub const UPDATED_ARG: &str = "--updated";

/// Versión de este programa.
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// "v0.10.2" -> (0, 10, 2).
fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.trim().trim_start_matches('v').split(['.', '-']).map(|p| p.parse::<u64>().ok());
    Some((it.next()??, it.next()??, it.next().flatten().unwrap_or(0)))
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// ¿Es la versión instalada? (junto al .exe está el desinstalador; la portable no lo tiene).
pub fn is_installed() -> bool {
    if cfg!(debug_assertions) {
        return false;
    }
    std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("Uninstall Smurf Vault.exe").exists())).unwrap_or(false)
}

/// Una actualización ya descargada y verificada.
#[derive(Clone, Debug)]
pub struct ReadyUpdate {
    pub version: String,
    pub installer: PathBuf,
}

/// Lee `sha512` del latest.yml (formato de electron-builder).
fn sha512_from_yml(yml: &str) -> Option<String> {
    yml.lines()
        .find_map(|l| l.trim().strip_prefix("sha512:").map(|v| v.trim().trim_matches('\'').trim_matches('"').to_string()))
}

/// Busca una versión nueva; si hay, baja el instalador a una carpeta temporal y lo verifica.
/// Ok(None) si ya está al día.
pub fn check_and_download() -> Result<Option<ReadyUpdate>, String> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(60)).user_agent("smurf-vault-updater").build();
    let release: Value = agent
        .get(&format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("No pude consultar las actualizaciones: {e}"))?
        .into_json()
        .map_err(|e| e.to_string())?;
    let tag = release["tag_name"].as_str().unwrap_or("");
    if !is_newer(tag, current_version()) {
        return Ok(None);
    }
    let version = tag.trim_start_matches('v').to_string();
    let assets = release["assets"].as_array().cloned().unwrap_or_default();
    let asset_url = |name: &str| {
        assets.iter().find(|a| a["name"] == name).and_then(|a| a["browser_download_url"].as_str().map(str::to_string))
    };
    let setup_name = format!("Smurf-Vault-Setup-{version}.exe");
    let setup_url = asset_url(&setup_name).ok_or("La versión nueva no trae instalador")?;
    let yml_url = asset_url("latest.yml").ok_or("La versión nueva no trae latest.yml")?;
    let yml = agent.get(&yml_url).call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())?;
    let expected = sha512_from_yml(&yml).ok_or("latest.yml sin sha512")?;

    let dir = std::env::temp_dir().join("smurf-vault-update");
    let _ = std::fs::create_dir_all(&dir);
    let installer = dir.join(&setup_name);
    // Si ya se bajó antes y está bien, no se vuelve a bajar.
    let ok = |path: &PathBuf| {
        std::fs::read(path).map(|b| base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(&b)) == expected).unwrap_or(false)
    };
    if !ok(&installer) {
        let mut bytes = Vec::new();
        agent
            .get(&setup_url)
            .call()
            .map_err(|e| format!("No pude bajar la actualización: {e}"))?
            .into_reader()
            .take(512 << 20)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(&bytes)) != expected {
            return Err("La actualización bajada no coincide con su firma (sha512)".into());
        }
        crate::paths::write_atomic(&installer, &bytes).map_err(|e| e.to_string())?;
    }
    Ok(Some(ReadyUpdate { version, installer }))
}

/// Abre el instalador para que actualice. `silent` sin ventanas; `run_after` vuelve a abrir la app.
/// Después hay que cerrar la app (el instalador la reemplaza).
pub fn launch_installer(update: &ReadyUpdate, silent: bool, run_after: bool) -> Result<(), String> {
    let mut args = vec![UPDATED_ARG];
    if silent {
        args.push("/S");
    }
    if run_after {
        args.push("--force-run");
    }
    crate::switcher::spawn_detached(&update.installer, &args).map_err(|_| "No pude abrir el instalador de la actualización".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_and_latest_yml() {
        assert!(is_newer("v0.10.0", "0.9.0"));
        assert!(is_newer("0.9.1", "0.9.0"));
        assert!(!is_newer("v0.9.0", "0.9.0"));
        assert!(!is_newer("v0.8.0", "0.9.0"));
        assert!(!is_newer("basura", "0.9.0"));
        let yml = "version: 0.9.0\nfiles:\n  - url: a.exe\n    sha512: AAA==\n    size: 1\npath: a.exe\nsha512: BBB==\n";
        assert_eq!(sha512_from_yml(yml).as_deref(), Some("AAA=="));
    }
}
