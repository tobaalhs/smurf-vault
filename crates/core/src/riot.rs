//! API oficial de Riot (necesita una API key de developer.riotgames.com). Igual que src/riot.js.
//! Bloqueante: se llama desde hilos de fondo.
use crate::model::{Account, Rank};
use crate::snapshot::{mastery_from, num, RankUpdates, Snapshot, MASTERY_TOP};
use serde_json::Value;
use std::time::Duration;

pub struct Server {
    pub platform: &'static str,
    pub region: &'static str,
    pub match_region: &'static str,
}

/// Servidor que se ve en el cliente -> "platform" de la API + región de account-v1 y match-v5.
pub fn server(code: &str) -> Option<Server> {
    let (platform, region, match_region) = match code {
        "LAS" => ("la2", "americas", "americas"),
        "LAN" => ("la1", "americas", "americas"),
        "NA" => ("na1", "americas", "americas"),
        "BR" => ("br1", "americas", "americas"),
        "OCE" => ("oc1", "americas", "sea"),
        "EUW" => ("euw1", "europe", "europe"),
        "EUNE" => ("eun1", "europe", "europe"),
        "TR" => ("tr1", "europe", "europe"),
        "RU" => ("ru", "europe", "europe"),
        "KR" => ("kr", "asia", "asia"),
        "JP" => ("jp1", "asia", "asia"),
        _ => return None,
    };
    Some(Server { platform, region, match_region })
}

/// Lo que devuelve el cliente (/riotclient/region-locale) -> nuestro código de servidor.
pub fn server_from_client(region: &str) -> String {
    let r = region.to_uppercase();
    match r.as_str() {
        "LA2" => "LAS",
        "LA1" => "LAN",
        "NA" | "NA1" => "NA",
        "BR" | "BR1" => "BR",
        "OC1" => "OCE",
        "EUW" | "EUW1" => "EUW",
        "EUNE" | "EUN1" => "EUNE",
        "TR" => "TR",
        "RU" => "RU",
        "KR" => "KR",
        "JP" => "JP",
        _ => return r,
    }
    .to_string()
}

#[derive(Debug)]
pub enum RiotError {
    /// Riot respondió un error con este status (400 = no acepta ese PUUID con esta key).
    Status(u16, String),
    Other(String),
}

impl std::fmt::Display for RiotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RiotError::Status(_, m) | RiotError::Other(m) => f.write_str(m),
        }
    }
}

struct Api<'a> {
    key: &'a str,
    agent: ureq::Agent,
}

impl Api<'_> {
    /// GET a la API. Ok(None) si es 404. Reintenta si Riot pide esperar (429).
    fn call(&self, host: &str, path: &str, step: &str) -> Result<Option<Value>, RiotError> {
        let url = format!("https://{host}.api.riotgames.com{path}");
        for attempt in 0..=3 {
            match self.agent.get(&url).set("X-Riot-Token", self.key).call() {
                Ok(r) => return r.into_json().map(Some).map_err(|e| RiotError::Other(e.to_string())),
                Err(ureq::Error::Status(404, _)) => return Ok(None),
                Err(ureq::Error::Status(401 | 403, _)) => {
                    return Err(RiotError::Other("API key de Riot inválida o caducada (las de desarrollo duran 24 h)".into()))
                }
                Err(ureq::Error::Status(429, r)) => {
                    if attempt == 3 {
                        return Err(RiotError::Other("Demasiadas consultas a Riot, espera un poco".into()));
                    }
                    let secs = r.header("retry-after").and_then(|s| s.parse().ok()).unwrap_or(5u64);
                    std::thread::sleep(Duration::from_secs(secs));
                }
                Err(ureq::Error::Status(status, r)) => {
                    let detail = r.into_json::<Value>().ok().and_then(|j| j["status"]["message"].as_str().map(str::to_string));
                    let detail = detail.map(|d| format!(" ({d})")).unwrap_or_default();
                    return Err(RiotError::Status(status, format!("Riot respondió {status} al consultar {step}{detail}")));
                }
                Err(e) => return Err(RiotError::Other(format!("No se pudo conectar con Riot: {e}"))),
            }
        }
        unreachable!()
    }
}

fn rank(entries: &Option<Value>, queue: &str) -> Option<Rank> {
    let e = entries.as_ref()?.as_array()?.iter().find(|x| x["queueType"] == queue)?;
    let tier = e["tier"].as_str().unwrap_or("").to_string();
    let apex = matches!(tier.as_str(), "MASTER" | "GRANDMASTER" | "CHALLENGER");
    Some(Rank {
        division: if apex { String::new() } else { e["rank"].as_str().unwrap_or("").to_string() },
        tier,
        lp: num(&e["leaguePoints"]).unwrap_or(0),
        wins: num(&e["wins"]).unwrap_or(0),
        losses: num(&e["losses"]).unwrap_or(0),
        ..Default::default()
    })
}

struct Details {
    summoner: Option<Value>,
    entries: Option<Value>,
    /// None = la key no tiene acceso a TFT (se conserva lo guardado).
    tft_entries: Option<Option<Value>>,
    last_match: Option<Value>,
    last_tft: Option<Value>,
    mastery: Option<Value>,
}

fn by_riot_id(api: &Api, srv: &Server, acc: &Account) -> Result<Option<Value>, RiotError> {
    let enc = |s: &str| urlencode(s);
    api.call(srv.region, &format!("/riot/account/v1/accounts/by-riot-id/{}/{}", enc(&acc.game_name), enc(&acc.tag_line)), "la cuenta por Riot ID")
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Nick, nivel, rangos y última partida de un PUUID válido para esta key.
fn details(api: &Api, srv: &Server, puuid: &str) -> Result<Details, RiotError> {
    let (p, m) = (srv.platform, srv.match_region);
    let (summoner, entries, ids, tft_entries, tft_ids, top, score) = std::thread::scope(|s| {
        let summoner = s.spawn(|| api.call(p, &format!("/lol/summoner/v4/summoners/by-puuid/{puuid}"), "el invocador"));
        let entries = s.spawn(|| api.call(p, &format!("/lol/league/v4/entries/by-puuid/{puuid}"), "el rango"));
        let ids = s.spawn(|| api.call(m, &format!("/lol/match/v5/matches/by-puuid/{puuid}/ids?start=0&count=1"), "el historial"));
        let tft_entries = s.spawn(|| api.call(p, &format!("/tft/league/v1/by-puuid/{puuid}"), "el rango de TFT"));
        let tft_ids = s.spawn(|| api.call(m, &format!("/tft/match/v1/matches/by-puuid/{puuid}/ids?start=0&count=1"), "el historial de TFT"));
        let top = s.spawn(|| {
            api.call(p, &format!("/lol/champion-mastery/v4/champion-masteries/by-puuid/{puuid}/top?count={MASTERY_TOP}"), "las maestrías")
        });
        let score = s.spawn(|| api.call(p, &format!("/lol/champion-mastery/v4/scores/by-puuid/{puuid}"), "el puntaje de maestría"));
        let j = |h: std::thread::ScopedJoinHandle<'_, Result<Option<Value>, RiotError>>| {
            h.join().unwrap_or_else(|_| Err(RiotError::Other("error interno".into())))
        };
        (j(summoner), j(entries), j(ids), j(tft_entries), j(tft_ids), j(top), j(score))
    });
    let (summoner, entries, ids) = (summoner?, entries?, ids?);
    // Lo de TFT y maestrías es opcional: una key sin acceso no rompe la actualización de LoL.
    let first = |v: &Option<Value>| v.as_ref().and_then(|v| v[0].as_str().map(str::to_string));
    let match_id = first(&ids);
    let tft_id = tft_ids.as_ref().ok().and_then(first);
    let last_match = match_id.and_then(|id| api.call(m, &format!("/lol/match/v5/matches/{id}"), "la última partida").ok().flatten());
    let last_tft = tft_id.and_then(|id| api.call(m, &format!("/tft/match/v1/matches/{id}"), "la última partida de TFT").ok().flatten());
    let mastery = match (top, score) {
        (Ok(Some(t)), score) => mastery_from(&t, &score.ok().flatten().unwrap_or(Value::Null)),
        _ => None,
    };
    Ok(Details { summoner, entries, tft_entries: tft_entries.ok(), last_match, last_tft, mastery })
}

/// Actualiza una cuenta con su PUUID o, si no lo tiene, con su Riot ID (Nombre#TAG).
pub fn lookup(api_key: &str, acc: &Account) -> Result<Snapshot, String> {
    let srv = server(&acc.server).ok_or_else(|| {
        format!("Servidor desconocido: {}", if acc.server.is_empty() { "(vacío)" } else { &acc.server })
    })?;
    let has_riot_id = !acc.game_name.is_empty() && !acc.tag_line.is_empty();
    let known = acc.api_puuid.clone().or_else(|| acc.puuid.clone());
    if known.is_none() && !has_riot_id {
        return Err("Necesita Riot ID (Nombre#TAG) o haberla detectado con el cliente".into());
    }
    let api = Api { key: api_key, agent: ureq::AgentBuilder::new().timeout(Duration::from_secs(15)).build() };

    let attempt = || -> Result<(Value, Details), RiotError> {
        let found = match &known {
            Some(p) => api.call(srv.region, &format!("/riot/account/v1/accounts/by-puuid/{p}"), "la cuenta")?,
            None => by_riot_id(&api, &srv, acc)?,
        };
        let found = found.ok_or_else(|| RiotError::Other("Riot no encontró la cuenta".into()))?;
        let info = details(&api, &srv, found["puuid"].as_str().unwrap_or(""))?;
        Ok((found, info))
    };
    let (found, info) = match attempt() {
        Ok(r) => r,
        // 400 con un PUUID = Riot no lo acepta con esta key: si hay Riot ID, se busca por nombre.
        Err(RiotError::Status(400, msg)) if known.is_some() => {
            if !has_riot_id {
                return Err(format!("{msg}. Agrégale su Riot ID (Nombre#TAG) en Editar para buscarla por nombre"));
            }
            let found = by_riot_id(&api, &srv, acc)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("Riot no encontró {}", acc.riot_id()))?;
            let info = details(&api, &srv, found["puuid"].as_str().unwrap_or("")).map_err(|e| e.to_string())?;
            (found, info)
        }
        Err(e) => return Err(e.to_string()),
    };

    let api_puuid = found["puuid"].as_str().unwrap_or("").to_string();
    let end_ms = [
        info.last_match.as_ref().and_then(|m| num(&m["info"]["gameEndTimestamp"]).or_else(|| num(&m["info"]["gameCreation"]))),
        info.last_tft.as_ref().and_then(|m| num(&m["info"]["game_datetime"])),
    ]
    .into_iter()
    .flatten()
    .max()
    .unwrap_or(0);
    let tft_rank = |q: &str| info.tft_entries.as_ref().map(|e| rank(e, q));
    let summoner = info.summoner.unwrap_or(Value::Null);
    Ok(Snapshot {
        // El PUUID del cliente se conserva para reconocer la cuenta al detectarla.
        puuid: acc.puuid.clone().unwrap_or_else(|| api_puuid.clone()),
        api_puuid: Some(api_puuid),
        game_name: found["gameName"].as_str().unwrap_or("").to_string(),
        tag_line: found["tagLine"].as_str().unwrap_or("").to_string(),
        level: num(&summoner["summonerLevel"]).or(acc.level),
        icon_id: num(&summoner["profileIconId"]).or(acc.icon_id),
        last_played_at: (end_ms > 0).then(|| crate::matches::iso(end_ms)),
        mastery: info.mastery,
        ranks: RankUpdates {
            solo: Some(rank(&info.entries, "RANKED_SOLO_5x5")),
            flex: Some(rank(&info.entries, "RANKED_FLEX_SR")),
            tft: tft_rank("RANKED_TFT"),
            double_up: tft_rank("RANKED_TFT_DOUBLE_UP"),
        },
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn servers_and_ranks() {
        assert_eq!(server_from_client("la2"), "LAS");
        assert_eq!(server_from_client("EUW1"), "EUW");
        assert_eq!(server("OCE").unwrap().match_region, "sea");
        let entries = Some(json!([{ "queueType": "RANKED_SOLO_5x5", "tier": "MASTER", "rank": "I", "leaguePoints": 12, "wins": 1, "losses": 2 }]));
        let r = rank(&entries, "RANKED_SOLO_5x5").unwrap();
        assert_eq!((r.division.as_str(), r.lp), ("", 12));
        assert!(rank(&entries, "RANKED_FLEX_SR").is_none());
        assert_eq!(urlencode("Señor Z#1"), "Se%C3%B1or%20Z%231");
    }
}
