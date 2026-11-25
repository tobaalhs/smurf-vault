//! Datos de cada cuenta que solo viven en este PC (no se suben a Drive, así la bóveda sincronizada
//! queda chica) y van cifrados con la misma clave de la bóveda: el historial de partidas y la
//! evolución de LP. Si se pierden, se vuelven a armar solos desde el cliente de LoL.
use crate::matches::{merge_matches, Match};
use crate::model::Ranks;
use crate::paths::write_atomic;
use crate::vault::{self, Envelope, Key};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Archivo cifrado con un objeto { idCuenta: datos }.
#[derive(Debug, Clone)]
pub struct LocalStore<T> {
    path: PathBuf,
    by_account: BTreeMap<String, T>,
    /// Entradas que esta versión no supo leer: se conservan tal cual para no perderlas al guardar.
    unparsed: Map<String, Value>,
}

impl<T: Serialize + DeserializeOwned + Clone> LocalStore<T> {
    pub fn empty(path: PathBuf) -> Self {
        Self { path, by_account: BTreeMap::new(), unparsed: Map::new() }
    }

    /// Lee el archivo con la clave de la bóveda recién abierta. Si no existe o es de otra bóveda, parte vacío.
    pub fn load(path: PathBuf, key: &Key) -> Self {
        let mut store = Self::empty(path);
        let all = std::fs::read_to_string(&store.path)
            .ok()
            .and_then(|s| serde_json::from_str::<Envelope>(&s).ok())
            .and_then(|env| vault::decrypt_with_key(key, &env).ok());
        if let Some(Value::Object(map)) = all {
            for (id, raw) in map {
                match serde_json::from_value::<T>(raw.clone()) {
                    Ok(v) => {
                        store.by_account.insert(id, v);
                    }
                    Err(_) => {
                        store.unparsed.insert(id, raw);
                    }
                }
            }
        }
        store
    }

    pub fn save(&self, key: &Key, salt: &[u8]) -> Result<(), String> {
        let mut all = self.unparsed.clone();
        for (id, v) in &self.by_account {
            all.insert(id.clone(), serde_json::to_value(v).map_err(|e| e.to_string())?);
        }
        let env = vault::encrypt_with_key(key, salt, &Value::Object(all))?;
        let json = serde_json::to_vec(&env).map_err(|e| e.to_string())?;
        write_atomic(&self.path, &json).map_err(|e| e.to_string())
    }

    pub fn get(&self, id: &str) -> Option<&T> {
        self.by_account.get(id)
    }

    pub fn set(&mut self, id: &str, value: T) {
        self.unparsed.remove(id);
        self.by_account.insert(id.to_string(), value);
    }

    /// Borra lo de una cuenta. Devuelve true si tenía algo.
    pub fn remove(&mut self, id: &str) -> bool {
        let a = self.by_account.remove(id).is_some();
        let b = self.unparsed.remove(id).is_some();
        a || b
    }

    pub fn contains(&self, id: &str) -> bool {
        self.by_account.contains_key(id) || self.unparsed.contains_key(id)
    }
}

/// Últimas partidas de LoL y TFT de cada cuenta.
pub type History = LocalStore<Vec<Match>>;

impl History {
    /// Junta partidas nuevas con las guardadas. Devuelve true si cambió algo.
    pub fn add(&mut self, id: &str, matches: &[Match]) -> bool {
        let before = self.get(id).cloned().unwrap_or_default();
        let merged = merge_matches(&before, matches);
        if merged == before {
            return false;
        }
        self.set(id, merged);
        true
    }
}

/// Un punto del gráfico de LP.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LpPoint {
    pub at: String,
    pub tier: String,
    #[serde(default)]
    pub division: String,
    #[serde(default)]
    pub lp: i64,
}

pub const LP_QUEUES: [&str; 3] = ["solo", "flex", "tft"];
/// Puntos por cola: sobra para una temporada completa.
const LP_KEEP: usize = 300;

/// Evolución del rango: { solo: [puntos], flex: [...], tft: [...] } por cuenta.
pub type LpLog = LocalStore<BTreeMap<String, Vec<LpPoint>>>;

impl LpLog {
    /// Anota los rangos si cambiaron desde el último punto. Devuelve true si anotó algo.
    pub fn record(&mut self, id: &str, ranks: &Ranks, at: &str) -> bool {
        let mut log = self.get(id).cloned().unwrap_or_default();
        let mut changed = false;
        for q in LP_QUEUES {
            let Some(r) = ranks.get(q).filter(|r| !r.tier.is_empty()) else { continue };
            let list = log.entry(q.to_string()).or_default();
            if list.last().is_some_and(|l| l.tier == r.tier && l.division == r.division && l.lp == r.lp) {
                continue;
            }
            list.push(LpPoint { at: at.to_string(), tier: r.tier.clone(), division: r.division.clone(), lp: r.lp });
            if list.len() > LP_KEEP {
                let extra = list.len() - LP_KEEP;
                list.drain(..extra);
            }
            changed = true;
        }
        if changed {
            self.set(id, log);
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Rank;

    fn rank(tier: &str, lp: i64) -> Option<Rank> {
        Some(Rank { tier: tier.into(), division: "II".into(), lp, ..Default::default() })
    }

    #[test]
    fn lp_only_records_changes() {
        let mut log = LpLog::empty(PathBuf::from("x"));
        let ranks = Ranks { solo: rank("GOLD", 10), ..Default::default() };
        assert!(log.record("a", &ranks, "2026-01-01T00:00:00.000Z"));
        assert!(!log.record("a", &ranks, "2026-01-02T00:00:00.000Z"), "mismo rango: no anota");
        let ranks = Ranks { solo: rank("GOLD", 30), ..Default::default() };
        assert!(log.record("a", &ranks, "2026-01-03T00:00:00.000Z"));
        assert_eq!(log.get("a").unwrap()["solo"].len(), 2);
    }

    #[test]
    fn save_and_load_roundtrip_keeping_unreadable_entries() {
        let dir = std::env::temp_dir().join(format!("sv-local-{}", std::process::id()));
        let (key, salt) = vault::create_key("x").unwrap();
        // Un archivo escrito "por otra versión" con una entrada que esta no entiende.
        let raw = serde_json::json!({ "a": [], "b": "formato-raro" });
        let env = vault::encrypt_with_key(&key, &salt, &raw).unwrap();
        write_atomic(&dir.join("h.dat"), &serde_json::to_vec(&env).unwrap()).unwrap();
        let h = History::load(dir.join("h.dat"), &key);
        assert!(h.contains("a") && h.contains("b"));
        h.save(&key, &salt).unwrap();
        let again = History::load(dir.join("h.dat"), &key);
        assert!(again.contains("b"), "la entrada rara no se pierde");
        let _ = std::fs::remove_dir_all(dir);
    }
}
