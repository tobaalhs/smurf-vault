//! Datos públicos del juego que la interfaz necesita: nombres de campeones, imágenes de minileyendas
//! (CommunityDragon), hechizos y la versión de Data Dragon (íconos de ítems). Es el mismo caché
//! gamedata.json que arma la versión Electron (src/gamedata.js).
use crate::paths::Paths;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

const CHAMPIONS_URL: &str = "https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/default/v1/champion-summary.json";
const VERSIONS_URL: &str = "https://ddragon.leagueoflegends.com/api/versions.json";
const COMPANIONS_URL: &str = "https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/default/v1/companions.json";
/// Hechizos de invocador con nombre en español (para el control desde el celular).
const SPELLS_URL: &str = "https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/es_mx/v1/summoner-spells.json";
const CDRAGON_ASSETS: &str = "https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/default/";
const MAX_AGE_MS: i64 = 24 * 60 * 60 * 1000;

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

    /// ¿Hay que volver a descargarlo? (más de un día, o un caché viejo sin minileyendas o hechizos).
    pub fn is_stale(&self) -> bool {
        let age = chrono::Utc::now().timestamp_millis() - self.fetched_at;
        self.companions.is_empty() || !self.spells.is_array() || !(0..MAX_AGE_MS).contains(&age)
    }

    /// Descarga los datos de CommunityDragon y Data Dragon y los guarda en el caché. Bloqueante.
    pub fn fetch_fresh(paths: &Paths) -> Result<Self, String> {
        let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(10)).build();
        let get = |url: &str| -> Result<Value, String> {
            agent.get(url).call().map_err(|e| format!("{url}: {e}"))?.into_json().map_err(|e| e.to_string())
        };
        let (champions, versions, companions, spells) = std::thread::scope(|s| {
            let a = s.spawn(|| get(CHAMPIONS_URL));
            let b = s.spawn(|| get(VERSIONS_URL));
            let c = s.spawn(|| get(COMPANIONS_URL));
            let d = s.spawn(|| get(SPELLS_URL));
            let j = |h: std::thread::ScopedJoinHandle<'_, Result<Value, String>>| h.join().unwrap_or(Err("error interno".into()));
            (j(a), j(b), j(c), j(d))
        });
        let (champions, versions, companions, spells) = (champions?, versions?, companions?, spells?);
        let asset = |p: &str| p.replace("/lol-game-data/assets/", "").to_lowercase();
        let mut data = GameData {
            ddragon: versions[0].as_str().unwrap_or("").to_string(),
            fetched_at: chrono::Utc::now().timestamp_millis(),
            ..Default::default()
        };
        for c in champions.as_array().into_iter().flatten() {
            if let (Some(id), Some(name)) = (c["id"].as_i64().filter(|id| *id > 0), c["name"].as_str()) {
                data.champions.insert(id.to_string(), name.to_string());
            }
        }
        for c in companions.as_array().into_iter().flatten() {
            if let (Some(id), Some(icon)) = (c["contentId"].as_str().filter(|s| !s.is_empty()), c["loadoutsIcon"].as_str()) {
                data.companions.insert(id.to_string(), asset(icon));
            }
        }
        // Solo los hechizos de Grieta y ARAM (el resto son de modos temporales).
        let main_mode = |m: &&Value| *m == "CLASSIC" || *m == "ARAM";
        let spells: Vec<Value> = spells
            .as_array()
            .into_iter()
            .flatten()
            .filter(|sp| sp["id"].as_i64().is_some_and(|id| id > 0) && sp["gameModes"].as_array().is_some_and(|m| m.iter().any(|x| main_mode(&x))))
            .map(|sp| {
                serde_json::json!({
                    "id": sp["id"],
                    "name": sp["name"],
                    "icon": format!("{CDRAGON_ASSETS}{}", asset(sp["iconPath"].as_str().unwrap_or(""))),
                    "modes": sp["gameModes"].as_array().map(|m| m.iter().filter(main_mode).cloned().collect::<Vec<_>>()),
                })
            })
            .collect();
        data.spells = Value::Array(spells);
        let json = serde_json::to_vec(&data).map_err(|e| e.to_string())?;
        crate::paths::write_atomic(&paths.game_data(), &json).map_err(|e| e.to_string())?;
        Ok(data)
    }

    pub fn champion(&self, id: i64) -> String {
        self.champions.get(&id.to_string()).cloned().unwrap_or_default()
    }

    /// URL de la imagen de una minileyenda, si se conoce.
    pub fn companion_url(&self, content_id: &str) -> Option<String> {
        self.companions
            .get(content_id)
            .map(|p| format!("https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/default/{p}"))
    }

    pub fn item_url(&self, id: i64) -> Option<String> {
        (!self.ddragon.is_empty() && id > 0)
            .then(|| format!("https://ddragon.leagueoflegends.com/cdn/{}/img/item/{id}.png", self.ddragon))
    }
}
