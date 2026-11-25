//! Lo que se sabe de una cuenta de Riot en un momento: viene del cliente de LoL (lcu.rs) o de la API
//! de Riot (riot.rs) y se aplica a la cuenta guardada con `Store::apply_snapshot`. Incluye los
//! formatos compartidos de src/mastery.js y src/ban.js.
use crate::matches::Match;
use crate::model::Rank;
use base64::Engine;
use serde_json::{json, Value};

/// Campeones con más puntos que se guardan.
pub const MASTERY_TOP: usize = 10;

/// Rango de una cola. `None` = no se pudo consultar (se conserva lo guardado);
/// `Some(None)` = sin rango.
pub type RankUpdate = Option<Option<Rank>>;

#[derive(Clone, Debug, Default)]
pub struct RankUpdates {
    pub solo: RankUpdate,
    pub flex: RankUpdate,
    pub tft: RankUpdate,
    pub double_up: RankUpdate,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    /// Usuario de login (solo para reconocer la cuenta; no se guarda).
    pub username: String,
    pub puuid: String,
    /// PUUID que acepta la API de Riot con la key del usuario.
    pub api_puuid: Option<String>,
    pub game_name: String,
    pub tag_line: String,
    pub level: Option<i64>,
    pub icon_id: Option<i64>,
    /// Código de servidor nuestro (LAS, NA...), o vacío si no se sabe.
    pub server: String,
    pub last_played_at: Option<String>,
    pub background_skin_id: Option<i64>,
    pub crest: Option<Value>,
    pub mastery: Option<Value>,
    /// `None` = no se sabe (API de Riot); `Some(None)` = sin baneo.
    pub ban: Option<Option<Value>>,
    pub ranks: RankUpdates,
    /// `None` si no se pudo leer el historial.
    pub matches: Option<Vec<Match>>,
}

impl Snapshot {
    pub fn riot_id(&self) -> String {
        format!("{}#{}", self.game_name, self.tag_line)
    }
}

pub(crate) fn num(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)).or_else(|| v.as_str()?.parse().ok())
}

/// { score, top: [{ champ, level, points }] } a partir de la lista del cliente o de la API.
pub fn mastery_from(list: &Value, score: &Value) -> Option<Value> {
    let list = list.as_array()?;
    let mut sorted: Vec<&Value> = list.iter().collect();
    sorted.sort_by_key(|m| std::cmp::Reverse(num(&m["championPoints"]).unwrap_or(0)));
    let top: Vec<Value> = sorted
        .iter()
        .take(MASTERY_TOP)
        .map(|m| {
            json!({
                "champ": m["championId"],
                "level": num(&m["championLevel"]).unwrap_or(0),
                "points": num(&m["championPoints"]).unwrap_or(0),
            })
        })
        .collect();
    let total = num(score).unwrap_or_else(|| list.iter().map(|m| num(&m["championLevel"]).unwrap_or(0)).sum());
    Some(json!({ "score": total, "top": top }))
}

const GAME_BANS: [&str; 4] = ["PERMANENT_BAN", "TIME_BAN", "TIME_BAN_ACCOUNT", "LEGACY_BAN"];

fn jwt_payload(token: &str) -> Option<Value> {
    let part = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(part.trim_end_matches('=')).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Baneo de juego desde /lol-login/v1/session: { permanent: true }, { permanent: false, until } o
/// None. Las restricciones de chat, ranked o reputación dejan jugar y no cuentan.
pub fn ban_from_session(session: &Value, now_ms: i64) -> Option<Value> {
    let restrictions = session["ban"]["restrictions"]
        .as_array()
        .cloned()
        .or_else(|| jwt_payload(session["idToken"].as_str()?)?["ban"]["restrictions"].as_array().cloned())?;
    let mut latest: Option<i64> = None;
    for r in &restrictions {
        let scope = r["scope"].as_str().unwrap_or("").to_lowercase();
        if !scope.is_empty() && scope != "lol" && scope != "league_of_legends" {
            continue;
        }
        let kind = r["type"].as_str().unwrap_or("");
        if !GAME_BANS.contains(&kind) {
            continue;
        }
        if kind == "PERMANENT_BAN" {
            return Some(json!({ "permanent": true }));
        }
        let Some(until) = num(&r["dat"]["expirationMillis"]).filter(|u| *u > now_ms) else { continue };
        // El que termina más tarde manda.
        latest = Some(latest.map_or(until, |l| l.max(until)));
    }
    latest.map(|ms| json!({ "permanent": false, "until": crate::matches::iso(ms) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mastery_keeps_the_top_by_points() {
        let list = json!([
            { "championId": 1, "championLevel": 5, "championPoints": 100 },
            { "championId": 2, "championLevel": 7, "championPoints": 900 }
        ]);
        let m = mastery_from(&list, &Value::Null).unwrap();
        assert_eq!(m["score"], 12);
        assert_eq!(m["top"][0]["champ"], 2);
        assert!(mastery_from(&Value::Null, &Value::Null).is_none());
    }

    #[test]
    fn ban_reads_time_bans_and_ignores_chat_restrictions() {
        let session = json!({ "ban": { "restrictions": [
            { "type": "TEXT_CHAT_RESTRICTION", "scope": "lol" },
            { "type": "TIME_BAN", "scope": "lol", "dat": { "expirationMillis": 2_000 } }
        ]}});
        assert_eq!(ban_from_session(&session, 1_000).unwrap()["permanent"], false);
        assert!(ban_from_session(&session, 3_000).is_none(), "ya venció");
        let perm = json!({ "ban": { "restrictions": [{ "type": "PERMANENT_BAN" }] } });
        assert_eq!(ban_from_session(&perm, 0).unwrap()["permanent"], true);
        // Dentro del idToken (JWT).
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"ban":{"restrictions":[{"type":"PERMANENT_BAN"}]}}"#);
        let jwt = json!({ "idToken": format!("x.{payload}.y") });
        assert!(ban_from_session(&jwt, 0).is_some());
    }
}
