//! Archivos de datos. Es la misma carpeta que usa la versión Electron (%APPDATA%\Smurf Vault), así
//! la app en Rust abre la bóveda, la configuración, el historial y las sesiones que ya existen.
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Paths {
    pub dir: PathBuf,
}

impl Default for Paths {
    fn default() -> Self {
        let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        Self { dir: base.join("Smurf Vault") }
    }
}

impl Paths {
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }
    pub fn vault(&self) -> PathBuf {
        self.dir.join("vault.dat")
    }
    pub fn vault_backup(&self) -> PathBuf {
        self.dir.join("vault.prev.dat")
    }
    /// Preferencias que no son secretas y deben funcionar con la bóveda bloqueada.
    pub fn config(&self) -> PathBuf {
        self.dir.join("config.json")
    }
    /// Historial de partidas: solo en este PC, cifrado con la clave de la bóveda.
    pub fn history(&self) -> PathBuf {
        self.dir.join("history.dat")
    }
    /// Evolución de LP: solo en este PC, cifrada con la clave de la bóveda.
    pub fn lp(&self) -> PathBuf {
        self.dir.join("lp.dat")
    }
    /// Sesiones del Riot Client por cuenta ("Mantener sesión iniciada"), cifradas.
    pub fn sessions(&self) -> PathBuf {
        self.dir.join("sessions")
    }
    pub fn session(&self, id: &str) -> PathBuf {
        self.sessions().join(format!("{id}.dat"))
    }
    pub fn credentials(&self) -> PathBuf {
        self.dir.join("credentials.json")
    }
    pub fn google_token(&self) -> PathBuf {
        self.dir.join("google-token.bin")
    }
    pub fn game_data(&self) -> PathBuf {
        self.dir.join("gamedata.json")
    }
    pub fn skins(&self) -> PathBuf {
        self.dir.join("skins.json")
    }
}

/// Escribe un archivo pasando por un .tmp, para no dejarlo a medias si algo falla.
pub fn write_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)
}
