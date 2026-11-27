//! Archivos que van dentro del ejecutable: los íconos que la app usa además de los que trae la
//! librería de componentes (Lucide, licencia en assets/icons/LICENSE-LUCIDE), el logo y la fuente
//! del nombre de la app (Bebas Neue, licencia en assets/fonts/LICENSE-BEBAS-NEUE.txt).
use gpui_kit::{AssetSource, Image, ImageFormat, SharedString};
use std::borrow::Cow;
use std::sync::{Arc, OnceLock};

#[derive(rust_embed::RustEmbed)]
#[folder = "assets"]
#[include = "icons/*.svg"]
#[include = "images/*.png"]
#[include = "images/ranks/*.png"]
#[include = "images/wings/*.webp"]
#[include = "images/crests/*.webp"]
#[include = "fonts/*.ttf"]
struct Embedded;

/// Fuente del nombre "Smurf Vault" de la barra de arriba (solo mayúsculas).
pub const BRAND_FONT: &str = "Bebas Neue";

/// Registra las fuentes que van dentro del ejecutable.
pub fn load_fonts(cx: &mut gpui_kit::App) {
    let fonts: Vec<Cow<'static, [u8]>> = Embedded::iter().filter(|p| p.ends_with(".ttf")).filter_map(|p| Embedded::get(&p)).map(|f| f.data).collect();
    if let Err(e) = cx.text_system().add_fonts(fonts) {
        eprintln!("no pude cargar las fuentes: {e}");
    }
}

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if let Some(file) = Embedded::get(path) {
            return Ok(Some(file.data));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        let mut all: Vec<SharedString> =
            Embedded::iter().filter(|p| p.starts_with(path)).map(|p| SharedString::from(p.to_string())).collect();
        all.extend(gpui_kit::assets::Assets.list(path)?);
        all.sort();
        all.dedup();
        Ok(all)
    }
}

/// Ruta de un ícono de Lucide de los que agregó la app (assets/icons/<nombre>.svg).
pub fn icon(name: &str) -> SharedString {
    format!("icons/{name}.svg").into()
}

/// El logo de Smurf Vault, decodificado una sola vez.
pub fn logo() -> Arc<Image> {
    static LOGO: OnceLock<Arc<Image>> = OnceLock::new();
    LOGO.get_or_init(|| {
        let bytes = Embedded::get("images/logo.png").expect("logo incluido").data.into_owned();
        Arc::new(Image::from_bytes(ImageFormat::Png, bytes))
    })
    .clone()
}

/// Ícono de la app (PNG), para la bandeja del sistema.
pub const ICON_PNG: &[u8] = include_bytes!("../assets/images/icon.png");
