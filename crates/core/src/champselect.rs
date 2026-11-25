//! Estado de la cola y de la selección de campeones, y las órdenes que se pueden dar desde el
//! celular. Todo pasa por la API local del cliente de LoL. Igual que src/champselect.js (el celular
//! recibe exactamente el mismo JSON). Bloqueante.
use crate::lcu::lcu;
use crate::snapshot::num;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

const CDRAGON: &str = "https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/default";

/// Ruta de un recurso del cliente (/lol-game-data/assets/...) -> URL pública en CommunityDragon.
pub fn asset_url(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let lower = path.to_lowercase();
    let rest = lower.strip_prefix("/lol-game-data/assets/").unwrap_or(&lower);
    Some(format!("{CDRAGON}/{rest}"))
}

fn position(p: &Value) -> &'static str {
    match p.as_str().unwrap_or("") {
        "top" => "Top",
        "jungle" => "Jungla",
        "middle" => "Mid",
        "bottom" => "ADC",
        "utility" => "Support",
        _ => "",
    }
}

fn actions(session: &Value) -> impl Iterator<Item = &Value> {
    session["actions"].as_array().into_iter().flatten().filter_map(|g| g.as_array()).flatten()
}

/// Acción de ban o pick que te toca ahora.
pub fn my_active_action(session: &Value) -> Option<&Value> {
    let me = &session["localPlayerCellId"];
    actions(session).find(|a| a["actorCellId"] == *me && a["isInProgress"] == true && a["completed"] != true)
}

/// true si ya elegiste (pick confirmado).
pub fn my_pick_done(session: &Value) -> bool {
    let me = &session["localPlayerCellId"];
    actions(session).any(|a| a["actorCellId"] == *me && a["type"] == "pick" && a["completed"] == true)
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[derive(Default)]
pub struct ChampSelect {
    /// Estilos de runas del cliente (nombre e ícono), se piden una vez.
    styles: Option<Value>,
    /// (fase, tiempo restante, cuándo se vio): el cliente solo actualiza el tiempo cuando pasa algo.
    timer_ref: Option<(String, i64, i64)>,
    all_champions: Option<Vec<i64>>,
}

fn get(path: &str) -> Option<Value> {
    lcu().get(path)
}

fn send(method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    let (status, json) = lcu().request(method, path, false, body.as_ref()).ok_or("El cliente de LoL no está abierto")?;
    if status >= 400 {
        return Err(json["message"].as_str().map(str::to_string).unwrap_or_else(|| format!("El cliente respondió {status}")));
    }
    Ok(json)
}

impl ChampSelect {
    /// Hora (reloj del PC) en que termina la fase; la cuenta regresiva corre sola entre eventos.
    fn phase_end(&mut self, timer: &Value) -> i64 {
        let left = num(&timer["adjustedTimeLeftInPhase"]).unwrap_or(0).max(0);
        let phase = timer["phase"].as_str().unwrap_or("").to_string();
        match &self.timer_ref {
            Some((p, l, _)) if *p == phase && *l == left => {}
            _ => self.timer_ref = Some((phase, left, now_ms())),
        }
        let (_, left, seen) = self.timer_ref.as_ref().unwrap();
        seen + left
    }

    /// Foto del estado para el celular.
    pub fn state(&mut self) -> Value {
        let Some(phase) = get("/lol-gameflow/v1/gameflow-phase").and_then(|p| p.as_str().map(str::to_string)) else {
            return json!({ "phase": "Offline" });
        };
        let mut out = json!({ "phase": phase });
        if phase == "Matchmaking" || phase == "ReadyCheck" {
            out["search"] = get("/lol-matchmaking/v1/search")
                .map(|s| json!({ "timeInQueue": s["timeInQueue"].as_f64().unwrap_or(0.), "estimated": s["estimatedQueueTime"].as_f64().unwrap_or(0.) }))
                .unwrap_or(Value::Null);
        }
        if phase == "ReadyCheck" {
            out["readyCheck"] = get("/lol-matchmaking/v1/ready-check")
                .map(|rc| json!({ "state": rc["state"], "response": rc["playerResponse"], "timer": rc["timer"].as_f64().unwrap_or(0.) }))
                .unwrap_or(Value::Null);
        }
        if phase == "ChampSelect" {
            out["champSelect"] = self.champ_select_state().unwrap_or(Value::Null);
        }
        out
    }

    fn champ_select_state(&mut self) -> Option<Value> {
        let session = get("/lol-champ-select/v1/session")?;
        let action = my_active_action(&session).cloned();
        let picked = my_pick_done(&session);
        let me = session["myTeam"].as_array().into_iter().flatten().find(|p| p["cellId"] == session["localPlayerCellId"]).cloned().unwrap_or(Value::Null);
        let available = action.as_ref().map(|a| self.options(a["type"].as_str().unwrap_or(""), &session)).unwrap_or_default();
        let has_champ = num(&me["championId"]).unwrap_or(0) != 0;
        let (skins, pages, current) = std::thread::scope(|s| {
            let skins = s.spawn(move || if has_champ { get("/lol-champ-select/v1/skin-carousel-skins") } else { None });
            let pages = s.spawn(|| get("/lol-perks/v1/pages"));
            let current = s.spawn(|| get("/lol-perks/v1/currentpage"));
            (skins.join().ok().flatten(), pages.join().ok().flatten(), current.join().ok().flatten())
        });
        if self.styles.is_none() {
            self.styles = get("/lol-perks/v1/styles");
        }
        let timer = session["timer"].clone();
        let ends_at = self.phase_end(&timer);
        let style_by_id: HashMap<i64, &Value> =
            self.styles.as_ref().and_then(|s| s.as_array()).into_iter().flatten().filter_map(|s| Some((num(&s["id"])?, s))).collect();
        let nz = |v: &Value| num(v).unwrap_or(0);
        let team = |key: &str, mine: bool| -> Vec<Value> {
            session[key]
                .as_array()
                .into_iter()
                .flatten()
                .map(|p| {
                    if mine {
                        json!({
                            "cellId": p["cellId"],
                            "me": p["cellId"] == session["localPlayerCellId"],
                            "championId": nz(&p["championId"]),
                            "hoverId": nz(&p["championPickIntent"]),
                            "position": position(&p["assignedPosition"]),
                        })
                    } else {
                        json!({ "cellId": p["cellId"], "championId": nz(&p["championId"]) })
                    }
                })
                .collect()
        };
        let skins: Vec<Value> = skins
            .as_ref()
            .and_then(|s| s.as_array())
            .into_iter()
            .flatten()
            .filter(|s| s["unlocked"] == true && s["disabled"] != true)
            .map(|s| {
                let path = s["tilePath"].as_str().filter(|p| !p.is_empty()).or(s["splashPath"].as_str()).unwrap_or("");
                json!({ "id": s["id"], "name": s["name"], "image": asset_url(path) })
            })
            .collect();
        let pages: Vec<Value> = pages
            .as_ref()
            .and_then(|p| p.as_array())
            .into_iter()
            .flatten()
            .filter(|p| p["isValid"] != false)
            .map(|p| {
                let style = num(&p["primaryStyleId"]).and_then(|id| style_by_id.get(&id));
                json!({
                    "id": p["id"],
                    "name": p["name"],
                    "editable": p["isEditable"] == true,
                    "style": style.and_then(|s| s["name"].as_str()).unwrap_or(""),
                    "icon": style.and_then(|s| s["iconPath"].as_str()).and_then(asset_url),
                })
            })
            .collect();
        let my_champ = num(&me["championId"]).filter(|c| *c != 0).or_else(|| num(&me["championPickIntent"])).unwrap_or(0);
        Some(json!({
            "timer": { "phase": timer["phase"], "endsAt": ends_at, "totalMs": nz(&timer["totalTimeInPhase"]) },
            "action": action.as_ref().map(|a| json!({ "id": a["id"], "type": a["type"], "championId": nz(&a["championId"]) })),
            "picked": picked,
            "me": {
                "championId": my_champ,
                "position": position(&me["assignedPosition"]),
                "spell1Id": me["spell1Id"],
                "spell2Id": me["spell2Id"],
                "skinId": me["selectedSkinId"],
            },
            "myTeam": team("myTeam", true),
            "theirTeam": team("theirTeam", false),
            "bans": {
                "mine": session["bans"]["myTeamBans"].as_array().cloned().unwrap_or_default(),
                "theirs": session["bans"]["theirTeamBans"].as_array().cloned().unwrap_or_default(),
            },
            "available": available,
            "skins": skins,
            "runes": { "currentId": current.as_ref().map(|c| c["id"].clone()).unwrap_or(Value::Null), "pages": pages },
        }))
    }

    /// Campeones que se pueden banear o elegir. Si el cliente entrega la lista vacía (pasa con los
    /// baneos), se prueba la ruta alternativa y si no, todos menos los baneados, elegidos o
    /// deshabilitados.
    fn options(&mut self, kind: &str, session: &Value) -> Vec<i64> {
        let kind = if kind == "ban" { "bannable" } else { "pickable" };
        for base in ["/lol-champ-select/v1", "/lol-lobby-team-builder/champ-select/v1"] {
            if let Some(ids) = get(&format!("{base}/{kind}-champion-ids")).and_then(|v| v.as_array().cloned()) {
                let ids: Vec<i64> = ids.iter().filter_map(num).filter(|id| *id > 0).collect();
                if !ids.is_empty() {
                    return ids;
                }
            }
        }
        if self.all_champions.is_none() {
            // Solo campeones de la Grieta (la lista trae también los de LoL Classic, ids desde 60000).
            self.all_champions = Some(
                get("/lol-game-data/assets/v1/champion-summary.json")
                    .and_then(|v| v.as_array().cloned())
                    .unwrap_or_default()
                    .iter()
                    .filter(|c| !c["alias"].as_str().unwrap_or("").starts_with("Jade_"))
                    .filter_map(|c| num(&c["id"]))
                    .filter(|id| *id > 0 && *id < 10000)
                    .collect(),
            );
        }
        let disabled: HashSet<i64> =
            get("/lol-champ-select/v1/disabled-champion-ids").and_then(|v| v.as_array().cloned()).unwrap_or_default().iter().filter_map(num).collect();
        let mut taken: HashSet<i64> = HashSet::new();
        for key in ["myTeamBans", "theirTeamBans"] {
            taken.extend(session["bans"][key].as_array().into_iter().flatten().filter_map(num));
        }
        taken.extend(actions(session).filter(|a| a["completed"] == true).filter_map(|a| num(&a["championId"])));
        for key in ["myTeam", "theirTeam"] {
            taken.extend(session[key].as_array().into_iter().flatten().filter_map(|p| num(&p["championId"])));
        }
        self.all_champions.clone().unwrap_or_default().into_iter().filter(|id| !taken.contains(id) && !disabled.contains(id)).collect()
    }

    // ---------- órdenes ----------

    pub fn run(&mut self, cmd: &Value) -> Result<(), String> {
        let n = |k: &str| num(&cmd[k]).unwrap_or(0);
        match cmd["type"].as_str().unwrap_or("") {
            "accept" => send("POST", "/lol-matchmaking/v1/ready-check/accept", None).map(|_| ()),
            "decline" => send("POST", "/lol-matchmaking/v1/ready-check/decline", None).map(|_| ()),
            "hover" => {
                let action = current_action()?;
                send("PATCH", &format!("/lol-champ-select/v1/session/actions/{}", action["id"]), Some(json!({ "championId": n("championId") }))).map(|_| ())
            }
            "lock" => lock(n("championId")),
            "skin" => send("PATCH", "/lol-champ-select/v1/session/my-selection", Some(json!({ "selectedSkinId": n("skinId") }))).map(|_| ()),
            "spells" => send(
                "PATCH",
                "/lol-champ-select/v1/session/my-selection",
                Some(json!({ "spell1Id": n("spell1Id"), "spell2Id": n("spell2Id") })),
            )
            .map(|_| ()),
            "runePage" => send("PUT", "/lol-perks/v1/currentpage", Some(json!(n("pageId")))).map(|_| ()),
            "recommendedRunes" => recommended_runes(),
            _ => Err("Orden desconocida".into()),
        }
    }
}

fn current_action() -> Result<Value, String> {
    let session = get("/lol-champ-select/v1/session").ok_or("No es tu turno")?;
    my_active_action(&session).cloned().ok_or_else(|| "No es tu turno".into())
}

fn is_completed(action_id: &Value) -> bool {
    let Some(session) = get("/lol-champ-select/v1/session") else { return true };
    let done = actions(&session).find(|a| a["id"] == *action_id).is_none_or(|a| a["completed"] == true);
    done
}

/// Confirmar el ban o el pick: el campeón y `completed: true` en la misma orden, como el cliente; si
/// no quedó confirmado, se prueba con la ruta de confirmar.
fn lock(champion_id: i64) -> Result<(), String> {
    let action = current_action()?;
    let id = if champion_id != 0 { champion_id } else { num(&action["championId"]).unwrap_or(0) };
    if id == 0 {
        return Err("Elige un campeón primero".into());
    }
    let path = format!("/lol-champ-select/v1/session/actions/{}", action["id"]);
    send("PATCH", &path, Some(json!({ "championId": id, "completed": true })))?;
    if is_completed(&action["id"]) {
        return Ok(());
    }
    send("POST", &format!("{path}/complete"), None)?;
    if !is_completed(&action["id"]) {
        return Err("El cliente no confirmó la selección: intenta de nuevo".into());
    }
    Ok(())
}

/// Crea (y deja activa) la página de runas que recomienda Riot para tu campeón y posición, en una
/// página propia "Smurf Vault" que se reemplaza cada vez.
fn recommended_runes() -> Result<(), String> {
    let session = get("/lol-champ-select/v1/session").ok_or("Elige un campeón primero")?;
    let me = session["myTeam"].as_array().into_iter().flatten().find(|p| p["cellId"] == session["localPlayerCellId"]).cloned().unwrap_or(Value::Null);
    let champ = num(&me["championId"]).filter(|c| *c != 0).or_else(|| num(&me["championPickIntent"])).filter(|c| *c != 0).ok_or("Elige un campeón primero")?;
    let position = me["assignedPosition"].as_str().filter(|p| !p.is_empty()).unwrap_or("none");
    let recs = get(&format!("/lol-perks/v1/recommended-pages/champion/{champ}/position/{position}/map/11"));
    let rec = recs.as_ref().and_then(|r| r.get(0)).ok_or("Riot no tiene runas recomendadas para este campeón")?;
    let pages = get("/lol-perks/v1/pages").and_then(|p| p.as_array().cloned()).unwrap_or_default();
    if let Some(mine) = pages.iter().find(|p| p["isEditable"] == true && p["name"].as_str().is_some_and(|n| n.starts_with("Smurf Vault"))) {
        send("DELETE", &format!("/lol-perks/v1/pages/{}", mine["id"]), None)?;
    }
    let perks: Vec<Value> = rec["perks"].as_array().into_iter().flatten().map(|p| p["id"].clone()).collect();
    send(
        "POST",
        "/lol-perks/v1/pages",
        Some(json!({
            "name": "Smurf Vault · recomendadas",
            "primaryStyleId": rec["primaryPerkStyleId"],
            "subStyleId": rec["secondaryPerkStyleId"],
            "selectedPerkIds": perks,
            "current": true,
        })),
    )
    .map(|_| ())
    .map_err(|e| format!("No se pudo crear la página de runas (¿no tienes espacio para otra?): {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_action_and_pick_state() {
        let session = json!({
            "localPlayerCellId": 2,
            "actions": [[{ "id": 1, "actorCellId": 2, "type": "ban", "isInProgress": false, "completed": true }],
                        [{ "id": 7, "actorCellId": 2, "type": "pick", "isInProgress": true, "completed": false }]]
        });
        assert_eq!(my_active_action(&session).unwrap()["id"], 7);
        assert!(!my_pick_done(&session));
        assert_eq!(asset_url("/lol-game-data/assets/v1/X.png").unwrap(), format!("{CDRAGON}/v1/x.png"));
    }
}
