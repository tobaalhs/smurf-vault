//! Estado compartido por todas las pantallas: la bóveda (`Store` del core) y los datos del juego.
//! Es una entidad de GPUI, así cada vista que la lee se vuelve a dibujar cuando cambia.
use gpui_kit::Context;
use sv_core::gamedata::GameData;
use sv_core::store::Store;

pub struct Vault {
    pub store: Store,
    pub game: GameData,
}

impl Vault {
    pub fn new() -> Self {
        let store = Store::new(Default::default());
        let game = GameData::load_cache(&store.paths);
        Self { store, game }
    }

    /// Cambia algo de la bóveda y avisa a las vistas para que se redibujen.
    pub fn change<R>(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Store) -> R) -> R {
        let out = f(&mut self.store);
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
