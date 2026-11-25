//! Vista raíz: la barra de título propia (la ventana no usa la de Windows) y la pantalla actual,
//! bloqueo o principal.
use crate::assets;
use crate::lock::{LockEvent, LockScreen};
use crate::main_screen::MainScreen;
use crate::state::Vault;
use crate::theme::palette;
use crate::ui::alpha;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{h_flex, v_flex, Icon, IconName, TitleBar, WindowExt};
use gpui_kit::*;

enum Screen {
    Lock(Entity<LockScreen>),
    Main(Entity<MainScreen>),
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
        let lock = cx.new(|cx| LockScreen::new(vault.clone(), window, cx));
        let mut this = Self { vault, screen: Screen::Lock(lock.clone()), _screen_sub: None };
        this.watch_lock(&lock, window, cx);
        this
    }

    fn watch_lock(&mut self, lock: &Entity<LockScreen>, window: &mut Window, cx: &mut Context<Self>) {
        self._screen_sub = Some(cx.subscribe_in(lock, window, |this, _, ev: &LockEvent, window, cx| match ev {
            LockEvent::Unlocked => this.show_main(window, cx),
        }));
    }

    fn show_lock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let lock = cx.new(|cx| LockScreen::new(self.vault.clone(), window, cx));
        self.watch_lock(&lock, window, cx);
        self.screen = Screen::Lock(lock);
        cx.notify();
    }

    fn show_main(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self._screen_sub = None;
        self.screen = Screen::Main(cx.new(|cx| MainScreen::new(self.vault.clone(), window, cx)));
        cx.notify();
    }

    fn lock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.vault.update(cx, |v, cx| v.change(cx, |s| s.lock()));
        self.show_lock(window, cx);
        window.push_notification(Notification::info("Bóveda bloqueada"), cx);
    }

    fn top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let brand = h_flex()
            .gap(px(10.))
            .child(img(assets::logo()).size(px(32.)).rounded(px(8.)))
            .child(div().text_size(px(15.)).font_weight(FontWeight::BOLD).text_color(p.text).child("Smurf Vault"));
        // Google Drive todavía no está portado: la bóveda queda solo en este PC.
        let sync = h_flex()
            .gap(px(6.))
            .px(px(10.))
            .py(px(5.))
            .rounded_full()
            .border_1()
            .border_color(p.border)
            .text_size(px(12.))
            .text_color(p.muted)
            .child(div().size(px(7.)).rounded_full().bg(p.muted))
            .child("Solo local");
        TitleBar::new()
            .h(px(60.))
            .bg(alpha(p.surface, 0.9))
            .border_color(p.border)
            .child(brand)
            .child(div().flex_1())
            .child(
                h_flex()
                    .gap(px(8.))
                    .pr(px(12.))
                    .child(sync)
                    .child(div().w(px(1.)).h(px(22.)).bg(p.border_strong).mx(px(4.)))
                    .child(
                        Button::new("lock-vault")
                            .icon(Icon::empty().path(assets::icon("lock-keyhole")))
                            .label("Bloquear")
                            .tooltip("Bloquear la bóveda: oculta tus cuentas hasta que pongas la contraseña maestra")
                            .on_click(cx.listener(|this, _, window, cx| this.lock(window, cx))),
                    )
                    .child(
                        Button::new("add-account")
                            .primary()
                            .icon(Icon::new(IconName::Plus))
                            .label("Nueva cuenta")
                            .on_click(|_, window, cx| window.push_notification(Notification::info("Agregar cuentas llega en la próxima parte de esta fase."), cx)),
                    )
                    .child(
                        Button::new("more")
                            .icon(Icon::new(IconName::Ellipsis))
                            .on_click(|_, window, cx| window.push_notification(Notification::info("El menú de opciones llega en la próxima parte de esta fase."), cx)),
                    ),
            )
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
                .child(div().absolute().top_0().left_0().right_0().child(TitleBar::new().h(px(60.)).bg(transparent_black()).border_0()))
                .into_any_element(),
            Screen::Main(main) => {
                let main = main.clone();
                v_flex().size_full().bg(p.bg).child(self.top_bar(cx)).child(div().flex_1().min_h_0().child(main)).into_any_element()
            }
        }
    }
}
