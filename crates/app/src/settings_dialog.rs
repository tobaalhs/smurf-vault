//! Diálogo de Ajustes: contraseña maestra, colores y API key de Riot. Igual que #settingsDialog de la
//! versión Electron. Google Drive y las opciones de ventana (bandeja, abrir con Windows) llegan en
//! sus fases.
use crate::dialogs::{below_bar, dialog_title};
use crate::state::Vault;
use crate::theme::{palette, Palette};
use crate::ui::alpha;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{h_flex, v_flex, Disableable, Icon, IconName, Sizable, WindowExt};
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
    ("ok", "LP ganados y éxito", 0x3ccf8e, "LP ganados, barras de winrate sobre 50%, sesión guardada y mensajes de éxito"),
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
    riot_key: Entity<InputState>,
    colors: Vec<(&'static str, Entity<ColorPickerState>)>,
    _subs: Vec<Subscription>,
}

impl SettingsPanel {
    fn new(vault: Entity<Vault>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let masked = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).masked(true).placeholder(placeholder))
        };
        let pw_current = masked("Contraseña actual", window, cx);
        let pw_new = masked("Nueva (mínimo 8 caracteres)", window, cx);
        let pw_new2 = masked("Repite la nueva", window, cx);
        let key = vault.read(cx).store.data().map(|d| d.settings.riot_api_key.clone()).unwrap_or_default();
        let riot_key = cx.new(|cx| InputState::new(window, cx).masked(true).placeholder("RGAPI-…").default_value(key));
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
        Self { vault, pw_current, pw_new, pw_new2, pw_error: None, pw_busy: false, riot_key, colors, _subs: subs }
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

    /// Guarda la API key de Riot (cifrada en la bóveda).
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let key = self.riot_key.read(cx).value().trim().to_string();
        let mut patch = serde_json::Map::new();
        patch.insert("riotApiKey".into(), key.into());
        match self.vault.update(cx, |v, cx| v.change(cx, |s| s.save_settings(patch))) {
            Ok(()) => {
                window.push_notification(Notification::success("Ajustes guardados"), cx);
                true
            }
            Err(e) => {
                window.push_notification(Notification::error(e), cx);
                false
            }
        }
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
    v_flex().gap(px(10.)).p(px(14.)).rounded(px(12.)).border_1().border_color(p.border).bg(p.bg)
}

impl Render for SettingsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        let max_h = f32::from(window.viewport_size().height) - 260.;
        v_flex()
            .id("settings-scroll")
            .max_h(px(max_h.max(240.)))
            .overflow_y_scroll()
            .pr(px(4.))
            .gap(px(12.))
            .child(
                block(&p)
                    .child(section(Icon::empty().path(crate::assets::icon("cloud")), "Google Drive", &p))
                    .child(hint("La sincronización con Google Drive todavía no está en la versión Rust: llega en una próxima fase. Tu bóveda queda guardada en este PC.", &p)),
            )
            .child(
                block(&p)
                    .child(section(Icon::empty().path(crate::assets::icon("lock-keyhole")), "Contraseña maestra", &p))
                    .child(hint("Para cambiarla necesitas la actual. La nueva tampoco se puede recuperar: si la olvidas, pierdes la bóveda.", &p))
                    .child(Input::new(&self.pw_current))
                    .child(h_flex().gap(px(8.)).child(div().flex_1().child(Input::new(&self.pw_new))).child(div().flex_1().child(Input::new(&self.pw_new2))))
                    .child(
                        h_flex()
                            .gap(px(10.))
                            .child(
                                Button::new("pw-change")
                                    .primary()
                                    .label("Cambiar contraseña")
                                    .loading(self.pw_busy)
                                    .on_click(cx.listener(|this, _, window, cx| this.change_password(window, cx))),
                            )
                            .when_some(self.pw_error.clone(), |d, e| d.child(div().text_size(px(13.)).text_color(p.danger).child(e))),
                    ),
            )
            .child(
                block(&p)
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
                    .child(h_flex().flex_wrap().gap(px(10.)).children(swatches)),
            )
            .child(
                block(&p)
                    .child(section(Icon::empty().path(crate::assets::icon("key-round")), "API key de Riot", &p))
                    .child(hint("Se usa para \"Actualizar todas con la API de Riot\". Queda guardada cifrada dentro de tu bóveda.", &p))
                    .child(Input::new(&self.riot_key).mask_toggle()),
            )
            .child(div().h(px(1.)).bg(alpha(p.border, 0.)))
    }
}

pub fn open_settings(vault: Entity<Vault>, window: &mut Window, cx: &mut App) {
    let panel = cx.new(|cx| SettingsPanel::new(vault, window, cx));
    window.open_dialog(cx, move |dialog, _, cx| {
        let p = palette(cx);
        let save = panel.clone();
        below_bar(dialog).title(dialog_title("Ajustes")).w(px(560.)).child(panel.clone()).footer(
            h_flex()
                .w_full()
                .gap(px(8.))
                .child(div().text_size(px(12.)).text_color(p.muted).child(format!("Smurf Vault v{}", env!("CARGO_PKG_VERSION"))))
                .child(div().flex_1())
                .child(Button::new("settings-close").label("Cerrar").on_click(|_, window, cx| window.close_dialog(cx)))
                .child(Button::new("settings-save").primary().label("Guardar").on_click(move |_, window, cx| {
                    if save.update(cx, |s, cx| s.save(window, cx)) {
                        window.close_dialog(cx);
                    }
                })),
        )
    });
    let _ = IconName::Settings;
}
