//! Vista raíz: la barra de título propia (la ventana no usa la de Windows) y la pantalla actual,
//! bloqueo o principal. La principal tiene una barra lateral angosta, solo con íconos (Cuentas,
//! Herramientas, Ajustes).
use crate::assets;
use crate::lock::{LockEvent, LockScreen};
use crate::main_screen::MainScreen;
use crate::settings_dialog::SettingsPanel;
use crate::state::{Page, Vault};
use crate::tools_dialog::ToolsPanel;
use crate::theme::palette;
use crate::ui::alpha;
use crate::dialogs;
use gpui_kit::component::button::{Button, ButtonVariants};
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

/// Ancho de la barra lateral abierta.
pub const SIDEBAR_W: f32 = 60.;

/// Aplica la transparencia de la ventana (en %): con algo de transparencia se ve el escritorio
/// detrás, desenfocado; en 0 la ventana es opaca.
pub fn apply_translucency(t: u32, window: &mut Window) {
    window.set_background_appearance(if t == 0 { WindowBackgroundAppearance::Opaque } else { WindowBackgroundAppearance::Blurred });
    window.refresh();
}

static TRANSLUCENCY: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static BARS_ONLY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Guarda la transparencia elegida (la leen las vistas al dibujar).
fn remember_translucency(config: &sv_core::config::Config) -> bool {
    use std::sync::atomic::Ordering::SeqCst;
    let t = TRANSLUCENCY.swap(config.translucency, SeqCst) != config.translucency;
    let b = BARS_ONLY.swap(config.translucent_bars_only, SeqCst) != config.translucent_bars_only;
    t || b
}

/// Cuánto se ve el fondo de las barras (arriba y lateral): 1 = opaco.
pub fn opacity(_: &App) -> f32 {
    1. - TRANSLUCENCY.load(std::sync::atomic::Ordering::SeqCst) as f32 / 100.
}

/// Cuánto se ve el fondo del contenido (cuentas, páginas, bloqueo): 1 = opaco. Con "solo las
/// barras", el contenido queda opaco.
pub fn content_opacity() -> f32 {
    if BARS_ONLY.load(std::sync::atomic::Ordering::SeqCst) {
        1.
    } else {
        1. - TRANSLUCENCY.load(std::sync::atomic::Ordering::SeqCst) as f32 / 100.
    }
}

/// Fondo de las tarjetas (Herramientas, Ajustes): con la ventana translúcida, vidrio oscuro en vez
/// de un negro sólido que contrasta.
pub fn card_bg(p: &crate::theme::Palette) -> Hsla {
    let o = content_opacity();
    if o < 1. {
        alpha(p.surface, 0.42 + 0.4 * o)
    } else {
        p.bg
    }
}

pub struct AppView {
    vault: Entity<Vault>,
    screen: Screen,
    page: Page,
    tools: Option<Entity<ToolsPanel>>,
    settings: Option<Entity<SettingsPanel>>,
    _screen_sub: Option<Subscription>,
    _vault_sub: Subscription,
    /// Ícono de la bandeja: existe solo si al cerrar la app sigue en segundo plano.
    tray: Option<crate::desktop::Tray>,
}

impl AppView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        window.set_window_title("Smurf Vault");
        crate::desktop::remember_window(window);
        let vault = cx.new(|_| Vault::new());
        // Cerrar la ventana la deja en la bandeja (si está activado), salvo con "Salir".
        let close_vault = vault.clone();
        window.on_window_should_close(cx, move |_, cx| {
            if crate::desktop::quitting() || !close_vault.read(cx).store.config.close_to_tray {
                return true;
            }
            crate::desktop::hide_main_window();
            // La primera vez se avisa dónde quedó, para que no parezca que se cerró.
            close_vault.update(cx, |v, _| {
                if !v.store.config.tray_hint_shown {
                    crate::desktop::notify(
                        "Smurf Vault sigue abierto",
                        "Quedó en la bandeja del sistema. Clic derecho en el ícono para salir. Puedes cambiarlo en Ajustes.",
                    );
                    v.store.config.tray_hint_shown = true;
                    let _ = v.store.save_config();
                }
            });
            false
        });
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
                        // Si estás en otra ventana, también con un aviso de Windows.
                        if !window.is_window_active() {
                            crate::desktop::notify("Smurf Vault", "Partida aceptada ✓");
                        }
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
        let lock = cx.new(|cx| LockScreen::new(vault.clone(), window, cx));
        let t = vault.read(cx).store.config.translucency;
        remember_translucency(&vault.read(cx).store.config);
        apply_translucency(t, window);
        // Otras vistas piden ir a una página (un chip, la paleta…) a través del estado compartido.
        let vault_sub = cx.observe_in(&vault, window, |this: &mut Self, vault, window, cx| {
            if remember_translucency(&vault.read(cx).store.config) {
                window.refresh();
            }
            if let Some(page) = vault.update(cx, |v, _| v.go_to.take()) {
                this.set_page(page, window, cx);
            }
        });
        let mut this = Self {
            vault,
            screen: Screen::Lock(lock.clone()),
            page: Page::Accounts,
            tools: None,
            settings: None,
            _screen_sub: None,
            _vault_sub: vault_sub,
            tray: None,
        };
        // Actualizaciones desde GitHub (solo la versión instalada): al abrir y cada 6 horas.
        if sv_core::update::is_installed() {
            cx.spawn_in(window, async move |_, cx| loop {
                let found = cx.background_executor().spawn(async { sv_core::update::check_and_download() }).await;
                if let Ok(Some(update)) = found {
                    let version = update.version.clone();
                    let first = crate::desktop::pending_update_version().as_deref() != Some(version.as_str());
                    crate::desktop::set_pending_update(update);
                    let alive = cx.update(|window, cx| {
                        if first {
                            window.push_notification(Notification::success(format!("Hay una versión nueva (v{version}) lista para instalar")), cx);
                        }
                        window.refresh();
                    });
                    if alive.is_err() {
                        break;
                    }
                }
                cx.background_executor().timer(std::time::Duration::from_secs(6 * 60 * 60)).await;
            })
            .detach();
        }
        this.sync_tray(cx);
        // La bandeja: clic en el ícono o en su menú.
        cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor().timer(std::time::Duration::from_millis(150)).await;
            let alive = this.update_in(cx, |this, window, cx| {
                this.sync_tray(cx);
                let actions = this.tray.as_ref().map(|t| t.poll()).unwrap_or_default();
                for action in actions {
                    match action {
                        crate::desktop::TrayAction::Open => crate::desktop::show_main_window(),
                        crate::desktop::TrayAction::Lock => {
                            if matches!(this.screen, Screen::Main(_, _)) {
                                this.lock(window, cx);
                            }
                        }
                        crate::desktop::TrayAction::Quit => {
                            crate::desktop::set_quitting();
                            crate::desktop::install_pending_on_exit();
                            cx.quit();
                        }
                    }
                }
            });
            if alive.is_err() {
                break;
            }
        })
        .detach();
        this.watch_lock(&lock, window, cx);
        this
    }

    fn watch_lock(&mut self, lock: &Entity<LockScreen>, window: &mut Window, cx: &mut Context<Self>) {
        self._screen_sub = Some(cx.subscribe_in(lock, window, |this, _, ev: &LockEvent, window, cx| match ev {
            LockEvent::Unlocked => this.show_main(window, cx),
        }));
    }

    fn show_lock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // La barra se puede usar con un diálogo abierto: al bloquear no queda ninguno encima.
        window.close_all_dialogs(cx);
        let lock = cx.new(|cx| LockScreen::new(self.vault.clone(), window, cx));
        self.watch_lock(&lock, window, cx);
        self.screen = Screen::Lock(lock);
        // Las páginas tienen datos de la bóveda abierta: se descartan al bloquear.
        self.page = Page::Accounts;
        self.tools = None;
        self.settings = None;
        cx.notify();
    }

    /// Cambia de página (Herramientas y Ajustes se crean la primera vez que se abren).
    pub fn set_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.screen, Screen::Main(_, _)) {
            return;
        }
        window.close_all_dialogs(cx);
        match page {
            Page::Tools if self.tools.is_none() => {
                let vault = self.vault.clone();
                self.tools = Some(cx.new(|cx| ToolsPanel::new(vault, window, cx)));
            }
            Page::Settings if self.settings.is_none() => {
                let vault = self.vault.clone();
                self.settings = Some(cx.new(|cx| SettingsPanel::new(vault, window, cx)));
            }
            _ => {}
        }
        self.page = page;
        cx.notify();
    }

    /// El ícono de la bandeja existe solo si al cerrar la app sigue en segundo plano.
    fn sync_tray(&mut self, cx: &App) {
        let wanted = self.vault.read(cx).store.config.close_to_tray;
        if wanted && self.tray.is_none() {
            self.tray = crate::desktop::Tray::new().ok();
        } else if !wanted && self.tray.is_some() {
            self.tray = None;
        }
    }

    fn show_main(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self._screen_sub = None;
        let main = cx.new(|cx| MainScreen::new(self.vault.clone(), window, cx));
        let bg = cx.new(|_| crate::stage_bg::StageBg::new(main.downgrade()));
        main.update(cx, |m, _| m.bg = Some(bg.clone()));
        self.screen = Screen::Main(main, bg);
        cx.notify();
    }

    pub fn lock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.vault.update(cx, |v, cx| v.change(cx, |s| s.lock()));
        self.show_lock(window, cx);
        window.push_notification(Notification::info("Bóveda bloqueada"), cx);
    }

    fn top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let o = opacity(cx);
        let brand = h_flex()
            .gap(px(10.))
            .child(img(assets::logo()).size(px(30.)).rounded(px(8.)))
            .child(div().text_size(px(15.)).font_weight(FontWeight::BOLD).text_color(p.text).child("Smurf Vault"));
        // Actualización descargada: clic para reiniciar e instalarla (si no, se instala al cerrar).
        let update_chip = crate::desktop::pending_update_version().map(|version| {
            h_flex()
                .id("update-chip")
                .gap(px(6.))
                .h(px(28.))
                .px(px(11.))
                .rounded_full()
                .border_1()
                .border_color(alpha(p.gold, 0.4))
                .bg(alpha(p.gold, 0.12))
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(p.gold_hi)
                .cursor_pointer()
                .occlude()
                .hover(|s| s.bg(alpha(p.gold, 0.2)))
                .child(Icon::new(IconName::ArrowDown).size(px(13.)))
                .child(format!("Actualizar a v{version}"))
                .tooltip(|window, cx| {
                    Tooltip::new("Se descargó una versión nueva. Clic para reiniciar e instalarla (si no, se instala al cerrar la app).").build(window, cx)
                })
                .on_click(|_, window, cx| {
                    if let Err(e) = crate::desktop::install_update_now(cx) {
                        window.push_notification(Notification::error(e), cx);
                    }
                })
        });
        // Herramientas encendidas y el estado del cliente: un chip cada uno, que abre Herramientas.
        let config = &self.vault.read(cx).store.config;
        let tool_chip = |id: &'static str, icon: Option<&'static str>, label: String, tip: &'static str, color: Hsla, vault: Entity<Vault>| {
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
                // Sobre la zona para arrastrar la ventana: sin esto el clic mueve la ventana.
                .occlude()
                .map(|d| match icon {
                    Some(icon) => d.child(Icon::empty().path(assets::icon(icon)).size(px(14.))),
                    None => d.child(div().size(px(7.)).rounded_full().bg(color)),
                })
                .child(label)
                .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
                .on_click(move |_, window, cx| crate::tools_dialog::open_tools(vault.clone(), window, cx))
        };
        let auto_chip = config.auto_accept.then(|| {
            let label = if config.auto_accept_delay == 0 { "Autoaceptar".to_string() } else { format!("Autoaceptar · {} s", config.auto_accept_delay) };
            tool_chip("auto-chip", Some("zap"), label, "Autoaceptar partidas activado · clic para configurar", p.cyan, self.vault.clone())
        });
        let offline_chip = config.appear_offline.then(|| {
            tool_chip("offline-chip", Some("ghost"), "Desconectado".into(), "Apareces desconectado en el chat del LoL · clic para configurar", p.text_2, self.vault.clone())
        });
        let client_chip = (sv_core::tools::tools().client_status() == sv_core::tools::ClientStatus::Connected).then(|| {
            tool_chip("client-chip", None, "Cliente LoL conectado".into(), "El cliente de LoL está abierto y Smurf Vault lo detecta", p.ok, self.vault.clone())
        });
        TitleBar::new()
            .h(px(60.))
            .bg(alpha(p.surface, 0.9 * o))
            .border_color(p.border)
            .child(brand)
            .child(div().flex_1())
            .child(
                // Los controles de la barra toman el clic ellos mismos: si no, Windows trata la
                // presión como "arrastrar la ventana" y el clic se pierde (pasaba con un diálogo abierto).
                h_flex()
                    .id("bar-actions")
                    .occlude()
                    .gap(px(8.))
                    .pr(px(12.))
                    .children(update_chip)
                    .children(auto_chip)
                    .children(offline_chip)
                    .children(client_chip)
                    .child({
                        let vault = self.vault.clone();
                        Button::new("add-account")
                            .primary()
                            .icon(Icon::new(IconName::Plus))
                            .label("Nueva cuenta")
                            .tooltip("Agrega una cuenta; sus datos se pueden traer del cliente de LoL (Ctrl N)")
                            .on_click(move |_, window, cx| { window.close_all_dialogs(cx); dialogs::open_add(vault.clone(), window, cx) })
                    }),
            )
    }

    /// Barra lateral: páginas arriba; abajo el estado de Google Drive y Bloquear.
    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let o = opacity(cx);
        let config = self.vault.read(cx).store.config.clone();
        let active_tools = [config.auto_accept, config.appear_offline, config.remote_enabled].iter().filter(|on| **on).count();
        let item = |id: &'static str, icon: Icon, label: &'static str, active: bool, badge: Option<usize>| {
            div()
                .id(id)
                .mx_auto()
                .size(px(42.))
                .flex()
                .items_center()
                .justify_center()
                .relative()
                .rounded(px(10.))
                .cursor_pointer()
                .when(active, |d| d.bg(p.surface_2).border_1().border_color(p.border_strong))
                .when(!active, |d| d.hover(|s| s.bg(alpha(p.surface_2, 0.7))))
                .child(icon.size(px(20.)).text_color(if active { p.gold_hi } else { p.text_2 }))
                .when_some(badge.filter(|n| *n > 0), |d, n| {
                    d.child(
                        div()
                            .absolute()
                            .top(px(3.))
                            .right(px(3.))
                            .size(px(15.))
                            .rounded_full()
                            .bg(p.cyan)
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(9.5))
                            .font_weight(FontWeight::BOLD)
                            .text_color(p.on_cyan)
                            .child(n.to_string()),
                    )
                })
                .when(!label.is_empty(), |d| d.tooltip(move |window, cx| Tooltip::new(label).build(window, cx)))
        };
        let nav = |page: Page| cx.listener(move |this: &mut Self, _: &ClickEvent, window, cx| this.set_page(page, window, cx));
        use sv_core::drive::SyncState;
        let (drive_label, drive_dot, drive_tip) = match self.vault.read(cx).drive.sync_state() {
            SyncState::Local => ("Local", p.muted, "Tu bóveda solo está en este PC. Clic para vincular Google Drive.".to_string()),
            SyncState::Syncing => ("Subiendo", p.cyan, "Subiendo cambios a Google Drive".to_string()),
            SyncState::Ok(at) => {
                let when = chrono::DateTime::parse_from_rfc3339(&at)
                    .map(|t| format!(" · {}", t.with_timezone(&chrono::Local).format("%H:%M:%S")))
                    .unwrap_or_default();
                ("Drive", p.ok, format!("Guardado en Google Drive{when}"))
            }
            SyncState::Error(e) => ("Error", p.danger, e),
        };
        v_flex()
            .id("sidebar")
            .w(px(SIDEBAR_W))
            .flex_none()
            .h_full()
            .py(px(10.))
            .gap(px(6.))
            .border_r_1()
            .border_color(p.border)
            .bg(alpha(p.surface, 0.72 * o))
            .child(item("nav-accounts", Icon::empty().path(assets::icon("users")), "Cuentas", self.page == Page::Accounts, None).on_click(nav(Page::Accounts)))
            .child(
                item("nav-tools", Icon::empty().path(assets::icon("wrench")), "Herramientas", self.page == Page::Tools, Some(active_tools))
                    .on_click(nav(Page::Tools)),
            )
            .child(item("nav-settings", Icon::empty().path(assets::icon("settings")), "Ajustes", self.page == Page::Settings, None).on_click(nav(Page::Settings)))
            .child(div().flex_1())
            .child(
                item("nav-drive", Icon::empty().path(assets::icon(if drive_dot == p.muted { "cloud-off" } else { "cloud" })), "", false, None)
                .child(div().absolute().bottom(px(7.)).right(px(7.)).size(px(8.)).rounded_full().border_1().border_color(p.surface).bg(drive_dot))
                .tooltip(move |window, cx| Tooltip::new(format!("{drive_label} · {drive_tip}")).build(window, cx))
                .on_click(nav(Page::Settings)),
            )
            .child(
                item("nav-lock", Icon::empty().path(assets::icon("lock-keyhole")), "Bloquear la bóveda (Ctrl L)", false, None)
                    .on_click(cx.listener(|this, _, window, cx| this.lock(window, cx))),
            )
    }

    /// Atajos de la pantalla principal: Ctrl+L bloquea y Ctrl+N agrega una cuenta.
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
        #[cfg(debug_assertions)]
        if std::env::var("SV_DEV_FPS").is_ok() {
            eprintln!("app");
        }
        let p = palette(cx);
        let o = opacity(cx);
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
                let page: AnyElement = match self.page {
                    // El fondo va detrás y la pantalla encima, en caché: al mover el mouse solo se
                    // redibuja el fondo.
                    Page::Accounts => div()
                        .size_full()
                        .relative()
                        .child(bg)
                        .child(AnyView::from(main).cached(StyleRefinement::default().absolute().inset_0().size_full()))
                        .into_any_element(),
                    Page::Tools => self.tools.clone().map(|t| t.into_any_element()).unwrap_or_else(|| div().into_any_element()),
                    Page::Settings => self.settings.clone().map(|s| s.into_any_element()).unwrap_or_else(|| div().into_any_element()),
                };
                v_flex()
                    .relative()
                    .size_full()
                    .bg(alpha(p.bg, o))
                    .on_key_down(cx.listener(Self::on_key))
                    .child(self.top_bar(cx))
                    .child(
                        h_flex()
                            .flex_1()
                            .min_h_0()
                            .child(self.sidebar(cx))
                            // Con "solo las barras", el contenido tapa el escritorio con su fondo opaco.
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .relative()
                                    .when(o < 1. && content_opacity() >= 1., |d| d.bg(p.bg))
                                    .child(page),
                            ),
                    )
                    .when(dialog_open, |d| {
                        d.child(div().id("dialog-backdrop").absolute().top(px(60.)).left_0().right_0().bottom_0().bg(alpha(p.bg, 0.86 * o.max(0.5))).occlude())
                    })
                    .into_any_element()
            }
        }
    }
}
