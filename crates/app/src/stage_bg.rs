//! Fondo de la pantalla principal: halo del color de la liga, el splash del perfil desenfocado y los
//! velos para que se lea el texto. Sigue un poco al mouse (parallax), como .stage-bg de Electron.
//!
//! Es una vista aparte y se dibuja detrás de la pantalla principal (que va en caché): así, al mover
//! el mouse solo se vuelve a dibujar el fondo, no la lista ni el detalle. Por eso se mueve fluido.
use crate::main_screen::MainScreen;
use crate::theme::palette;
use crate::ui::alpha;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::time::Instant;

/// Cuánto se mueve como máximo (px), igual que Electron: 22 de ancho y 14 de alto.
const RANGE_X: f32 = 22.;
const RANGE_Y: f32 = 14.;
/// Fracción del camino que recorre en cada cuadro a 60 fps (Electron: 0.06).
const EASE: f32 = 0.06;

pub struct StageBg {
    main: WeakEntity<MainScreen>,
    x: f32,
    y: f32,
    tx: f32,
    ty: f32,
    last: Option<Instant>,
}

impl StageBg {
    pub fn new(main: WeakEntity<MainScreen>) -> Self {
        Self { main, x: 0., y: 0., tx: 0., ty: 0., last: None }
    }

    /// Hacia dónde ir según el mouse (fracción 0..1 del ancho y del alto del fondo).
    pub fn follow(&mut self, fx: f32, fy: f32, cx: &mut Context<Self>) {
        let (tx, ty) = (-(fx - 0.5) * RANGE_X, -(fy - 0.5) * RANGE_Y);
        if (tx - self.tx).abs() > 0.01 || (ty - self.ty).abs() > 0.01 {
            self.tx = tx;
            self.ty = ty;
            cx.notify();
        }
    }

    /// Avanza hacia el destino según el tiempo real entre cuadros (igual de rápido a 60 o 144 Hz).
    fn advance(&mut self, window: &mut Window) {
        if (self.tx - self.x).abs() < 0.02 && (self.ty - self.y).abs() < 0.02 {
            self.x = self.tx;
            self.y = self.ty;
            self.last = None;
            return;
        }
        let now = Instant::now();
        let dt = self.last.map(|t| now.duration_since(t).as_secs_f32()).unwrap_or(1. / 60.).min(0.1);
        self.last = Some(now);
        let k = 1. - (1. - EASE).powf(dt * 60.);
        self.x += (self.tx - self.x) * k;
        self.y += (self.ty - self.y) * k;
        window.request_animation_frame();
    }
}

/// Franja de degradado horizontal entre dos posiciones (en fracción del ancho).
fn band(from: f32, to: f32, a: Hsla, b: Hsla) -> Div {
    div().absolute().top_0().bottom_0().left(relative(from)).w(relative(to - from)).bg(linear_gradient(90., linear_color_stop(a, 0.), linear_color_stop(b, 1.)))
}

impl Render for StageBg {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        self.advance(window);
        let (splash, tier, empty) = self.main.upgrade().map(|m| m.read(cx).bg_scene(cx)).unwrap_or((None, crate::ui::tier_color(None), true));
        let base = div().absolute().inset_0().overflow_hidden().bg(p.bg);
        if empty {
            return base;
        }
        let viewport = window.viewport_size();
        let (vw, vh) = (f32::from(viewport.width), f32::from(viewport.height) - 60.);
        let glow = |x: f32, y: f32, color: Hsla, blur: f32| {
            div().absolute().left(relative(x)).top(relative(y)).size(px(2.)).shadow(vec![BoxShadow {
                inset: false,
                color,
                offset: point(px(0.), px(0.)),
                blur_radius: px(blur),
                spread_radius: px(blur * 0.55),
            }])
        };
        base
            // Halo del color de la liga y el splash del perfil, que se mueven con el mouse.
            .child(
                div()
                    .absolute()
                    .left(px(-30. + self.x))
                    .top(px(-30. + self.y))
                    .w(px(vw + 60.))
                    .h(px(vh + 60.))
                    .child(glow(0.75, 0.3, alpha(tier, 0.22), 420.))
                    .when_some(splash, |d, url| {
                        // Desenfocado 2 px, como el fondo de la versión Electron.
                        d.child(img(SharedString::from(crate::http::blurred(&url, 2))).absolute().inset_0().size_full().object_fit(ObjectFit::Cover))
                            .child(div().absolute().inset_0().bg(hsla(0., 0., 0., 0.16)))
                    }),
            )
            // Sombra suave detrás del nombre y los rangos, y velos para que la rueda y el texto se lean.
            .child(glow(0.6, 0.38, alpha(p.bg, 0.32), 260.))
            .child(band(0., 0.36, alpha(p.bg, 0.74), alpha(p.bg, 0.58)))
            .child(band(0.36, 0.62, alpha(p.bg, 0.58), alpha(p.bg, 0.28)))
            .child(band(0.62, 1.0, alpha(p.bg, 0.28), alpha(p.bg, 0.12)))
            .child(div().absolute().left_0().right_0().bottom_0().h(relative(0.4)).bg(linear_gradient(
                180.,
                linear_color_stop(alpha(p.bg, 0.), 0.),
                linear_color_stop(alpha(p.bg, 0.92), 1.),
            )))
    }
}
