//! La bóveda abierta y lo que se hace con ella: crear, desbloquear, guardar, cuentas, ajustes,
//! contraseña maestra. Reemplaza esa parte de main.js.
//!
//! Lo lento (derivar la clave con scrypt) está en `Opened` y en `vault::*`, para que la interfaz lo
//! haga fuera de su hilo; la `Store` solo hace lo rápido (leer y escribir archivos).
use crate::config::Config;
use crate::local::{History, LpLog};
use crate::matches::Match;
use crate::model::{Account, Ranks, VaultData};
use crate::paths::{write_atomic, Paths};
use crate::snapshot::Snapshot;
use crate::vault::{self, Envelope, Key};
use serde_json::{Map, Value};

pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn norm(s: &str) -> String {
    s.trim().to_lowercase()
}

/// Una bóveda ya descifrada (o recién creada), lista para abrirse en la `Store`.
pub struct Opened {
    pub key: Key,
    pub salt: Vec<u8>,
    pub data: VaultData,
}

impl Opened {
    /// Descifra un sobre con la contraseña maestra (lento: scrypt).
    pub fn from_envelope(env: &Envelope, password: &str) -> Result<Self, String> {
        let (key, salt, raw) = vault::decrypt_envelope(env, password)?;
        let data = serde_json::from_value(raw).map_err(|e| format!("La bóveda está dañada: {e}"))?;
        Ok(Self { key, salt, data })
    }

    /// Bóveda nueva y vacía para esa contraseña (lento: scrypt).
    pub fn new_vault(password: &str) -> Result<Self, String> {
        if password.chars().count() < 8 {
            return Err("Usa al menos 8 caracteres".into());
        }
        let (key, salt) = vault::create_key(password)?;
        Ok(Self { key, salt, data: VaultData::default() })
    }
}

/// La bóveda abierta, con el historial y el LP locales (cifrados con la misma clave).
pub struct Session {
    pub key: Key,
    pub salt: Vec<u8>,
    pub data: VaultData,
    pub history: History,
    pub lp: LpLog,
}

/// Campos de una cuenta que se editan a mano. `None` = no se toca.
#[derive(Default, Clone, Debug)]
pub struct AccountInput {
    pub id: Option<String>,
    pub label: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub server: Option<String>,
    pub game_name: Option<String>,
    pub tag_line: Option<String>,
    pub notes: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ImportRow {
    pub username: String,
    pub password: String,
    pub server: String,
}

pub struct Store {
    pub paths: Paths,
    pub config: Config,
    session: Option<Session>,
    /// Última bóveda guardada que falta subir a Google Drive.
    pending_upload: Option<Envelope>,
}

impl Store {
    pub fn new(paths: Paths) -> Self {
        let config = Config::load(&paths);
        Self { paths, config, session: None, pending_upload: None }
    }

    pub fn save_config(&self) -> Result<(), String> {
        self.config.save(&self.paths).map_err(|e| e.to_string())
    }

    // ---------- bóveda ----------

    pub fn has_local(&self) -> bool {
        self.paths.vault().exists()
    }

    pub fn read_local(&self) -> Option<Envelope> {
        serde_json::from_str(&std::fs::read_to_string(self.paths.vault()).ok()?).ok()
    }

    pub fn is_unlocked(&self) -> bool {
        self.session.is_some()
    }

    pub fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    pub fn session_mut(&mut self) -> Option<&mut Session> {
        self.session.as_mut()
    }

    pub fn data(&self) -> Option<&VaultData> {
        self.session.as_ref().map(|s| &s.data)
    }

    fn need(&mut self) -> Result<&mut Session, String> {
        self.session.as_mut().ok_or_else(|| "La bóveda está bloqueada".to_string())
    }

    fn open(&mut self, o: Opened) {
        let history = History::load(self.paths.history(), &o.key);
        let lp = LpLog::load(self.paths.lp(), &o.key);
        self.session = Some(Session { key: o.key, salt: o.salt, data: o.data, history, lp });
        // Las sesiones del Riot Client que guardaban versiones anteriores ya no se usan.
        let _ = std::fs::remove_dir_all(self.paths.old_sessions());
    }

    /// Guarda una bóveda recién creada y la deja abierta. Devuelve el sobre (para subirlo a Drive).
    pub fn finish_create(&mut self, opened: Opened) -> Result<Envelope, String> {
        if self.has_local() {
            return Err("Ya existe una bóveda en este PC".into());
        }
        self.open(opened);
        self.persist()
    }

    /// Deja abierta una bóveda ya descifrada: carga historial y LP, anota el primer punto de LP de
    /// cada cuenta y pasa al archivo local el historial que versiones viejas guardaban en la bóveda.
    /// Devuelve el sobre si hubo que volver a guardarla.
    pub fn finish_unlock(&mut self, opened: Opened) -> Result<Option<Envelope>, String> {
        self.open(opened);
        let s = self.session.as_mut().expect("recién abierta");
        let mut lp_changed = false;
        for a in &s.data.accounts {
            if !s.lp.contains(&a.id) {
                let at = a.last_synced_at.clone().or_else(|| a.updated_at.clone()).unwrap_or_else(now_iso);
                lp_changed |= s.lp.record(&a.id, &a.ranks, &at);
            }
        }
        if lp_changed {
            s.lp.save(&s.key, &s.salt)?;
        }
        let mut migrated = false;
        for a in s.data.accounts.iter_mut() {
            if let Some(old) = a.extra.remove("matches") {
                if let Ok(list) = serde_json::from_value::<Vec<Match>>(old) {
                    s.history.add(&a.id, &list);
                }
                migrated = true;
            }
        }
        if migrated {
            s.history.save(&s.key, &s.salt)?;
        }
        self.remember_lock_splashes();
        if migrated {
            Ok(Some(self.persist()?))
        } else {
            Ok(None)
        }
    }

    pub fn lock(&mut self) {
        self.session = None;
    }

    /// Cifra la bóveda y la escribe en vault.dat, dejando la anterior en vault.prev.dat.
    /// Devuelve el sobre, que es lo que se sube a Drive.
    pub fn persist(&mut self) -> Result<Envelope, String> {
        let s = self.session.as_ref().ok_or_else(|| "La bóveda está bloqueada".to_string())?;
        let value = serde_json::to_value(&s.data).map_err(|e| e.to_string())?;
        let env = vault::encrypt_with_key(&s.key, &s.salt, &value)?;
        self.write_local(&env)?;
        self.pending_upload = Some(env.clone());
        Ok(env)
    }

    /// La última bóveda guardada que falta subir a Drive (se entrega una sola vez).
    pub fn take_pending_upload(&mut self) -> Option<Envelope> {
        self.pending_upload.take()
    }

    /// Escribe un sobre como bóveda local (también uno bajado de Drive).
    pub fn write_local(&self, env: &Envelope) -> Result<(), String> {
        let path = self.paths.vault();
        if path.exists() {
            std::fs::copy(&path, self.paths.vault_backup()).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_vec(env).map_err(|e| e.to_string())?;
        write_atomic(&path, &json).map_err(|e| e.to_string())
    }

    pub fn save_history(&self) -> Result<(), String> {
        let s = self.session.as_ref().ok_or("La bóveda está bloqueada")?;
        s.history.save(&s.key, &s.salt)
    }

    pub fn save_lp(&self) -> Result<(), String> {
        let s = self.session.as_ref().ok_or("La bóveda está bloqueada")?;
        s.lp.save(&s.key, &s.salt)
    }

    /// Splash de fondo de las cuentas para la pantalla de bloqueo. Va sin cifrar en config.json
    /// porque se muestra antes de desbloquear; son solo números de skin.
    pub fn remember_lock_splashes(&mut self) {
        let Some(s) = &self.session else { return };
        let mut skins: Vec<i64> = Vec::new();
        for id in s.data.accounts.iter().filter_map(|a| a.background_skin_id) {
            if id != 0 && !skins.contains(&id) {
                skins.push(id);
            }
        }
        if skins != self.config.lock_skins {
            self.config.lock_skins = skins;
            let _ = self.save_config();
        }
    }

    // ---------- contraseña maestra ----------

    /// Validaciones de la contraseña nueva (la actual se comprueba con `vault::check_password`).
    pub fn validate_new_password(current: &str, next: &str) -> Result<(), String> {
        if next.chars().count() < 8 {
            return Err("La nueva contraseña debe tener al menos 8 caracteres".into());
        }
        if next == current {
            return Err("La nueva contraseña es igual a la actual".into());
        }
        Ok(())
    }

    /// Clave y salt actuales, para comprobar la contraseña actual fuera del hilo de la interfaz.
    pub fn current_key(&self) -> Option<(Key, Vec<u8>)> {
        self.session.as_ref().map(|s| (s.key, s.salt.clone()))
    }

    /// Cambia la clave de la bóveda (ya derivada de la contraseña nueva) y vuelve a cifrar todo lo
    /// local: la bóveda, su copia de respaldo, el historial y el LP.
    pub fn rekey(&mut self, key: Key, salt: Vec<u8>) -> Result<Envelope, String> {
        {
            let s = self.need()?;
            s.key = key;
            s.salt = salt;
        }
        let env = self.persist()?;
        // La copia de respaldo quedó con la contraseña vieja: se reemplaza por la nueva.
        std::fs::copy(self.paths.vault(), self.paths.vault_backup()).map_err(|e| e.to_string())?;
        self.save_history()?;
        self.save_lp()?;
        Ok(env)
    }

    // ---------- cuentas ----------

    /// Crea o edita una cuenta. Si cambian el Riot ID a mano, es otra cuenta: se olvidan el PUUID y
    /// los datos de la anterior para que "Actualizar rangos" la busque por el nombre nuevo.
    pub fn save_account(&mut self, input: AccountInput) -> Result<String, String> {
        let now = now_iso();
        let s = self.need()?;
        let idx = match input.id.as_deref().and_then(|id| s.data.accounts.iter().position(|a| a.id == id)) {
            Some(i) => i,
            None => {
                s.data.accounts.push(Account {
                    id: uuid::Uuid::new_v4().to_string(),
                    created_at: Some(now.clone()),
                    ranks: Ranks::default(),
                    ..Default::default()
                });
                s.data.accounts.len() - 1
            }
        };
        let acc = &mut s.data.accounts[idx];
        let riot_id_changed = input.game_name.as_deref().is_some_and(|g| norm(g) != norm(&acc.game_name))
            || input.tag_line.as_deref().is_some_and(|t| norm(t) != norm(&acc.tag_line));
        let mut forget = false;
        if riot_id_changed && acc.puuid.is_some() {
            acc.puuid = None;
            acc.api_puuid = None;
            acc.level = None;
            acc.icon_id = None;
            acc.last_played_at = None;
            acc.last_synced_at = None;
            acc.background_skin_id = None;
            acc.crest = None;
            acc.mastery = None;
            acc.ranks = Ranks::default();
            forget = true;
        }
        let set = |field: &mut String, v: Option<String>| {
            if let Some(v) = v {
                *field = v.trim().to_string();
            }
        };
        set(&mut acc.label, input.label);
        set(&mut acc.username, input.username);
        set(&mut acc.password, input.password);
        set(&mut acc.server, input.server);
        set(&mut acc.game_name, input.game_name);
        set(&mut acc.tag_line, input.tag_line);
        set(&mut acc.notes, input.notes);
        acc.updated_at = Some(now);
        let id = acc.id.clone();
        if forget {
            if s.history.remove(&id) {
                self.save_history()?;
            }
            if self.need()?.lp.remove(&id) {
                self.save_lp()?;
            }
        }
        self.persist()?;
        Ok(id)
    }

    pub fn import_accounts(&mut self, rows: Vec<ImportRow>) -> Result<usize, String> {
        let now = now_iso();
        let s = self.need()?;
        let n = rows.len();
        for r in rows {
            s.data.accounts.push(Account {
                id: uuid::Uuid::new_v4().to_string(),
                created_at: Some(now.clone()),
                updated_at: Some(now.clone()),
                username: r.username,
                password: r.password,
                server: r.server,
                ranks: Ranks::default(),
                ..Default::default()
            });
        }
        self.persist()?;
        Ok(n)
    }

    pub fn set_favorite(&mut self, id: &str, favorite: bool) -> Result<(), String> {
        let acc = self.need()?.data.account_mut(id).ok_or("Cuenta no encontrada")?;
        acc.favorite = favorite.then_some(true);
        self.persist().map(|_| ())
    }

    /// Borra la cuenta, su historial y su LP.
    pub fn delete_account(&mut self, id: &str) -> Result<(), String> {
        let s = self.need()?;
        s.data.accounts.retain(|a| a.id != id);
        let (had_history, had_lp) = (s.history.remove(id), s.lp.remove(id));
        if had_history {
            self.save_history()?;
        }
        if had_lp {
            self.save_lp()?;
        }
        self.persist().map(|_| ())
    }

    /// Mezcla `patch` en los ajustes de la bóveda (p. ej. { riotApiKey }).
    pub fn save_settings(&mut self, patch: Map<String, Value>) -> Result<(), String> {
        let s = self.need()?;
        let mut v = serde_json::to_value(&s.data.settings).map_err(|e| e.to_string())?;
        if let Some(obj) = v.as_object_mut() {
            obj.extend(patch);
        }
        s.data.settings = serde_json::from_value(v).map_err(|e| e.to_string())?;
        self.persist().map(|_| ())
    }

    // ---------- datos desde el cliente o la API de Riot ----------

    /// Aplica a la cuenta lo leído del cliente o de la API (no guarda la bóveda; sí el historial y
    /// el LP). Si cambió la cuenta de Riot vinculada, se olvida lo de la anterior.
    pub fn apply_snapshot(&mut self, id: &str, snap: &Snapshot) -> Result<(), String> {
        let now = now_iso();
        let s = self.need()?;
        let acc = s.data.account_mut(id).ok_or("Cuenta no encontrada")?;
        let (mut history_changed, mut lp_changed) = (false, false);
        if acc.puuid.as_deref() != Some(snap.puuid.as_str()) {
            acc.api_puuid = None;
            if acc.puuid.is_some() {
                history_changed |= s.history.remove(id);
                lp_changed |= s.lp.remove(id);
            }
        }
        if snap.api_puuid.is_some() {
            acc.api_puuid = snap.api_puuid.clone();
        }
        acc.puuid = Some(snap.puuid.clone());
        acc.game_name = snap.game_name.clone();
        acc.tag_line = snap.tag_line.clone();
        acc.level = snap.level;
        acc.icon_id = snap.icon_id;
        acc.last_synced_at = Some(now.clone());
        // None = no se pudo consultar: se conserva lo anterior.
        for (queue, update) in [("solo", &snap.ranks.solo), ("flex", &snap.ranks.flex), ("tft", &snap.ranks.tft), ("doubleUp", &snap.ranks.double_up)] {
            if let (Some(value), Some(slot)) = (update, acc.ranks.slot(queue)) {
                *slot = value.clone();
            }
        }
        if !snap.server.is_empty() {
            acc.server = snap.server.clone();
        }
        // Nos quedamos con la partida más reciente que conozcamos.
        if let Some(at) = &snap.last_played_at {
            if acc.last_played_at.as_ref().is_none_or(|prev| prev <= at) {
                acc.last_played_at = Some(at.clone());
            }
        }
        if snap.background_skin_id.is_some() {
            acc.background_skin_id = snap.background_skin_id;
        }
        if snap.crest.is_some() {
            acc.crest = snap.crest.clone();
        }
        if snap.mastery.is_some() {
            acc.mastery = snap.mastery.clone();
        }
        // El baneo solo lo sabe el cliente: lo de la API (None) no lo pisa.
        if let Some(ban) = &snap.ban {
            acc.ban = ban.clone();
        }
        let ranks = acc.ranks.clone();
        lp_changed |= s.lp.record(id, &ranks, &now);
        if let Some(m) = &snap.matches {
            history_changed |= s.history.add(id, m);
        }
        if history_changed {
            self.save_history()?;
        }
        if lp_changed {
            self.save_lp()?;
        }
        Ok(())
    }

    /// Cuenta guardada que está abierta en el cliente: 1) por PUUID, 2) por usuario de login,
    /// 3) por Riot ID. En 2 y 3 solo si hay exactamente una coincidencia, para no vincular mal.
    pub fn find_detected(&self, snap: &Snapshot) -> Option<String> {
        let accounts = &self.data()?.accounts;
        if let Some(a) = accounts.iter().find(|a| a.owns_puuid(&snap.puuid)) {
            return Some(a.id.clone());
        }
        let unique = |list: Vec<&Account>| (list.len() == 1).then(|| list[0].id.clone());
        if !snap.username.is_empty() {
            if let Some(id) = unique(accounts.iter().filter(|a| norm(&a.username) == norm(&snap.username)).collect()) {
                return Some(id);
            }
        }
        if !snap.game_name.is_empty() && !snap.tag_line.is_empty() {
            return unique(
                accounts
                    .iter()
                    .filter(|a| a.puuid.is_none() && norm(&a.game_name) == norm(&snap.game_name) && norm(&a.tag_line) == norm(&snap.tag_line))
                    .collect(),
            );
        }
        None
    }

    /// Vincula lo leído del cliente a la cuenta `id` (una cuenta de Riot solo puede estar en una
    /// entrada) y guarda la bóveda solo si cambió algo. Devuelve true si cambió.
    pub fn link_snapshot(&mut self, id: &str, snap: &Snapshot) -> Result<bool, String> {
        let s = self.need()?;
        let before = serde_json::to_value(&s.data.accounts).ok();
        for a in s.data.accounts.iter_mut() {
            if a.id != id && a.puuid.as_deref() == Some(snap.puuid.as_str()) {
                a.puuid = None;
            }
        }
        self.apply_snapshot(id, snap)?;
        // lastSyncedAt cambia siempre: no cuenta como cambio de verdad.
        let strip = |v: Option<Value>| {
            v.map(|mut v| {
                if let Some(list) = v.as_array_mut() {
                    for a in list {
                        a.as_object_mut().map(|o| o.remove("lastSyncedAt"));
                    }
                }
                v
            })
        };
        let after = serde_json::to_value(&self.need()?.data.accounts).ok();
        let changed = strip(before) != strip(after);
        if changed {
            self.persist()?;
            self.remember_lock_splashes();
        }
        Ok(changed)
    }

    /// La partida terminó: la cuenta del PUUID jugó ahora. Devuelve su id si cambió.
    pub fn mark_played(&mut self, puuid: &str, at: &str) -> Option<String> {
        let acc = self.session.as_mut()?.data.accounts.iter_mut().find(|a| a.owns_puuid(puuid))?;
        if acc.last_played_at.as_deref().is_some_and(|prev| prev > at) {
            return None;
        }
        acc.last_played_at = Some(at.to_string());
        let id = acc.id.clone();
        self.persist().ok()?;
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(name: &str) -> Store {
        let dir = std::env::temp_dir().join(format!("sv-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Store::new(Paths::at(dir))
    }

    fn unlock(store: &mut Store, password: &str) -> Result<(), String> {
        let env = store.read_local().ok_or("sin bóveda")?;
        let opened = Opened::from_envelope(&env, password)?;
        store.finish_unlock(opened).map(|_| ())
    }

    #[test]
    fn create_save_lock_and_unlock_again() {
        let mut store = temp_store("cycle");
        store.finish_create(Opened::new_vault("clave-segura").unwrap()).unwrap();
        let id = store
            .save_account(AccountInput { username: Some("  user1 ".into()), password: Some("pw".into()), ..Default::default() })
            .unwrap();
        store.set_favorite(&id, true).unwrap();
        store.lock();
        assert!(unlock(&mut store, "otra").is_err());
        unlock(&mut store, "clave-segura").unwrap();
        let acc = store.data().unwrap().account(&id).unwrap();
        assert_eq!(acc.username, "user1", "se guarda sin espacios");
        assert!(acc.is_favorite());
        let _ = std::fs::remove_dir_all(&store.paths.dir);
    }

    #[test]
    fn changing_the_riot_id_forgets_the_linked_account() {
        let mut store = temp_store("riotid");
        store.finish_create(Opened::new_vault("clave-segura").unwrap()).unwrap();
        let id = store.save_account(AccountInput { game_name: Some("Uno".into()), tag_line: Some("LAS".into()), ..Default::default() }).unwrap();
        store.need().unwrap().data.account_mut(&id).unwrap().puuid = Some("p1".into());
        // Mismo Riot ID con otras mayúsculas: no cambia nada.
        store.save_account(AccountInput { id: Some(id.clone()), game_name: Some("uno".into()), ..Default::default() }).unwrap();
        assert!(store.data().unwrap().account(&id).unwrap().puuid.is_some());
        store.save_account(AccountInput { id: Some(id.clone()), game_name: Some("Dos".into()), ..Default::default() }).unwrap();
        assert!(store.data().unwrap().account(&id).unwrap().puuid.is_none());
        let _ = std::fs::remove_dir_all(&store.paths.dir);
    }

    #[test]
    fn detected_account_is_found_linked_and_updated() {
        use crate::model::Rank;
        use crate::snapshot::{RankUpdates, Snapshot};
        let mut store = temp_store("detect");
        store.finish_create(Opened::new_vault("clave-segura").unwrap()).unwrap();
        let a = store.save_account(AccountInput { username: Some("Main_User".into()), ..Default::default() }).unwrap();
        let b = store.save_account(AccountInput { username: Some("otra".into()), ..Default::default() }).unwrap();
        let gold = Rank { tier: "GOLD".into(), division: "I".into(), lp: 50, ..Default::default() };
        let snap = Snapshot {
            username: "main_user".into(),
            puuid: "P1".into(),
            game_name: "Uno".into(),
            tag_line: "LAS".into(),
            server: "LAS".into(),
            ban: Some(None),
            ranks: RankUpdates { solo: Some(Some(gold)), flex: Some(None), ..Default::default() },
            ..Default::default()
        };
        // Por usuario de login, sin importar mayúsculas.
        assert_eq!(store.find_detected(&snap).as_deref(), Some(a.as_str()));
        assert!(store.link_snapshot(&a, &snap).unwrap(), "primera vez cambia");
        assert!(!store.link_snapshot(&a, &snap).unwrap(), "lo mismo otra vez no guarda");
        let acc = store.data().unwrap().account(&a).unwrap();
        assert_eq!((acc.puuid.as_deref(), acc.riot_id().as_str()), (Some("P1"), "Uno#LAS"));
        assert_eq!(acc.ranks.solo.as_ref().unwrap().lp, 50);
        // Ya vinculada: se reconoce por PUUID aunque cambie el usuario.
        let other_user = Snapshot { username: "x".into(), ..snap.clone() };
        assert_eq!(store.find_detected(&other_user).as_deref(), Some(a.as_str()));
        // Vincularla a otra entrada la quita de la primera.
        store.link_snapshot(&b, &snap).unwrap();
        assert!(store.data().unwrap().account(&a).unwrap().puuid.is_none());
        // Un rango que no se pudo consultar (None) no borra el guardado.
        let partial = Snapshot { ranks: RankUpdates::default(), ..snap };
        store.link_snapshot(&b, &partial).unwrap();
        assert_eq!(store.data().unwrap().account(&b).unwrap().ranks.solo.as_ref().unwrap().tier, "GOLD");
        let _ = std::fs::remove_dir_all(&store.paths.dir);
    }
}
