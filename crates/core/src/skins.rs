//! Catálogo de skins para el cambiador de fondo de perfil: una sola descarga de CommunityDragon trae
//! todas con su id (= backgroundSkinId), nombre y miniatura. Caché local renovado cada semana; sin
//! internet se usa el caché. Mismo skins.json que src/skins.js.
use crate::lcu::lcu;
use crate::paths::Paths;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const SKINS_URL_ES: &str = "https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/es_mx/v1/skins.json";
const SKINS_URL_EN: &str = "https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/default/v1/skins.json";
const CDRAGON: &str = "https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/default/";
const MAX_AGE_MS: i64 = 7 * 24 * 60 * 60 * 1000;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Skin {
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub champ: i64,
    #[serde(default)]
    pub tile: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkinCatalog {
    #[serde(default)]
    pub skins: Vec<Skin>,
    #[serde(default)]
    pub fetched_at: i64,
}

fn asset_url(p: &str) -> Option<String> {
    (!p.is_empty()).then(|| format!("{CDRAGON}{}", p.replace("/lol-game-data/assets/", "").to_lowercase()))
}

impl SkinCatalog {
    /// El caché si está fresco; si no, lo descarga (o se queda con el caché viejo). Bloqueante.
    pub fn load(paths: &Paths) -> Result<Self, String> {
        let cached: Option<Self> = std::fs::read_to_string(paths.skins()).ok().and_then(|s| serde_json::from_str(&s).ok());
        let now = chrono::Utc::now().timestamp_millis();
        if let Some(c) = &cached {
            if !c.skins.is_empty() && now - c.fetched_at < MAX_AGE_MS {
                return Ok(c.clone());
            }
        }
        match Self::fetch(paths) {
            Ok(fresh) => Ok(fresh),
            Err(e) => cached.filter(|c| !c.skins.is_empty()).ok_or(e),
        }
    }

    fn fetch(paths: &Paths) -> Result<Self, String> {
        let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(20)).build();
        let get = |url: &str| -> Result<Value, String> { agent.get(url).call().map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string()) };
        let raw = get(SKINS_URL_ES).or_else(|_| get(SKINS_URL_EN))?;
        let mut skins: Vec<Skin> = raw
            .as_object()
            .into_iter()
            .flat_map(|m| m.values())
            .filter_map(|s| {
                let id = s["id"].as_i64()?;
                Some(Skin {
                    id,
                    name: s["name"].as_str().unwrap_or("").to_string(),
                    // id = campeón*1000 + número de skin (así lo interpreta el cliente).
                    champ: id / 1000,
                    tile: s["tilePath"].as_str().and_then(asset_url),
                })
            })
            .collect();
        skins.sort_by_key(|s| s.id);
        let data = SkinCatalog { skins, fetched_at: chrono::Utc::now().timestamp_millis() };
        if let Ok(json) = serde_json::to_vec(&data) {
            let _ = crate::paths::write_atomic(&paths.skins(), &json);
        }
        Ok(data)
    }
}

/// Texto para buscar sin tildes ni mayúsculas.
pub fn norm_text(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            c => c,
        })
        .collect()
}

/// Cambia el fondo del perfil de LoL a cualquier skin (la tengas o no), como League Profile Tool.
/// Necesita el LoL abierto. Bloqueante.
pub fn set_profile_background(skin_id: i64) -> Result<(), String> {
    if skin_id <= 0 {
        return Err("Skin inválida".into());
    }
    let body = json!({ "key": "backgroundSkinId", "value": skin_id });
    match lcu().request("POST", "/lol-summoner/v1/current-summoner/summoner-profile", true, Some(&body)) {
        None => Err("Abre el cliente de LoL para cambiar el fondo".into()),
        Some((s, _)) if s >= 400 => Err(format!("El cliente rechazó el cambio ({s})")),
        Some(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_ignores_accents_and_case() {
        assert_eq!(norm_text("Ahri Guardiana Estelar ÉPICA"), "ahri guardiana estelar epica");
        assert_eq!(asset_url("/lol-game-data/assets/ASSETS/X.jpg").unwrap(), format!("{CDRAGON}assets/x.jpg"));
    }
}
