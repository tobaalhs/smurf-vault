//! Contenido de la bóveda: las cuentas y los ajustes. Es el mismo JSON que guarda la versión Electron.
//! Cada estructura conserva en `extra` los campos que esta versión no conoce, para no perder nada.
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

/// Acepta `null` como el valor por defecto (la bóveda puede tener campos en null).
pub(crate) fn null_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Rank {
    #[serde(default, deserialize_with = "null_default")]
    pub tier: String,
    #[serde(default, deserialize_with = "null_default")]
    pub division: String,
    #[serde(default, deserialize_with = "null_default")]
    pub lp: i64,
    #[serde(default, deserialize_with = "null_default")]
    pub wins: i64,
    #[serde(default, deserialize_with = "null_default")]
    pub losses: i64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Rango por cola. `None` = sin rango.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Ranks {
    #[serde(default)]
    pub solo: Option<Rank>,
    #[serde(default)]
    pub flex: Option<Rank>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tft: Option<Rank>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub double_up: Option<Rank>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Ranks {
    pub fn get(&self, queue: &str) -> Option<&Rank> {
        match queue {
            "solo" => self.solo.as_ref(),
            "flex" => self.flex.as_ref(),
            "tft" => self.tft.as_ref(),
            "doubleUp" => self.double_up.as_ref(),
            _ => None,
        }
    }

    pub fn slot(&mut self, queue: &str) -> Option<&mut Option<Rank>> {
        match queue {
            "solo" => Some(&mut self.solo),
            "flex" => Some(&mut self.flex),
            "tft" => Some(&mut self.tft),
            "doubleUp" => Some(&mut self.double_up),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub label: String,
    #[serde(default, deserialize_with = "null_default")]
    pub username: String,
    #[serde(default, deserialize_with = "null_default")]
    pub password: String,
    #[serde(default, deserialize_with = "null_default")]
    pub server: String,
    #[serde(default, deserialize_with = "null_default")]
    pub game_name: String,
    #[serde(default, deserialize_with = "null_default")]
    pub tag_line: String,
    #[serde(default, deserialize_with = "null_default")]
    pub notes: String,
    #[serde(default, deserialize_with = "null_default")]
    pub ranks: Ranks,
    /// PUUID de la cuenta vinculada en el cliente de LoL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub puuid: Option<String>,
    /// PUUID que entrega la API de Riot (con la API key del usuario), si es distinto.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_puuid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_synced_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_played_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_skin_id: Option<i64>,
    /// Marco del ícono: { type: 'level', theme } o { type: 'ranked' }.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crest: Option<Value>,
    /// Maestrías de campeones (resumen que arma el cliente).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mastery: Option<Value>,
    /// Baneo de LoL que reporta el cliente: { permanent } o { until }.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ban: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub favorite: Option<bool>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Account {
    /// Riot ID "Nombre#TAG", o vacío si la cuenta no lo tiene.
    pub fn riot_id(&self) -> String {
        if self.game_name.is_empty() {
            String::new()
        } else {
            format!("{}#{}", self.game_name, self.tag_line)
        }
    }

    /// Nombre para mostrar: etiqueta, Riot ID o usuario.
    pub fn display_name(&self) -> String {
        [self.label.clone(), self.riot_id(), self.username.clone()]
            .into_iter()
            .find(|s| !s.is_empty())
            .unwrap_or_default()
    }

    pub fn is_favorite(&self) -> bool {
        self.favorite == Some(true)
    }

    /// ¿Esta cuenta es la del PUUID dado (del cliente o de la API)?
    pub fn owns_puuid(&self, puuid: &str) -> bool {
        self.puuid.as_deref() == Some(puuid) || self.api_puuid.as_deref() == Some(puuid)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default, deserialize_with = "null_default")]
    pub riot_api_key: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { riot_api_key: String::new(), extra: Map::new() }
    }
}

/// Todo lo que va cifrado en vault.dat (y en Drive).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VaultData {
    #[serde(default, deserialize_with = "null_default")]
    pub accounts: Vec<Account>,
    #[serde(default, deserialize_with = "null_default")]
    pub settings: Settings,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl VaultData {
    pub fn account(&self, id: &str) -> Option<&Account> {
        self.accounts.iter().find(|a| a.id == id)
    }
    pub fn account_mut(&mut self, id: &str) -> Option<&mut Account> {
        self.accounts.iter_mut().find(|a| a.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn electron_accounts_roundtrip_without_losing_fields() {
        let raw = serde_json::json!({
            "accounts": [{
                "id": "a1", "label": "", "username": "u", "password": "p", "server": "LAS",
                "gameName": null, "tagLine": "", "notes": "", "ranks": { "solo": { "tier": "GOLD", "division": "II", "lp": 40, "wins": 3, "losses": 2 }, "flex": null },
                "puuid": "x", "favorite": true, "somethingNew": [1, 2]
            }],
            "settings": { "riotApiKey": "RGAPI", "other": true }
        });
        let data: VaultData = serde_json::from_value(raw).unwrap();
        let acc = &data.accounts[0];
        assert_eq!(acc.game_name, "", "null se lee como vacío");
        assert_eq!(acc.ranks.solo.as_ref().unwrap().lp, 40);
        assert!(acc.is_favorite());
        let back = serde_json::to_value(&data).unwrap();
        assert_eq!(back["accounts"][0]["somethingNew"], serde_json::json!([1, 2]));
        assert_eq!(back["settings"]["other"], true);
        assert_eq!(back["accounts"][0]["ranks"]["flex"], Value::Null);
    }

    #[test]
    fn display_name_prefers_label_then_riot_id_then_username() {
        let mut a = Account { username: "user".into(), ..Default::default() };
        assert_eq!(a.display_name(), "user");
        a.game_name = "Faker".into();
        a.tag_line = "KR1".into();
        assert_eq!(a.display_name(), "Faker#KR1");
        a.label = "Main".into();
        assert_eq!(a.display_name(), "Main");
    }
}
