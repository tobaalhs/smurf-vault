// Sin consola de Windows en la versión compilada (release).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod assets;
mod client;
mod desktop;
mod detail;
mod dialogs;
mod google;
mod http;
mod lock;
mod main_screen;
mod settings_dialog;
mod sounds;
mod stage_bg;
mod switch;
mod state;
mod tabs;
mod tools_dialog;
mod theme;
mod ui;
mod wheel;

use gpui_kit::component::TitleBar;
use gpui_kit::*;

fn main() {
    let paths = sv_core::paths::Paths::default();
    // Una sola instancia: si ya estaba abierta (p. ej. en la bandeja), se muestra esa.
    // Solo en desarrollo: SV_DEV_MULTI deja abrir una copia de prueba junto a la real.
    let multi = cfg!(debug_assertions) && std::env::var("SV_DEV_MULTI").is_ok();
    if !multi && !desktop::single_instance(&paths.dir) {
        return;
    }
    desktop::set_app_id();
    // Al iniciar Windows parte en la bandeja; sin bandeja no tendría cómo abrirse, así que se muestra.
    let start_hidden = std::env::args().any(|a| a == desktop::STARTUP_ARG) && sv_core::config::Config::load(&paths).close_to_tray;
    gpui_kit::application().with_assets(assets::Assets).run(move |cx| {
        gpui_kit::init(cx);
        cx.set_http_client(http::ImageHttpClient::install(paths.dir.join("image-cache")));
        let config = sv_core::config::Config::load(&paths);
        theme::Palette::from_theme(&config.theme).apply(cx);

        let bounds = Bounds::centered(None, size(px(1180.), px(780.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(820.), px(560.))),
            app_id: Some("com.tobaalhs.smurfvault".into()),
            // Sin límite de cuadros sin foco: el fondo sigue al mouse aunque la ventana no tenga el
            // foco (como en Electron). Sin nada animándose no se dibuja nada igual.
            inactive_frame_interval: None,
            show: !start_hidden,
            focus: !start_hidden,
            ..TitleBar::window_options()
        };
        gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| app::AppView::new(window, cx)))
            .expect("no se pudo abrir la ventana");
        cx.on_window_closed(|cx, _| {
            desktop::install_pending_on_exit();
            cx.quit()
        })
        .detach();
    });
}
