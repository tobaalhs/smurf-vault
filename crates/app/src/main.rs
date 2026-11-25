// Sin consola de Windows en la versión compilada (release).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod assets;
mod http;
mod lock;
mod state;
mod theme;

use gpui_kit::component::TitleBar;
use gpui_kit::*;
use std::sync::Arc;
use std::time::Duration;

fn main() {
    gpui_kit::application().with_assets(assets::Assets).run(move |cx| {
        gpui_kit::init(cx);
        cx.set_http_client(Arc::new(http::ImageHttpClient::new()));
        let config = sv_core::config::Config::load(&Default::default());
        theme::Palette::from_theme(&config.theme).apply(cx);

        let bounds = Bounds::centered(None, size(px(1180.), px(780.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(820.), px(560.))),
            app_id: Some("com.tobaalhs.smurfvault".into()),
            // La app pasa mucho tiempo abierta sin uso: sin foco, dibuja lo justo.
            inactive_frame_interval: Some(Duration::from_millis(250)),
            ..TitleBar::window_options()
        };
        gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| app::AppView::new(window, cx)))
            .expect("no se pudo abrir la ventana");
        cx.on_window_closed(|cx, _| cx.quit()).detach();
    });
}
