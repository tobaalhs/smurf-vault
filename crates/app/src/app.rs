//! Vista raíz: la barra de título propia (la ventana no usa la de Windows) y la pantalla actual,
//! bloqueo o principal.
use crate::assets;
use crate::lock::{LockEvent, LockScreen};
use crate::state::Vault;
use crate::theme::palette;
use gpui_kit::component::button::Button;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{h_flex, v_flex, Icon, TitleBar, WindowExt};
use gpui_kit::*;

enum Screen {
    Lock(Entity<LockScreen>),
    Main,
}

pub struct AppView {
    vault: Entity<Vault>,
    screen: Screen,
    _screen_sub: Option<Subscription>,
}

impl AppView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        window.set_window_title("Smurf Vault");
        let vault = cx.new(|_| Vault::new());
        cx.observe(&vault, |_, _, cx| cx.notify()).detach();
        let mut this = Self { vault, screen: Screen::Main, _screen_sub: None };
        this.show_lock(window, cx);
        this
    }

    fn show_lock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let lock = cx.new(|cx| LockScreen::new(self.vault.clone(), window, cx));
        self._screen_sub = Some(cx.subscribe_in(&lock, window, |this, _, ev: &LockEvent, window, cx| match ev {
            LockEvent::Unlocked => this.show_main(window, cx),
        }));
        self.screen = Screen::Lock(lock);
        cx.notify();
    }

    fn show_main(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self._screen_sub = None;
        self.screen = Screen::Main;
        cx.notify();
    }

    fn lock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.vault.update(cx, |v, cx| v.change(cx, |s| s.lock()));
        self.show_lock(window, cx);
        window.push_notification(Notification::info("Bóveda bloqueada"), cx);
    }

    fn brand(&self, cx: &App) -> impl IntoElement {
        let p = palette(cx);
        h_flex()
            .gap(px(10.))
            .child(img(assets::logo()).size(px(32.)).rounded(px(8.)))
            .child(div().text_size(px(15.)).font_weight(FontWeight::BOLD).text_color(p.text).child("Smurf Vault"))
    }

    /// Pantalla principal provisoria: lista las cuentas de la bóveda abierta.
    fn main_placeholder(&self, cx: &App) -> impl IntoElement {
        let p = palette(cx);
        let accounts = self.vault.read(cx).store.data().map(|d| d.accounts.clone()).unwrap_or_default();
        v_flex()
            .flex_1()
            .p(px(24.))
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(p.muted)
                    .child(format!("{} cuentas en la bóveda (la pantalla principal se está construyendo)", accounts.len())),
            )
            .children(accounts.into_iter().map(|a| {
                let solo = a.ranks.solo.as_ref().map(|r| format!("{} {} · {} LP", r.tier, r.division, r.lp));
                h_flex()
                    .gap(px(12.))
                    .px(px(14.))
                    .py(px(10.))
                    .rounded(px(10.))
                    .bg(p.surface)
                    .border_1()
                    .border_color(p.border)
                    .child(div().text_color(p.text).child(a.display_name()))
                    .child(div().text_color(p.muted).text_size(px(12.)).child(solo.unwrap_or_else(|| "Sin rango".into())))
            }))
    }
}

impl Render for AppView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        match &self.screen {
            // En el bloqueo, la barra va transparente sobre el fondo: solo para mover y cerrar la ventana.
            Screen::Lock(lock) => div()
                .size_full()
                .relative()
                .child(lock.clone())
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .child(TitleBar::new().h(px(60.)).bg(transparent_black()).border_0()),
                ),
            Screen::Main => v_flex()
                .size_full()
                .bg(p.bg)
                .child(
                    TitleBar::new()
                        .h(px(60.))
                        .bg(Hsla { a: 0.9, ..p.surface })
                        .border_color(p.border)
                        .child(self.brand(cx))
                        .child(div().flex_1())
                        .child(
                            Button::new("lock-vault")
                                .outline()
                                .icon(Icon::empty().path(assets::icon("lock")))
                                .label("Bloquear")
                                .on_click(cx.listener(|this, _, window, cx| this.lock(window, cx))),
                        ),
                )
                .child(self.main_placeholder(cx)),
        }
    }
}
