//! Página de Ajustes (barra lateral), en tres grupos: Sincronización (Google Drive, API key de
//! Riot), Cuenta (contraseña maestra) y Personalización (ventana, transparencia y colores). Todo se
//! guarda al cambiarlo.
use crate::state::Vault;
use crate::theme::{palette, Palette};
use crate::ui::alpha;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState, SliderValue};
use gpui_kit::component::{h_flex, v_flex, Disableable, Icon, Sizable, WindowExt};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use sv_core::store::Store;
use sv_core::vault;

/// [clave, nombre, color predeterminado, dónde se ve] — igual que THEME_FIELDS de theme.js.
const THEME_FIELDS: [(&str, &str, u32, &str); 10] = [
    ("bg", "Fondo de la app", 0x080b10, "El fondo general de todas las pantallas"),
    ("surface", "Tarjetas y paneles", 0x0f141b, "Tarjetas de cuentas, rangos, diálogos y campos"),
    ("text", "Texto principal", 0xebe7de, "Nombres, títulos y la mayoría de los textos"),
    ("muted", "Texto de ayuda", 0x7d8796, "Textos chicos: explicaciones, fechas, etiquetas"),
    ("gold", "Botón Jugar y destacados", 0xd6b36a, "Botones principales (Jugar, Nueva cuenta), estrellas de favoritas y la cuenta seleccionada"),
    ("cyan", "Interruptores y enlaces", 0x1fd6d0, "Interruptores encendidos, enlaces, íconos de datos y el botón Iniciar sesión"),
    ("ok", "LP ganados y éxito", 0x3ccf8e, "LP ganados, barras de winrate sobre 50%, cuentas listas para Jugar y mensajes de éxito"),
    ("danger", "Errores", 0xf06a5f, "Mensajes de error y botones para eliminar"),
    ("win", "Victorias", 0x5b9cf2, "Victorias en el historial de LoL"),
    ("loss", "Derrotas y LP perdidos", 0xe84057, "Derrotas en el historial y LP perdidos"),
];

fn hex_of(c: Hsla) -> String {
    let r: Rgba = c.into();
    let b = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(r.r), b(r.g), b(r.b))
}

fn color_of(hex: &str) -> Option<Hsla> {
    let v = u32::from_str_radix(hex.strip_prefix('#')?, 16).ok()?;
    Some(rgba((v << 8) | 0xff).into())
}

pub struct SettingsPanel {
    vault: Entity<Vault>,
    pw_current: Entity<InputState>,
    pw_new: Entity<InputState>,
    pw_new2: Entity<InputState>,
    pw_error: Option<SharedString>,
    pw_busy: bool,
    colors: Vec<(&'static str, Entity<ColorPickerState>)>,
    translucency: Entity<SliderState>,
    _subs: Vec<Subscription>,
}

impl SettingsPanel {
    pub(crate) fn new(vault: Entity<Vault>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let masked = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).masked(true).placeholder(placeholder))
        };
        let pw_current = masked("Contraseña actual", window, cx);
        let pw_new = masked("Nueva (mínimo 8 caracteres)", window, cx);
        let pw_new2 = masked("Repite la nueva", window, cx);
        let theme = vault.read(cx).store.config.theme.clone();
        let mut subs = Vec::new();
        let colors: Vec<_> = THEME_FIELDS
            .iter()
            .map(|(k, _, default, _)| {
                let value = theme.get(*k).and_then(|h| color_of(h)).unwrap_or_else(|| rgba((default << 8) | 0xff).into());
                let state = cx.new(|cx| ColorPickerState::new(window, cx).default_value(value));
                let key = *k;
                subs.push(cx.subscribe_in(&state, window, move |this, _, ev: &ColorPickerEvent, window, cx| {
                    let ColorPickerEvent::Change(Some(c)) = ev else { return };
                    this.set_color(key, Some(hex_of(*c)), window, cx);
                }));
                (key, state)
            })
            .collect();
        let current = vault.read(cx).store.config.translucency;
        let translucency = cx.new(|_| SliderState::new().min(0.).max(80.).step(5.).default_value(current as f32));
        subs.push(cx.subscribe_in(&translucency, window, |this, _, ev: &SliderEvent, window, cx| {
            let SliderEvent::Change(SliderValue::Single(v)) = ev else { return };
            let t = v.round() as u32;
            this.vault.update(cx, |v, cx| {
                if v.store.config.translucency != t {
                    v.store.config.translucency = t;
                    let _ = v.store.save_config();
                    cx.notify();
                }
            });
            crate::app::apply_translucency(t, window);
            cx.notify();
        }));
        Self { vault, pw_current, pw_new, pw_new2, pw_error: None, pw_busy: false, colors, translucency, _subs: subs }
    }

    /// Cambia un color (None = el predeterminado): se ve al tiro y se guarda en este PC.
    fn set_color(&mut self, key: &str, hex: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let default = THEME_FIELDS.iter().find(|f| f.0 == key).map(|f| format!("#{:06x}", f.2));
        self.vault.update(cx, |v, _| {
            let theme = &mut v.store.config.theme;
            match hex.filter(|h| Some(h) != default.as_ref()) {
                Some(h) => theme.insert(key.to_string(), h),
                None => theme.remove(key),
            };
            v.store.config.theme = sv_core::config::Config::clean_theme(&v.store.config.theme);
            let _ = v.store.save_config();
        });
        let theme = self.vault.read(cx).store.config.theme.clone();
        crate::theme::Palette::from_theme(&theme).apply(cx);
        window.refresh();
        cx.notify();
    }

    fn reset_colors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for (key, default) in THEME_FIELDS.iter().map(|f| (f.0, f.2)) {
            self.set_color(key, None, window, cx);
            if let Some((_, state)) = self.colors.iter().find(|(k, _)| *k == key) {
                let c: Hsla = rgba((default << 8) | 0xff).into();
                state.update(cx, |s, cx| s.set_value(c, window, cx));
            }
        }
    }

    fn change_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pw_busy {
            return;
        }
        let v = |s: &Entity<InputState>, cx: &App| s.read(cx).value().to_string();
        let (current, next, next2) = (v(&self.pw_current, cx), v(&self.pw_new, cx), v(&self.pw_new2, cx));
        self.pw_error = None;
        if next != next2 {
            self.pw_error = Some("Las contraseñas nuevas no coinciden".into());
            cx.notify();
            return;
        }
        if let Err(e) = Store::validate_new_password(&current, &next) {
            self.pw_error = Some(e.into());
            cx.notify();
            return;
        }
        let Some((key, salt)) = self.vault.read(cx).store.current_key() else { return };
        self.pw_busy = true;
        cx.notify();
        // scrypt tarda: se comprueba la actual y se deriva la nueva fuera del hilo de la interfaz.
        let work = cx.background_spawn(async move {
            if !vault::check_password(&current, &salt, &key) {
                return Err("La contraseña actual no es correcta".to_string());
            }
            vault::create_key(&next)
        });
        cx.spawn_in(window, async move |this, cx| {
            let res = work.await;
            this.update_in(cx, |this, window, cx| {
                this.pw_busy = false;
                let res = res.and_then(|(key, salt)| this.vault.update(cx, |v, cx| v.change(cx, |s| s.rekey(key, salt))).map(|_| ()));
                match res {
                    Ok(()) => {
                        for s in [&this.pw_current, &this.pw_new, &this.pw_new2] {
                            s.update(cx, |s, cx| s.set_value("", window, cx));
                        }
                        window.push_notification(Notification::success("Contraseña maestra cambiada"), cx);
                    }
                    Err(e) => this.pw_error = Some(e.into()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Google Drive: estado, cargar credentials.json, vincular, subir ahora y desvincular.
    fn google_block(&self, cx: &mut Context<Self>) -> Div {
        let p = palette(cx);
        let g = self.vault.read(cx).drive.status();
        let text = if !g.has_credentials {
            "Para sincronizar con Drive, primero carga el credentials.json de Google (esta versión no lo trae incluido).".to_string()
        } else if g.linked {
            format!(
                "Tu bóveda cifrada se guarda en una carpeta oculta del Drive de {}. Cada cambio se sube solo.",
                g.email.clone().unwrap_or_else(|| "tu cuenta de Google".into())
            )
        } else {
            "Tu bóveda solo está en este PC. Vincula Google para tener un respaldo y usarla en otros PCs.".to_string()
        };
        let (badge_bg, badge_fg) = if g.linked { (alpha(p.ok, 0.14), p.ok) } else { (p.surface_3, p.text_2) };
        let refresh = cx.entity().downgrade();
        let done = move |_: &mut Window, cx: &mut App| {
            refresh.update(cx, |_, cx| cx.notify()).ok();
        };
        let vault = self.vault.clone();
        block(&p)
            .child(
                h_flex()
                    .child(section(Icon::empty().path(crate::assets::icon("cloud")), "Google Drive", &p))
                    .child(div().flex_1())
                    .child(
                        div()
                            .px(px(8.))
                            .py(px(2.))
                            .rounded(px(6.))
                            .bg(badge_bg)
                            .text_size(px(11.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(badge_fg)
                            .child(if g.linked { "Conectado" } else { "No conectado" }),
                    ),
            )
            .child(div().text_size(px(12.5)).text_color(p.text_2).child(text))
            .child(
                h_flex()
                    .gap(px(8.))
                    .when(!g.has_credentials, |d| {
                        let (vault, done) = (vault.clone(), done.clone());
                        d.child(Button::new("google-creds").primary().icon(Icon::empty().path(crate::assets::icon("upload"))).label("Cargar credentials.json").on_click(
                            move |_, window, cx| crate::google::import_credentials(vault.clone(), window, cx, done.clone()),
                        ))
                    })
                    .when(g.has_credentials && !g.linked, |d| {
                        let (vault, done) = (vault.clone(), done.clone());
                        d.child(
                            Button::new("google-link")
                                .primary()
                                .label("Vincular cuenta de Google")
                                .on_click(move |_, window, cx| crate::google::link(vault.clone(), window, cx, done.clone())),
                        )
                    })
                    .when(g.linked, |d| {
                        let (sync, unlink) = (vault.clone(), vault.clone());
                        let done = done.clone();
                        d.child(Button::new("google-sync").label("Subir ahora").on_click(move |_, window, cx| {
                            sync.update(cx, |v, cx| {
                                let _ = v.store.persist();
                                v.flush_upload();
                                cx.notify();
                            });
                            window.push_notification(Notification::info("Subiendo tu bóveda a Google Drive…"), cx);
                        }))
                        .child(Button::new("google-unlink").ghost().text_color(p.danger).label("Desvincular").on_click(move |_, window, cx| {
                            let drive = unlink.read(cx).drive.clone();
                            cx.background_spawn(async move { drive.unlink() }).detach();
                            window.push_notification(Notification::info("Google desvinculado. El archivo en Drive no se borra."), cx);
                            unlink.update(cx, |_, cx| cx.notify());
                            done(window, cx);
                        }))
                    }),
            )
    }

    /// Versión actual y actualizaciones: buscar a mano, el avance de la descarga y reiniciar para
    /// instalar (sin ventanas; la app se vuelve a abrir sola).
    fn update_box(&self, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::desktop::UpdateStatus;
        let p = palette(cx);
        let installed = sv_core::update::is_installed();
        let ready = crate::desktop::pending_update_version();
        let status = crate::desktop::update_status();
        let busy = matches!(status, UpdateStatus::Checking | UpdateStatus::Downloading(..));
        let line: Option<(SharedString, Hsla)> = match (&ready, &status) {
            (Some(v), _) => Some((format!("v{v} lista para instalar").into(), p.ok)),
            (_, UpdateStatus::Checking) => Some(("Buscando…".into(), p.muted)),
            (_, UpdateStatus::Downloading(v, Some(pct))) => Some((format!("Bajando v{v}… {pct}%").into(), p.cyan)),
            (_, UpdateStatus::Downloading(v, None)) => Some((format!("Bajando v{v}…").into(), p.cyan)),
            (_, UpdateStatus::UpToDate) => Some(("Estás al día".into(), p.ok)),
            (_, UpdateStatus::Failed(e)) => Some((e.clone().into(), p.danger)),
            (_, UpdateStatus::Idle) => None,
        };
        let button = if ready.is_some() {
            Button::new("update-install").primary().small().label("Reiniciar y actualizar").on_click(|_, window, cx| {
                if let Err(e) = crate::desktop::install_update_now(cx) {
                    window.push_notification(Notification::error(e), cx);
                }
            })
        } else {
            Button::new("update-check")
                .small()
                .label("Buscar actualizaciones")
                .loading(busy)
                .disabled(!installed || busy)
                .tooltip(if installed { "Busca y baja la versión nueva; después la instalas con un clic" } else { "Solo funciona en la versión instalada" })
                .on_click(cx.listener(|this, _, window, cx| this.check_updates(window, cx)))
        };
        v_flex()
            .items_end()
            .gap(px(4.))
            .child(
                h_flex()
                    .gap(px(10.))
                    .child(div().text_size(px(12.)).text_color(p.muted).child(format!("Smurf Vault v{}", env!("CARGO_PKG_VERSION"))))
                    .child(button),
            )
            .when_some(line, |d, (text, color)| d.child(div().max_w(px(320.)).text_size(px(12.)).text_color(color).child(text)))
    }

    /// Busca una actualización a mano; mientras baja, se redibuja para mostrar el avance.
    fn check_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let work = cx.background_spawn(async { crate::desktop::check_for_update() });
        // Mientras trabaja, se redibuja cada medio segundo para mostrar el avance.
        cx.spawn_in(window, async move |this, cx| {
            let mut work = std::pin::pin!(work);
            loop {
                let tick = cx.background_executor().timer(std::time::Duration::from_millis(500));
                match futures::future::select(work.as_mut(), std::pin::pin!(tick)).await {
                    futures::future::Either::Left((found, _)) => {
                        this.update_in(cx, |_, window, cx| {
                            if let Some(v) = found {
                                window.push_notification(Notification::success(format!("v{v} lista: reinicia para actualizar")), cx);
                            }
                            window.refresh();
                            cx.notify();
                        })
                        .ok();
                        break;
                    }
                    futures::future::Either::Right(_) => {
                        if this.update(cx, |_, cx| cx.notify()).is_err() {
                            break;
                        }
                    }
                }
            }
        })
        .detach();
        cx.notify();
    }

    /// API key de Riot: si hay una guardada y el botón para cambiarla.
    fn riot_key_block(&self, cx: &mut Context<Self>) -> Div {
        let p = palette(cx);
        let key = self.vault.read(cx).store.data().map(|d| d.settings.riot_api_key.clone()).unwrap_or_default();
        let masked = if key.len() > 10 { format!("RGAPI-••••••••-{}", &key[key.len() - 4..]) } else { "Sin key guardada".to_string() };
        let vault = self.vault.clone();
        block(&p)
            .child(section(Icon::empty().path(crate::assets::icon("key-round")), "API key de Riot", &p))
            .child(hint("Se usa para actualizar todas las cuentas sin abrir el juego. Queda cifrada dentro de tu bóveda.", &p))
            .child(
                h_flex()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .h(px(32.))
                            .px(px(12.))
                            .flex()
                            .items_center()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(p.border_strong)
                            .text_size(px(13.))
                            .text_color(if key.is_empty() { p.muted } else { p.text_2 })
                            .child(masked),
                    )
                    .child(
                        Button::new("set-riot-key")
                            .label(if key.is_empty() { "Agregar key" } else { "Cambiar key" })
                            .on_click(move |_, window, cx| crate::dialogs::open_riot_key(vault.clone(), None, false, window, cx)),
                    ),
            )
    }

    /// Transparencia de la ventana: se ve el escritorio detrás, desenfocado.
    fn appearance_block(&self, cx: &mut Context<Self>) -> Div {
        let p = palette(cx);
        let t = self.vault.read(cx).store.config.translucency;
        let bars_only = self.vault.read(cx).store.config.translucent_bars_only;
        block(&p)
            .child(section(Icon::empty().path(crate::assets::icon("image")), "Transparencia de la ventana", &p))
            .child(hint("Deja ver el escritorio detrás, desenfocado por Windows (el desenfoque es parejo en toda la ventana). En 0 la ventana es opaca.", &p))
            .child(
                h_flex()
                    .gap(px(12.))
                    .child(div().flex_1().text_size(px(13.)).text_color(p.text_2).child("Aplicar a"))
                    .child(crate::tabs::segmented(&p).mb(px(0.)).children([("Toda la ventana", false), ("Solo las barras", true)].map(|(label, bars)| {
                        crate::tabs::segment(SharedString::from(format!("transl-{bars}")), label, None, bars_only == bars, &p).on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.vault.update(cx, |v, cx| {
                                    v.store.config.translucent_bars_only = bars;
                                    let _ = v.store.save_config();
                                    cx.notify();
                                });
                                window.refresh();
                            },
                        ))
                    }))),
            )
            .child(
                h_flex()
                    .gap(px(14.))
                    .child(div().flex_1().child(Slider::new(&self.translucency)))
                    .child(div().w(px(44.)).text_size(px(13.)).text_color(p.text_2).child(if t == 0 { "Opaca".to_string() } else { format!("{t}%") })),
            )
    }

    /// Ventana: seguir en la bandeja al cerrar y abrir al iniciar Windows.
    fn window_block(&self, cx: &mut Context<Self>) -> Div {
        let p = palette(cx);
        let tray = self.vault.read(cx).store.config.close_to_tray;
        let at_login = crate::desktop::open_at_login();
        let row = |title: &'static str, hint_text: &'static str, toggle: gpui_kit::component::switch::Switch| {
            h_flex()
                .gap(px(12.))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(2.))
                        .child(div().text_size(px(13.5)).font_weight(FontWeight::SEMIBOLD).child(title))
                        .child(div().text_size(px(12.)).text_color(p.muted).child(hint_text)),
                )
                .child(toggle)
        };
        let vault = self.vault.clone();
        let entity = cx.entity().downgrade();
        block(&p)
            .child(section(Icon::empty().path(crate::assets::icon("panel-left")), "Ventana", &p))
            .child(row(
                "Al cerrar, seguir en la bandeja",
                "Queda junto al reloj y autoaceptar y la detección de cuentas siguen funcionando. Para salir del todo: clic derecho en el ícono → Salir.",
                gpui_kit::component::switch::Switch::new("set-tray").checked(tray).color(p.cyan).on_click(move |v: &bool, _, cx| {
                    let v = *v;
                    vault.update(cx, |vault, cx| {
                        vault.store.config.close_to_tray = v;
                        let _ = vault.store.save_config();
                        cx.notify();
                    });
                }),
            ))
            .child(row(
                "Abrir al iniciar Windows",
                if cfg!(debug_assertions) { "Solo funciona en la versión instalada." } else { "Parte en la bandeja, sin abrir la ventana." },
                gpui_kit::component::switch::Switch::new("set-login").checked(at_login).color(p.cyan).on_click(move |v: &bool, window, cx| {
                    if let Err(e) = crate::desktop::set_open_at_login(*v) {
                        window.push_notification(Notification::error(e), cx);
                    }
                    entity.update(cx, |_, cx| cx.notify()).ok();
                }),
            ))
    }
}

fn section(icon: Icon, title: &'static str, p: &Palette) -> Div {
    h_flex()
        .gap(px(8.))
        .child(icon.size(px(16.)).text_color(p.gold))
        .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child(title))
}

fn hint(text: &'static str, p: &Palette) -> Div {
    div().text_size(px(12.5)).text_color(p.muted).child(text)
}

fn block(p: &Palette) -> Div {
    // flex_none: dentro de la zona con scroll no se achica (si no, el scroll no llega al final).
    v_flex().flex_none().gap(px(10.)).p(px(14.)).rounded(px(12.)).border_1().border_color(p.border).bg(crate::app::card_bg(p))
}

impl Render for SettingsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let customized = !self.vault.read(cx).store.config.theme.is_empty();
        let swatches = self.colors.iter().map(|(key, state)| {
            let (_, label, _, tip) = THEME_FIELDS.iter().find(|f| f.0 == *key).copied().unwrap_or(("", "", 0, ""));
            h_flex()
                .id(SharedString::from(format!("swatch-{key}")))
                .w(px(230.))
                .gap(px(10.))
                .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tip).build(window, cx))
                .child(div().p(px(2.)).rounded(px(6.)).border_1().border_color(p.border_strong).child(ColorPicker::new(state).small()))
                .child(div().text_size(px(13.)).text_color(p.text_2).child(label))
        });
        let password = block(&p)
            .child(section(Icon::empty().path(crate::assets::icon("lock-keyhole")), "Contraseña maestra", &p))
            .child(hint("Para cambiarla necesitas la actual. La nueva tampoco se puede recuperar: si la olvidas, pierdes la bóveda.", &p))
            .child(
                h_flex()
                    .gap(px(8.))
                    .child(div().flex_1().child(Input::new(&self.pw_current)))
                    .child(div().flex_1().child(Input::new(&self.pw_new)))
                    .child(div().flex_1().child(Input::new(&self.pw_new2))),
            )
            .child(
                h_flex()
                    .gap(px(10.))
                    .child(
                        Button::new("pw-change")
                            .label("Cambiar contraseña")
                            .loading(self.pw_busy)
                            .on_click(cx.listener(|this, _, window, cx| this.change_password(window, cx))),
                    )
                    .when_some(self.pw_error.clone(), |d, e| d.child(div().text_size(px(13.)).text_color(p.danger).child(e))),
            );
        let colors = block(&p)
            .child(
                h_flex()
                    .child(section(Icon::empty().path(crate::assets::icon("sparkles")), "Colores", &p))
                    .child(div().flex_1())
                    .child(
                        Button::new("theme-reset")
                            .small()
                            .label("Restablecer colores")
                            .disabled(!customized)
                            .on_click(cx.listener(|this, _, window, cx| this.reset_colors(window, cx))),
                    ),
            )
            .child(hint("Cambia los colores a tu gusto: se ven al tiro y se guardan en este PC.", &p))
            .child(h_flex().flex_wrap().gap(px(10.)).children(swatches));
        let group = |text: &'static str| div().mt(px(10.)).text_size(px(11.5)).font_weight(FontWeight::SEMIBOLD).text_color(p.muted).child(text);
        v_flex()
            .id("settings-page")
            .size_full()
            .overflow_y_scrollbar()
            .child(
                v_flex()
                    .w_full()
                    .max_w(px(820.))
                    .px(px(30.))
                    .pt(px(24.))
                    .pb(px(30.))
                    .gap(px(12.))
                    .child(
                        h_flex()
                            .items_end()
                            .child(div().flex_1().text_size(px(26.)).font_weight(FontWeight::BOLD).text_color(p.text).child("Ajustes"))
                            .child(self.update_box(cx)),
                    )
                    .child(group("SINCRONIZACIÓN"))
                    .child(self.google_block(cx))
                    .child(self.riot_key_block(cx))
                    .child(group("CUENTA"))
                    .child(password)
                    .child(group("PERSONALIZACIÓN"))
                    .child(self.window_block(cx))
                    .child(self.appearance_block(cx))
                    .child(colors),
            )
    }
}

/// Abre la página de Ajustes.
pub fn open_settings(vault: Entity<Vault>, window: &mut Window, cx: &mut App) {
    window.close_all_dialogs(cx);
    vault.update(cx, |v, cx| v.go(crate::state::Page::Settings, cx));
}
