//! Pantalla de bloqueo: crear la bóveda la primera vez o desbloquearla con la contraseña maestra.
//! El fondo es el splash de alguna de tus cuentas, al azar (como en la versión Electron).
use crate::assets;
use crate::state::{splash_url, Vault};
use crate::theme::palette;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{v_flex, Icon, Sizable};
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
    /// Bóveda encontrada en Google Drive cuando no hay una en este PC.
    from_drive: bool,
    searching_drive: bool,
    _subs: Vec<Subscription>,
}

impl EventEmitter<LockEvent> for LockScreen {}

impl LockScreen {
    pub fn new(vault: Entity<Vault>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = &vault.read(cx).store;
        let mode = if store.has_local() { Mode::Unlock } else { Mode::Create };
        // Desenfocado 3 px, como en la versión Electron.
        let splash = store.config.lock_skins.choose(&mut rand::thread_rng()).map(|id| crate::http::blurred(&splash_url(*id), 3).into());
        // Los demás fondos posibles quedan en el disco para la próxima vez.
        crate::http::prefetch(store.config.lock_skins.iter().map(|id| crate::http::blurred(&splash_url(*id), 3)));
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
        let mut this = Self { vault, pw1, pw2, mode, error: None, busy: false, splash, from_drive: false, searching_drive: false, _subs: subs };
        this.look_in_drive(window, cx);
        this
    }

    /// Sin bóveda en este PC pero con Google vinculado: se busca en Drive para desbloquearla.
    fn look_in_drive(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let v = self.vault.read(cx);
        if v.store.has_local() || !v.drive.status().linked {
            return;
        }
        let drive = v.drive.clone();
        self.searching_drive = true;
        cx.notify();
        let work = cx.background_spawn(async move { drive.download() });
        cx.spawn_in(window, async move |this, cx| {
            let res = work.await;
            this.update(cx, |this, cx| {
                this.searching_drive = false;
                match res {
                    Ok(Some(_)) => {
                        this.mode = Mode::Unlock;
                        this.from_drive = true;
                    }
                    Ok(None) => {}
                    Err(e) => this.error = Some(e.into()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
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
        let local = self.vault.read(cx).store.read_local();
        if self.mode == Mode::Unlock && local.is_none() && !self.from_drive {
            self.error = Some("No hay bóveda para desbloquear".into());
            cx.notify();
            return;
        }
        let drive = self.vault.read(cx).drive.clone();
        let linked = drive.status().linked;
        self.busy = true;
        cx.notify();
        // scrypt tarda y Drive es red: todo fuera del hilo de la interfaz. Al desbloquear se usa la
        // versión más nueva entre la de este PC y la de Drive.
        let create = self.mode == Mode::Create;
        let work = cx.background_spawn(async move {
            let (remote, remote_error) = if linked {
                match drive.download() {
                    Ok(r) => (r, None),
                    Err(e) => (None, Some(e)),
                }
            } else {
                (None, None)
            };
            if create {
                if remote.is_some() {
                    return Err("Ya tienes una bóveda en Drive: desbloquéala en vez de crear otra".to_string());
                }
                return Opened::new_vault(&pw).map(|o| (o, None, local, remote_error));
            }
            let (env, from_remote) = sv_core::drive::newest(local.clone(), remote.clone()).ok_or_else(|| {
                remote_error.clone().unwrap_or_else(|| "No hay bóveda para desbloquear".to_string())
            })?;
            let opened = Opened::from_envelope(&env, &pw)?;
            Ok((opened, Some((env, from_remote, remote)), local, remote_error))
        });
        let mode = self.mode;
        cx.spawn_in(window, async move |this, cx| {
            let opened = work.await;
            this.update_in(cx, |this, window, cx| {
                this.busy = false;
                let result = opened.and_then(|(opened, chosen, local, remote_error)| {
                    this.vault.update(cx, |v, cx| {
                        let res = v.change(cx, |store| match mode {
                            Mode::Create => store.finish_create(opened).map(|_| ()),
                            Mode::Unlock => store.finish_unlock(opened).map(|_| ()),
                        });
                        if res.is_ok() {
                            if let Some((env, from_remote, remote)) = chosen {
                                let local_at = local.as_ref().map(|l| l.updated_at.clone());
                                if from_remote && local_at.as_ref() != Some(&env.updated_at) {
                                    // Drive tenía una versión más nueva: queda también en este PC.
                                    let _ = v.store.write_local(&env);
                                } else if !from_remote && remote.as_ref().map(|r| &r.updated_at) != local_at.as_ref() {
                                    v.drive.queue_upload(env);
                                }
                            }
                            if let Some(e) = remote_error {
                                v.drive.set_sync(sv_core::drive::SyncState::Error(e));
                            }
                        }
                        res
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
        let hint = if self.searching_drive {
            "Buscando tu bóveda en Google Drive…"
        } else if create {
            "Crea tu contraseña maestra. No se puede recuperar: si la olvidas, pierdes la bóveda."
        } else if self.from_drive {
            "Encontramos tu bóveda en Google Drive. Ingresa tu contraseña maestra."
        } else {
            "Bóveda bloqueada. Ingresa tu contraseña maestra."
        };
        let google = self.vault.read(cx).drive.status();
        let lock_icon = || Icon::empty().path(assets::icon("lock")).text_color(p.muted);

        let card = v_flex()
            .w(px(380.))
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
            .child(Input::new(&self.pw1).prefix(lock_icon()).h(px(40.)))
            .when(create, |d| d.child(Input::new(&self.pw2).prefix(lock_icon()).h(px(40.))))
            .child(
                Button::new("lock-submit")
                    .primary()
                    .w_full()
                    .h(px(40.))
                    .text_size(px(14.))
                    .label(if create { "Crear bóveda" } else { "Desbloquear" })
                    .loading(self.busy)
                    .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
            )
            .when_some(self.error.clone(), |d, e| d.child(div().text_size(px(13.)).text_color(p.danger).child(e)))
            .child(
                v_flex()
                    .w_full()
                    .mt(px(14.))
                    .pt(px(16.))
                    .gap(px(6.))
                    .items_center()
                    .border_t_1()
                    .border_color(p.border)
                    .child(div().text_size(px(12.)).text_color(p.muted).child(if google.linked {
                        format!("Sincronizada con {}", google.email.clone().unwrap_or_else(|| "Google Drive".into()))
                    } else {
                        "¿Ya usabas Smurf Vault en otro PC?".to_string()
                    }))
                    .when(!google.linked, |d| {
                        d.child(
                            Button::new("lock-google")
                                .ghost()
                                .small()
                                .icon(Icon::empty().path(assets::icon("cloud")))
                                .label("Vincular Google Drive")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    let screen = cx.entity().downgrade();
                                    crate::google::link(this.vault.clone(), window, cx, move |window, cx| {
                                        // Ya vinculado: si hay una bóveda en Drive, se ofrece desbloquearla.
                                        screen.update(cx, |this, cx| this.look_in_drive(window, cx)).ok();
                                    });
                                })),
                        )
                    }),
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
