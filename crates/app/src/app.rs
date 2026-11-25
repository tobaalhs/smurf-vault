//! Vista raíz: la barra de título propia (la ventana no usa la de Windows) y la pantalla actual,
//! bloqueo o principal.
use crate::assets;
use crate::lock::{LockEvent, LockScreen};
use crate::main_screen::MainScreen;
use crate::state::Vault;
use crate::theme::palette;
use crate::ui::alpha;
use crate::dialogs;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex, Icon, IconName, TitleBar, WindowExt};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

enum Screen {
    Lock(Entity<LockScreen>),
    /// La pantalla principal y su fondo, que se dibuja aparte (ver stage_bg.rs).
    Main(Entity<MainScreen>, Entity<crate::stage_bg::StageBg>),
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
        crate::client::refresh_game_data(vault.clone(), cx);
        // Autoaceptar y aparecer desconectado corren solos (también con la bóveda bloqueada).
        crate::tools_dialog::apply_config(&vault, cx);
        if let Err(e) = vault.update(cx, |v, _| v.sync_remote()) {
            window.push_notification(Notification::error(e), cx);
        }
        let sync_vault = vault.clone();
        cx.spawn_in(window, async move |_, cx| {
            let mut seen = sv_core::tools::tools().accepted_count();
            let mut last_sync: Option<sv_core::drive::SyncState> = None;
            loop {
                cx.background_executor().timer(std::time::Duration::from_secs(1)).await;
                let now = sv_core::tools::tools().accepted_count();
                let alive = cx.update(|window, cx| {
                    if now > seen {
                        seen = now;
                        window.push_notification(Notification::success("Partida aceptada ✓"), cx);
                    }
                    // Estado de Drive: se redibuja el chip y un mismo error se avisa una sola vez.
                    let sync = sync_vault.read(cx).drive.sync_state();
                    if last_sync.as_ref() != Some(&sync) {
                        if let sv_core::drive::SyncState::Error(e) = &sync {
                            window.push_notification(Notification::error(format!("Google Drive: {e}")), cx);
                        }
                        last_sync = Some(sync);
                        window.refresh();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        let lock =cx.new(|cx| LockScreen::new(vault.clone(), window, cx));
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
        let main = cx.new(|cx| MainScreen::new(self.vault.clone(), window, cx));
        let bg = cx.new(|_| crate::stage_bg::StageBg::new(main.downgrade()));
        main.update(cx, |m, _| m.bg = Some(bg.clone()));
        self.screen = Screen::Main(main, bg);
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
        // Estado de Google Drive; clic para abrir Ajustes.
        use sv_core::drive::SyncState;
        let (text, tip, dot, fg, border) = match self.vault.read(cx).drive.sync_state() {
            SyncState::Local => ("Solo local", "Tu bóveda solo está en este PC. Clic para vincular Google Drive.".to_string(), p.muted, p.muted, p.border),
            SyncState::Syncing => ("Sincronizando…", "Subiendo cambios a Google Drive".to_string(), p.cyan, p.muted, p.border),
            SyncState::Ok(at) => {
                let when = chrono::DateTime::parse_from_rfc3339(&at)
                    .map(|t| format!(" · {}", t.with_timezone(&chrono::Local).format("%H:%M:%S")))
                    .unwrap_or_default();
                ("Drive al día", format!("Guardado en Google Drive{when}"), p.ok, p.muted, p.border)
            }
            SyncState::Error(e) => ("Error de Drive", e, p.danger, p.danger, alpha(p.danger, 0.3)),
        };
        let settings_vault = self.vault.clone();
        let sync = h_flex()
            .id("sync-chip")
            .gap(px(8.))
            .h(px(28.))
            .px(px(11.))
            .rounded_full()
            .border_1()
            .border_color(border)
            .text_size(px(12.))
            .text_color(fg)
            .cursor_pointer()
            .hover(|s| s.bg(p.surface_2))
            .child(div().size(px(7.)).rounded_full().bg(dot))
            .child(text)
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .on_click(move |_, window, cx| crate::settings_dialog::open_settings(settings_vault.clone(), window, cx));
        // Herramientas encendidas: un chip cada una, que abre Herramientas.
        let config = &self.vault.read(cx).store.config;
        let tool_chip = |id: &'static str, icon: &'static str, label: &'static str, tip: &'static str, color: Hsla, vault: Entity<Vault>| {
            h_flex()
                .id(id)
                .gap(px(6.))
                .h(px(28.))
                .px(px(11.))
                .rounded_full()
                .border_1()
                .border_color(alpha(color, 0.35))
                .bg(alpha(color, 0.1))
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color)
                .cursor_pointer()
                .child(Icon::empty().path(assets::icon(icon)).size(px(14.)))
                .child(label)
                .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
                .on_click(move |_, window, cx| crate::tools_dialog::open_tools(vault.clone(), window, cx))
        };
        let auto_chip = config.auto_accept.then(|| {
            tool_chip("auto-chip", "zap", "Autoaceptar", "Autoaceptar partidas activado · clic para configurar", p.cyan, self.vault.clone())
        });
        let offline_chip = config.appear_offline.then(|| {
            tool_chip("offline-chip", "ghost", "Desconectado", "Apareces desconectado en el chat del LoL · clic para configurar", p.text_2, self.vault.clone())
        });
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
                    .children(auto_chip)
                    .children(offline_chip)
                    .child(sync)
                    .child(div().w(px(1.)).h(px(22.)).bg(p.border_strong).mx(px(4.)))
                    .child(
                        Button::new("lock-vault")
                            .icon(Icon::empty().path(assets::icon("lock-keyhole")))
                            .label("Bloquear")
                            .tooltip("Bloquear la bóveda: oculta tus cuentas hasta que pongas la contraseña maestra")
                            .on_click(cx.listener(|this, _, window, cx| this.lock(window, cx))),
                    )
                    .child({
                        let vault = self.vault.clone();
                        Button::new("add-account")
                            .primary()
                            .icon(Icon::new(IconName::Plus))
                            .label("Nueva cuenta")
                            .tooltip("Inicia sesión en el Riot Client y la cuenta se guarda sola (Ctrl N)")
                            .on_click(move |_, window, cx| dialogs::open_add(vault.clone(), window, cx))
                    })
                    .child(self.menu(cx)),
            )
    }

    /// Menú ⋯ con las acciones que no tienen botón propio.
    fn menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let vault = self.vault.clone();
        let view = cx.entity().downgrade();
        Button::new("more").icon(Icon::new(IconName::Ellipsis)).dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
            let (manual, import, riot, tools, settings, view) =
                (vault.clone(), vault.clone(), vault.clone(), vault.clone(), vault.clone(), view.clone());
            menu.item(
                PopupMenuItem::new("Agregar cuenta a mano")
                    .icon(Icon::empty().path(assets::icon("pencil")))
                    .on_click(move |_, window, cx| dialogs::open_account(manual.clone(), None, window, cx)),
            )
            .item(PopupMenuItem::new("Actualizar todas con la API de Riot").icon(Icon::new(IconName::RefreshCw)).on_click(move |_, window, cx| {
                crate::client::refresh_riot(riot.clone(), None, window, cx)
            }))
            .item(PopupMenuItem::new("Herramientas").icon(Icon::empty().path(assets::icon("wrench"))).on_click(move |_, window, cx| {
                crate::tools_dialog::open_tools(tools.clone(), window, cx)
            }))
            .item(
                PopupMenuItem::new("Importar desde txt")
                    .icon(Icon::empty().path(assets::icon("upload")))
                    .on_click(move |_, window, cx| dialogs::open_import(import.clone(), window, cx)),
            )
            .item(PopupMenuItem::new("Ajustes").icon(Icon::new(IconName::Settings)).on_click(move |_, window, cx| {
                crate::settings_dialog::open_settings(settings.clone(), window, cx)
            }))
            .separator()
            .item(PopupMenuItem::new("Bloquear bóveda  (Ctrl L)").icon(Icon::empty().path(assets::icon("lock"))).on_click(move |_, window, cx| {
                view.update(cx, |this, cx| this.lock(window, cx)).ok();
            }))
        })
    }

    /// Atajos de la pantalla principal: Ctrl+L bloquea, Ctrl+N agrega una cuenta.
    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !ev.keystroke.modifiers.control || !matches!(self.screen, Screen::Main(_, _)) {
            return;
        }
        match ev.keystroke.key.as_str() {
            "l" => self.lock(window, cx),
            "n" => dialogs::open_add(self.vault.clone(), window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        // Con un diálogo abierto se oscurece solo lo que está bajo la barra: la barra sigue usable.
        let dialog_open = window.has_active_dialog(cx);
        match &self.screen {
            // En el bloqueo, la barra va transparente sobre el fondo: solo para mover y cerrar la ventana.
            Screen::Lock(lock) => div()
                .size_full()
                .relative()
                .child(lock.clone())
                .child(div().absolute().top_0().left_0().right_0().child(TitleBar::new().h(px(60.)).bg(transparent_black()).border_0()))
                .into_any_element(),
            Screen::Main(main, bg) => {
                let (main, bg) = (main.clone(), bg.clone());
                v_flex()
                    .relative()
                    .size_full()
                    .bg(p.bg)
                    .on_key_down(cx.listener(Self::on_key))
                    .child(self.top_bar(cx))
                    // El fondo va detrás y la pantalla encima, en caché: al mover el mouse solo se
                    // redibuja el fondo.
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .relative()
                            .child(bg)
                            .child(AnyView::from(main).cached(StyleRefinement::default().absolute().inset_0().size_full())),
                    )
                    .when(dialog_open, |d| {
                        d.child(div().id("dialog-backdrop").absolute().top(px(60.)).left_0().right_0().bottom_0().bg(alpha(p.bg, 0.86)).occlude())
                    })
                    .into_any_element()
            }
        }
    }
}
