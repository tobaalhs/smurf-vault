//! Panel de detalle de la cuenta elegida: encabezado (avatar con marco, nombre, chips), pestañas
//! (Resumen, Historial, Progreso, Campeones) y acciones (Jugar, Editar, Actualizar).
//! Medidas y colores sacados de styles.css de la versión Electron.
use crate::assets;
use crate::main_screen::{MainScreen, Tab};
use crate::theme::{palette, Palette};
use crate::ui::{self, alpha, hex};
use gpui_kit::component::button::Button;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{h_flex, v_flex, Icon, IconName, Sizable, WindowExt};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use sv_core::gamedata::GameData;
use sv_core::local::LpPoint;
use sv_core::matches::Match;
use sv_core::model::{Account, Rank};
use sv_core::ranks::{lp_delta, rank_label};

/// Separa las letras con un espacio finito (el letter-spacing del CSS).
fn spaced(text: &str) -> String {
    text.chars().map(String::from).collect::<Vec<_>>().join("\u{200A}")
}

/// Colores de puesto en TFT: oro, plata, bronce y gris para el 4.º.
fn place_color(place: Option<i64>, p: &Palette) -> Hsla {
    match place {
        Some(1) => hex(0xf0c75e),
        Some(2) => hex(0xcfd8e2),
        Some(3) => hex(0xd08a5c),
        Some(4) => hex(0x8e98a8),
        _ => p.muted,
    }
}

/// Color del resultado de una partida.
pub(crate) fn result_color(m: &Match, p: &Palette) -> Hsla {
    if m.is_remake() {
        p.muted
    } else if m.mode == "tft" {
        place_color(m.placement, p)
    } else if m.win {
        p.win
    } else {
        p.loss
    }
}

/// Título chico en mayúsculas (SOLO/DÚO, ÚLTIMAS DE LOL, USUARIO…).
pub(crate) fn caps(text: &str, p: &Palette) -> Div {
    div().text_size(px(10.5)).font_weight(FontWeight::BOLD).text_color(p.muted).child(text.to_uppercase())
}

fn delta_badge(points: Option<&Vec<LpPoint>>, p: &Palette) -> Option<Div> {
    let (diff, from, to) = lp_delta(points?)?;
    if diff == 0 {
        return None;
    }
    let up = diff > 0;
    let text = if from.tier != to.tier || from.division != to.division {
        format!("{} de {}", if up { "Subió" } else { "Bajó" }, rank_label(&from.tier, &from.division))
    } else {
        format!("{}{} LP", if up { "+" } else { "−" }, diff.abs())
    };
    Some(div().font_weight(FontWeight::SEMIBOLD).text_color(if up { p.ok } else { p.loss }).child(format!("{} {text}", if up { "▲" } else { "▼" })))
}

fn rank_card(title: &'static str, r: Option<&Rank>, lp: Option<&Vec<LpPoint>>, width: f32, p: &Palette) -> Div {
    let base = h_flex()
        .gap(px(12.))
        .w(px(width))
        .px(px(14.))
        .py(px(12.))
        .rounded(px(12.))
        .bg(alpha(p.surface, 0.74))
        .border_1()
        .border_color(p.border);
    let Some(r) = r else {
        return base.opacity(0.65).child(
            v_flex().gap(px(2.)).py(px(8.)).child(caps(title, p)).child(div().text_size(px(17.)).font_weight(FontWeight::BOLD).text_color(p.muted).child("Sin rango")),
        );
    };
    let games = r.wins + r.losses;
    let wr = if games > 0 { (r.wins as f32 / games as f32 * 100.).round() as i64 } else { 0 };
    base.child(img(ui::rank_emblem(r)).size(px(60.)).mx(px(-4.)).my(px(-8.)).object_fit(ObjectFit::Contain)).child(
        v_flex()
            .flex_1()
            .min_w_0()
            .child(caps(title, p))
            .child(
                h_flex()
                    .items_baseline()
                    .child(div().text_size(px(17.)).font_weight(FontWeight::BOLD).text_color(ui::tier_color(Some(&r.tier))).child(rank_label(&r.tier, &r.division)))
                    .child(div().ml(px(5.)).text_size(px(12.5)).font_weight(FontWeight::MEDIUM).text_color(p.text_2).child(format!("{} LP", r.lp))),
            )
            .child(
                h_flex()
                    .gap(px(6.))
                    .mt(px(1.))
                    .text_size(px(11.5))
                    .text_color(p.muted)
                    .child(if games > 0 { format!("{}V {}D · {wr}%", r.wins, r.losses) } else { "Sin partidas".into() })
                    .children(delta_badge(lp, p)),
            )
            .child(
                div()
                    .mt(px(5.))
                    .h(px(3.))
                    .w_full()
                    .rounded_full()
                    .bg(hsla(0., 0., 1., 0.08))
                    .child(div().h_full().w(relative(wr as f32 / 100.)).rounded_full().bg(if wr >= 50 { p.ok } else { p.muted })),
            ),
    )
}

/// Miniatura de una partida: el campeón (LoL) o el puesto sobre la minileyenda (TFT).
pub(crate) fn mini_match(m: &Match, game: &GameData, p: &Palette) -> Div {
    let color = result_color(m, p);
    let base = div().relative().size(px(32.)).flex_none().rounded(px(7.)).overflow_hidden().bg(p.surface_3);
    if m.mode == "tft" {
        let legend = m.companion.as_deref().and_then(|c| game.companion_url(c));
        let number = div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .text_color(if legend.is_some() { hex(0xffffff) } else if m.placement.unwrap_or(9) <= 4 { color } else { p.muted })
            .font_weight(FontWeight::BOLD)
            .text_size(px(13.))
            .child(m.placement.map(|x| x.to_string()).unwrap_or_else(|| "?".into()));
        return match legend {
            Some(url) => base
                .child(img(url).absolute().inset_0().size_full().object_fit(ObjectFit::Cover).opacity(0.5))
                .child(div().absolute().inset_0().bg(alpha(color, 0.5)).border_2().border_color(color).rounded(px(7.)))
                .child(number),
            None => base.bg(alpha(color, 0.14)).child(number).child(div().absolute().left_0().right_0().bottom_0().h(px(3.)).bg(color)),
        };
    }
    base.when_some(m.champ, |d, c| d.child(img(ui::champion_icon_url(c)).size_full().object_fit(ObjectFit::Cover)))
        .child(div().absolute().left_0().right_0().bottom_0().h(px(3.)).bg(color))
}

/// Paralelogramo inclinado (como skewX(-11deg)) con esquinas redondeadas, para el botón Jugar.
fn skewed_path(o: Point<Pixels>, w: f32, h: f32, r: f32) -> Option<Path> {
    let k = 0.194_380_3 * h / 2.;
    let at = |x: f32, y: f32| point(o.x + px(x), o.y + px(y));
    let pts = [at(k, 0.), at(w + k, 0.), at(w - k, h), at(-k, h)];
    let mut b = PathBuilder::fill();
    for i in 0..4 {
        let (prev, c, next) = (pts[(i + 3) % 4], pts[i], pts[(i + 1) % 4]);
        let toward = |to: Point<Pixels>| {
            let (dx, dy) = (f32::from(to.x - c.x), f32::from(to.y - c.y));
            let len = (dx * dx + dy * dy).sqrt().max(0.001);
            point(c.x + px(dx / len * r), c.y + px(dy / len * r))
        };
        if i == 0 {
            b.move_to(toward(prev));
        } else {
            b.line_to(toward(prev));
        }
        b.curve_to(toward(next), c);
    }
    b.close();
    b.build().ok()
}

fn play_triangle(o: Point<Pixels>, size: f32) -> Option<Path> {
    let mut b = PathBuilder::fill();
    b.move_to(point(o.x + px(size * 0.18), o.y));
    b.line_to(point(o.x + px(size * 0.95), o.y + px(size / 2.)));
    b.line_to(point(o.x + px(size * 0.18), o.y + px(size)));
    b.close();
    b.build().ok()
}

impl MainScreen {
    fn toggle_favorite(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let result = self.vault.update(cx, |v, cx| {
            let fav = v.store.data().and_then(|d| d.account(&id)).is_some_and(|a| a.is_favorite());
            v.change(cx, |s| s.set_favorite(&id, !fav))
        });
        match result {
            Ok(()) => crate::sounds::pop(),
            Err(e) => window.push_notification(Notification::error(e), cx),
        }
    }

    fn hero(&self, a: &Account, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let frame = ui::crest_frame(a);
        let space = frame.as_ref().map(|(_, _, s)| *s).unwrap_or(10.);
        let ban = ui::ban_info(a);
        let id = a.id.clone();
        let framed = frame.is_some();

        // Orden de dibujo: el ícono, encima el marco y encima de todo el nivel.
        let avatar = div()
            .relative()
            .size(px(92.))
            .flex_none()
            .when(framed, |d| d.ml(px(64.)).mr(px(58.)))
            .child(ui::avatar_circle(a, 92., framed, cx))
            .when_some(frame, |d, (src, b, _)| {
                d.child(img(src).absolute().left(b.origin.x).top(b.origin.y).w(b.size.width).h(b.size.height).object_fit(ObjectFit::Contain))
            })
            .when_some(a.level, |d, lvl| d.child(ui::level_badge(lvl, 92., cx)));

        let name = h_flex().items_baseline().min_w_0().map(|d| {
            if a.game_name.is_empty() {
                d.child(div().truncate().child(if a.label.is_empty() { a.username.clone() } else { a.label.clone() }))
            } else {
                d.child(div().truncate().child(a.game_name.clone()))
                    .child(div().flex_none().text_size(px(26.9)).font_weight(FontWeight::MEDIUM).text_color(p.text_2).child(format!("#{}", a.tag_line)))
            }
        });

        let chip = |text: String, fg: Hsla, bg: Hsla, border: Hsla| ui::chip(text, fg, bg, border).h(px(23.)).py_0();
        let chips = h_flex()
            .gap(px(6.))
            .mt(px(9.))
            .flex_wrap()
            .when(!a.server.is_empty(), |d| d.child(chip(a.server.clone(), p.text_2, alpha(p.surface, 0.78), p.border_strong)))
            .when(!a.label.is_empty(), |d| d.child(chip(a.label.clone(), p.gold_hi, alpha(p.gold, 0.12), alpha(p.gold, 0.3))))
            // Sin contraseña, Jugar no puede iniciar sesión solo: se avisa.
            .when(a.password.is_empty(), |d| d.child(chip("Sin contraseña".into(), p.gold_hi, alpha(p.gold, 0.08), alpha(p.gold, 0.35))))
            .when_some(ban, |d, (text, _, _)| {
                d.child(chip(text, p.loss, alpha(p.loss, 0.12), alpha(p.loss, 0.4)).child(Icon::new(IconName::Ban).size(px(12.))).flex_row_reverse())
            });

        let fav = a.is_favorite();
        h_flex()
            .gap(px(22.))
            .pt(px(space))
            .child(avatar)
            .child(v_flex().flex_1().min_w_0().child(div().text_size(px(32.)).font_weight(FontWeight(750.)).text_color(p.text).child(name)).child(chips))
            .child(
                div()
                    .id("fav")
                    .self_start()
                    .size(px(38.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(10.))
                    .border_1()
                    .border_color(if fav { alpha(p.gold, 0.5) } else { p.border_strong })
                    .bg(alpha(p.surface, 0.7))
                    .cursor_pointer()
                    .hover(|s| s.border_color(alpha(p.gold, 0.45)))
                    .child(Icon::new(if fav { IconName::StarFill } else { IconName::Star }).text_color(if fav { p.gold } else { p.muted }))
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(if fav { "Quitar de favoritas" } else { "Favorita: queda siempre arriba" }).build(window, cx)
                    })
                    .on_click(cx.listener(move |this, _, window, cx| this.toggle_favorite(id.clone(), window, cx))),
            )
    }

    fn tabs(&self, a: &Account, count: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let underline = ui::account_tier_color(a);
        let tab = |t: Tab, label: &'static str, badge: Option<usize>, cx: &mut Context<Self>| {
            let on = self.tab == t;
            div()
                .id(label)
                .relative()
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(12.))
                .pt(px(8.))
                .pb(px(10.))
                .cursor_pointer()
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if on { p.text } else { p.text_2 })
                .hover(|s| s.text_color(p.text))
                .child(label)
                .when_some(badge.filter(|n| *n > 0), |d, n| {
                    d.child(div().px(px(6.)).rounded_full().bg(p.surface_3).text_size(px(11.)).text_color(p.text_2).child(n.to_string()))
                })
                .when(on, |d| d.child(div().absolute().left(px(10.)).right(px(10.)).bottom(px(-1.)).h(px(2.)).rounded(px(2.)).bg(underline)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.tab != t {
                        this.tab = t;
                        cx.notify();
                    }
                }))
        };
        h_flex()
            .mt(px(30.))
            .gap(px(4.))
            .border_b_1()
            .border_color(p.border)
            .child(tab(Tab::Summary, "Resumen", None, cx))
            .child(tab(Tab::History, "Historial", Some(count), cx))
            .child(tab(Tab::Progress, "Progreso", None, cx))
            .child(tab(Tab::Champions, "Campeones", None, cx))
    }

    fn summary(&self, a: &Account, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let vault = self.vault.read(cx);
        let session = vault.store.session();
        let matches: Vec<Match> = session.and_then(|s| s.history.get(&a.id)).cloned().unwrap_or_default();
        let lp = session.and_then(|s| s.lp.get(&a.id)).cloned().unwrap_or_default();
        let game = vault.game.clone();
        // Grilla de columnas iguales (CSS: repeat(auto-fill, minmax(230px, 1fr)), máx. 780 px).
        let avail = self.detail_w.min(780.);
        let cols = ((avail + 10.) / 240.).floor().max(1.);
        let card_w = (avail - 10. * (cols - 1.)) / cols;

        let strip = |mode: &'static str, label: &'static str, cx: &mut Context<Self>| {
            let list: Vec<&Match> = matches.iter().filter(|m| m.mode == mode).take(10).collect();
            (!list.is_empty()).then(|| {
                h_flex()
                    .gap(px(12.))
                    .mt(px(18.))
                    .flex_wrap()
                    .child(caps(label, &p))
                    .child(h_flex().gap(px(5.)).children(list.into_iter().map(|m| mini_match(m, &game, &p))))
                    .child(
                        div()
                            .id(SharedString::from(format!("see-{mode}")))
                            .cursor_pointer()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.cyan)
                            .hover(|s| s.underline())
                            .child("Ver historial")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.tab = Tab::History;
                                this.history_mode = mode;
                                cx.notify();
                            })),
                    )
            })
        };

        let cred = |id: &'static str, label: &'static str, value: String, secret: bool, cx: &mut Context<Self>| {
            let shown = if value.is_empty() {
                div().text_size(px(12.)).text_color(p.muted).child("Sin guardar")
            } else if secret {
                div().text_size(px(13.)).text_color(p.text).child(spaced("••••••••"))
            } else {
                div().text_size(px(13.)).text_color(p.text).truncate().child(value.clone())
            };
            div()
                .id(id)
                .relative()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .pl(px(12.))
                .pr(px(38.))
                .py(px(9.))
                .rounded(px(10.))
                .bg(alpha(p.surface, 0.74))
                .border_1()
                .border_color(p.border)
                .cursor_pointer()
                .hover(|s| s.border_color(alpha(p.gold, 0.45)).bg(p.bg))
                .child(caps(label, &p))
                .child(shown)
                .child(div().absolute().right(px(10.)).top_0().bottom_0().flex().items_center().child(Icon::new(IconName::Copy).small().text_color(p.muted)))
                .on_click(cx.listener(move |_, _, window, cx| {
                    if value.is_empty() {
                        return;
                    }
                    ui::copy_secret(value.clone(), cx);
                    window.push_notification(Notification::success(format!("{label} copiado (se borra en 30 s)")), cx);
                }))
        };

        v_flex()
            .pt(px(16.))
            .pb(px(12.))
            .pr(px(6.))
            .child(
                h_flex()
                    .max_w(px(780.))
                    .gap(px(10.))
                    .flex_wrap()
                    .child(rank_card("Solo/Dúo", a.ranks.solo.as_ref(), lp.get("solo"), card_w, &p))
                    .child(rank_card("Flex", a.ranks.flex.as_ref(), lp.get("flex"), card_w, &p))
                    .when_some(a.ranks.tft.as_ref(), |d, r| d.child(rank_card("TFT", Some(r), lp.get("tft"), card_w, &p)))
                    .when_some(a.ranks.double_up.as_ref(), |d, r| d.child(rank_card("Double Up", Some(r), None, card_w, &p))),
            )
            .children(strip("lol", "Últimas de LoL", cx))
            .children(strip("tft", "Últimas de TFT", cx))
            .child(
                h_flex()
                    .gap(px(18.))
                    .mt(px(16.))
                    .text_size(px(12.5))
                    .text_color(p.text_2)
                    .child(h_flex().gap(px(6.)).child(Icon::empty().path(assets::icon("gamepad-2")).size(px(14.)).text_color(p.cyan)).child(match &a.last_played_at {
                        Some(t) => format!("Jugó {}", ui::ago(Some(t))),
                        None => "Sin partidas registradas".into(),
                    }))
                    .child(h_flex().gap(px(6.)).child(Icon::new(IconName::RefreshCw).size(px(14.)).text_color(p.cyan)).child(match &a.last_synced_at {
                        Some(t) => format!("Datos de {}", ui::ago(Some(t))),
                        None => "Datos sin actualizar".into(),
                    })),
            )
            .child(
                h_flex()
                    .max_w(px(560.))
                    .gap(px(10.))
                    .mt(px(16.))
                    .child(cred("copy-user", "Usuario", a.username.clone(), false, cx))
                    .child(cred("copy-pass", "Contraseña", a.password.clone(), true, cx)),
            )
            .when(!a.notes.is_empty(), |d| d.child(div().max_w(px(560.)).mt(px(12.)).text_size(px(12.5)).text_color(p.text_2).child(a.notes.clone())))
    }

    fn play_button(&self, can_play: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let hovered = self.play_hover;
        let (top, bottom, text_color, glow) = if can_play {
            (p.gold_hi, p.gold, p.on_accent, alpha(p.gold, if hovered { 0.38 } else { 0.28 }))
        } else {
            let cyan: Rgba = p.cyan.into();
            let mix = |t: f32, c: f32| Hsla::from(Rgba { r: cyan.r * t + c * (1. - t), g: cyan.g * t + c * (1. - t), b: cyan.b * t + c * (1. - t), a: 1. });
            (mix(0.88, 1.), mix(0.82, 0.), p.on_cyan, alpha(p.cyan, 0.22))
        };
        let (top, bottom) = if hovered { (ui::lighten(top, 0.06), ui::lighten(bottom, 0.06)) } else { (top, bottom) };
        let label = if can_play { "JUGAR" } else { "INICIAR SESIÓN" };
        let width = if can_play { 250. } else { 290. };
        div()
            .id("play")
            .relative()
            .w(px(width))
            .h(px(58.))
            .flex_none()
            .mt(px(if hovered { -2. } else { 0. }))
            .cursor_pointer()
            // Mientras se cambia de cuenta, el botón se apaga un poco (como .play-big.loading).
            .when(self.switching, |d| d.opacity(0.65).cursor_default())
            .child(div().absolute().top(px(4.)).bottom(px(4.)).left(px(10.)).right(px(10.)).rounded(px(9.)).shadow(vec![BoxShadow { inset: false,
                color: glow,
                offset: point(px(0.), px(10.)),
                blur_radius: px(if hovered { 40. } else { 34. }),
                spread_radius: px(0.),
            }]))
            .child(
                canvas(
                    |_, _, _| (),
                    move |b, _, window, _| {
                        if let Some(path) = skewed_path(b.origin, f32::from(b.size.width), f32::from(b.size.height), 9.) {
                            window.paint_path(path, linear_gradient(180., linear_color_stop(top, 0.), linear_color_stop(bottom, 1.)));
                        }
                        // Brillo fino arriba (inset 0 1px 0 rgba(255,255,255,.4)).
                        let k = 0.194_380_3 * f32::from(b.size.height) / 2.;
                        window.paint_quad(fill(Bounds::new(point(b.origin.x + px(k + 8.), b.origin.y + px(1.)), size(b.size.width - px(16.), px(1.))), hsla(0., 0., 1., 0.4)));
                        if can_play {
                            let s = 19.;
                            let x = f32::from(b.size.width) / 2. - 58.;
                            if let Some(path) = play_triangle(point(b.origin.x + px(x), b.origin.y + (b.size.height - px(s)) / 2.), s) {
                                window.paint_path(path, text_color);
                            }
                        }
                    },
                )
                .absolute()
                .inset_0(),
            )
            .child(
                h_flex()
                    .absolute()
                    .inset_0()
                    .justify_center()
                    .gap(px(12.))
                    .when(can_play, |d| d.pl(px(30.)))
                    .when(!can_play, |d| d.child(Icon::empty().path(assets::icon("log-in")).size(px(20.)).text_color(text_color)))
                    .child(div().text_size(px(21.)).font_weight(FontWeight::EXTRA_BOLD).text_color(text_color).child(spaced(label))),
            )
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.play_hover = *hovered;
                cx.notify();
            }))
            .on_click(cx.listener(|this, _, window, cx| this.play_selected(window, cx)))
    }

    fn actions(&self, a: &Account, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let can_play = !a.username.is_empty() && !a.password.is_empty();
        let vault = self.vault.read(cx);
        let (_, caption) = ui::splash_for(a, vault.store.session().map(|s| &s.history), |c| vault.game.champion(c));
        let subtle = |id: &'static str, icon: Icon, label: &'static str| {
            Button::new(id)
                .icon(icon.size(px(16.)))
                .label(label)
                .h(px(42.))
                .px(px(14.))
                .text_size(px(13.5))
                .bg(alpha(p.surface, 0.7))
                .border_color(p.border_strong)
                .text_color(p.text)
        };
        let (vault_edit, acc, acc_id) = (self.vault.clone(), a.clone(), a.id.clone());
        h_flex()
            .gap(px(10.))
            .pt(px(16.))
            .pb(px(24.))
            .child(self.play_button(can_play, cx))
            .child(
                subtle("edit", Icon::empty().path(assets::icon("pencil")), "Editar")
                    .on_click(move |_, window, cx| crate::dialogs::open_account(vault_edit.clone(), Some(acc.clone()), window, cx)),
            )
            .child(
                subtle("refresh", Icon::new(IconName::RefreshCw), "Actualizar")
                    .tooltip("Lee rangos, historial y maestrías desde el cliente de LoL (esta cuenta tiene que estar abierta ahí)")
                    .loading(self.detecting)
                    .on_click(cx.listener(move |this, _, window, cx| this.detect(true, Some(acc_id.clone()), window, cx))),
            )
            .child(div().flex_1())
            .child(div().text_size(px(11.5)).text_color(p.muted).child(caption))
    }

    pub(crate) fn detail(&mut self, a: &Account, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.vault.read(cx).store.session().and_then(|s| s.history.get(&a.id)).map(|m| m.len()).unwrap_or(0);
        let body = match self.tab {
            Tab::Summary => self.summary(a, cx).into_any_element(),
            Tab::History => self.history_tab(a, cx),
            Tab::Progress => self.progress_tab(a, cx),
            Tab::Champions => self.champions_tab(a, cx),
        };
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .pt(px(30.))
            .pl(px(22.))
            .pr(px(36.))
            .child(self.hero(a, cx))
            .child(self.tabs(a, count, cx))
            .child(div().id("detail-body").flex_1().min_h_0().overflow_y_scroll().child(body))
            .child(self.actions(a, cx))
    }
}
