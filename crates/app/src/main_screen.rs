//! Pantalla principal: las cuentas en una lista a la izquierda (búsqueda, orden y filtro "dúo con
//! amigo") y la elegida en detalle a la derecha, con el splash de su fondo de perfil detrás.
//! Igual que la vista carrusel de la versión Electron (renderer/carousel.js).
use crate::assets;
use crate::state::Vault;
use crate::theme::palette;
use crate::ui::{self, alpha};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::{h_flex, v_flex, Icon, IconName, IndexPath, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use serde_json::{json, Value};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;
use sv_core::model::Account;
use sv_core::ranks::{best_rank, can_duo, rank_label, tier_es, top_rank, DUO_TIERS};

#[derive(Clone, Copy, PartialEq)]
pub enum SortKey {
    Rank,
    Level,
    Name,
    Played,
}

const SORTS: [(SortKey, &str, &str); 4] = [
    (SortKey::Rank, "rank", "Mayor rango"),
    (SortKey::Level, "level", "Mayor nivel"),
    (SortKey::Name, "name", "Nombre A-Z"),
    (SortKey::Played, "played", "Última partida"),
];
const DIVISIONS: [&str; 4] = ["I", "II", "III", "IV"];
const NO_FRIEND: &str = "— nadie —";

#[derive(Clone, Copy, PartialEq)]
pub enum Tab {
    Summary,
    History,
    Progress,
    Champions,
}

type Choice = SelectState<Vec<SharedString>>;

pub struct MainScreen {
    pub(crate) vault: Entity<Vault>,
    focus: FocusHandle,
    search: Entity<InputState>,
    sort_select: Entity<Choice>,
    tier_select: Entity<Choice>,
    div_select: Entity<Choice>,
    sort: SortKey,
    friend_tier: String,
    friend_div: String,
    pub(crate) selected: Option<String>,
    pub(crate) tab: Tab,
    pub(crate) history_mode: &'static str,
    pub(crate) progress_queue: &'static str,
    pub(crate) sessions: BTreeMap<String, String>,
    /// Cambio de LP bajo el mouse en el gráfico de Progreso, y dónde quedó dibujado el gráfico.
    pub(crate) lp_hover: Option<usize>,
    pub(crate) chart_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    list_scroll: ScrollHandle,
    _subs: Vec<Subscription>,
}

fn riot_id_or_user(a: &Account) -> String {
    let id = a.riot_id();
    if id.is_empty() {
        a.username.clone()
    } else {
        id
    }
}

impl MainScreen {
    pub fn new(vault: Entity<Vault>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let extra = vault.read(cx).store.config.extra.clone();
        let sort = extra
            .get("sort")
            .and_then(Value::as_str)
            .and_then(|k| SORTS.iter().find(|(_, key, _)| *key == k))
            .map(|(s, _, _)| *s)
            .unwrap_or(SortKey::Rank);
        let friend = extra.get("friendElo").cloned().unwrap_or(Value::Null);
        let friend_tier = friend["tier"].as_str().filter(|t| DUO_TIERS.contains(t)).unwrap_or("").to_string();
        let friend_div = friend["div"].as_str().filter(|d| DIVISIONS.contains(d)).unwrap_or("I").to_string();

        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Buscar nick, usuario, nota…"));
        let sort_labels: Vec<SharedString> = SORTS.iter().map(|(_, _, l)| SharedString::from(*l)).collect();
        let sort_ix = SORTS.iter().position(|(s, _, _)| *s == sort).unwrap_or(0);
        let sort_select = cx.new(|cx| SelectState::new(sort_labels, Some(IndexPath::new(sort_ix)), window, cx));
        let tiers: Vec<SharedString> =
            std::iter::once(NO_FRIEND.into()).chain(DUO_TIERS.iter().map(|t| SharedString::from(tier_es(t)))).collect();
        let tier_ix = DUO_TIERS.iter().position(|t| *t == friend_tier).map(|i| i + 1).unwrap_or(0);
        let tier_select = cx.new(|cx| SelectState::new(tiers, Some(IndexPath::new(tier_ix)), window, cx));
        let divs: Vec<SharedString> = DIVISIONS.iter().map(|d| SharedString::from(*d)).collect();
        let div_ix = DIVISIONS.iter().position(|d| *d == friend_div).unwrap_or(0);
        let div_select = cx.new(|cx| SelectState::new(divs, Some(IndexPath::new(div_ix)), window, cx));

        let subs = vec![
            cx.observe(&vault, |this, _, cx| {
                this.refresh_sessions(cx);
                cx.notify();
            }),
            cx.subscribe_in(&search, window, |this, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::Change => {
                    this.keep_selection_visible(cx);
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.play_selected(window, cx),
                _ => {}
            }),
            cx.subscribe_in(&sort_select, window, |this, _, ev: &SelectEvent<Vec<SharedString>>, _, cx| {
                if let SelectEvent::Confirm(Some(label)) = ev {
                    if let Some((s, _, _)) = SORTS.iter().find(|(_, _, l)| *l == label.as_ref()) {
                        this.sort = *s;
                        this.save_prefs(cx);
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&tier_select, window, |this, _, ev: &SelectEvent<Vec<SharedString>>, _, cx| {
                if let SelectEvent::Confirm(Some(label)) = ev {
                    this.friend_tier =
                        DUO_TIERS.iter().find(|t| tier_es(t) == label.as_ref()).map(|t| t.to_string()).unwrap_or_default();
                    this.save_prefs(cx);
                    this.keep_selection_visible(cx);
                    cx.notify();
                }
            }),
            cx.subscribe_in(&div_select, window, |this, _, ev: &SelectEvent<Vec<SharedString>>, _, cx| {
                if let SelectEvent::Confirm(Some(label)) = ev {
                    this.friend_div = label.to_string();
                    this.save_prefs(cx);
                    this.keep_selection_visible(cx);
                    cx.notify();
                }
            }),
        ];
        let mut this = Self {
            vault,
            focus: cx.focus_handle(),
            search,
            sort_select,
            tier_select,
            div_select,
            sort,
            friend_tier,
            friend_div,
            selected: None,
            tab: Tab::Summary,
            history_mode: "lol",
            progress_queue: "solo",
            sessions: BTreeMap::new(),
            lp_hover: None,
            chart_bounds: Rc::new(Cell::new(None)),
            list_scroll: ScrollHandle::new(),
            _subs: subs,
        };
        this.refresh_sessions(cx);
        // Solo en desarrollo: SV_DEV_SELECT y SV_DEV_TAB eligen la cuenta y la pestaña al abrir.
        #[cfg(debug_assertions)]
        {
            this.selected = std::env::var("SV_DEV_SELECT").ok();
            this.tab = match std::env::var("SV_DEV_TAB").as_deref() {
                Ok("history") => Tab::History,
                Ok("progress") => Tab::Progress,
                Ok("champions") => Tab::Champions,
                _ => Tab::Summary,
            };
        }
        this.keep_selection_visible(cx);
        this.focus.focus(window, cx);
        this
    }

    fn refresh_sessions(&mut self, cx: &App) {
        self.sessions = self.vault.read(cx).store.session_index();
    }

    /// El orden y el filtro de dúo se recuerdan en este PC (config.json).
    fn save_prefs(&self, cx: &mut Context<Self>) {
        let sort = SORTS.iter().find(|(s, _, _)| *s == self.sort).map(|(_, k, _)| *k).unwrap_or("rank");
        let friend = json!({ "tier": self.friend_tier, "div": self.friend_div });
        self.vault.update(cx, |v, _| {
            v.store.config.extra.insert("sort".into(), json!(sort));
            v.store.config.extra.insert("friendElo".into(), friend);
            let _ = v.store.save_config();
        });
    }

    fn friend_active(&self) -> bool {
        DUO_TIERS.contains(&self.friend_tier.as_str())
    }

    /// Cuentas que se ven: filtradas por la búsqueda y el dúo, favoritas arriba y en el orden elegido.
    pub(crate) fn visible(&self, cx: &App) -> Vec<Account> {
        let Some(data) = self.vault.read(cx).store.data() else { return Vec::new() };
        let q = self.search.read(cx).value().trim().to_lowercase();
        let mut list: Vec<Account> = data
            .accounts
            .iter()
            .filter(|a| {
                q.is_empty()
                    || [&a.label, &a.username, &a.game_name, &a.tag_line, &a.notes, &a.server]
                        .iter()
                        .any(|v| v.to_lowercase().contains(&q))
            })
            .filter(|a| !self.friend_active() || can_duo(a.ranks.solo.as_ref(), &self.friend_tier, &self.friend_div))
            .cloned()
            .collect();
        let level = |a: &Account| a.level.unwrap_or(0);
        list.sort_by(|a, b| {
            b.is_favorite().cmp(&a.is_favorite()).then_with(|| match self.sort {
                SortKey::Rank => best_rank(b).cmp(&best_rank(a)).then(level(b).cmp(&level(a))),
                SortKey::Level => level(b).cmp(&level(a)),
                SortKey::Name => riot_id_or_user(a).to_lowercase().cmp(&riot_id_or_user(b).to_lowercase()),
                // Las cuentas sin partidas registradas quedan al final.
                SortKey::Played => b.last_played_at.cmp(&a.last_played_at),
            })
        });
        list
    }

    /// Si la cuenta elegida ya no está en la lista (búsqueda, filtro, borrada), elige la primera.
    fn keep_selection_visible(&mut self, cx: &mut Context<Self>) {
        let list = self.visible(cx);
        if !list.iter().any(|a| Some(&a.id) == self.selected.as_ref()) {
            self.selected = list.first().map(|a| a.id.clone());
        }
    }

    pub(crate) fn select(&mut self, id: String, cx: &mut Context<Self>) {
        if self.selected.as_ref() != Some(&id) {
            self.selected = Some(id);
            self.lp_hover = None;
            cx.notify();
        }
    }

    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let list = self.visible(cx);
        if list.is_empty() {
            return;
        }
        let i = list.iter().position(|a| Some(&a.id) == self.selected.as_ref()).unwrap_or(0) as isize;
        let next = (i + delta).clamp(0, list.len() as isize - 1) as usize;
        self.selected = Some(list[next].id.clone());
        self.list_scroll.scroll_to_item(next);
        cx.notify();
    }

    pub(crate) fn selected_account(&self, cx: &App) -> Option<Account> {
        let id = self.selected.as_ref()?;
        self.vault.read(cx).store.data()?.account(id).cloned()
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev.keystroke.key.as_str() {
            "up" => self.move_selection(-1, cx),
            "down" => self.move_selection(1, cx),
            "enter" => self.play_selected(window, cx),
            "f" if ev.keystroke.modifiers.control => self.search.update(cx, |s, cx| s.focus(window, cx)),
            _ => return,
        }
        cx.stop_propagation();
    }

    fn list_item(&self, a: &Account, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let selected = self.selected.as_ref() == Some(&a.id);
        let tier = ui::account_tier_color(a);
        let store = &self.vault.read(cx).store;
        let (splash, _) = ui::splash_for(a, store.session().map(|s| &s.history), |_| String::new());
        let saved = self.sessions.contains_key(&a.id);
        let sub = [a.server.clone(), a.label.clone(), a.last_played_at.as_deref().map(|t| ui::ago(Some(t))).unwrap_or_default()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        let id = a.id.clone();

        let name = h_flex()
            .gap(px(5.))
            .min_w_0()
            .text_size(px(15.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(p.text)
            .when(a.is_favorite(), |d| d.child(Icon::new(IconName::StarFill).xsmall().text_color(p.gold)))
            .map(|d| {
                if a.game_name.is_empty() {
                    d.child(div().truncate().child(if a.label.is_empty() { a.username.clone() } else { a.label.clone() }))
                } else {
                    d.child(div().truncate().child(a.game_name.clone()))
                        .child(div().text_color(p.muted).font_weight(FontWeight::MEDIUM).text_size(px(13.)).child(format!("#{}", a.tag_line)))
                }
            });

        let tier_block = if let Some((_, short, _)) = ui::ban_info(a) {
            h_flex()
                .gap(px(5.))
                .text_color(p.loss)
                .child(Icon::new(IconName::Ban).small())
                .child(div().font_weight(FontWeight::BOLD).text_size(px(13.)).child(short))
                .into_any_element()
        } else if let Some(r) = top_rank(a) {
            h_flex()
                .gap(px(6.))
                .child(
                    v_flex()
                        .items_end()
                        .child(div().text_size(px(13.)).font_weight(FontWeight::BOLD).text_color(ui::tier_color(Some(&r.tier))).child(rank_label(&r.tier, &r.division)))
                        .child(div().text_size(px(11.5)).text_color(p.muted).child(format!("{} LP", r.lp))),
                )
                .child(img(ui::rank_emblem(r)).size(px(40.)).object_fit(ObjectFit::Contain))
                .into_any_element()
        } else {
            div().text_size(px(13.)).font_weight(FontWeight::BOLD).text_color(p.muted).child("Sin rango").into_any_element()
        };

        div()
            .id(SharedString::from(format!("acc-{}", a.id)))
            .relative()
            .h(px(74.))
            .flex_none()
            .rounded(px(10.))
            .overflow_hidden()
            .border_1()
            .cursor_pointer()
            .map(|d| {
                if selected {
                    d.bg(alpha(p.surface_2, 0.92)).border_color(tier).shadow_lg()
                } else {
                    d.bg(alpha(p.surface, 0.86)).border_color(p.border_strong).hover(|s| s.border_color(alpha(p.text, 0.24)))
                }
            })
            .when_some(splash, |d, url| {
                d.child(img(url).absolute().inset_0().size_full().object_fit(ObjectFit::Cover).opacity(if selected { 0.4 } else { 0.15 }))
            })
            .child(div().absolute().top_0().bottom_0().right_0().w(px(5.)).bg(alpha(tier, 0.9)))
            .child(
                h_flex()
                    .relative()
                    .size_full()
                    .gap(px(14.))
                    .pl(px(18.))
                    .pr(px(22.))
                    .child(ui::avatar(a, 50., cx))
                    .child(
                        v_flex().flex_1().min_w_0().gap(px(3.)).child(name).child(
                            h_flex()
                                .gap(px(6.))
                                .text_size(px(12.))
                                .text_color(p.muted)
                                .child(div().size(px(7.)).rounded_full().bg(if saved { p.ok } else { alpha(p.muted, 0.5) }))
                                .child(div().truncate().child(sub)),
                        ),
                    )
                    .child(tier_block),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.select(id.clone(), cx)))
    }

    fn side(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let list = self.visible(cx);
        let total = self.vault.read(cx).store.data().map(|d| d.accounts.len()).unwrap_or(0);
        let count = if list.is_empty() && self.friend_active() {
            let div = if self.friend_tier == "MASTER" { String::new() } else { format!(" {}", self.friend_div) };
            format!("Ninguna cuenta puede hacer dúo con {}{div}", tier_es(&self.friend_tier))
        } else if list.len() == total {
            format!("{} cuentas", list.len())
        } else {
            format!("{} de {} cuentas", list.len(), total)
        };
        let items: Vec<AnyElement> = list.iter().map(|a| self.list_item(a, cx).into_any_element()).collect();
        let friend_on = self.friend_active();

        v_flex()
            .w(px(440.))
            .flex_none()
            .h_full()
            .min_h_0()
            .child(
                v_flex()
                    .gap(px(8.))
                    .pt(px(14.))
                    .pb(px(6.))
                    .pl(px(18.))
                    .pr(px(30.))
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .child(div().flex_1().child(Input::new(&self.search).prefix(Icon::new(IconName::Search).text_color(p.muted)).cleanable(true)))
                            .child(div().w(px(150.)).child(Select::new(&self.sort_select))),
                    )
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .text_size(px(12.))
                            .child(div().text_color(if friend_on { p.gold } else { p.muted }).child("Dúo con amigo"))
                            .child(div().w(px(130.)).child(Select::new(&self.tier_select).small()))
                            .when(friend_on && self.friend_tier != "MASTER", |d| d.child(div().w(px(70.)).child(Select::new(&self.div_select).small())))
                            .when(friend_on, |d| {
                                d.child(
                                    Button::new("friend-clear")
                                        .ghost()
                                        .small()
                                        .icon(Icon::empty().path(assets::icon("x")))
                                        .tooltip("Quitar filtro")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.friend_tier.clear();
                                            this.tier_select.update(cx, |s, cx| s.set_selected_index(Some(IndexPath::new(0)), window, cx));
                                            this.save_prefs(cx);
                                            this.keep_selection_visible(cx);
                                            cx.notify();
                                        })),
                                )
                            }),
                    ),
            )
            .child(h_flex().justify_between().pt(px(2.)).pb(px(6.)).pl(px(20.)).pr(px(30.)).text_size(px(12.)).text_color(p.muted).child(count))
            .child(
                div()
                    .id("account-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.list_scroll)
                    .child(
                        v_flex().gap(px(10.)).pl(px(18.)).pr(px(30.)).pb(px(12.)).children(items).when(list.is_empty(), |d| {
                            d.child(div().p(px(20.)).text_color(p.muted).text_size(px(13.)).child("Ninguna cuenta coincide con la búsqueda"))
                        }),
                    ),
            )
            .child(
                h_flex()
                    .gap(px(14.))
                    .pt(px(8.))
                    .pb(px(14.))
                    .px(px(20.))
                    .text_size(px(11.5))
                    .text_color(p.muted)
                    .child("↑ ↓ elegir")
                    .child("Enter jugar")
                    .child("Ctrl F buscar"),
            )
    }

    fn empty_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        div().size_full().flex().items_center().justify_center().child(
            v_flex()
                .items_center()
                .gap(px(10.))
                .max_w(px(420.))
                .child(img(assets::logo()).size(px(72.)).rounded(px(18.)).opacity(0.85))
                .child(div().text_size(px(18.)).font_weight(FontWeight::SEMIBOLD).text_color(p.text).child("Tu bóveda está vacía"))
                .child(
                    div()
                        .text_center()
                        .text_color(p.muted)
                        .text_size(px(13.))
                        .child("Agrega tu primera cuenta iniciando sesión en el cliente, o importa la lista que tenías en un txt."),
                ),
        )
    }
}

impl Focusable for MainScreen {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for MainScreen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let has_accounts = self.vault.read(cx).store.data().is_some_and(|d| !d.accounts.is_empty());
        if !has_accounts {
            return div().size_full().child(self.empty_view(cx)).into_any_element();
        }
        self.keep_selection_visible(cx);
        let account = self.selected_account(cx);
        let tier = account.as_ref().map(ui::account_tier_color).unwrap_or_else(|| ui::tier_color(None));
        let splash = account.as_ref().and_then(|a| {
            let store = &self.vault.read(cx).store;
            ui::splash_for(a, store.session().map(|s| &s.history), |_| String::new()).0
        });

        div()
            .id("stage")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(p.bg)
            // Splash de la cuenta elegida, oscurecido, y el halo del color de su liga.
            .child(div().absolute().inset_0().bg(radial_glow(tier)))
            .when_some(splash, |d, url| d.child(img(url).absolute().inset_0().size_full().object_fit(ObjectFit::Cover).opacity(0.62)))
            .child(div().absolute().inset_0().bg(linear_gradient(
                90.,
                linear_color_stop(alpha(p.bg, 0.97), 0.),
                linear_color_stop(alpha(p.bg, 0.3), 1.),
            )))
            .child(div().absolute().inset_0().bg(linear_gradient(
                0.,
                linear_color_stop(alpha(p.bg, 0.96), 0.),
                linear_color_stop(alpha(p.bg, 0.), 0.4),
            )))
            .child(h_flex().relative().size_full().items_start().child(self.side(cx)).when_some(account, |d, a| d.child(self.detail(&a, cx))))
            .into_any_element()
    }
}

/// Fondo tenue del color de la liga (en Electron, un radial-gradient detrás del splash).
fn radial_glow(tier: Hsla) -> Background {
    linear_gradient(200., linear_color_stop(alpha(tier, 0.18), 0.), linear_color_stop(alpha(tier, 0.), 0.7))
}
