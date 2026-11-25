// Sin consola de Windows en la versión compilada (release).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use gpui_kit::component::label::Label;
use gpui_kit::component::*;
use gpui_kit::*;

struct SmurfVault;

impl Render for SmurfVault {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .child(Label::new("Smurf Vault"))
    }
}

fn main() {
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| SmurfVault))
            .expect("no se pudo abrir la ventana");
    });
}
