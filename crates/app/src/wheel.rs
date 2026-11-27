//! Rueda de cuentas estilo osu! (igual que carousel.js): la elegida al centro, las demás curvándose
//! hacia la izquierda, más chicas y tenues, sobre placas inclinadas con el splash de cada cuenta.
//! La rueda del mouse recorre sin elegir, se arrastra con inercia y un clic elige.
use crate::main_screen::MainScreen;
use crate::theme::{palette, Palette};
use crate::ui::{self, alpha};
use gpui_kit::component::{h_flex, v_flex, Icon, IconName};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};
use sv_core::model::Account;
use sv_core::ranks::{rank_label, top_rank};

/// Alto de cada cuenta + separación, y alto de la placa.
const STEP: f32 = 84.;
const ITEM_H: f32 = 74.;
/// Inclinación de las placas (CSS skewX(-11deg)).
const SKEW: f32 = 0.194_380_3;

struct Drag {
    y: f32,
    start: f32,
    moved: bool,
    last_y: f32,
    last_t: Instant,
    v: f32,
    down: Option<usize>,
}

pub(crate) struct WheelState {
    /// Posición actual y destino, en cuentas (con decimales).
    pub pos: f32,
    pub target: f32,
    last_frame: Option<Instant>,
    tick_index: Option<i32>,
    drag: Option<Drag>,
    hovered: Option<usize>,
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    snap: Option<Task<()>>,
}

impl Default for WheelState {
    fn default() -> Self {
        Self { pos: 0., target: 0., last_frame: None, tick_index: None, drag: None, hovered: None, bounds: Rc::new(Cell::new(None)), snap: None }
    }
}

impl WheelState {
    /// Avanza la animación hacia el destino. Devuelve true si todavía se mueve.
    fn advance(&mut self) -> bool {
        let now = Instant::now();
        let dt = self.last_frame.map(|t| now.duration_since(t).as_secs_f32()).unwrap_or(1. / 60.).min(0.1);
        self.last_frame = Some(now);
        let diff = self.target - self.pos;
        if diff.abs() < 0.0005 {
            self.pos = self.target;
            self.last_frame = None;
            return false;
        }
        // 17 % por cuadro a 60 fps, sin importar la frecuencia real de dibujo.
        let k = 1. - (1. - 0.17f32).powf(dt * 60.);
        self.pos += diff * k;
        true
    }

    /// Índice de la cuenta bajo el mouse (por la altura).
    fn index_at(&self, y: Pixels, len: usize) -> Option<usize> {
        let b = self.bounds.get()?;
        let d = f32::from(y - b.origin.y - b.size.height / 2.) / STEP;
        let i = (self.pos + d).round();
        (i >= 0. && (i as usize) < len).then_some(i as usize)
    }
}

/// Esquina redondeada de un trazado: línea hasta `r` antes de la esquina y curva hasta `r` después.
fn round_corner(b: &mut PathBuilder, prev: Point<Pixels>, c: Point<Pixels>, next: Point<Pixels>, r: f32, first: bool) {
    let toward = |from: Point<Pixels>, to: Point<Pixels>| {
        let (dx, dy) = (f32::from(to.x - from.x), f32::from(to.y - from.y));
        let len = (dx * dx + dy * dy).sqrt().max(0.001);
        point(from.x + px(dx / len * r), from.y + px(dy / len * r))
    };
    let (a, z) = (toward(c, prev), toward(c, next));
    if first {
        b.move_to(a);
    } else {
        b.line_to(a);
    }
    b.curve_to(z, c);
}

/// Contorno de la placa: un paralelogramo inclinado con las esquinas redondeadas.
fn plate_path(o: Point<Pixels>, w: f32, h: f32, r: f32, stroke: Option<f32>) -> Option<Path> {
    let k = SKEW * h / 2.;
    let at = |x: f32, y: f32| point(o.x + px(x), o.y + px(y));
    let pts = [at(k, 0.), at(w + k, 0.), at(w - k, h), at(-k, h)];
    let mut b = match stroke {
        Some(width) => PathBuilder::stroke(px(width)),
        None => PathBuilder::fill(),
    };
    for i in 0..4 {
        round_corner(&mut b, pts[(i + 3) % 4], pts[i], pts[(i + 1) % 4], r, i == 0);
    }
    b.close();
    b.build().ok()
}

/// Franja del color de la liga en el borde derecho de la placa.
fn stripe_path(o: Point<Pixels>, w: f32, h: f32, s: f32) -> Option<Path> {
    let k = SKEW * h / 2.;
    let wid = 5. * s;
    let inset = 6. * s; // deja libres las esquinas redondeadas
    let ky = SKEW * inset;
    let at = |x: f32, y: f32| point(o.x + px(x), o.y + px(y));
    let mut b = PathBuilder::fill();
    b.move_to(at(w + k - ky - wid, inset));
    b.line_to(at(w + k - ky, inset));
    b.line_to(at(w - k + ky, h - inset));
    b.line_to(at(w - k + ky - wid, h - inset));
    b.close();
    b.build().ok()
}

/// El splash de la cuenta recortado con la forma de la placa (bordes inclinados y esquinas
/// redondeadas). GPUI no recorta imágenes con una forma, así que se dibuja en franjas horizontales
/// finas, cada una recortada a lo ancho que tiene la placa a esa altura.
fn plate_splash(url: SharedString, w: f32, h: f32, r: f32, opacity: f32) -> Div {
    const STRIP: f32 = 3.;
    let k = SKEW * h / 2.;
    let mut out = div().absolute().inset_0();
    let mut y = 0.;
    while y < h {
        let dh = STRIP.min(h - y);
        let mid = y + dh / 2.;
        // Bordes de la placa a esta altura (la izquierda va de +k arriba a -k abajo; la derecha igual).
        let t = mid / h;
        let (mut left, mut right) = (k - 2. * k * t, w + k - 2. * k * t);
        // Esquinas redondeadas: cerca de arriba y de abajo se entra un poco más.
        let edge = mid.min(h - mid);
        if edge < r {
            let inset = r - (r * r - (r - edge) * (r - edge)).max(0.).sqrt();
            left += inset;
            right -= inset;
        }
        out = out.child(
            div()
                .absolute()
                .left(px(left))
                .top(px(y))
                .w(px((right - left).max(0.)))
                .h(px(dh))
                .overflow_hidden()
                .child(
                    img(url.clone())
                        .absolute()
                        .left(px(-left - k))
                        .top(px(-y))
                        .w(px(w + 2. * k))
                        .h(px(h))
                        .object_fit(ObjectFit::Cover)
                        .opacity(opacity),
                ),
        );
        y += dh;
    }
    out
}

fn item_content(a: &Account, s: f32, saved: bool, p: &Palette, cx: &App) -> Div {
    let sub = [a.server.clone(), a.label.clone(), a.last_played_at.as_deref().map(|t| ui::ago(Some(t))).unwrap_or_default()]
        .into_iter()
        .filter(|x| !x.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    let name = h_flex()
        .min_w_0()
        .items_baseline()
        .text_size(px(15.5 * s))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(p.text)
        .when(a.is_favorite(), |d| d.child(div().mr(px(5. * s)).child(Icon::new(IconName::StarFill).size(px(13. * s)).text_color(p.gold))))
        .map(|d| {
            if a.game_name.is_empty() {
                d.child(div().truncate().child(if a.label.is_empty() { a.username.clone() } else { a.label.clone() }))
            } else {
                d.child(div().truncate().child(a.game_name.clone()))
                    .child(div().flex_none().text_size(px(13. * s)).font_weight(FontWeight::MEDIUM).text_color(p.text_2).child(format!("#{}", a.tag_line)))
            }
        });
    let tier = if let Some((_, short, _)) = ui::ban_info(a) {
        h_flex()
            .gap(px(5. * s))
            .text_color(p.loss)
            .child(Icon::new(IconName::Ban).size(px(14. * s)))
            .child(div().text_size(px(13. * s)).font_weight(FontWeight::BOLD).child(short))
            .into_any_element()
    } else if let Some(r) = top_rank(a) {
        h_flex()
            .gap(px(6. * s))
            .child(
                v_flex()
                    .items_end()
                    .child(div().text_size(px(13. * s)).font_weight(FontWeight::BOLD).text_color(ui::tier_color(Some(&r.tier))).child(rank_label(&r.tier, &r.division)))
                    .child(div().text_size(px(11.5 * s)).text_color(p.muted).child(format!("{} LP", r.lp))),
            )
            .child(img(ui::rank_emblem(r)).size(px(40. * s)).mx(px(-4. * s)).my(px(-6. * s)).object_fit(ObjectFit::Contain))
            .into_any_element()
    } else {
        div().text_size(px(13. * s)).font_weight(FontWeight::BOLD).text_color(p.muted).child("Sin rango").into_any_element()
    };
    h_flex()
        .absolute()
        .inset_0()
        .gap(px(14. * s))
        .pl(px(56. * s))
        .pr(px(30. * s))
        .child(scaled_avatar(a, 50. * s, cx))
        .child(
            v_flex().flex_1().min_w_0().gap(px(3. * s)).child(name).child(
                h_flex()
                    .gap(px(6. * s))
                    .text_size(px(12. * s))
                    .text_color(p.text_2)
                    .child(
                        div()
                            .size(px(6.))
                            .flex_none()
                            .rounded_full()
                            .bg(if saved { p.ok } else { p.muted })
                            .when(saved, |d| d.shadow(vec![BoxShadow { inset: false, color: alpha(p.ok, 0.15), offset: point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(3.) }])),
                    )
                    .child(div().truncate().child(sub)),
            ),
        )
        .child(tier)
}

fn scaled_avatar(a: &Account, size: f32, cx: &App) -> Div {
    div()
        .relative()
        .size(px(size))
        .flex_none()
        .child(ui::avatar_circle(a, size, false, cx))
        .when_some(a.level, |d, lvl| d.child(ui::level_badge(lvl, size, cx)))
}

impl MainScreen {
    /// Avanza la animación de la rueda y suena el tick al pasar a otra cuenta.
    pub(crate) fn advance_wheel(&mut self, len: usize, window: &mut Window) {
        if self.wheel.advance() {
            window.request_animation_frame();
        }
        let idx = self.wheel.pos.round() as i32;
        if self.wheel.tick_index != Some(idx) {
            if self.wheel.tick_index.is_some() && len > 0 {
                crate::sounds::tick();
            }
            self.wheel.tick_index = Some(idx);
        }
    }

    /// Lleva la rueda a la cuenta `i` (de golpe con `jump`).
    pub(crate) fn wheel_to(&mut self, i: usize, jump: bool) {
        self.wheel.target = i as f32;
        if jump {
            self.wheel.pos = i as f32;
        }
    }

    /// "3 / 5": la cuenta frente a la que está la rueda.
    pub(crate) fn wheel_position_label(&self, len: usize) -> String {
        if len == 0 {
            return String::new();
        }
        format!("{} / {len}", (self.wheel.pos.round().max(0.) as usize).min(len - 1) + 1)
    }

    fn schedule_snap(&mut self, len: usize, cx: &mut Context<Self>) {
        self.wheel.snap = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(140)).await;
            this.update(cx, |this, cx| {
                this.wheel.target = this.wheel.target.round().clamp(0., len.saturating_sub(1) as f32);
                cx.notify();
            })
            .ok();
        }));
    }

    pub(crate) fn wheel_view(&mut self, list: &[Account], side_w: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let len = list.len();
        let item_w = side_w - 4.;
        let height = self.wheel.bounds.get().map(|b| f32::from(b.size.height)).unwrap_or(600.);
        let center = height / 2. - ITEM_H / 2.;
        let visible = height / STEP / 2. + 2.;
        let bounds_cell = self.wheel.bounds.clone();

        // Las más lejanas primero: la elegida queda encima de todas.
        let mut order: Vec<usize> = (0..len).filter(|i| (*i as f32 - self.wheel.pos).abs() <= visible).collect();
        order.sort_by(|a, b| (*b as f32 - self.wheel.pos).abs().total_cmp(&(*a as f32 - self.wheel.pos).abs()));

        let mut items: Vec<AnyElement> = Vec::new();
        for i in order {
            let a = &list[i];
            let d = i as f32 - self.wheel.pos;
            let sel = self.selected.as_ref() == Some(&a.id);
            let hovered = self.wheel.hovered == Some(i);
            // Curva: la del centro sobresale hacia la derecha y las de los extremos se van a la izquierda.
            let x = -(d * d * 7.5).min(170.) + if sel { 18. } else { 0. };
            let s = 1. - (d.abs() * 0.018).min(0.08);
            let (w, h) = (item_w * s, ITEM_H * s);
            let left = -30. + x + item_w * (1. - s) / 2.;
            let top = center + d * STEP + ITEM_H * (1. - s) / 2.;
            // Opacidad por distancia y desvanecido arriba y abajo (la máscara de la rueda en CSS).
            let yc = top + h / 2.;
            let edge = (yc / (height * 0.09)).clamp(0., 1.) * ((height - yc) / (height * 0.09)).clamp(0., 1.);
            let opacity = (1. - (d.abs() * 0.06).min(0.45)) * edge;
            if opacity <= 0.01 {
                continue;
            }
            let tier = ui::account_tier_color(a);
            let (fill, border) = if sel {
                (alpha(p.surface_2, 0.92), tier)
            } else if hovered {
                (alpha(p.surface, 0.86), alpha(p.text, 0.24))
            } else {
                (alpha(p.surface, 0.86), p.border_strong)
            };
            let vault = self.vault.read(cx);
            let (splash, _) = ui::splash_for(a, vault.store.session().map(|s| &s.history), |_| String::new());
            // Punto verde: lista para Jugar (tiene usuario y contraseña).
            let saved = !a.username.is_empty() && !a.password.is_empty();
            let r = 10. * s;
            let paint_fill = move |b: Bounds<Pixels>, _: (), window: &mut Window, _: &mut App| {
                if let Some(path) = plate_path(b.origin, f32::from(b.size.width), f32::from(b.size.height), r, None) {
                    window.paint_path(path, fill);
                }
            };
            let paint_edges = move |b: Bounds<Pixels>, _: (), window: &mut Window, _: &mut App| {
                let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
                if let Some(path) = stripe_path(b.origin, w, h, s) {
                    window.paint_path(path, alpha(tier, 0.9));
                }
                if let Some(path) = plate_path(b.origin, w, h, r, Some(1.)) {
                    window.paint_path(path, border);
                }
            };
            items.push(
                div()
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(w))
                    .h(px(h))
                    .opacity(opacity)
                    // Brillo del color de la liga detrás de la elegida.
                    .when(sel, |d| {
                        d.child(div().absolute().inset_0().rounded(px(r)).shadow(vec![
                            BoxShadow { inset: false, color: hsla(0., 0., 0., 0.55), offset: point(px(0.), px(10.)), blur_radius: px(34.), spread_radius: px(0.) },
                            BoxShadow { inset: false, color: alpha(tier, 0.8), offset: point(px(0.), px(0.)), blur_radius: px(26.), spread_radius: px(-4.) },
                        ]))
                    })
                    .child(canvas(|_, _, _| (), paint_fill).absolute().inset_0())
                    // El splash de la cuenta, tenue, como la imagen de cada mapa en osu!.
                    .when_some(splash, |d, url| d.child(plate_splash(url, w, h, r, if sel { 0.4 } else { 0.15 })))
                    .child(canvas(|_, _, _| (), paint_edges).absolute().inset_0())
                    .child(item_content(a, s, saved, &p, cx))
                    .into_any_element(),
            );
        }

        div()
            .id("wheel")
            .relative()
            .flex_1()
            .min_h_0()
            .cursor(if self.wheel.drag.as_ref().is_some_and(|d| d.moved) { CursorStyle::ClosedHand } else { CursorStyle::OpenHand })
            .child(canvas(move |b, _, _| bounds_cell.set(Some(b)), |_, _, _, _| {}).absolute().inset_0())
            // Se extiende 80 px sobre el panel derecho, para que el brillo de la elegida no se corte.
            .child(div().absolute().top_0().bottom_0().left_0().w(px(side_w + 80.)).children(items))
            .when(len == 0, |d| {
                d.child(div().absolute().inset_0().flex().items_center().justify_center().text_color(p.muted).child("Ninguna cuenta coincide con la búsqueda"))
            })
            .on_scroll(cx.listener(move |this, ev: &ScrollEvent, _, cx| {
                if len == 0 {
                    return;
                }
                let dy = f32::from(ev.delta.pixel_delta(px(33.)).y);
                this.wheel.target = (this.wheel.target - dy / 95.).clamp(-0.4, len as f32 - 0.6);
                this.schedule_snap(len, cx);
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    if len == 0 {
                        return;
                    }
                    this.wheel.snap = None;
                    let y = f32::from(ev.position.y);
                    let down = this.wheel.index_at(ev.position.y, len);
                    this.wheel.drag = Some(Drag { y, start: this.wheel.target, moved: false, last_y: y, last_t: Instant::now(), v: 0., down });
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(move |this, ev: &MouseMoveEvent, _, cx| {
                let hovered = this.wheel.index_at(ev.position.y, len);
                if hovered != this.wheel.hovered {
                    this.wheel.hovered = hovered;
                    cx.notify();
                }
                let Some(drag) = this.wheel.drag.as_mut() else { return };
                if ev.pressed_button != Some(MouseButton::Left) {
                    return;
                }
                let y = f32::from(ev.position.y);
                let dy = y - drag.y;
                if dy.abs() > 5. {
                    drag.moved = true;
                }
                if !drag.moved {
                    return;
                }
                let now = Instant::now();
                let dt = now.duration_since(drag.last_t).as_secs_f32().max(0.001) * 1000.;
                drag.v = 0.8 * drag.v + 0.2 * (-(y - drag.last_y) / STEP / dt);
                drag.last_y = y;
                drag.last_t = now;
                let t = (drag.start - dy / STEP).clamp(-0.4, len as f32 - 0.6);
                this.wheel.target = t;
                this.wheel.pos = t;
                cx.notify();
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(move |this, _: &MouseUpEvent, window, cx| this.end_drag(len, window, cx)))
            .on_mouse_up_out(MouseButton::Left, cx.listener(move |this, _: &MouseUpEvent, window, cx| this.end_drag(len, window, cx)))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !hovered && this.wheel.hovered.is_some() {
                    this.wheel.hovered = None;
                    cx.notify();
                }
            }))
    }

    /// Al soltar: si se arrastró, sigue con la inercia hasta una cuenta; si fue un clic, la elige.
    fn end_drag(&mut self, len: usize, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.wheel.drag.take() else { return };
        if drag.moved {
            self.wheel.target = (self.wheel.target + drag.v * 220.).round().clamp(0., len.saturating_sub(1) as f32);
        } else if let Some(i) = drag.down {
            let list = self.visible(cx);
            if let Some(a) = list.get(i) {
                self.select(a.id.clone(), cx);
            }
        }
        cx.notify();
    }
}
