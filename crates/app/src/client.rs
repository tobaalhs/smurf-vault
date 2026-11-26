//! Lo que conecta la app con el juego: detectar la cuenta abierta en el cliente de LoL (cada 30 s y
//! con Actualizar), vigilar el fin de las partidas, actualizar con la API de Riot y descargar los
//! datos del juego. Igual que detect/refreshRanks de renderer.js y detectClient/onGameEnd de main.js.
//!
//! Las consultas son bloqueantes y van en hilos de fondo; lo que cambia la bóveda vuelve al hilo de
//! la interfaz.
use crate::assets;
use crate::dialogs::{below_bar, dialog_title};
use crate::main_screen::MainScreen;
use crate::state::{RiotProgress, Vault};
use crate::theme::palette;
use crate::ui::{self, alpha};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{h_flex, v_flex, Icon, IconName, Sizable, WindowExt};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::time::Duration;
use sv_core::gamedata::GameData;
use sv_core::lcu::lcu;
use sv_core::snapshot::Snapshot;

const DETECT_EVERY: Duration = Duration::from_secs(30);
const GAMEFLOW_EVERY: Duration = Duration::from_secs(5);
const PLAYING: [&str; 2] = ["InProgress", "Reconnect"];

impl MainScreen {
    /// Arranca la detección periódica y el vigilante de partidas. Se cortan solos cuando la pantalla
    /// principal se cierra (al bloquear).
    pub(crate) fn start_client_watch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let detect = cx.spawn_in(window, async move |this, cx| loop {
            if this.update_in(cx, |this, window, cx| this.detect(false, None, window, cx)).is_err() {
                break;
            }
            cx.background_executor().timer(DETECT_EVERY).await;
        });
        let gameflow = cx.spawn_in(window, async move |this, cx| {
            let mut prev: Option<String> = None;
            loop {
                cx.background_executor().timer(GAMEFLOW_EVERY).await;
                let phase = cx.background_executor().spawn(async { lcu().gameflow_phase() }).await;
                // Estaba jugando y ahora el cliente está en otra fase (estadísticas, lobby...). Si el
                // cliente se cerró (None) no cuenta.
                let ended = prev.as_deref().is_some_and(|p| PLAYING.contains(&p))
                    && phase.as_deref().is_some_and(|p| !PLAYING.contains(&p));
                prev = phase;
                if ended && this.update_in(cx, |this, window, cx| this.on_game_end(window, cx)).is_err() {
                    break;
                }
            }
        });
        self.client_tasks = vec![detect, gameflow];
    }

    /// Lee la cuenta abierta en el cliente y, si está guardada, la actualiza. `manual` avisa también
    /// cuando no hay cliente; `expected` es la cuenta desde la que se tocó Actualizar.
    pub(crate) fn detect(&mut self, manual: bool, expected: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        if self.detecting {
            return;
        }
        self.detecting = true;
        cx.notify();
        let work = cx.background_spawn(async move { lcu().current_account(manual) });
        cx.spawn_in(window, async move |this, cx| {
            let snap = work.await;
            this.update_in(cx, |this, window, cx| {
                this.detecting = false;
                this.apply_detected(snap, manual, expected, window, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn apply_detected(&mut self, snap: Option<Snapshot>, manual: bool, expected: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snap) = snap else {
            if manual {
                window.push_notification(Notification::error("No encontré el cliente de LoL abierto con una sesión iniciada"), cx);
            }
            return;
        };
        self.detect_hits += 1;
        self.guide = None;
        let name = snap.riot_id();
        let found = self.vault.read(cx).store.find_detected(&snap);
        let Some(id) = found else {
            // Una cuenta que no está guardada: se ofrece vincularla (una vez, salvo que se pida).
            if manual || !self.seen_puuids.contains(&snap.puuid) {
                self.seen_puuids.insert(snap.puuid.clone());
                self.detected = Some(snap);
            }
            if let Some(expected) = expected {
                self.warn_other_account(&expected, &name, window, cx);
            }
            return;
        };
        let (auto_linked, label) = {
            let acc = self.vault.read(cx).store.data().and_then(|d| d.account(&id));
            let label = acc.map(|a| if a.label.is_empty() { a.username.clone() } else { a.label.clone() }).unwrap_or_default();
            (acc.is_some_and(|a| a.puuid.as_deref() != Some(snap.puuid.as_str())), label)
        };
        if let Err(e) = self.vault.update(cx, |v, cx| v.change(cx, |s| s.link_snapshot(&id, &snap))) {
            window.push_notification(Notification::error(e), cx);
            return;
        }
        if auto_linked {
            window.push_notification(Notification::success(format!("{name} reconocida y vinculada a \"{label}\"")), cx);
        } else if manual || !self.seen_puuids.contains(&snap.puuid) {
            window.push_notification(Notification::success(format!("{name} actualizada desde el cliente")), cx);
        }
        self.seen_puuids.insert(snap.puuid.clone());
        self.detected = None;
        if let Some(expected) = expected.filter(|e| *e != id) {
            self.warn_other_account(&expected, &name, window, cx);
        }
    }

    fn warn_other_account(&self, _expected: &str, open: &str, window: &mut Window, cx: &mut App) {
        window.push_notification(
            Notification::error(format!("En el cliente está abierta {open}, no esta cuenta. Entra con Jugar para actualizarla.")),
            cx,
        );
    }

    /// Terminó una partida: la cuenta jugó ahora, y el rango se vuelve a leer cuando el cliente lo
    /// actualiza (tarda unos segundos).
    fn on_game_end(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let vault = self.vault.clone();
        cx.spawn_in(window, async move |this, cx| {
            let puuid = cx
                .background_executor()
                .spawn(async { lcu().get("/lol-summoner/v1/current-summoner").and_then(|s| s["puuid"].as_str().map(str::to_string)) })
                .await;
            if let Some(puuid) = puuid {
                let at = sv_core::store::now_iso();
                vault.update(cx, |v, cx| v.change(cx, |s| s.mark_played(&puuid, &at)));
            }
            for wait in [15, 45] {
                cx.background_executor().timer(Duration::from_secs(wait)).await;
                if this.update_in(cx, |this, window, cx| this.detect(false, None, window, cx)).is_err() {
                    return;
                }
            }
        })
        .detach();
    }

    /// Vincula la cuenta detectada a una guardada (desde el diálogo "¿Qué cuenta es?").
    pub(crate) fn link_detected(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snap) = self.detected.take() else { return };
        let res = self.vault.update(cx, |v, cx| v.change(cx, |s| s.link_snapshot(id, &snap)));
        match res {
            Ok(_) => window.push_notification(Notification::success("Cuenta vinculada"), cx),
            Err(e) => window.push_notification(Notification::error(e), cx),
        }
        self.select(id.to_string(), cx);
    }

    /// Abre Nueva cuenta ya rellenada con la cuenta detectada: se guarda al tocar Guardar.
    pub(crate) fn save_detected_as_new(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snap) = self.detected.take() else { return };
        cx.notify();
        crate::dialogs::open_add_detected(self.vault.clone(), snap, window, cx);
    }
}

impl MainScreen {
    /// Aviso de cuenta detectada sin vincular (arriba) y avance de la API de Riot (abajo a la derecha).
    pub(crate) fn client_overlays(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let p = palette(cx);
        let mut out = Vec::new();
        // Arriba del detalle, sin tapar la búsqueda de la izquierda: la guía del inicio de sesión y
        // el aviso de cuenta detectada sin vincular.
        let mut top: Vec<AnyElement> = Vec::new();
        if let Some(text) = self.guide.clone() {
            top.push(
                h_flex()
                    .id("guide-banner")
                    .occlude()
                    .w_full()
                    .gap(px(12.))
                    .py(px(10.))
                    .pl(px(14.))
                    .pr(px(8.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(alpha(p.gold, 0.35))
                    .bg(p.surface)
                    .shadow_lg()
                    .child(Icon::empty().path(assets::icon("log-in")).text_color(p.gold))
                    .child(div().flex_1().min_w_0().text_size(px(13.)).text_color(p.text_2).child(text))
                    .child(Button::new("guide-close").ghost().small().icon(Icon::new(IconName::Close)).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.guide = None;
                            cx.notify();
                        },
                    )))
                    .into_any_element(),
            );
        }
        if let Some(snap) = &self.detected {
            let who = format!("{} · {} · nivel {}", snap.riot_id(), snap.server, snap.level.unwrap_or(0));
            let intro = "Abierta en el cliente y sin guardar:";
            top.push(
                h_flex()
                    .id("detect-banner")
                    .occlude()
                    .w_full()
                    .gap(px(12.))
                    .py(px(8.))
                    .pl(px(14.))
                    .pr(px(8.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(alpha(p.cyan, 0.25))
                    .bg(p.surface)
                    .shadow_lg()
                    .child(Icon::empty().path(assets::icon("gamepad-2")).text_color(p.cyan))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().text_size(px(12.)).text_color(p.text_2).child(intro))
                            .child(div().text_size(px(13.5)).font_weight(FontWeight::BOLD).text_color(p.text).truncate().child(who)),
                    )
                    .child(Button::new("banner-link").small().label("Es una que ya tengo").on_click(cx.listener(
                        |this, _, window, cx| this.open_link_dialog(window, cx),
                    )))
                    .child(Button::new("banner-new").primary().small().label("Agregar cuenta").on_click(cx.listener(
                        |this, _, window, cx| this.save_detected_as_new(window, cx),
                    )))
                    .child(Button::new("banner-close").ghost().small().icon(Icon::new(IconName::Close)).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.detected = None;
                            cx.notify();
                        },
                    )))
                    .into_any_element(),
            );
        }
        if !top.is_empty() {
            out.push(
                v_flex()
                    .absolute()
                    .top(px(14.))
                    .left(px(self.side_w + 22.))
                    .right(px(22.))
                    .gap(px(8.))
                    .children(top)
                    .into_any_element(),
            );
        }
        if let Some(prog) = self.vault.read(cx).riot_progress.clone() {
            let pct = if prog.total == 0 { 0. } else { prog.done as f32 / prog.total as f32 };
            let detail = format!(
                "{} de {}{}",
                (prog.done + 1).min(prog.total),
                prog.total,
                if prog.name.is_empty() { String::new() } else { format!(" · {}", prog.name) }
            );
            out.push(
                v_flex()
                    .absolute()
                    .right(px(22.))
                    .bottom(px(22.))
                    .w(px(320.))
                    .gap(px(3.))
                    .p(px(12.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(p.border_strong)
                    .bg(p.surface_2)
                    .shadow_lg()
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .child(Icon::new(IconName::LoaderCircle).text_color(p.cyan))
                            .child(div().text_size(px(13.)).font_weight(FontWeight::BOLD).child("Actualizando con la API de Riot")),
                    )
                    .child(div().text_size(px(12.)).text_color(p.text_2).truncate().child(detail))
                    .child(
                        div()
                            .mt(px(2.))
                            .h(px(3.))
                            .rounded(px(3.))
                            .bg(p.surface_3)
                            .child(div().h_full().w(relative(pct)).rounded(px(3.)).bg(p.cyan)),
                    )
                    .into_any_element(),
            );
        }
        out
    }

    /// "¿Qué cuenta es?": elige la cuenta guardada con la que se inició sesión, o guárdala como nueva.
    fn open_link_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snap) = self.detected.clone() else { return };
        let screen = cx.entity().downgrade();
        let vault = self.vault.clone();
        window.open_dialog(cx, move |dialog, _, cx| {
            let p = palette(cx);
            let accounts = vault.read(cx).store.data().map(|d| d.accounts.clone()).unwrap_or_default();
            // Primero las que no están vinculadas.
            let mut list: Vec<_> = accounts.iter().filter(|a| a.puuid.is_none()).collect();
            list.extend(accounts.iter().filter(|a| a.puuid.is_some()));
            let rows = list.into_iter().map(|a| {
                let (id, screen) = (a.id.clone(), screen.clone());
                let sub = [
                    if a.riot_id().is_empty() { if a.label.is_empty() { "Sin Riot ID".to_string() } else { a.label.clone() } } else { a.riot_id() },
                    a.server.clone(),
                ]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
                h_flex()
                    .id(SharedString::from(format!("link-{}", a.id)))
                    .gap(px(10.))
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(p.border)
                    .bg(p.surface_2)
                    .cursor_pointer()
                    .hover(|s| s.bg(p.surface_3).border_color(p.border_strong))
                    .child(Icon::new(IconName::User).text_color(p.muted))
                    .child(
                        v_flex()
                            .flex_1()
                            .child(div().text_size(px(13.5)).font_weight(FontWeight::SEMIBOLD).child(a.username.clone()))
                            .child(div().text_size(px(12.)).text_color(p.muted).child(sub)),
                    )
                    .when(a.puuid.is_some(), |d| d.child(ui::chip("Ya vinculada", p.text_2, p.surface_3, p.border)))
                    .on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        screen.update(cx, |s, cx| s.link_detected(&id, window, cx)).ok();
                    })
            });
            let screen_new = screen.clone();
            below_bar(dialog)
                .title(dialog_title("¿Qué cuenta es?"))
                .w(px(520.))
                .child(
                    v_flex()
                        .gap(px(10.))
                        .child(div().text_size(px(13.)).text_color(p.text_2).child(format!(
                            "En el cliente está {} ({}, nivel {}). Elige con qué cuenta guardada iniciaste sesión:",
                            snap.riot_id(),
                            snap.server,
                            snap.level.unwrap_or(0)
                        )))
                        .child(
                            v_flex()
                                .id("link-list")
                                .max_h(px(360.))
                                .overflow_y_scroll()
                                .gap(px(6.))
                                .children(rows),
                        ),
                )
                .footer(
                    h_flex().w_full().child(
                        Button::new("link-new").icon(Icon::new(IconName::Plus)).label("Es una cuenta nueva").on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            screen_new.update(cx, |s, cx| s.save_detected_as_new(window, cx)).ok();
                        }),
                    ),
                )
        });
    }
}

/// Actualiza con la API de Riot las cuentas `ids` (todas si es None). Sin API key, la pide primero.
pub fn refresh_riot(vault: Entity<Vault>, ids: Option<Vec<String>>, window: &mut Window, cx: &mut App) {
    let key = vault.read(cx).store.data().map(|d| d.settings.riot_api_key.clone()).unwrap_or_default();
    if key.is_empty() {
        crate::dialogs::open_riot_key(vault, ids, window, cx);
        return;
    }
    if vault.read(cx).riot_busy {
        window.push_notification(Notification::error("Ya se están actualizando las cuentas"), cx);
        return;
    }
    let targets: Vec<_> = {
        let Some(data) = vault.read(cx).store.data() else { return };
        match &ids {
            Some(ids) => data.accounts.iter().filter(|a| ids.contains(&a.id)).cloned().collect(),
            None => data.accounts.clone(),
        }
    };
    let all = ids.is_none();
    let total = targets.len();
    vault.update(cx, |v, cx| {
        v.riot_busy = true;
        v.riot_progress = all.then(|| RiotProgress { done: 0, total, name: String::new() });
        cx.notify();
    });
    window
        .spawn(cx, async move |cx| {
            let mut errors: Vec<String> = Vec::new();
            for (i, acc) in targets.into_iter().enumerate() {
                let name = acc.display_name();
                vault
                    .update(cx, |v, cx| {
                        if all {
                            v.riot_progress = Some(RiotProgress { done: i, total, name: name.clone() });
                        }
                        cx.notify();
                    });
                let key = key.clone();
                let acc_bg = acc.clone();
                let res = cx.background_executor().spawn(async move { sv_core::riot::lookup(&key, &acc_bg) }).await;
                let applied = vault.update(cx, |v, cx| match res {
                    Ok(snap) => v.change(cx, |s| s.apply_snapshot(&acc.id, &snap)).map(|_| true),
                    Err(e) => {
                        errors.push(format!("{name}: {e}"));
                        Ok(true)
                    }
                });
                // Si se bloqueó la bóveda a mitad, se corta acá.
                if !matches!(applied, Ok(true)) {
                    break;
                }
                cx.background_executor().timer(Duration::from_millis(150)).await;
            }
            cx.update(|window, cx| {
                let saved = vault.update(cx, |v, cx| {
                    v.riot_busy = false;
                    v.riot_progress = None;
                    let saved = v.store.is_unlocked().then(|| v.store.persist().map(|_| ()));
                    v.flush_upload();
                    cx.notify();
                    saved
                });
                if let Some(Err(e)) = saved {
                    window.push_notification(Notification::error(e), cx);
                }
                let ok = total - errors.len();
                if all {
                    let text = format!("{ok} de {total} cuentas actualizadas con la API de Riot");
                    let n = if errors.is_empty() { Notification::success(text) } else { Notification::info(text) };
                    window.push_notification(n, cx);
                } else if errors.is_empty() {
                    window.push_notification(Notification::success("Cuenta actualizada"), cx);
                }
                for e in errors.iter().take(3) {
                    window.push_notification(Notification::error(e.clone()), cx);
                }
                if errors.len() > 3 {
                    window.push_notification(Notification::error(format!("…y {} errores más", errors.len() - 3)), cx);
                }
            })
            .ok();
        })
        .detach();
}

/// Renueva en segundo plano los datos del juego si el caché tiene más de un día.
pub fn refresh_game_data(vault: Entity<Vault>, cx: &mut App) {
    if !vault.read(cx).game.is_stale() {
        return;
    }
    let paths = vault.read(cx).store.paths.clone();
    cx.spawn(async move |cx| {
        let fresh = cx.background_executor().spawn(async move { GameData::fetch_fresh(&paths) }).await;
        // Sin internet se sigue con el caché.
        if let Ok(game) = fresh {
            vault
                .update(cx, |v, cx| {
                    v.game = game;
                    cx.notify();
                });
        }
    })
    .detach();
}
