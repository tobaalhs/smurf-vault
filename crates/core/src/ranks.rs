//! Rangos de LoL/TFT: orden, nombres en español, LP continuo para el gráfico y reglas de dúo de
//! clasificatoria. Igual que renderer.js, carousel.js y duo.js de la versión Electron.
use crate::local::LpPoint;
use crate::model::{Account, Rank};

pub const TIERS: [&str; 10] =
    ["IRON", "BRONZE", "SILVER", "GOLD", "PLATINUM", "EMERALD", "DIAMOND", "MASTER", "GRANDMASTER", "CHALLENGER"];
const APEX: [&str; 3] = ["MASTER", "GRANDMASTER", "CHALLENGER"];
/// Ligas que se pueden elegir en el filtro "dúo con amigo" (de Hierro a Maestro).
pub const DUO_TIERS: [&str; 8] = ["IRON", "BRONZE", "SILVER", "GOLD", "PLATINUM", "EMERALD", "DIAMOND", "MASTER"];

pub fn tier_es(tier: &str) -> &'static str {
    match tier {
        "IRON" => "Hierro",
        "BRONZE" => "Bronce",
        "SILVER" => "Plata",
        "GOLD" => "Oro",
        "PLATINUM" => "Platino",
        "EMERALD" => "Esmeralda",
        "DIAMOND" => "Diamante",
        "MASTER" => "Maestro",
        "GRANDMASTER" => "Gran Maestro",
        "CHALLENGER" => "Retador",
        _ => "",
    }
}

fn tier_index(tier: &str) -> Option<usize> {
    TIERS.iter().position(|t| *t == tier)
}

/// IV=0 … I=3 (para ordenar).
fn div_order(division: &str) -> i64 {
    match division {
        "III" => 1,
        "II" => 2,
        "I" => 3,
        _ => 0,
    }
}

/// "Oro II" (o "Maestro", sin división).
pub fn rank_label(tier: &str, division: &str) -> String {
    let name = tier_es(tier);
    let name = if name.is_empty() { tier } else { name };
    if division.is_empty() {
        name.to_string()
    } else {
        format!("{name} {division}")
    }
}

/// Puntaje para ordenar: liga, división y LP. Sin rango = -1.
pub fn rank_score(r: Option<&Rank>) -> i64 {
    match r.and_then(|r| tier_index(&r.tier).map(|i| (i, r))) {
        Some((i, r)) => i as i64 * 10_000 + div_order(&r.division) * 1000 + r.lp,
        None => -1,
    }
}

/// Mejor rango entre LoL Solo/Dúo y TFT (el orden "Mayor rango").
pub fn best_rank(a: &Account) -> i64 {
    rank_score(a.ranks.solo.as_ref()).max(rank_score(a.ranks.tft.as_ref()))
}

/// El mejor rango entre Solo/Dúo, Flex y TFT: define el color y el marco de la cuenta.
pub fn top_rank(a: &Account) -> Option<&Rank> {
    [a.ranks.solo.as_ref(), a.ranks.flex.as_ref(), a.ranks.tft.as_ref()]
        .into_iter()
        .flatten()
        .filter(|r| tier_index(&r.tier).is_some())
        .max_by_key(|r| rank_score(Some(r)))
}

/// Rango a un número continuo: 100 por división, 400 por liga; de Maestro para arriba, solo LP.
pub fn lp_value(p: &LpPoint) -> i64 {
    if APEX.contains(&p.tier.as_str()) {
        return 2800 + p.lp;
    }
    tier_index(&p.tier).unwrap_or(0) as i64 * 400 + div_order(&p.division) * 100 + p.lp
}

/// Último cambio de LP en una cola: (diferencia, desde, hasta).
pub fn lp_delta(points: &[LpPoint]) -> Option<(i64, &LpPoint, &LpPoint)> {
    let [.., from, to] = points else { return None };
    Some((lp_value(to) - lp_value(from), from, to))
}

fn div_num(division: &str) -> Option<i64> {
    match division.trim().to_uppercase().as_str() {
        "I" => Some(1),
        "II" => Some(2),
        "III" => Some(3),
        "IV" => Some(4),
        _ => None,
    }
}

/// ¿La cuenta con este Solo/Dúo puede hacer dúo con un amigo de esa liga y división?
/// Reglas de clasificatoria Solo/Dúo ("Friend elo"), portadas de League Accounts (MIT).
/// Maestro+, Gran Maestro y Retador casi no pueden hacer dúo; por eso varias combinaciones dan false.
pub fn can_duo(rank: Option<&Rank>, friend_tier: &str, friend_division: &str) -> bool {
    let Some(rank) = rank.filter(|r| !r.tier.is_empty()) else { return false };
    let tier = rank.tier.to_lowercase();
    let friend = friend_tier.to_lowercase();
    let own = div_num(&rank.division);
    let theirs = div_num(friend_division);
    let is = |t: &str| tier == t;
    match friend.as_str() {
        "master" => is("master") || (is("diamond") && own == Some(1)),
        "diamond" => {
            if let (true, Some(o), Some(t)) = (is("diamond"), own, theirs) {
                if (o - t).abs() <= 2 {
                    return true;
                }
            }
            (is("emerald") && own == Some(1) && theirs == Some(4)) || (is("master") && theirs == Some(1))
        }
        "emerald" => is("emerald") || is("platinum") || (theirs == Some(1) && is("diamond") && own == Some(4)),
        "platinum" => ["platinum", "emerald", "gold"].contains(&tier.as_str()),
        "iron" => ["iron", "bronze", "silver"].contains(&tier.as_str()),
        "bronze" | "silver" | "gold" => {
            let i = TIERS.iter().position(|t| t.to_lowercase() == friend).unwrap_or(0);
            let mut allowed = vec![i.checked_sub(1), Some(i)];
            if i < TIERS.len() - 3 {
                allowed.push(Some(i + 1));
            }
            allowed.into_iter().flatten().any(|j| TIERS[j].to_lowercase() == tier)
        }
        _ => false, // Gran Maestro y Retador no hacen dúo
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(tier: &str, division: &str) -> Rank {
        Rank { tier: tier.into(), division: division.into(), ..Default::default() }
    }

    /// Los mismos casos que test/duo.test.js de la versión Electron.
    #[test]
    fn duo_rules_match_the_electron_version() {
        let duo = |t: &str, d: &str, ft: &str, fd: &str| can_duo(Some(&r(t, d)), ft, fd);
        assert!(!can_duo(None, "GOLD", "II"), "sin rango no puede");
        assert!(duo("GOLD", "I", "GOLD", "IV"));
        assert!(duo("SILVER", "II", "GOLD", "II"));
        assert!(duo("PLATINUM", "IV", "GOLD", "II"));
        assert!(!duo("EMERALD", "I", "GOLD", "II"), "muy arriba");
        assert!(!duo("BRONZE", "I", "GOLD", "II"), "muy abajo");
        assert!(duo("EMERALD", "III", "PLATINUM", "II"));
        assert!(!duo("DIAMOND", "IV", "PLATINUM", "I"));
        assert!(duo("DIAMOND", "IV", "EMERALD", "I"));
        assert!(!duo("DIAMOND", "IV", "EMERALD", "II"));
        assert!(duo("DIAMOND", "IV", "DIAMOND", "II"));
        assert!(!duo("DIAMOND", "I", "DIAMOND", "IV"), "más de 2 divisiones");
        assert!(duo("MASTER", "", "DIAMOND", "I"));
        assert!(duo("DIAMOND", "I", "MASTER", ""));
        assert!(!duo("DIAMOND", "II", "MASTER", ""));
        assert!(duo("MASTER", "", "MASTER", ""));
        assert!(duo("SILVER", "IV", "IRON", "II"));
        assert!(!duo("GOLD", "IV", "IRON", "II"));
    }

    #[test]
    fn scores_and_lp_values() {
        assert!(rank_score(Some(&r("GOLD", "I"))) > rank_score(Some(&r("GOLD", "IV"))));
        assert!(rank_score(Some(&r("PLATINUM", "IV"))) > rank_score(Some(&r("GOLD", "I"))));
        assert_eq!(rank_score(None), -1);
        let p = |t: &str, d: &str, lp| LpPoint { at: String::new(), tier: t.into(), division: d.into(), lp };
        assert_eq!(lp_value(&p("GOLD", "IV", 50)) + 50, lp_value(&p("GOLD", "III", 0)));
        assert_eq!(lp_value(&p("MASTER", "", 120)), 2920);
        assert_eq!(rank_label("GOLD", "II"), "Oro II");
        assert_eq!(rank_label("MASTER", ""), "Maestro");
    }
}
