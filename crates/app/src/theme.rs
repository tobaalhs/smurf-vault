//! Colores de la app. Son los mismos 10 personalizables de la versión Electron (renderer/theme.js);
//! los tonos intermedios se calculan a partir de ellos igual que allá, y si un color no se cambió se
//! usan los valores exactos de styles.css.
use crate::ui::lighten;
use gpui_kit::component::theme::{Theme, ThemeMode, ThemeToken, ThemeTokens};
use gpui_kit::{linear_color_stop, linear_gradient, px, rgba, App, Hsla, Rgba};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub bg: Hsla,
    pub surface: Hsla,
    pub surface_2: Hsla,
    pub surface_3: Hsla,
    pub border: Hsla,
    pub border_strong: Hsla,
    pub text: Hsla,
    pub text_2: Hsla,
    pub muted: Hsla,
    pub gold: Hsla,
    pub gold_hi: Hsla,
    pub gold_lo: Hsla,
    pub on_accent: Hsla,
    pub cyan: Hsla,
    pub on_cyan: Hsla,
    pub ok: Hsla,
    pub danger: Hsla,
    pub win: Hsla,
    pub loss: Hsla,
}

fn parse(hex: &str) -> Option<Rgba> {
    let h = hex.strip_prefix('#')?;
    (h.len() == 6).then(|| u32::from_str_radix(h, 16).ok()).flatten().map(|v| rgba((v << 8) | 0xff))
}

fn hex(v: u32) -> Rgba {
    rgba((v << 8) | 0xff)
}

/// Como color-mix(in srgb, a p, b) de CSS.
fn mix(a: Rgba, p: f32, b: Rgba) -> Rgba {
    let m = |x: f32, y: f32| x * p + y * (1.0 - p);
    Rgba { r: m(a.r, b.r), g: m(a.g, b.g), b: m(a.b, b.b), a: m(a.a, b.a) }
}

fn with_alpha(c: Rgba, a: f32) -> Rgba {
    Rgba { a, ..c }
}

fn luminance(c: Rgba) -> f32 {
    let lin = |v: f32| if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// Texto legible encima de un color: oscuro sobre colores claros, blanco sobre oscuros.
fn readable_on(c: Rgba) -> Rgba {
    if luminance(c) > 0.35 {
        hex(0x15110a)
    } else {
        hex(0xffffff)
    }
}

impl Palette {
    pub fn from_theme(theme: &BTreeMap<String, String>) -> Self {
        let get = |k: &str, default: u32| theme.get(k).and_then(|v| parse(v)).unwrap_or(hex(default));
        let custom = |k: &str| theme.get(k).and_then(|v| parse(v)).is_some();
        let (bg, surface, text) = (get("bg", 0x080b10), get("surface", 0x0f141b), get("text", 0xebe7de));
        let (gold, cyan) = (get("gold", 0xd6b36a), get("cyan", 0x1fd6d0));
        let white = hex(0xffffff);
        let black = hex(0x000000);

        let (surface_2, surface_3) = if custom("surface") || custom("text") {
            (mix(surface, 0.94, text), mix(surface, 0.88, text))
        } else {
            (hex(0x151b24), hex(0x1b2330))
        };
        let (text_2, border, border_strong) = if custom("text") || custom("bg") {
            (mix(text, 0.76, bg), with_alpha(text, 0.08), with_alpha(text, 0.15))
        } else {
            (hex(0xb7b2a7), with_alpha(white, 0.07), with_alpha(white, 0.13))
        };
        let (gold_hi, gold_lo, on_accent) = if custom("gold") {
            (mix(gold, 0.72, white), mix(gold, 0.75, black), readable_on(gold))
        } else {
            (hex(0xecd294), hex(0xa9853f), hex(0x1a1408))
        };
        let on_cyan = if custom("cyan") { readable_on(cyan) } else { hex(0x04201f) };

        Self {
            bg: bg.into(),
            surface: surface.into(),
            surface_2: surface_2.into(),
            surface_3: surface_3.into(),
            border: border.into(),
            border_strong: border_strong.into(),
            text: text.into(),
            text_2: text_2.into(),
            muted: get("muted", 0x7d8796).into(),
            gold: gold.into(),
            gold_hi: gold_hi.into(),
            gold_lo: gold_lo.into(),
            on_accent: on_accent.into(),
            cyan: cyan.into(),
            on_cyan: on_cyan.into(),
            ok: get("ok", 0x3ccf8e).into(),
            danger: get("danger", 0xf06a5f).into(),
            win: get("win", 0x5b9cf2).into(),
            loss: get("loss", 0xe84057).into(),
        }
    }

    /// Aplica la paleta a los componentes (botones, campos, diálogos, menús...) y la guarda como global.
    pub fn apply(self, cx: &mut App) {
        Theme::change(ThemeMode::Dark, None, cx);
        let t = Theme::global_mut(cx);
        t.font_family = "Segoe UI".into();
        // Base de los rem de la librería: con 16 px los campos y botones miden 32 px (como los 34 de Electron).
        t.font_size = px(16.);
        t.radius = px(8.);
        t.radius_lg = px(12.);
        // Avisos: bajo la barra de arriba (60 px) y separados del borde, alineados con el borde de
        // arriba de los diálogos (que se abren 22 px bajo la barra).
        let m = &mut t.notification.margins;
        m.top = px(60. + 22.);
        m.right = px(22.);
        m.bottom = px(22.);
        m.left = px(22.);
        t.sheet.margin_top = px(60.);
        // Avisos abajo a la derecha (como en el diseño), máximo 3 a la vez.
        t.notification.placement = gpui_kit::Anchor::BottomRight;
        t.notification.max_items = 3;

        t.background = self.bg;
        t.foreground = self.text;
        t.border = self.border_strong;
        t.input = self.border_strong;
        t.ring = self.gold;
        t.caret = self.gold;
        t.muted = self.surface_2;
        t.muted_foreground = self.muted;
        t.accent = self.surface_3;
        t.accent_foreground = self.text;
        t.popover = self.surface_2;
        t.popover_foreground = self.text;
        t.secondary = self.surface_2;
        t.secondary_hover = self.surface_3;
        t.secondary_active = self.surface_3;
        t.secondary_foreground = self.text;
        t.primary = self.gold;
        t.primary_hover = self.gold_hi;
        t.primary_active = self.gold_lo;
        t.primary_foreground = self.on_accent;
        // Los botones tienen colores propios, aparte de los generales.
        t.button = self.surface_2;
        t.button_hover = self.surface_3;
        t.button_active = self.surface_3;
        t.button_foreground = self.text;
        t.button_primary = self.gold;
        t.button_primary_hover = self.gold_hi;
        t.button_primary_active = self.gold_lo;
        t.button_primary_foreground = self.on_accent;
        t.button_secondary = self.surface_2;
        t.button_secondary_hover = self.surface_3;
        t.button_secondary_active = self.surface_3;
        t.button_secondary_foreground = self.text;
        t.button_danger = self.danger;
        t.button_danger_hover = self.danger;
        t.button_danger_active = self.danger;
        t.button_danger_foreground = self.text;
        t.danger = self.danger;
        t.danger_foreground = self.text;
        t.success = self.ok;
        t.success_foreground = self.on_accent;
        t.selection = Hsla { a: 0.3, ..self.gold };
        t.overlay = Hsla { a: 0.72, ..self.bg };
        t.tab_active = self.surface_3;
        t.tab_active_foreground = self.text;
        t.tab_foreground = self.muted;
        t.switch_thumb = self.text;
        t.link = self.cyan;
        t.link_hover = self.cyan;
        t.link_active = self.cyan;
        t.scrollbar_thumb = self.border_strong;
        t.scrollbar_thumb_hover = self.muted;
        t.title_bar = self.surface;
        t.title_bar_border = self.border;
        t.switch = self.surface_3;
        // Los componentes leen tokens calculados a partir de los colores: se recalculan y se pasan
        // a la capa base (scrollbars, bordes de redimensionar).
        t.tokens = ThemeTokens::from(&t.colors);
        // Botón principal con el degradado dorado de Electron (más claro al pasar el mouse).
        let grad = |a: Hsla, b: Hsla| linear_gradient(180., linear_color_stop(a, 0.), linear_color_stop(b, 1.));
        t.tokens.button_primary = ThemeToken::new(self.gold, grad(self.gold_hi, self.gold_lo));
        t.tokens.button_primary_hover = ThemeToken::new(self.gold_hi, grad(lighten(self.gold_hi, 0.07), lighten(self.gold, 0.07)));
        t.tokens.button_primary_active = ThemeToken::new(self.gold, grad(self.gold, self.gold_lo));
        // La raíz de la librería pinta este fondo debajo de todo: va transparente y cada pantalla pinta
        // el suyo, así la ventana translúcida deja ver el escritorio.
        t.tokens.background = gpui_kit::transparent_black().into();
        Theme::sync_base(cx);
        cx.set_global(self);
    }
}

impl gpui_kit::Global for Palette {}

/// La paleta actual, para dibujar los colores propios de la app.
pub fn palette(cx: &App) -> Palette {
    *cx.global::<Palette>()
}
