//! Piezas y formatos que comparten las pantallas: colores por liga, "hace X", baneos, splash de
//! fondo, avatar con su marco y copiar al portapapeles.
use crate::theme::{palette, Palette};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::time::Duration;
use sv_core::local::History;
use sv_core::model::{Account, Rank};
use sv_core::ranks::top_rank;

pub const CDRAGON: &str = "https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/default/v1";

pub fn hex(v: u32) -> Hsla {
    rgb(v).into()
}

/// Color de cada liga (igual que TIER_COLOR de carousel.js).
pub fn tier_color(tier: Option<&str>) -> Hsla {
    hex(match tier.unwrap_or("") {
        "IRON" => 0xa39a95,
        "BRONZE" => 0xcd8d63,
        "SILVER" => 0xb7c3cf,
        "GOLD" => 0xe6bb58,
        "PLATINUM" => 0x4cc5b6,
        "EMERALD" => 0x3fd68c,
        "DIAMOND" => 0x8fa9ff,
        "MASTER" => 0xc985ff,
        "GRANDMASTER" => 0xff6b6b,
        "CHALLENGER" => 0xf5d97e,
        _ => 0x5b6573,
    })
}

pub fn account_tier_color(a: &Account) -> Hsla {
    tier_color(top_rank(a).map(|r| r.tier.as_str()))
}

pub fn rank_emblem(r: &Rank) -> SharedString {
    format!("images/ranks/{}.png", r.tier.to_lowercase()).into()
}

pub fn profile_icon_url(id: i64) -> SharedString {
    format!("{CDRAGON}/profile-icons/{id}.jpg").into()
}

pub fn champion_icon_url(id: i64) -> SharedString {
    format!("{CDRAGON}/champion-icons/{id}.png").into()
}

pub fn alpha(c: Hsla, a: f32) -> Hsla {
    Hsla { a, ..c }
}

fn parse_iso(iso: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(iso).ok().map(|d| d.with_timezone(&chrono::Utc))
}

/// "hace 5 min", "hace 3 h", "hace 2 días"…
pub fn ago(iso: Option<&str>) -> String {
    let Some(at) = iso.and_then(parse_iso) else { return "nunca".into() };
    let m = (chrono::Utc::now() - at).num_seconds() as f64 / 60.0;
    let m = m.round() as i64;
    if m < 1 {
        return "recién".into();
    }
    if m < 60 {
        return format!("hace {m} min");
    }
    let h = (m as f64 / 60.0).round() as i64;
    if h < 48 {
        return format!("hace {h} h");
    }
    let d = (h as f64 / 24.0).round() as i64;
    if d < 60 {
        return format!("hace {d} días");
    }
    format!("hace {} meses", (d as f64 / 30.0).round() as i64)
}

/// Tiempo que falta hasta `iso`, p. ej. "2 días 5 h". Vacío si ya pasó.
pub fn until_text(iso: &str) -> String {
    let Some(at) = parse_iso(iso) else { return String::new() };
    let mut min = ((at - chrono::Utc::now()).num_seconds() as f64 / 60.0).round() as i64;
    if min <= 0 {
        return String::new();
    }
    let d = min / 1440;
    min -= d * 1440;
    let h = min / 60;
    let m = min - h * 60;
    if d > 0 {
        let days = if d == 1 { "día" } else { "días" };
        return if h > 0 { format!("{d} {days} {h} h") } else { format!("{d} {days}") };
    }
    if h > 0 {
        return if m > 0 { format!("{h} h {m} min") } else { format!("{h} h") };
    }
    format!("{m} min")
}

/// Baneo de LoL de la cuenta: (texto del chip, texto corto, detalle). None si no hay o ya terminó.
pub fn ban_info(a: &Account) -> Option<(String, String, String)> {
    let ban = a.ban.as_ref()?;
    if ban["permanent"].as_bool() == Some(true) {
        return Some(("Baneada permanentemente".into(), "Baneada".into(), "Cuenta con baneo permanente de LoL".into()));
    }
    let until = ban["until"].as_str()?;
    let left = until_text(until);
    if left.is_empty() {
        return None;
    }
    let when = parse_iso(until).map(|d| d.with_timezone(&chrono::Local).format("%d-%m-%Y %H:%M").to_string());
    Some((format!("Baneada · {left}"), left, format!("Termina {}", when.unwrap_or_default())))
}

/// El campeón que más aparece en las últimas partidas de LoL.
pub fn most_played_champion(history: Option<&History>, id: &str) -> Option<i64> {
    let matches = history?.get(id)?;
    let mut count: Vec<(i64, usize)> = Vec::new();
    for c in matches.iter().filter(|m| m.mode == "lol").filter_map(|m| m.champ) {
        match count.iter_mut().find(|(id, _)| *id == c) {
            Some((_, n)) => *n += 1,
            None => count.push((c, 1)),
        }
    }
    count.into_iter().max_by_key(|(_, n)| *n).map(|(c, _)| c)
}

/// Splash del fondo de perfil; si no lo conocemos, el del campeón más jugado. (url, leyenda)
pub fn splash_for(a: &Account, history: Option<&History>, champ_name: impl Fn(i64) -> String) -> (Option<SharedString>, String) {
    if let Some(skin) = a.background_skin_id.filter(|s| *s > 0) {
        let name = champ_name(skin / 1000);
        let caption = if name.is_empty() { "Fondo de perfil".to_string() } else { format!("Fondo de perfil · {name}") };
        return (Some(crate::state::splash_url(skin).into()), caption);
    }
    if let Some(champ) = most_played_champion(history, &a.id) {
        let url = format!("https://cdn.communitydragon.org/latest/champion/{champ}/splash-art");
        return (Some(url.into()), format!("Más jugado · {}", champ_name(champ)));
    }
    (None, String::new())
}

/// Ícono de perfil redondo con el borde del color de su liga y el nivel abajo.
pub fn avatar(a: &Account, size: f32, cx: &App) -> Div {
    let p = palette(cx);
    let initial: String = a.game_name.chars().next().or_else(|| a.username.chars().next()).unwrap_or('?').to_uppercase().collect();
    let border = account_tier_color(a);
    let circle = div()
        .size(px(size))
        .rounded_full()
        .border(px(if size > 60. { 3. } else { 2. }))
        .border_color(border)
        .bg(p.surface_3)
        .overflow_hidden()
        .flex()
        .items_center()
        .justify_center()
        .text_color(p.text_2)
        .font_weight(FontWeight::BOLD)
        .text_size(px(size * 0.33));
    let circle = match a.icon_id {
        Some(id) => circle.child(img(profile_icon_url(id)).size_full().object_fit(ObjectFit::Cover)),
        None => circle.child(initial),
    };
    div().relative().size(px(size)).flex_none().child(circle).when_some(a.level, |d, lvl| {
        d.child(
            div().absolute().left_0().right_0().bottom(px(if size > 60. { -9. } else { -5. })).flex().justify_center().child(
                div()
                    .px(px(if size > 60. { 9. } else { 6. }))
                    .rounded_full()
                    .bg(p.bg)
                    .border_1()
                    .border_color(p.border_strong)
                    .text_size(px(if size > 60. { 12. } else { 10. }))
                    .font_weight(FontWeight::BOLD)
                    .text_color(p.text_2)
                    .child(lvl.to_string()),
            ),
        )
    })
}

/// Marco del ícono del detalle: el de nivel si la cuenta eligió ese en el cliente; si no, las alas
/// de su mejor rango. Devuelve (imagen, posición y tamaño respecto del avatar de 92 px, espacio extra arriba).
pub fn crest_frame(a: &Account) -> Option<(SharedString, Bounds<Pixels>, f32)> {
    const AV: f32 = 92.0;
    if a.crest.as_ref().and_then(|c| c["type"].as_str()) == Some("level") {
        let theme = a.crest.as_ref()?["theme"].as_i64()?;
        // Imagen cuadrada con el anillo un poco arriba del centro (CSS: 268%, -46.58%).
        let w = AV * 2.68;
        let bounds = Bounds::new(point(px(AV / 2. - w / 2.), px(AV / 2. - w * 0.4658)), size(px(w), px(w)));
        let space = if theme >= 13 { 44. } else { 16. };
        return Some((format!("images/crests/level-{theme}.webp").into(), bounds, space));
    }
    let r = top_rank(a)?;
    let tier = r.tier.to_lowercase();
    // Alas de 520x640 (CSS: 285.2% de ancho, subidas un 66.6% de su alto).
    let w = AV * 2.852;
    let h = w * 640. / 520.;
    let bounds = Bounds::new(point(px(AV / 2. - w / 2.), px(AV / 2. - h * 0.666)), size(px(w), px(h)));
    let space = match tier.as_str() {
        "gold" | "platinum" | "emerald" => 22.,
        "diamond" => 48.,
        "master" => 64.,
        "grandmaster" => 92.,
        "challenger" => 120.,
        _ => 10.,
    };
    Some((format!("images/wings/{tier}.webp").into(), bounds, space))
}

/// Chip chico de texto (servidor, etiqueta, sesión guardada…).
pub fn chip(text: impl Into<SharedString>, fg: Hsla, bg: Hsla, border: Hsla) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(5.))
        .px(px(9.))
        .py(px(3.))
        .rounded_full()
        .bg(bg)
        .border_1()
        .border_color(border)
        .text_size(px(11.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(fg)
        .child(text.into())
}

/// Copia un secreto y lo borra del portapapeles a los 30 s si nadie copió otra cosa.
pub fn copy_secret(value: String, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(value.clone()));
    cx.spawn(async move |cx| {
        cx.background_executor().timer(Duration::from_secs(30)).await;
        cx.update(|cx| {
            if cx.read_from_clipboard().and_then(|c| c.text()).as_deref() == Some(value.as_str()) {
                cx.write_to_clipboard(ClipboardItem::new_string(String::new()));
            }
        });
    })
    .detach();
}

pub fn section_label(text: &'static str, p: &Palette) -> Div {
    div().text_size(px(11.)).font_weight(FontWeight::SEMIBOLD).text_color(p.muted).child(text.to_uppercase())
}
