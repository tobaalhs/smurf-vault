//! Estado compartido por todas las pantallas: la bóveda (`Store` del core) y los datos del juego.
//! Es una entidad de GPUI, así cada vista que la lee se vuelve a dibujar cuando cambia.
use gpui_kit::Context;
use sv_core::gamedata::GameData;
use sv_core::store::Store;

/// Avance de "Actualizar con la API de Riot": cuántas van, de cuántas y cuál se está consultando.
#[derive(Clone, Debug)]
pub struct RiotProgress {
    pub done: usize,
    pub total: usize,
    pub name: String,
}

/// Páginas de la pantalla principal (barra lateral).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Accounts,
    Tools,
    Settings,
}

pub struct Vault {
    pub store: Store,
    pub game: GameData,
    /// Hay una actualización con la API de Riot en curso (y su avance, si son varias cuentas).
    pub riot_busy: bool,
    pub riot_progress: Option<RiotProgress>,
    /// Servidor del control desde el celular, mientras está encendido.
    pub remote: Option<sv_core::remote::RemoteServer>,
    /// Google Drive: vinculación y subidas en segundo plano.
    pub drive: std::sync::Arc<sv_core::drive::Drive>,
    /// Página pedida desde otra vista (un chip, la paleta...); la raíz la atiende.
    pub go_to: Option<Page>,
}

impl Vault {
    /// Pide ir a una página de la pantalla principal.
    pub fn go(&mut self, page: Page, cx: &mut Context<Self>) {
        self.go_to = Some(page);
        cx.notify();
    }

    pub fn new() -> Self {
        let store = Store::new(Default::default());
        let game = GameData::load_cache(&store.paths);
        let drive = sv_core::drive::Drive::new(store.paths.clone());
        Self { store, game, riot_busy: false, riot_progress: None, remote: None, drive, go_to: None }
    }

    /// Enciende o apaga el control desde el celular según la configuración. El código se guarda al
    /// generarlo: así el QR ya escaneado sigue sirviendo después de reiniciar.
    pub fn sync_remote(&mut self) -> Result<(), String> {
        if !self.store.config.remote_enabled {
            self.remote = None;
            return Ok(());
        }
        if self.store.config.remote_token.is_none() {
            self.store.config.remote_token = Some(sv_core::remote::new_token());
            let _ = self.store.save_config();
        }
        if self.remote.is_none() {
            let token = self.store.config.remote_token.clone().unwrap_or_default();
            self.remote = Some(sv_core::remote::RemoteServer::start(token, self.store.paths.clone())?);
        }
        Ok(())
    }

    /// Código nuevo: los celulares conectados con el anterior quedan afuera.
    pub fn new_remote_code(&mut self) {
        let token = sv_core::remote::new_token();
        self.store.config.remote_token = Some(token.clone());
        let _ = self.store.save_config();
        if let Some(r) = &self.remote {
            r.set_token(token);
        }
    }

    /// Sube a Google Drive la última versión guardada, si hay una pendiente.
    pub fn flush_upload(&mut self) {
        if let Some(env) = self.store.take_pending_upload() {
            self.drive.queue_upload(env);
        }
    }

    /// Cambia algo de la bóveda y avisa a las vistas para que se redibujen.
    pub fn change<R>(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Store) -> R) -> R {
        let out = f(&mut self.store);
        self.flush_upload();
        cx.notify();
        out
    }
}

/// URL del splash de una skin (id = campeón*1000 + número de skin), de CommunityDragon.
pub fn splash_url(skin_id: i64) -> String {
    format!(
        "https://cdn.communitydragon.org/latest/champion/{}/splash-art/skin/{}",
        skin_id / 1000,
        skin_id % 1000
    )
}
