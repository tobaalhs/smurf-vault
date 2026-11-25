//! Datos públicos del juego que la interfaz necesita: nombres de campeones, imágenes de minileyendas
//! (CommunityDragon), hechizos y la versión de Data Dragon (íconos de ítems). Es el mismo caché
//! gamedata.json que arma la versión Electron (src/gamedata.js).
use crate::paths::Paths;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct GameData {
    /// id de campeón -> nombre.
    #[serde(default)]
    pub champions: BTreeMap<String, String>,
    /// contentId de la minileyenda -> ruta de su imagen dentro de CommunityDragon.
    #[serde(default)]
    pub companions: BTreeMap<String, String>,
    #[serde(default)]
    pub spells: Value,
    /// Versión de Data Dragon, p. ej. "15.23.1".
    #[serde(default)]
    pub ddragon: String,
    #[serde(default)]
    pub fetched_at: i64,
}

impl GameData {
    /// Lo que haya en el caché (sin internet), o vacío.
    pub fn load_cache(paths: &Paths) -> Self {
        std::fs::read_to_string(paths.game_data()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn champion(&self, id: i64) -> String {
        self.champions.get(&id.to_string()).cloned().unwrap_or_default()
    }

    /// URL de la imagen de una minileyenda, si se conoce.
    pub fn companion_url(&self, content_id: i64) -> Option<String> {
        self.companions
            .get(&content_id.to_string())
            .map(|p| format!("https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/default/{p}"))
    }

    pub fn item_url(&self, id: i64) -> Option<String> {
        (!self.ddragon.is_empty() && id > 0)
            .then(|| format!("https://ddragon.leagueoflegends.com/cdn/{}/img/item/{id}.png", self.ddragon))
    }
}
