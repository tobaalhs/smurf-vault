//! Pestañas del detalle: Historial (partidas de LoL y TFT con su resumen), Progreso (gráfico
//! escalonado de LP con crosshair) y Campeones (maestrías). Igual que carousel.js, con las medidas
//! de styles.css.
use crate::detail::{caps, result_color};
use crate::main_screen::MainScreen;
use crate::theme::{palette, Palette};
use crate::ui::{self, alpha, hex};
use gpui_kit::component::{h_flex, v_flex, Icon, IconName};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::collections::BTreeMap;
use sv_core::gamedata::GameData;
use sv_core::local::LpPoint;
use sv_core::matches::Match;
use sv_core::model::Account;
use sv_core::ranks::{lp_value, rank_label, tier_es, TIERS};

fn queue_name(m: &Match) -> String {
    let q = m.queue_id.unwrap_or(-1);
    if m.mode == "tft" {
        return match q {
            1090 => "TFT Normal",
            1100 => "TFT Clasificatoria",
            1130 => "TFT Hiper Rápida",
            1160 => "TFT Doble",
            _ => "TFT",
        }
        .into();
    }
    match q {
        0 => "Personalizada",
        400 | 430 => "Normal",
        420 => "Solo/Dúo",
        440 => "Flex",
        450 | 2400 => "ARAM",
        490 => "Rápida",
        700 => "Clash",
        830 | 840 | 850 | 870 | 880 | 890 => "Coop vs IA",
        900 | 1900 => "URF",
        1020 => "Uno para todos",
        1300 => "Nexus Blitz",
        1700 | 1710 => "Arena",
        _ => "Partida",
    }
    .into()
}

fn result_text(m: &Match) -> String {
    if m.is_remake() {
        return "Remake".into();
    }
    if m.mode == "tft" {
        return match m.placement {
            Some(1) => "¡Primer lugar!".into(),
            Some(p) => format!("Puesto {p}"),
            None => "Puesto ?".into(),
        };
    }
    if m.win { "Victoria" } else { "Derrota" }.into()
}

/// 512340 -> "512.340" (como toLocaleString en español).
fn thousands(n: i64) -> String {
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push('.');
        }
        out.push(c);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

/// Fecha local como 15/11/2026 (toLocaleDateString en español).
fn local_date(iso: &str, with_time: bool) -> String {
    chrono::DateTime::parse_from_rfc3339(iso)
        .map(|d| d.with_timezone(&chrono::Local).format(if with_time { "%d/%m/%Y, %H:%M" } else { "%d/%m/%Y" }).to_string())
        .unwrap_or_default()
}

/// LP que dio una partida clasificatoria: el cambio del registro de LP anotado justo después de
/// terminarla (la app revisa el rango a los pocos segundos de cada partida). None si no se sabe.
fn match_lp(m: &Match, lp: &BTreeMap<String, Vec<LpPoint>>) -> Option<i64> {
    let queue = match m.queue_id? {
        420 => "solo",
        440 => "flex",
        1100 => "tft",
        _ => return None,
    };
    if m.is_remake() {
        return None;
    }
    let list = lp.get(queue)?;
    let end = chrono::DateTime::parse_from_rfc3339(&m.at).ok()?.timestamp_millis();
    let i = list.iter().position(|p| {
        chrono::DateTime::parse_from_rfc3339(&p.at)
            .map(|t| {
                let t = t.timestamp_millis();
                t >= end - 60_000 && t <= end + 45 * 60_000
            })
            .unwrap_or(false)
    })?;
    (i > 0).then(|| lp_value(&list[i]) - lp_value(&list[i - 1])).filter(|d| *d != 0)
}

fn lp_chip(diff: Option<i64>, p: &Palette) -> Option<Div> {
    let d = diff?;
    Some(div().ml(px(6.)).text_size(px(11.5)).font_weight(FontWeight::BOLD).text_color(if d > 0 { p.ok } else { p.loss }).child(format!(
        "{}{} LP",
        if d > 0 { "+" } else { "−" },
        d.abs()
    )))
}

/// Un número grande con su leyenda abajo (sin recuadro, como .h-stat).
fn stat(value: impl Into<SharedString>, label: impl Into<SharedString>, color: Hsla, size: f32, p: &Palette) -> Div {
    v_flex()
        .child(div().text_size(px(size)).font_weight(FontWeight::BOLD).text_color(color).child(value.into()))
        .child(div().text_size(px(11.5)).text_color(p.muted).child(label.into()))
}

/// Selector segmentado (LoL / TFT, Solo/Dúo / Flex / TFT).
fn segmented(p: &Palette) -> Div {
    h_flex().gap(px(2.)).p(px(2.)).mb(px(14.)).rounded(px(9.)).bg(alpha(p.surface, 0.7)).border_1().border_color(p.border_strong)
}

fn segment(id: SharedString, label: &str, count: Option<usize>, on: bool, p: &Palette) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(6.))
        .h(px(28.))
        .px(px(12.))
        .rounded(px(7.))
        .cursor_pointer()
        .text_size(px(12.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(if on { p.text } else { p.muted })
        .when(on, |d| d.bg(p.surface_3))
        .hover(|s| s.text_color(p.text))
        .child(label.to_string())
        .when_some(count, |d, n| d.child(div().px(px(6.)).rounded_full().bg(p.surface_3).text_size(px(11.)).text_color(p.text_2).child(n.to_string())))
}

fn empty_tab(icon: Icon, lines: [&'static str; 2], p: &Palette) -> Div {
    h_flex()
        .gap(px(14.))
        .py(px(18.))
        .px(px(4.))
        .text_color(p.muted)
        .child(icon.size(px(28.)).text_color(p.muted))
        .child(v_flex().gap(px(3.)).text_size(px(13.)).child(lines[0]).child(lines[1]))
}

fn match_row(m: &Match, lp: &BTreeMap<String, Vec<LpPoint>>, game: &GameData, p: &Palette) -> Div {
    let color = result_color(m, p);
    let mins = (m.dur as f64 / 60.).round() as i64;
    let res = v_flex()
        .w(px(120.))
        .flex_none()
        .child(h_flex().child(div().text_size(px(13.5)).font_weight(FontWeight::BOLD).text_color(color).child(result_text(m))).children(lp_chip(match_lp(m, lp), p)))
        .child(div().text_size(px(11.5)).text_color(p.muted).child(format!("{} · {mins} min", queue_name(m))));
    let when = div().flex_none().text_right().text_size(px(12.)).text_color(p.muted).child(ui::ago(Some(&m.at)));
    let row = h_flex()
        .gap(px(12.))
        .max_w(px(860.))
        .pl(px(7.))
        .pr(px(12.))
        .py(px(7.))
        .rounded(px(10.))
        .bg(alpha(p.surface, 0.76))
        .border_1()
        .border_l(px(3.))
        .border_color(p.border)
        .child(div().absolute().left_0().top_0().bottom_0().w(px(3.)).rounded_l(px(10.)).bg(color));
    let tile = |d: Div| d.size(px(42.)).flex_none().rounded(px(8.)).overflow_hidden();
    if m.mode == "tft" {
        let legend = m.companion.and_then(|c| game.companion_url(c));
        let number = div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(19.))
            .font_weight(FontWeight::EXTRA_BOLD)
            .text_color(if legend.is_some() { hex(0xffffff) } else { color })
            .child(m.placement.map(|x| x.to_string()).unwrap_or_else(|| "?".into()));
        let place = tile(div().relative().bg(p.surface_3))
            .when_some(legend, |d, url| d.child(img(url).absolute().inset_0().size_full().object_fit(ObjectFit::Cover).opacity(0.5)).child(div().absolute().inset_0().bg(alpha(color, 0.45))))
            .child(number);
        return row
            .relative()
            .child(place)
            .child(res)
            .child(
                v_flex().w(px(110.)).flex_none().when_some(m.lvl, |d, l| {
                    d.child(div().text_size(px(13.5)).font_weight(FontWeight::BOLD).text_color(p.text).child(format!("Nivel {l}")))
                        .child(div().text_size(px(11.5)).text_color(p.muted).child("al terminar"))
                }),
            )
            .child(div().flex_1())
            .child(when);
    }
    let (k, d, a) = (m.k.unwrap_or(0), m.d.unwrap_or(0), m.a.unwrap_or(0));
    let ratio = if d > 0 { format!("{:.1}", (k + a) as f64 / d as f64) } else { "Perfecto".into() };
    let items = m.items.clone().unwrap_or_default();
    let n = items.len();
    let items = h_flex().gap(px(3.)).flex_1().min_w_0().overflow_hidden().children(items.into_iter().enumerate().map(|(i, id)| {
        // El último es el trinket: redondo y un poco separado.
        let trinket = i + 1 == n;
        let slot = div().size(px(26.)).flex_none().overflow_hidden().bg(hsla(0., 0., 1., 0.05)).when(trinket, |d| d.ml(px(4.))).map(|d| if trinket { d.rounded_full() } else { d.rounded(px(5.)) });
        match game.item_url(id) {
            Some(url) => slot.child(img(url).size_full()),
            None => slot,
        }
    }));
    row.relative()
        .child(match m.champ {
            Some(c) => tile(div()).child(img(ui::champion_icon_url(c)).size_full()).into_any_element(),
            None => tile(div().bg(p.surface_3)).into_any_element(),
        })
        .child(res)
        .child(
            v_flex()
                .w(px(110.))
                .flex_none()
                .child(div().text_size(px(13.5)).font_weight(FontWeight::BOLD).text_color(p.text).child(format!("{k} / {d} / {a}")))
                .child(div().text_size(px(11.5)).text_color(p.muted).child(format!("{ratio} KDA · {} CS", m.cs.unwrap_or(0)))),
        )
        .child(items)
        .child(when)
}

fn lol_summary(matches: &[&Match], p: &Palette) -> Option<Div> {
    let valid: Vec<&&Match> = matches.iter().filter(|m| !m.is_remake()).collect();
    let first = valid.first()?;
    let wins = valid.iter().filter(|m| m.win).count();
    let wr = (wins as f64 / valid.len() as f64 * 100.).round();
    let streak = valid.iter().take_while(|m| m.win == first.win).count();
    let mut champs: Vec<(i64, usize, usize)> = Vec::new();
    for m in &valid {
        let Some(c) = m.champ else { continue };
        match champs.iter_mut().find(|(id, _, _)| *id == c) {
            Some((_, g, w)) => {
                *g += 1;
                *w += m.win as usize;
            }
            None => champs.push((c, 1, m.win as usize)),
        }
    }
    champs.sort_by(|a, b| b.1.cmp(&a.1));
    Some(
        h_flex()
            .gap(px(22.))
            .mb(px(12.))
            .flex_wrap()
            .child(stat(format!("{wins}V {}D", valid.len() - wins), format!("Últimas {} · {wr}%", valid.len()), p.text, 16., p))
            .when(streak >= 2, |d| {
                d.child(stat(
                    format!("{streak} {}", if first.win { "victorias" } else { "derrotas" }),
                    "seguidas",
                    if first.win { p.win } else { p.loss },
                    16.,
                    p,
                ))
            })
            .child(div().flex_1())
            .child(h_flex().gap(px(14.)).children(champs.into_iter().take(3).map(|(id, games, w)| {
                h_flex()
                    .gap(px(8.))
                    .text_size(px(12.))
                    .text_color(p.text_2)
                    .child(img(ui::champion_icon_url(id)).size(px(30.)).rounded_full().border_1().border_color(p.border_strong))
                    .child(
                        v_flex()
                            .child(h_flex().gap(px(4.)).child(div().font_weight(FontWeight::BOLD).text_color(p.text).child(games.to_string())).child(if games == 1 {
                                "partida"
                            } else {
                                "partidas"
                            }))
                            .child(div().text_size(px(11.)).text_color(p.muted).child(format!("{}% victorias", (w as f64 / games as f64 * 100.).round()))),
                    )
            }))),
    )
}

fn tft_summary(matches: &[&Match], p: &Palette) -> Option<Div> {
    let placed: Vec<i64> = matches.iter().filter_map(|m| m.placement).collect();
    if placed.is_empty() {
        return None;
    }
    let n = placed.len() as f64;
    let avg = placed.iter().sum::<i64>() as f64 / n;
    let top4 = placed.iter().filter(|x| **x <= 4).count();
    let firsts = placed.iter().filter(|x| **x == 1).count();
    let dist = h_flex().gap(px(4.)).items_end().h(px(52.)).children((1..=8).map(|place| {
        let count = placed.iter().filter(|x| **x == place).count();
        let color = if place <= 4 { result_color(&Match { placement: Some(place), mode: "tft".into(), ..empty_match() }, p) } else { p.muted };
        v_flex()
            .items_center()
            .gap(px(2.))
            .child(div().w(px(14.)).h(px((count as f32 / n as f32) * 36. + 2.)).rounded_t(px(3.)).bg(color))
            .child(div().text_size(px(10.)).text_color(p.muted).child(place.to_string()))
    }));
    Some(
        h_flex()
            .gap(px(22.))
            .mb(px(12.))
            .flex_wrap()
            .child(stat(format!("{avg:.1}"), format!("Puesto promedio · últimas {}", placed.len()), p.text, 16., p))
            .child(stat(format!("{}%", (top4 as f64 / n * 100.).round()), format!("Top 4 ({top4})"), p.text, 16., p))
            .when(firsts > 0, |d| d.child(stat(firsts.to_string(), if firsts == 1 { "Primer lugar" } else { "Primeros lugares" }, hex(0xf0c75e), 16., p)))
            .child(dist),
    )
}

fn empty_match() -> Match {
    serde_json::from_value(serde_json::json!({ "id": "", "mode": "", "at": "" })).unwrap()
}

/// Nombre corto de una línea guía del eje: la liga/división que empieza ahí.
fn tick_label(v: i64) -> String {
    if v >= 2800 {
        return if v == 2800 { "Maestro".into() } else { format!("{} LP", v - 2800) };
    }
    let tier = TIERS[(v / 400).clamp(0, 9) as usize];
    let div = ["IV", "III", "II", "I"][((v % 400) / 100) as usize];
    format!("{} {div}", tier_es(tier))
}

fn ms(iso: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(iso).map(|t| t.timestamp_millis()).unwrap_or(0)
}

const CHART_H: f32 = 200.;
const PAD_L: f32 = 92.;
const PAD_R: f32 = 58.;
const PAD_T: f32 = 14.;
const PAD_B: f32 = 24.;

/// Escalas del gráfico para un ancho dado.
struct ChartGeom {
    w: f32,
    t0: i64,
    t1: i64,
    lo: f64,
    hi: f64,
}

impl ChartGeom {
    fn new(list: &[LpPoint], w: f32, now: i64) -> Self {
        let t0 = ms(&list[0].at);
        let t1 = now.max(t0 + 60_000);
        let vals: Vec<i64> = list.iter().map(lp_value).collect();
        let (lo, hi) = (*vals.iter().min().unwrap() as f64, *vals.iter().max().unwrap() as f64);
        let span = (hi - lo).max(100.);
        Self { w, t0, t1, lo: (lo - span * 0.25).max(0.), hi: hi + span * 0.25 }
    }
    fn x(&self, t: i64) -> f32 {
        PAD_L + ((t - self.t0) as f64 / (self.t1 - self.t0) as f64) as f32 * (self.w - PAD_L - PAD_R)
    }
    fn y(&self, v: f64) -> f32 {
        PAD_T + (1. - ((v - self.lo) / (self.hi - self.lo)) as f32) * (CHART_H - PAD_T - PAD_B)
    }
    /// Líneas guía en los cambios de liga (o de división si el rango es chico).
    fn ticks(&self) -> Vec<i64> {
        let step = if self.hi - self.lo > 700. { 400 } else { 100 };
        let mut out: Vec<i64> = Vec::new();
        let mut v = ((self.lo / step as f64).ceil() as i64) * step;
        while (v as f64) <= self.hi {
            out.push(v);
            v += step;
        }
        if self.lo < 2800. && self.hi > 2800. && !out.contains(&2800) {
            out.push(2800);
        }
        out
    }
}

fn paint_text(text: String, origin: Point<Pixels>, size: f32, color: Hsla, bold: bool, right: Option<f32>, window: &mut Window, cx: &mut App) {
    let mut f = font("Segoe UI");
    if bold {
        f.weight = FontWeight::BOLD;
    }
    let run = TextRun { len: text.len(), font: f, color, background_color: None, underline: None, strikethrough: None };
    let line = window.text_system().shape_line(text.into(), px(size), &[run], None);
    let (origin, align, width) = match right {
        Some(w) => (point(origin.x - px(w), origin.y), TextAlign::Right, Some(px(w))),
        None => (origin, TextAlign::Left, None),
    };
    let _ = line.paint(origin, px(size * 1.3), align, width, window, cx);
}

fn paint_chart(b: Bounds<Pixels>, list: &[LpPoint], color: Hsla, p: &Palette, hover: Option<usize>, window: &mut Window, cx: &mut App) {
    let now = chrono::Utc::now().timestamp_millis();
    let g = ChartGeom::new(list, f32::from(b.size.width), now);
    let at = |x: f32, y: f32| point(b.origin.x + px(x), b.origin.y + px(y));
    for v in g.ticks() {
        let y = g.y(v as f64);
        window.paint_quad(fill(Bounds::new(at(PAD_L, y), size(px(g.w - PAD_L - PAD_R), px(1.))), alpha(p.text, 0.07)));
        paint_text(tick_label(v), at(PAD_L - 10., y - 8.), 11., p.muted, false, Some(80.), window, cx);
    }
    paint_text(local_date(&list[0].at, false), at(PAD_L, CHART_H - 18.), 11., p.muted, false, None, window, cx);
    paint_text("hoy".into(), at(g.w - PAD_R, CHART_H - 18.), 11., p.muted, false, Some(40.), window, cx);

    // Escalones: horizontal hasta el siguiente punto, luego vertical; al final se estira hasta hoy.
    let pts: Vec<(f32, f32)> = list.iter().map(|pt| (g.x(ms(&pt.at)), g.y(lp_value(pt) as f64))).collect();
    let end_x = g.x(now);
    let mut line = PathBuilder::stroke(px(2.));
    let mut area = PathBuilder::fill();
    line.move_to(at(pts[0].0, pts[0].1));
    area.move_to(at(pts[0].0, pts[0].1));
    for i in 1..pts.len() {
        for builder in [&mut line, &mut area] {
            builder.line_to(at(pts[i].0, pts[i - 1].1));
            builder.line_to(at(pts[i].0, pts[i].1));
        }
    }
    let last = *pts.last().unwrap();
    line.line_to(at(end_x, last.1));
    area.line_to(at(end_x, last.1));
    area.line_to(at(end_x, CHART_H - PAD_B));
    area.line_to(at(pts[0].0, CHART_H - PAD_B));
    area.close();
    if let Ok(path) = area.build() {
        window.paint_path(path, linear_gradient(180., linear_color_stop(alpha(color, 0.22), 0.), linear_color_stop(alpha(color, 0.02), 1.)));
    }
    if let Ok(path) = line.build() {
        window.paint_path(path, color);
    }
    let dot = |x: f32, y: f32, window: &mut Window| {
        window.paint_quad(fill(Bounds::new(at(x - 5., y - 5.), size(px(10.), px(10.))), color).corner_radii(px(5.)).border_widths(px(2.)).border_color(p.bg));
    };
    dot(end_x, last.1, window);
    paint_text(format!("{} LP", list.last().unwrap().lp), at(end_x + 10., last.1 - 8.), 11.5, p.text, true, None, window, cx);
    if let Some(i) = hover.filter(|i| *i < pts.len()) {
        let (x, y) = pts[i];
        window.paint_quad(fill(Bounds::new(at(x, PAD_T), size(px(1.), px(CHART_H - PAD_T - PAD_B))), alpha(p.text, 0.35)));
        dot(x, y, window);
    }
}

impl MainScreen {
    pub(crate) fn history_tab(&mut self, a: &Account, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let vault = self.vault.read(cx);
        let matches: Vec<Match> = vault.store.session().and_then(|s| s.history.get(&a.id)).cloned().unwrap_or_default();
        let lp = vault.store.session().and_then(|s| s.lp.get(&a.id)).cloned().unwrap_or_default();
        let game = vault.game.clone();
        let mut modes: Vec<&'static str> = Vec::new();
        for m in &matches {
            let mode = if m.mode == "tft" { "tft" } else { "lol" };
            if !modes.contains(&mode) {
                modes.push(mode);
            }
        }
        if modes.is_empty() {
            return empty_tab(
                Icon::empty().path(crate::assets::icon("clock")),
                ["Todavía no hay partidas guardadas de esta cuenta.", "Se cargan al detectarla en el cliente y se actualizan solas al terminar cada partida."],
                &p,
            )
            .into_any_element();
        }
        // Si la cuenta no tiene partidas del modo elegido, se muestra el que sí tenga.
        let mode = if modes.contains(&self.history_mode) { self.history_mode } else { modes[0] };
        let list: Vec<&Match> = matches.iter().filter(|m| m.mode == mode).collect();
        let count = |m: &str| matches.iter().filter(|x| x.mode == m).count();
        let switcher = (modes.len() > 1).then(|| {
            h_flex().child(segmented(&p).children([("lol", "League of Legends"), ("tft", "TFT")].map(|(key, label)| {
                segment(format!("mode-{key}").into(), label, Some(count(key)), key == mode, &p).on_click(cx.listener(move |this, _, _, cx| {
                    this.history_mode = key;
                    cx.notify();
                }))
            })))
        });
        let summary = if mode == "tft" { tft_summary(&list, &p) } else { lol_summary(&list, &p) };
        v_flex()
            .pt(px(16.))
            .pb(px(12.))
            .pr(px(6.))
            .children(switcher)
            .children(summary)
            .child(v_flex().gap(px(6.)).children(list.iter().map(|m| match_row(m, &lp, &game, &p))))
            .into_any_element()
    }

    pub(crate) fn progress_tab(&mut self, a: &Account, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let lp = self.vault.read(cx).store.session().and_then(|s| s.lp.get(&a.id)).cloned().unwrap_or_default();
        let queues: Vec<(&'static str, &'static str)> =
            [("solo", "Solo/Dúo"), ("flex", "Flex"), ("tft", "TFT")].into_iter().filter(|(q, _)| lp.get(*q).is_some_and(|l| !l.is_empty())).collect();
        if queues.is_empty() {
            return empty_tab(
                Icon::empty().path(crate::assets::icon("chart-line")),
                ["Todavía no hay registro de LP de esta cuenta.", "Se anota solo cada vez que cambia su rango (al detectarla en el cliente o al actualizar)."],
                &p,
            )
            .into_any_element();
        }
        let q = if queues.iter().any(|(k, _)| *k == self.progress_queue) { self.progress_queue } else { queues[0].0 };
        let list = lp[q].clone();
        let (first, last) = (&list[0], list.last().unwrap());
        let total = lp_value(last) - lp_value(first);
        let label = |pt: &LpPoint| format!("{} · {} LP", rank_label(&pt.tier, &pt.division), pt.lp);
        let changes: Vec<(LpPoint, i64)> =
            list.windows(2).map(|w| (w[1].clone(), lp_value(&w[1]) - lp_value(&w[0]))).filter(|(_, d)| *d != 0).rev().take(8).collect();
        // El color de la línea es el de la liga actual de esa cola.
        let color = ui::tier_color(Some(&last.tier));

        let switcher = (queues.len() > 1).then(|| {
            h_flex().child(segmented(&p).children(queues.iter().map(|(key, name)| {
                let key = *key;
                segment(format!("queue-{key}").into(), name, None, key == q, &p).on_click(cx.listener(move |this, _, _, cx| {
                    this.progress_queue = key;
                    this.lp_hover = None;
                    cx.notify();
                }))
            })))
        });

        // El gráfico se dibuja con el ancho real; el crosshair se pega al cambio más cercano al mouse.
        let bounds_cell = self.chart_bounds.clone();
        let (paint_list, paint_p, hover) = (list.clone(), p, self.lp_hover);
        let hover_list = list.clone();
        let chart = div()
            .id("lp-chart")
            .relative()
            .w_full()
            .h(px(CHART_H))
            .child(
                canvas(
                    move |b, _, _| {
                        bounds_cell.set(Some(b));
                    },
                    move |b, _, window, cx| paint_chart(b, &paint_list, color, &paint_p, hover, window, cx),
                )
                .size_full(),
            )
            .on_mouse_move(cx.listener(move |this, ev: &MouseMoveEvent, _, cx| {
                let Some(b) = this.chart_bounds.get() else { return };
                let g = ChartGeom::new(&hover_list, f32::from(b.size.width), chrono::Utc::now().timestamp_millis());
                let mx = f32::from(ev.position.x - b.origin.x);
                let nearest = (0..hover_list.len()).min_by(|i, j| {
                    let di = (g.x(ms(&hover_list[*i].at)) - mx).abs();
                    let dj = (g.x(ms(&hover_list[*j].at)) - mx).abs();
                    di.total_cmp(&dj)
                });
                if this.lp_hover != nearest {
                    this.lp_hover = nearest;
                    cx.notify();
                }
            }))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !hovered {
                    this.lp_hover = None;
                    cx.notify();
                }
            }))
            .when_some(self.lp_hover.zip(self.chart_bounds.get()), |d, (i, b)| {
                let Some(pt) = list.get(i) else { return d };
                let g = ChartGeom::new(&list, f32::from(b.size.width), chrono::Utc::now().timestamp_millis());
                let x = g.x(ms(&pt.at)).clamp(80., g.w - 80.);
                let y = g.y(lp_value(pt) as f64);
                let diff = if i > 0 { lp_value(pt) - lp_value(&list[i - 1]) } else { 0 };
                d.child(
                    v_flex()
                        .absolute()
                        .left(px(x - 80.))
                        .top(px(y - 72.))
                        .w(px(160.))
                        .items_center()
                        .px(px(10.))
                        .py(px(6.))
                        .rounded(px(8.))
                        .bg(p.surface_2)
                        .border_1()
                        .border_color(p.border_strong)
                        .shadow_lg()
                        .child(div().text_size(px(12.5)).font_weight(FontWeight::SEMIBOLD).text_color(p.text).child(label(pt)))
                        .child(div().text_size(px(12.)).text_color(if diff > 0 { p.ok } else if diff < 0 { p.loss } else { p.muted }).child(if diff == 0 {
                            "Primer registro".to_string()
                        } else {
                            format!("{}{} LP", if diff > 0 { "+" } else { "−" }, diff.abs())
                        }))
                        .child(div().text_size(px(11.)).text_color(p.muted).child(local_date(&pt.at, true))),
                )
            });

        v_flex()
            .pt(px(16.))
            .pb(px(12.))
            .pr(px(6.))
            .children(switcher)
            .child(
                h_flex()
                    .gap(px(26.))
                    .items_end()
                    .mb(px(10.))
                    .child(stat(label(last), "Ahora", p.text, 17., &p))
                    .when(list.len() > 1, |d| {
                        d.child(stat(
                            format!("{}{} LP", if total > 0 { "+" } else if total < 0 { "−" } else { "" }, total.abs()),
                            format!("desde {}", local_date(&first.at, false)),
                            if total > 0 { p.ok } else if total < 0 { p.loss } else { p.text },
                            17.,
                            &p,
                        ))
                    }),
            )
            .child(chart)
            .child(if changes.is_empty() {
                div().mt(px(10.)).text_size(px(12.5)).text_color(p.muted).child("El gráfico se va llenando con cada cambio de rango.").into_any_element()
            } else {
                v_flex()
                    .gap(px(4.))
                    .mt(px(14.))
                    .max_w(px(520.))
                    .child(caps("Últimos cambios", &p).mb(px(4.)))
                    .children(changes.iter().map(|(pt, diff)| {
                        h_flex()
                            .gap(px(10.))
                            .px(px(10.))
                            .py(px(5.))
                            .rounded(px(8.))
                            .bg(alpha(p.surface, 0.6))
                            .text_size(px(12.5))
                            .text_color(p.text_2)
                            .child(div().w(px(110.)).text_color(if *diff > 0 { p.ok } else { p.loss }).child(format!(
                                "{} {}{} LP",
                                if *diff > 0 { "▲" } else { "▼" },
                                if *diff > 0 { "+" } else { "−" },
                                diff.abs()
                            )))
                            .child(div().flex_1().child(label(pt)))
                            .child(div().text_color(p.muted).child(ui::ago(Some(&pt.at))))
                    }))
                    .into_any_element()
            })
            .into_any_element()
    }

    pub(crate) fn champions_tab(&mut self, a: &Account, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let game = self.vault.read(cx).game.clone();
        let top: Vec<(i64, i64, i64)> = a
            .mastery
            .as_ref()
            .and_then(|m| m["top"].as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|c| Some((c["champ"].as_i64()?, c["level"].as_i64().unwrap_or(0), c["points"].as_i64().unwrap_or(0))))
                    .collect()
            })
            .unwrap_or_default();
        if top.is_empty() {
            return empty_tab(
                Icon::new(IconName::Star),
                ["Todavía no hay maestrías de esta cuenta.", "Se cargan al detectarla en el cliente, o con Actualizar si tienes API key de Riot."],
                &p,
            )
            .into_any_element();
        }
        let score = a.mastery.as_ref().and_then(|m| m["score"].as_i64()).unwrap_or(0);
        let total: i64 = top.iter().map(|(_, _, pts)| pts).sum();
        let max = top[0].2.max(1);
        let short = |n: i64| {
            if n >= 1_000_000 {
                format!("{:.1} M", n as f64 / 1_000_000.)
            } else if n >= 1000 {
                format!("{} mil", (n as f64 / 1000.).round())
            } else {
                n.to_string()
            }
        };
        v_flex()
            .pt(px(16.))
            .pb(px(12.))
            .pr(px(6.))
            .child(
                h_flex()
                    .gap(px(26.))
                    .items_end()
                    .mb(px(10.))
                    .child(stat(score.to_string(), "Puntaje de maestría", p.text, 17., &p))
                    .child(stat(short(total), format!("Puntos en su top {}", top.len()), p.text, 17., &p)),
            )
            .child(v_flex().gap(px(6.)).max_w(px(620.)).children(top.iter().enumerate().map(|(i, (champ, level, points))| {
                let podium = i < 3;
                let name = Some(game.champion(*champ)).filter(|n| !n.is_empty()).unwrap_or_else(|| "Campeón".into());
                h_flex()
                    .gap(px(12.))
                    .pl(px(8.))
                    .pr(px(12.))
                    .py(px(7.))
                    .rounded(px(10.))
                    .bg(alpha(p.surface, 0.72))
                    .border_1()
                    .border_color(if podium { alpha(p.gold, 0.28) } else { p.border })
                    .child(div().w(px(18.)).text_center().text_size(px(12.)).font_weight(FontWeight::BOLD).text_color(if podium { p.gold_hi } else { p.muted }).child((i + 1).to_string()))
                    .child(
                        div()
                            .relative()
                            .size(px(40.))
                            .flex_none()
                            .child(img(ui::champion_icon_url(*champ)).size_full().rounded_full().border_2().border_color(alpha(p.gold, 0.55)))
                            .child(
                                div()
                                    .absolute()
                                    .right(px(-6.))
                                    .bottom(px(-4.))
                                    .min_w(px(20.))
                                    .h(px(18.))
                                    .px(px(5.))
                                    .rounded_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(linear_gradient(180., linear_color_stop(p.gold_hi, 0.), linear_color_stop(p.gold, 1.)))
                                    .text_size(px(10.5))
                                    .font_weight(FontWeight::EXTRA_BOLD)
                                    .text_color(p.on_accent)
                                    .child(level.to_string()),
                            ),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                h_flex()
                                    .justify_between()
                                    .gap(px(10.))
                                    .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(p.text).child(name))
                                    .child(div().text_size(px(12.)).text_color(p.text_2).child(format!("{} pts", thousands(*points)))),
                            )
                            .child(
                                div()
                                    .mt(px(6.))
                                    .h(px(3.))
                                    .w_full()
                                    .rounded(px(3.))
                                    .bg(hsla(0., 0., 1., 0.08))
                                    .child(div().h_full().w(relative(*points as f32 / max as f32)).rounded(px(3.)).bg(p.gold)),
                            ),
                    )
            })))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn thousands_like_spanish_locale() {
        assert_eq!(super::thousands(512340), "512.340");
        assert_eq!(super::thousands(1234567), "1.234.567");
        assert_eq!(super::thousands(999), "999");
    }
}
