//! config.json: preferencias que no son secretas y deben funcionar con la bóveda bloqueada.
//! Mismos campos y valores por defecto que la versión Electron.
use crate::paths::{write_atomic, Paths};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    pub auto_accept: bool,
    pub auto_accept_delay: u32,
    pub close_to_tray: bool,
    pub tray_hint_shown: bool,
    /// Herramienta: aparecer desconectado en el chat del LoL.
    pub appear_offline: bool,
    /// Inicio de sesión automático (inyección en el login de Riot).
    pub auto_type: bool,
    /// Colores que el usuario cambió (el resto son los predeterminados), como "#rrggbb".
    pub theme: BTreeMap<String, String>,
    /// Herramienta: controlar la selección de campeones desde el celular.
    pub remote_enabled: bool,
    /// Código secreto que va en el QR del control desde el celular.
    pub remote_token: Option<String>,
    /// Splash de fondo de las cuentas, para la pantalla de bloqueo (solo números de skin).
    pub lock_skins: Vec<i64>,
    /// Barra lateral (Cuentas, Herramientas, Ajustes) abierta.
    pub sidebar_open: bool,
    /// Transparencia de la ventana en %, para ver el escritorio detrás (0 = opaca).
    pub translucency: u32,
    /// Campos que esta versión no conoce: se conservan tal cual al guardar.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            auto_accept: false,
            auto_accept_delay: 0,
            close_to_tray: true,
            tray_hint_shown: false,
            appear_offline: false,
            auto_type: true,
            theme: BTreeMap::new(),
            remote_enabled: false,
            remote_token: None,
            lock_skins: Vec::new(),
            sidebar_open: true,
            translucency: 0,
            extra: Map::new(),
        }
    }
}

/// Nombres de colores que se pueden personalizar.
pub const THEME_KEYS: [&str; 10] = ["bg", "surface", "text", "muted", "gold", "cyan", "ok", "danger", "win", "loss"];

impl Config {
    pub fn load(paths: &Paths) -> Self {
        std::fs::read_to_string(paths.config())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, paths: &Paths) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        write_atomic(&paths.config(), json.as_bytes())
    }

    /// Solo nombres conocidos y colores #rrggbb (igual que theme:set en Electron).
    pub fn clean_theme(theme: &BTreeMap<String, String>) -> BTreeMap<String, String> {
        theme
            .iter()
            .filter(|(k, v)| {
                THEME_KEYS.contains(&k.as_str())
                    && v.len() == 7
                    && v.starts_with('#')
                    && v[1..].chars().all(|c| c.is_ascii_hexdigit())
            })
            .map(|(k, v)| (k.clone(), v.to_lowercase()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_fields_survive_a_save() {
        let cfg: Config = serde_json::from_str(r#"{"autoAccept":true,"futureFlag":{"a":1}}"#).unwrap();
        assert!(cfg.auto_accept);
        assert!(cfg.close_to_tray, "lo que falta toma el valor por defecto");
        let out = serde_json::to_value(&cfg).unwrap();
        assert_eq!(out["futureFlag"]["a"], 1);
    }

    #[test]
    fn theme_only_keeps_known_hex_colors() {
        let theme = BTreeMap::from([
            ("gold".to_string(), "#AABBCC".to_string()),
            ("gold2".to_string(), "#aabbcc".to_string()),
            ("bg".to_string(), "red".to_string()),
        ]);
        assert_eq!(Config::clean_theme(&theme), BTreeMap::from([("gold".to_string(), "#aabbcc".to_string())]));
    }
}
