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
        // Solo en desarrollo: SV_DEV_FPS anota cada cuánto llega un cuadro del fondo.
        #[cfg(debug_assertions)]
        if std::env::var("SV_DEV_FPS").is_ok() {
            thread_local!(static LAST: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) });
            let gap = LAST.with(|l| l.replace(Some(Instant::now()))).map(|t| t.elapsed().as_secs_f64() * 1000.).unwrap_or(0.);
            eprintln!("bg {gap:.1} {:.2} {:.2}", self.x, self.tx);
        }
        // Solo en desarrollo: SV_DEV_SWAY mueve el fondo solo, lento, para revisar que se deslice.
        #[cfg(debug_assertions)]
        if std::env::var("SV_DEV_SWAY").is_ok() {
            let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f32();
            self.tx = (t * 0.7).sin() * 11.;
            self.ty = (t * 0.5).cos() * 7.;
        }
        self.advance(window);
        let (splash, tier, empty) = self.main.upgrade().map(|m| m.read(cx).bg_scene(cx)).unwrap_or((None, crate::ui::tier_color(None), true));
        // Con la ventana translúcida, el fondo y el splash dejan ver el escritorio detrás.
        let o = crate::app::content_opacity();
        // El splash se ve más que el color de fondo: si no, con mucha transparencia casi desaparece y
        // solo queda el escritorio.
        let splash_o = 0.4 + 0.6 * o;
        let base = div().absolute().inset_0().overflow_hidden();
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
            // Halo del color de la liga, que se mueve con el mouse.
            .child(
                div()
                    .absolute()
                    .left(px(-30. + self.x))
                    .top(px(-30. + self.y))
                    .w(px(vw + 60.))
                    .h(px(vh + 60.))
                    .child(glow(0.75, 0.3, alpha(tier, 0.22), 420.)),
            )
            // El splash: GPUI pinta las imágenes en píxeles enteros, y el parallax avanza de a
            // fracciones de píxel; así se vería a saltos. Se dibuja en los cuatro píxeles enteros
            // vecinos mezclados según la parte fraccionaria (interpolación bilineal), como hace la
            // GPU con el translate3d de Electron: se desliza suave.
            .when_some(splash, |d, url| {
                let url = SharedString::from(crate::http::blurred(&url, 2));
                let (ix, iy) = (self.x.floor(), self.y.floor());
                let (fx, fy) = (self.x - ix, self.y - iy);
                // Opacidades para que, pintando una encima de otra, cada copia aporte su peso.
                let w11 = fx * fy;
                let w01 = if w11 < 0.999 { fy * (1. - fx) / (1. - w11) } else { 0. };
                let layer = |dx: f32, dy: f32, opacity: f32| {
                    img(url.clone())
                        .absolute()
                        .left(px(-30. + ix + dx))
                        .top(px(-30. + iy + dy))
                        .w(px(vw + 60.))
                        .h(px(vh + 60.))
                        .object_fit(ObjectFit::Cover)
                        .opacity(opacity * splash_o)
                };
                if o < 1. {
                    // Con la ventana translúcida, una sola copia en el píxel más cercano.
                    let (dx, dy) = (self.x.round() - ix, self.y.round() - iy);
                    return d.child(layer(dx, dy, 1.)).child(div().absolute().inset_0().bg(hsla(0., 0., 0., 0.16 * o)));
                }
                d.child(layer(0., 0., 1.))
                    .when(fx > 0.002, |d| d.child(layer(1., 0., fx)))
                    .when(w01 > 0.002, |d| d.child(layer(0., 1., w01)))
                    .when(w11 > 0.002, |d| d.child(layer(1., 1., w11)))
                    .child(div().absolute().inset_0().bg(hsla(0., 0., 0., 0.16)))
            })
            // Sombra suave detrás del nombre y los rangos, y velos para que la rueda y el texto se lean.
            .child(glow(0.6, 0.38, alpha(p.bg, 0.32 * o), 260.))
            .child(band(0., 0.36, alpha(p.bg, 0.74 * o), alpha(p.bg, 0.58 * o)))
            .child(band(0.36, 0.62, alpha(p.bg, 0.58 * o), alpha(p.bg, 0.28 * o)))
            .child(band(0.62, 1.0, alpha(p.bg, 0.28 * o), alpha(p.bg, 0.12 * o)))
            .child(div().absolute().left_0().right_0().bottom_0().h(relative(0.4)).bg(linear_gradient(
                180.,
                linear_color_stop(alpha(p.bg, 0.), 0.),
                linear_color_stop(alpha(p.bg, 0.92 * o), 1.),
            )))
    }
}
