//! Panel de detalle de la cuenta elegida: encabezado (avatar con marco, nombre, chips), pestañas
//! (Resumen, Historial, Progreso, Campeones) y acciones (Jugar, Editar, Actualizar).
use crate::assets;
use crate::main_screen::{MainScreen, Tab};
use crate::theme::{palette, Palette};
use crate::ui::{self, alpha, hex};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{h_flex, v_flex, Icon, IconName, Sizable, WindowExt};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use sv_core::local::LpPoint;
use sv_core::matches::Match;
use sv_core::model::{Account, Rank};
use sv_core::ranks::{lp_delta, rank_label};

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

/// Color del resultado de una partida (borde de abajo en las miniaturas).
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
    Some(
        div()
            .text_size(px(11.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(if up { p.ok } else { p.loss })
            .child(format!("{} {text}", if up { "▲" } else { "▼" })),
    )
}

fn rank_card(title: &'static str, r: Option<&Rank>, lp: Option<&Vec<LpPoint>>, p: &Palette) -> Div {
    let base = h_flex()
        .gap(px(12.))
        .min_w(px(220.))
        .flex_1()
        .p(px(14.))
        .rounded(px(12.))
        .bg(alpha(p.surface, 0.82))
        .border_1()
        .border_color(p.border_strong);
    let Some(r) = r else {
        return base.child(
            v_flex()
                .gap(px(2.))
                .child(div().text_size(px(12.)).text_color(p.muted).child(title))
                .child(div().text_size(px(16.)).font_weight(FontWeight::BOLD).text_color(p.muted).child("Sin rango")),
        );
    };
    let games = r.wins + r.losses;
    let wr = if games > 0 { (r.wins as f32 / games as f32 * 100.).round() as i64 } else { 0 };
    base.child(img(ui::rank_emblem(r)).size(px(56.)).object_fit(ObjectFit::Contain)).child(
        v_flex()
            .flex_1()
            .gap(px(3.))
            .child(div().text_size(px(12.)).text_color(p.muted).child(title))
            .child(
                h_flex()
                    .gap(px(6.))
                    .items_baseline()
                    .child(div().text_size(px(16.)).font_weight(FontWeight::BOLD).text_color(ui::tier_color(Some(&r.tier))).child(rank_label(&r.tier, &r.division)))
                    .child(div().text_size(px(12.)).text_color(p.text_2).child(format!("{} LP", r.lp))),
            )
            .child(
                h_flex()
                    .gap(px(8.))
                    .text_size(px(12.))
                    .text_color(p.text_2)
                    .child(if games > 0 { format!("{}V {}D · {wr}%", r.wins, r.losses) } else { "Sin partidas".into() })
                    .children(delta_badge(lp, p)),
            )
            .child(
                div()
                    .h(px(4.))
                    .w_full()
                    .rounded_full()
                    .bg(p.surface_3)
                    .child(div().h_full().w(relative(wr as f32 / 100.)).rounded_full().bg(if wr >= 50 { p.ok } else { p.muted })),
            ),
    )
}

/// Miniatura de una partida: el campeón (LoL) o el puesto (TFT), con el color del resultado abajo.
pub(crate) fn mini_match(m: &Match, p: &Palette) -> Div {
    let color = result_color(m, p);
    let inner = if m.mode == "tft" {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(alpha(color, 0.18))
            .text_color(color)
            .font_weight(FontWeight::BOLD)
            .text_size(px(13.))
            .child(m.placement.map(|x| x.to_string()).unwrap_or_else(|| "?".into()))
            .into_any_element()
    } else {
        match m.champ {
            Some(c) => img(ui::champion_icon_url(c)).size_full().object_fit(ObjectFit::Cover).into_any_element(),
            None => div().into_any_element(),
        }
    };
    div()
        .relative()
        .size(px(32.))
        .flex_none()
        .rounded(px(6.))
        .overflow_hidden()
        .bg(p.surface_3)
        .child(inner)
        .child(div().absolute().left_0().right_0().bottom_0().h(px(3.)).bg(color))
}

impl MainScreen {
    pub(crate) fn play_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_none() {
            return;
        }
        window.push_notification(
            Notification::info("Jugar con un clic llega en la fase de cambio de cuenta (sesiones guardadas e inyección)."),
            cx,
        );
    }

    fn toggle_favorite(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let result = self.vault.update(cx, |v, cx| {
            let fav = v.store.data().and_then(|d| d.account(&id)).is_some_and(|a| a.is_favorite());
            v.change(cx, |s| s.set_favorite(&id, !fav))
        });
        if let Err(e) = result {
            window.push_notification(Notification::error(e), cx);
        }
    }

    fn hero(&self, a: &Account, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let frame = ui::crest_frame(a);
        let space = frame.as_ref().map(|(_, _, s)| *s).unwrap_or(10.);
        let saved = self.sessions.get(&a.id).cloned();
        let ban = ui::ban_info(a);
        let id = a.id.clone();

        // El marco sobresale del avatar: se dibuja encima, centrado sobre él.
        let avatar = div()
            .relative()
            .size(px(92.))
            .flex_none()
            .when(frame.is_some(), |d| d.ml(px(64.)).mr(px(58.)))
            .child(ui::avatar(a, 92., cx))
            .when_some(frame, |d, (src, b, _)| {
                d.child(img(src).absolute().left(b.origin.x).top(b.origin.y).w(b.size.width).h(b.size.height).object_fit(ObjectFit::Contain))
            });

        let name = h_flex().gap(px(8.)).items_baseline().map(|d| {
            if a.game_name.is_empty() {
                d.child(if a.label.is_empty() { a.username.clone() } else { a.label.clone() })
            } else {
                d.child(a.game_name.clone()).child(div().text_size(px(22.)).font_weight(FontWeight::MEDIUM).text_color(p.text_2).child(format!("#{}", a.tag_line)))
            }
        });

        let chips = h_flex()
            .gap(px(6.))
            .flex_wrap()
            .when(!a.server.is_empty(), |d| d.child(ui::chip(a.server.clone(), p.text_2, alpha(p.surface_3, 0.8), p.border_strong)))
            .when(!a.label.is_empty(), |d| d.child(ui::chip(a.label.clone(), p.gold_hi, alpha(p.gold, 0.12), alpha(p.gold, 0.3))))
            .child(match &saved {
                Some(at) => ui::chip(format!("Sesión guardada · {}", ui::ago(Some(at))), p.ok, alpha(p.ok, 0.1), alpha(p.ok, 0.35)),
                None => ui::chip("Sin sesión guardada", p.muted, alpha(p.surface_3, 0.6), p.border_strong),
            })
            .when_some(ban, |d, (chip, _, _)| d.child(ui::chip(chip, p.loss, alpha(p.loss, 0.12), alpha(p.loss, 0.4))));

        h_flex()
            .gap(px(22.))
            .pt(px(space))
            .child(avatar)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(8.))
                    .child(div().text_size(px(30.)).font_weight(FontWeight::BOLD).text_color(p.text).child(name))
                    .child(chips),
            )
            .child(
                Button::new("fav")
                    .ghost()
                    .icon(Icon::new(if a.is_favorite() { IconName::StarFill } else { IconName::Star }).text_color(if a.is_favorite() { p.gold } else { p.muted }))
                    .tooltip(if a.is_favorite() { "Quitar de favoritas" } else { "Favorita: queda siempre arriba" })
                    .on_click(cx.listener(move |this, _, window, cx| this.toggle_favorite(id.clone(), window, cx))),
            )
    }

    fn tabs(&self, count: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let tab = |t: Tab, label: String, cx: &mut Context<Self>| {
            let on = self.tab == t;
            div()
                .id(SharedString::from(format!("tab-{label}")))
                .px(px(14.))
                .py(px(9.))
                .cursor_pointer()
                .text_size(px(13.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if on { p.text } else { p.text_2 })
                .border_b_2()
                .border_color(if on { p.gold } else { transparent_black() })
                .hover(|s| s.text_color(p.text))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.tab = t;
                    cx.notify();
                }))
        };
        let history = if count > 0 { format!("Historial ({count})") } else { "Historial".into() };
        h_flex()
            .mt(px(26.))
            .gap(px(4.))
            .border_b_1()
            .border_color(p.border)
            .child(tab(Tab::Summary, "Resumen".into(), cx))
            .child(tab(Tab::History, history, cx))
            .child(tab(Tab::Progress, "Progreso".into(), cx))
            .child(tab(Tab::Champions, "Campeones".into(), cx))
    }

    fn summary(&self, a: &Account, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let session = self.vault.read(cx).store.session();
        let matches: Vec<Match> = session.and_then(|s| s.history.get(&a.id)).cloned().unwrap_or_default();
        let lp = session.and_then(|s| s.lp.get(&a.id)).cloned().unwrap_or_default();

        let strip = |mode: &'static str, label: &'static str, cx: &mut Context<Self>| {
            let list: Vec<&Match> = matches.iter().filter(|m| m.mode == mode).take(10).collect();
            (!list.is_empty()).then(|| {
                h_flex()
                    .gap(px(12.))
                    .child(div().w(px(110.)).text_size(px(12.)).text_color(p.muted).child(label))
                    .child(h_flex().gap(px(5.)).children(list.into_iter().map(|m| mini_match(m, &p))))
                    .child(
                        Button::new(SharedString::from(format!("see-{mode}")))
                            .ghost()
                            .small()
                            .label("Ver historial")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.tab = Tab::History;
                                this.history_mode = mode;
                                cx.notify();
                            })),
                    )
            })
        };

        let cred = |id: &'static str, label: &'static str, value: Option<String>, secret: String, cx: &mut Context<Self>| {
            let shown = match &value {
                Some(v) if id == "copy-pass" && !v.is_empty() => "••••••••".to_string(),
                Some(v) if !v.is_empty() => v.clone(),
                _ => "Sin guardar".into(),
            };
            div()
                .id(id)
                .flex_1()
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(14.))
                .py(px(10.))
                .rounded(px(10.))
                .bg(alpha(p.surface, 0.82))
                .border_1()
                .border_color(p.border_strong)
                .cursor_pointer()
                .hover(|s| s.border_color(alpha(p.gold, 0.5)))
                .child(div().text_size(px(11.5)).text_color(p.muted).child(label))
                .child(div().flex_1().truncate().text_size(px(13.5)).text_color(p.text).child(shown))
                .child(Icon::new(IconName::Copy).small().text_color(p.muted))
                .on_click(cx.listener(move |_, _, window, cx| {
                    if secret.is_empty() {
                        return;
                    }
                    ui::copy_secret(secret.clone(), cx);
                    window.push_notification(Notification::success(format!("{label} copiado (se borra en 30 s)")), cx);
                }))
        };

        v_flex()
            .gap(px(16.))
            .pt(px(18.))
            .child(
                h_flex()
                    .gap(px(12.))
                    .flex_wrap()
                    .child(rank_card("Solo/Dúo", a.ranks.solo.as_ref(), lp.get("solo"), &p))
                    .child(rank_card("Flex", a.ranks.flex.as_ref(), lp.get("flex"), &p))
                    .when_some(a.ranks.tft.as_ref(), |d, r| d.child(rank_card("TFT", Some(r), lp.get("tft"), &p)))
                    .when_some(a.ranks.double_up.as_ref(), |d, r| d.child(rank_card("Double Up", Some(r), None, &p))),
            )
            .children(strip("lol", "Últimas de LoL", cx))
            .children(strip("tft", "Últimas de TFT", cx))
            .child(
                h_flex()
                    .gap(px(18.))
                    .text_size(px(12.5))
                    .text_color(p.text_2)
                    .child(
                        h_flex().gap(px(6.)).child(Icon::empty().path(assets::icon("gamepad-2")).small().text_color(p.cyan)).child(match &a.last_played_at {
                            Some(t) => format!("Jugó {}", ui::ago(Some(t))),
                            None => "Sin partidas registradas".into(),
                        }),
                    )
                    .child(
                        h_flex().gap(px(6.)).child(Icon::new(IconName::RefreshCw).small().text_color(p.cyan)).child(match &a.last_synced_at {
                            Some(t) => format!("Datos de {}", ui::ago(Some(t))),
                            None => "Datos sin actualizar".into(),
                        }),
                    ),
            )
            .child(
                h_flex()
                    .gap(px(10.))
                    .child(cred("copy-user", "Usuario", Some(a.username.clone()), a.username.clone(), cx))
                    .child(cred("copy-pass", "Contraseña", Some(a.password.clone()), a.password.clone(), cx)),
            )
            .when(!a.notes.is_empty(), |d| {
                d.child(div().px(px(14.)).py(px(10.)).rounded(px(10.)).bg(alpha(p.surface, 0.7)).text_size(px(13.)).text_color(p.text_2).child(a.notes.clone()))
            })
    }

    fn placeholder_tab(&self, text: &'static str, cx: &App) -> Div {
        let p = palette(cx);
        div().pt(px(40.)).flex().justify_center().text_color(p.muted).text_size(px(13.)).child(text)
    }

    fn actions(&self, a: &Account, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let can_play = self.sessions.contains_key(&a.id) || (!a.username.is_empty() && !a.password.is_empty());
        let session = self.vault.read(cx).store.session();
        let (_, caption) = ui::splash_for(a, session.map(|s| &s.history), |_| String::new());
        h_flex()
            .gap(px(10.))
            .py(px(18.))
            .child(
                Button::new("play")
                    .map(|b| if can_play { b.primary() } else { b.outline() })
                    .large()
                    .w(px(200.))
                    .icon(if can_play { Icon::new(IconName::Play) } else { Icon::empty().path(assets::icon("log-in")) })
                    .label(if can_play { "JUGAR" } else { "INICIAR SESIÓN" })
                    .on_click(cx.listener(|this, _, window, cx| this.play_selected(window, cx))),
            )
            .child(
                Button::new("edit")
                    .icon(Icon::empty().path(assets::icon("pencil")))
                    .label("Editar")
                    .on_click(|_, window, cx| window.push_notification(Notification::info("Editar cuentas llega en la próxima parte de esta fase."), cx)),
            )
            .child(
                Button::new("refresh")
                    .icon(Icon::new(IconName::RefreshCw))
                    .label("Actualizar")
                    .tooltip("Lee rangos, historial y maestrías desde el cliente de LoL (esta cuenta tiene que estar abierta ahí)")
                    .on_click(|_, window, cx| window.push_notification(Notification::info("Leer la cuenta desde el cliente de LoL llega en la fase 5."), cx)),
            )
            .child(div().flex_1())
            .child(div().text_size(px(12.)).text_color(p.muted).child(caption))
    }

    pub(crate) fn detail(&mut self, a: &Account, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.vault.read(cx).store.session().and_then(|s| s.history.get(&a.id)).map(|m| m.len()).unwrap_or(0);
        let body = match self.tab {
            Tab::Summary => self.summary(a, cx).into_any_element(),
            Tab::History => self.placeholder_tab("El historial llega en la próxima parte de esta fase.", cx).into_any_element(),
            Tab::Progress => self.placeholder_tab("El gráfico de LP llega en la próxima parte de esta fase.", cx).into_any_element(),
            Tab::Champions => self.placeholder_tab("Las maestrías llegan en la próxima parte de esta fase.", cx).into_any_element(),
        };
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .pt(px(30.))
            .pl(px(22.))
            .pr(px(36.))
            .child(self.hero(a, cx))
            .child(self.tabs(count, cx))
            .child(div().id("detail-body").flex_1().min_h_0().overflow_y_scroll().child(body))
            .child(self.actions(a, cx))
    }
}
