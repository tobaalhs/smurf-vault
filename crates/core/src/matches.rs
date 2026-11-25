//! Historial de partidas: convierte lo que entrega el cliente (LCU) a un formato chico que se guarda
//! en este PC, y mezcla lo nuevo con lo guardado. Igual que src/matches.js.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Partidas que se guardan por modo (LoL y TFT por separado).
pub const KEEP: usize = 20;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Match {
    pub id: String,
    /// "lol" o "tft".
    pub mode: String,
    /// Fin de la partida, ISO 8601.
    pub at: String,
    #[serde(default)]
    pub dur: i64,
    #[serde(default)]
    pub queue_id: Option<i64>,
    #[serde(default)]
    pub win: bool,
    // LoL
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub champ: Option<i64>,
    /// Menos de 5 min: no cuenta como victoria ni derrota.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remake: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub k: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub d: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cs: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<i64>>,
    // LoL y TFT
    #[serde(default)]
    pub lvl: Option<i64>,
    // TFT
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<i64>,
    /// Minileyenda usada en la partida (contentId, un UUID).
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "companion_id")]
    pub companion: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Match {
    pub fn is_remake(&self) -> bool {
        self.remake == Some(true)
    }
}

fn num(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))
}

/// Id como texto (el cliente lo manda como texto o número); None si falta o es 0.
fn id_text(v: &Value) -> Option<String> {
    let s = match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    (!s.is_empty() && s != "0").then_some(s)
}

fn companion_id<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(id_text(&Value::deserialize(d)?))
}

pub(crate) fn iso(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .unwrap_or_default()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Partida de LoL del endpoint current-summoner/matches (solo trae al jugador actual).
pub fn from_lol_game(g: &Value) -> Option<Match> {
    let p = &g["participants"][0];
    let s = &p["stats"];
    let game_id = num(&g["gameId"])?;
    if !s.is_object() {
        return None;
    }
    let dur = num(&g["gameDuration"]).unwrap_or(0);
    let stat = |k: &str| num(&s[k]).unwrap_or(0);
    Some(Match {
        id: format!("lol-{game_id}"),
        mode: "lol".into(),
        at: iso(num(&g["gameCreation"]).unwrap_or(0) + dur * 1000),
        dur,
        queue_id: num(&g["queueId"]),
        champ: num(&p["championId"]),
        win: s["win"].as_bool().unwrap_or(false),
        remake: Some(dur > 0 && dur < 300),
        k: Some(stat("kills")),
        d: Some(stat("deaths")),
        a: Some(stat("assists")),
        cs: Some(stat("totalMinionsKilled") + stat("neutralMinionsKilled")),
        lvl: num(&s["champLevel"]),
        items: Some((0..7).map(|i| stat(&format!("item{i}"))).collect()),
        placement: None,
        companion: None,
        extra: Map::new(),
    })
}

/// Partida de TFT (formato de la API match-v1 de TFT, dentro de `json`).
pub fn from_tft_game(g: &Value, puuid: &str) -> Option<Match> {
    let j = &g["json"];
    let start = num(&j["game_datetime"])?;
    let me = j["participants"].as_array()?.iter().find(|p| p["puuid"].as_str() == Some(puuid))?;
    let length = j["game_length"].as_f64().unwrap_or(0.0);
    let id = g["metadata"]["match_id"].as_str().map(str::to_owned).unwrap_or_else(|| start.to_string());
    let placement = num(&me["placement"]);
    Some(Match {
        id: format!("tft-{id}"),
        mode: "tft".into(),
        at: iso(start + (length * 1000.0) as i64),
        dur: length.round() as i64,
        queue_id: num(&j["queue_id"]),
        win: placement.unwrap_or(9) <= 4,
        champ: None,
        remake: None,
        k: None,
        d: None,
        a: None,
        cs: None,
        items: None,
        lvl: num(&me["level"]),
        placement,
        companion: id_text(&me["companion"]["content_ID"]),
        extra: Map::new(),
    })
}

/// Junta partidas nuevas con las guardadas, sin repetir, y deja las `KEEP` más recientes de cada modo
/// (LoL y TFT por separado, para que jugar mucho TFT no borre el historial de LoL).
pub fn merge_matches(saved: &[Match], fresh: &[Match]) -> Vec<Match> {
    let mut by_id: Vec<Match> = Vec::new();
    for m in saved.iter().chain(fresh) {
        match by_id.iter_mut().find(|x| x.id == m.id) {
            Some(slot) => *slot = m.clone(),
            None => by_id.push(m.clone()),
        }
    }
    by_id.sort_by(|a, b| b.at.cmp(&a.at));
    let mut out: Vec<Match> = ["lol", "tft"]
        .iter()
        .flat_map(|mode| by_id.iter().filter(|m| m.mode == *mode).take(KEEP).cloned().collect::<Vec<_>>())
        .collect();
    out.sort_by(|a, b| b.at.cmp(&a.at));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn lol(id: i64, creation: i64) -> Match {
        from_lol_game(&json!({
            "gameId": id, "gameCreation": creation, "gameDuration": 1800, "queueId": 420,
            "participants": [{ "championId": 103, "stats": { "win": true, "kills": 5, "deaths": 2, "assists": 7, "totalMinionsKilled": 150, "neutralMinionsKilled": 10, "champLevel": 16, "item0": 3089 } }]
        }))
        .unwrap()
    }

    #[test]
    fn converts_a_lol_game_like_the_electron_version() {
        let m = lol(1, 1_700_000_000_000);
        assert_eq!(m.id, "lol-1");
        assert_eq!(m.at, "2023-11-14T22:43:20.000Z");
        assert_eq!((m.k, m.d, m.a, m.cs), (Some(5), Some(2), Some(7), Some(160)));
        assert_eq!(m.items.as_ref().unwrap()[0], 3089);
        assert!(!m.is_remake());
    }

    #[test]
    fn converts_a_tft_game_for_the_given_player() {
        let g = json!({ "metadata": { "match_id": "LA2_1" }, "json": { "game_datetime": 1_700_000_000_000i64, "game_length": 1500.4, "queue_id": 1100,
            "participants": [{ "puuid": "other", "placement": 1 }, { "puuid": "me", "placement": 3, "level": 8, "companion": { "content_ID": "91f2e228" } }] } });
        let m = from_tft_game(&g, "me").unwrap();
        assert_eq!((m.id.as_str(), m.placement, m.win, m.companion.as_deref()), ("tft-LA2_1", Some(3), true, Some("91f2e228")));
        assert!(from_tft_game(&g, "nobody").is_none());
    }

    #[test]
    fn merge_keeps_the_latest_per_mode_without_duplicates() {
        let saved: Vec<Match> = (0..KEEP as i64).map(|i| lol(i, i * 1000)).collect();
        let fresh = vec![lol(5, 5000), lol(100, 999_999)];
        let merged = merge_matches(&saved, &fresh);
        assert_eq!(merged.len(), KEEP);
        assert_eq!(merged[0].id, "lol-100", "la más nueva primero");
        assert!(!merged.iter().any(|m| m.id == "lol-0"), "se cae la más vieja");
    }
}
