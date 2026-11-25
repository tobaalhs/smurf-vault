//! Pantalla de bloqueo: crear la bóveda la primera vez o desbloquearla con la contraseña maestra.
//! El fondo es el splash de alguna de tus cuentas, al azar (como en la versión Electron).
use crate::assets;
use crate::state::{splash_url, Vault};
use crate::theme::palette;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{v_flex, Icon, Sizable, WindowExt};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rand::seq::SliceRandom;
use sv_core::store::Opened;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Create,
    Unlock,
}

pub enum LockEvent {
    Unlocked,
}

pub struct LockScreen {
    vault: Entity<Vault>,
    pw1: Entity<InputState>,
    pw2: Entity<InputState>,
    mode: Mode,
    error: Option<SharedString>,
    busy: bool,
    splash: Option<SharedString>,
    _subs: Vec<Subscription>,
}

impl EventEmitter<LockEvent> for LockScreen {}

impl LockScreen {
    pub fn new(vault: Entity<Vault>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = &vault.read(cx).store;
        let mode = if store.has_local() { Mode::Unlock } else { Mode::Create };
        let splash = store.config.lock_skins.choose(&mut rand::thread_rng()).map(|id| splash_url(*id).into());
        let pw1 = cx.new(|cx| InputState::new(window, cx).masked(true).placeholder("Contraseña maestra"));
        let pw2 = cx.new(|cx| InputState::new(window, cx).masked(true).placeholder("Repite la contraseña"));
        let on_enter = |this: &mut Self, _: &Entity<InputState>, ev: &InputEvent, window: &mut Window, cx: &mut Context<Self>| {
            if let InputEvent::PressEnter { .. } = ev {
                this.submit(window, cx);
            }
        };
        let subs = vec![cx.subscribe_in(&pw1, window, on_enter), cx.subscribe_in(&pw2, window, on_enter)];
        pw1.update(cx, |s, cx| s.focus(window, cx));
        // Solo en desarrollo: SV_DEV_UNLOCK desbloquea sola una bóveda de prueba (ver examples/demo_vault).
        #[cfg(debug_assertions)]
        if let Ok(pw) = std::env::var("SV_DEV_UNLOCK") {
            cx.spawn_in(window, async move |this, cx| {
                this.update_in(cx, |this, window, cx| {
                    this.pw1.update(cx, |s, cx| s.set_value(pw.clone(), window, cx));
                    this.submit(window, cx);
                })
                .ok();
            })
            .detach();
        }
        Self { vault, pw1, pw2, mode, error: None, busy: false, splash, _subs: subs }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let pw = self.pw1.read(cx).value().to_string();
        self.error = None;
        if self.mode == Mode::Create && pw != self.pw2.read(cx).value().as_ref() {
            self.error = Some("Las contraseñas no coinciden".into());
            cx.notify();
            return;
        }
        let envelope = match self.mode {
            Mode::Unlock => match self.vault.read(cx).store.read_local() {
                Some(env) => Some(env),
                None => {
                    self.error = Some("No hay bóveda para desbloquear".into());
                    cx.notify();
                    return;
                }
            },
            Mode::Create => None,
        };
        self.busy = true;
        cx.notify();
        // scrypt tarda: se calcula fuera del hilo de la interfaz.
        let work = cx.background_spawn(async move {
            match envelope {
                Some(env) => Opened::from_envelope(&env, &pw),
                None => Opened::new_vault(&pw),
            }
        });
        let mode = self.mode;
        cx.spawn_in(window, async move |this, cx| {
            let opened = work.await;
            this.update_in(cx, |this, window, cx| {
                this.busy = false;
                let result = opened.and_then(|opened| {
                    this.vault.update(cx, |v, cx| {
                        v.change(cx, |store| match mode {
                            Mode::Create => store.finish_create(opened).map(|_| ()),
                            Mode::Unlock => store.finish_unlock(opened).map(|_| ()),
                        })
                    })
                });
                match result {
                    Ok(()) => cx.emit(LockEvent::Unlocked),
                    Err(e) => {
                        this.error = Some(e.into());
                        this.pw1.update(cx, |s, cx| {
                            s.focus(window, cx);
                            s.select_all(window, cx);
                        });
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for LockScreen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let create = self.mode == Mode::Create;
        let hint = if create {
            "Crea tu contraseña maestra. No se puede recuperar: si la olvidas, pierdes la bóveda."
        } else {
            "Bóveda bloqueada. Ingresa tu contraseña maestra."
        };
        let lock_icon = || Icon::empty().path(assets::icon("lock")).text_color(p.muted);

        let card = v_flex()
            .w(px(360.))
            .px(px(30.))
            .pt(px(34.))
            .pb(px(24.))
            .gap(px(12.))
            .items_center()
            .rounded(px(18.))
            .bg(Hsla { a: 0.9, ..p.surface })
            .border_1()
            .border_color(p.border_strong)
            .shadow_lg()
            .child(img(assets::logo()).size(px(84.)).rounded(px(20.)))
            .child(div().text_size(px(22.)).font_weight(FontWeight::BOLD).text_color(p.text).child("Smurf Vault"))
            .child(div().text_center().text_size(px(13.)).text_color(p.muted).child(hint))
            .child(Input::new(&self.pw1).prefix(lock_icon()))
            .when(create, |d| d.child(Input::new(&self.pw2).prefix(lock_icon())))
            .child(
                Button::new("lock-submit")
                    .primary()
                    .w_full()
                    .label(if create { "Crear bóveda" } else { "Desbloquear" })
                    .loading(self.busy)
                    .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
            )
            .when_some(self.error.clone(), |d, e| d.child(div().text_size(px(13.)).text_color(p.danger).child(e)))
            .child(
                v_flex()
                    .mt(px(6.))
                    .gap(px(6.))
                    .items_center()
                    .child(div().text_size(px(12.)).text_color(p.muted).child("¿Ya usabas Smurf Vault en otro PC?"))
                    .child(
                        Button::new("lock-google")
                            .ghost()
                            .small()
                            .icon(Icon::empty().path(assets::icon("cloud")))
                            .label("Vincular Google Drive")
                            .on_click(|_, window, cx| {
                                window.push_notification(
                                    Notification::info("Google Drive todavía no está en la versión Rust: llega en una próxima fase."),
                                    cx,
                                )
                            }),
                    ),
            );

        div()
            .size_full()
            .relative()
            .bg(p.bg)
            .overflow_hidden()
            .when_some(self.splash.clone(), |d, url| {
                d.child(img(url).absolute().inset_0().size_full().object_fit(ObjectFit::Cover).opacity(0.5))
            })
            .child(div().absolute().inset_0().bg(linear_gradient(
                180.,
                linear_color_stop(Hsla { a: 0.35, ..p.bg }, 0.),
                linear_color_stop(Hsla { a: 0.85, ..p.bg }, 1.),
            )))
            .child(div().absolute().inset_0().flex().items_center().justify_center().child(card))
    }
}
