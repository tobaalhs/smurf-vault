//! Pantalla principal: la rueda de cuentas a la izquierda (búsqueda, orden y filtro "dúo con amigo")
//! y la elegida en detalle a la derecha, con el splash de su fondo de perfil detrás.
//! Igual que la vista carrusel de la versión Electron (renderer/carousel.js).
use crate::assets;
use crate::state::Vault;
use crate::theme::{palette, Palette};
use crate::ui::{self, alpha};
use crate::wheel::WheelState;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState, SliderValue};
use gpui_kit::component::{h_flex, v_flex, Icon, IconName, IndexPath, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use serde_json::{json, Value};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;
use sv_core::model::Account;
use sv_core::ranks::{best_rank, can_duo, tier_es, DUO_TIERS};

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
    volume: Entity<SliderState>,
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
    pub(crate) wheel: WheelState,
    /// El mouse está sobre el botón Jugar (se aclara y sube un poco).
    pub(crate) play_hover: bool,
    /// Ancho útil del panel de detalle en el último dibujo.
    pub(crate) detail_w: f32,
    /// Ancho del panel de la rueda en el último dibujo.
    pub(crate) side_w: f32,
    /// El fondo (vista aparte, ver stage_bg.rs); se le avisa dónde está el mouse.
    pub(crate) bg: Option<Entity<crate::stage_bg::StageBg>>,
    /// Detección del cliente de LoL (ver client.rs): consulta en curso, cuentas ya avisadas, la
    /// cuenta abierta que no está guardada (para el aviso de vincular) y las tareas periódicas.
    pub(crate) detecting: bool,
    pub(crate) seen_puuids: std::collections::HashSet<String>,
    pub(crate) detected: Option<sv_core::snapshot::Snapshot>,
    /// El aviso de cuenta detectada viene de "Abrir login de Riot" (se ofrece guardarla como nueva).
    pub(crate) detected_adding: bool,
    pub(crate) client_tasks: Vec<Task<()>>,
    /// Cambio de cuenta (switch.rs): en curso, guía de arriba con el paso actual, si se está
    /// agregando una cuenta nueva por el login, cuántas veces se encontró el cliente y la búsqueda
    /// rápida después de cambiar.
    pub(crate) switching: bool,
    pub(crate) guide: Option<SharedString>,
    pub(crate) adding_account: bool,
    pub(crate) detect_hits: u64,
    pub(crate) detect_soon_task: Option<Task<()>>,
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

/// Tecla dibujada como en el teclado (Ctrl F, ↑, Enter…).
pub(crate) fn kbd(text: &'static str, p: &Palette) -> Div {
    div()
        .px(px(5.))
        .rounded(px(4.))
        .border_1()
        .border_b_2()
        .border_color(p.border_strong)
        .bg(alpha(p.surface_2, 0.8))
        .text_size(px(10.5))
        .line_height(px(15.))
        .text_color(p.muted)
        .child(text)
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
        // Sonido: encendido y volumen se recuerdan en este PC.
        let sound_on = extra.get("sound").and_then(Value::as_str) != Some("off");
        let volume = extra.get("soundVolume").and_then(Value::as_f64).unwrap_or(0.5) as f32;
        crate::sounds::configure(sound_on, volume);

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
        let volume_state = cx.new(|_| SliderState::new().min(0.).max(100.).step(5.).default_value((volume * 100.).round()));

        let subs = vec![
            cx.observe_in(&vault, window, |this, vault, window, cx| {
                this.refresh_sessions(cx);
                // "Abrir login de Riot" se pide desde el diálogo de agregar, que no conoce esta pantalla.
                let requested = vault.update(cx, |v, _| std::mem::take(&mut v.open_login_requested));
                if requested {
                    this.open_riot_login(window, cx);
                }
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
                        this.keep_selection_visible(cx);
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
            cx.subscribe(&volume_state, |this, _, ev: &SliderEvent, cx| {
                let SliderEvent::Change(SliderValue::Single(v)) = ev else { return };
                let (enabled, _) = crate::sounds::state();
                let vol = v / 100.;
                // Subir el volumen con el sonido apagado lo vuelve a encender.
                crate::sounds::configure(enabled || vol > 0., vol);
                this.save_prefs(cx);
                cx.notify();
            }),
        ];
        let mut this = Self {
            vault,
            focus: cx.focus_handle(),
            search,
            sort_select,
            tier_select,
            div_select,
            volume: volume_state,
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
            wheel: WheelState::default(),
            play_hover: false,
            detail_w: 600.,
            side_w: 400.,
            bg: None,
            detecting: false,
            seen_puuids: Default::default(),
            detected: None,
            detected_adding: false,
            client_tasks: Vec::new(),
            switching: false,
            guide: None,
            adding_account: false,
            detect_hits: 0,
            detect_soon_task: None,
            _subs: subs,
        };
        this.refresh_sessions(cx);
        this.start_client_watch(window, cx);
        this.prefetch_images(cx);
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
            // SV_DEV_DETECTED simula una cuenta abierta en el cliente que no está guardada.
            if std::env::var("SV_DEV_DETECTED").as_deref() == Ok("adding") {
                this.detected_adding = true;
                this.guide = Some("Escribiendo tus credenciales…".into());
            }
            if std::env::var("SV_DEV_DETECTED").is_ok() {
                this.detected = Some(sv_core::snapshot::Snapshot {
                    puuid: "dev".into(),
                    game_name: "Cuenta Nueva".into(),
                    tag_line: "LAS".into(),
                    server: "LAS".into(),
                    level: Some(87),
                    ..Default::default()
                });
            }
            // SV_DEV_TOAST muestra avisos de prueba (para ver dónde y sobre qué quedan).
            if std::env::var("SV_DEV_TOAST").is_ok() {
                let after: u64 = std::env::var("SV_DEV_TOAST").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
                window
                    .spawn(cx, async move |cx| {
                        cx.background_executor().timer(std::time::Duration::from_millis(after)).await;
                        let _ = cx.update(|window, cx| {
                            use gpui_kit::component::{notification::Notification, WindowExt};
                            for i in 1..=4 {
                                window.push_notification(Notification::error(format!("Aviso de prueba {i}: API key de Riot inválida o caducada")), cx);
                            }
                            window.push_notification(Notification::success("Aviso de prueba: cuentas actualizadas"), cx);
                        });
                    })
                    .detach();
            }
            // SV_DEV_DIALOG abre un diálogo al arrancar (para revisarlos sin hacer clic).
            if let Ok(which) = std::env::var("SV_DEV_DIALOG") {
                let (vault, selected) = (this.vault.clone(), this.selected.clone());
                // SV_DEV_DIALOG_DELAY (ms) lo abre un rato después (p. ej. con avisos ya en pantalla).
                let delay: u64 = std::env::var("SV_DEV_DIALOG_DELAY").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
                window
                    .spawn(cx, async move |cx| {
                        // "tools>settings" abre uno, lo cierra y abre el siguiente.
                        for (i, which) in which.split('>').enumerate() {
                            cx.background_executor().timer(std::time::Duration::from_millis(if i == 0 { delay } else { 800 })).await;
                            let (vault, selected) = (vault.clone(), selected.clone());
                            let _ = cx.update(|window, cx| {
                                use gpui_kit::component::WindowExt as _;
                                if i > 0 {
                                    window.close_dialog(cx);
                                }
                                let acc = selected.and_then(|id| vault.read(cx).store.data()?.account(&id).cloned());
                                match which {
                                    "account" => crate::dialogs::open_account(vault, acc, window, cx),
                                    "add" => crate::dialogs::open_add(vault, window, cx),
                                    "import" => crate::dialogs::open_import(vault, window, cx),
                                    "riotkey" => crate::dialogs::open_riot_key(vault, None, window, cx),
                                    "tools" => crate::tools_dialog::open_tools(vault, window, cx),
                                    "settings" => crate::settings_dialog::open_settings(vault, window, cx),
                                    _ => {}
                                }
                            });
                        }
                    })
                    .detach();
            }
        }
        this.keep_selection_visible(cx);
        let list = this.visible(cx);
        if let Some(i) = list.iter().position(|a| Some(&a.id) == this.selected.as_ref()) {
            this.wheel_to(i, true);
        }
        this.focus.focus(window, cx);
        this
    }

    fn refresh_sessions(&mut self, cx: &App) {
        self.sessions = self.vault.read(cx).store.session_index();
    }

    /// El orden, el filtro de dúo y el sonido se recuerdan en este PC (config.json).
    fn save_prefs(&self, cx: &mut Context<Self>) {
        let sort = SORTS.iter().find(|(s, _, _)| *s == self.sort).map(|(_, k, _)| *k).unwrap_or("rank");
        let friend = json!({ "tier": self.friend_tier, "div": self.friend_div });
        let (enabled, volume) = crate::sounds::state();
        self.vault.update(cx, |v, _| {
            let extra = &mut v.store.config.extra;
            extra.insert("sort".into(), json!(sort));
            extra.insert("friendElo".into(), friend);
            extra.insert("sound".into(), json!(if enabled { "on" } else { "off" }));
            extra.insert("soundVolume".into(), json!(volume));
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

    /// Si la cuenta elegida ya no está en la lista (búsqueda, filtro, borrada), elige la primera de
    /// golpe; si sigue, la rueda va a su nueva posición.
    fn keep_selection_visible(&mut self, cx: &mut Context<Self>) {
        let list = self.visible(cx);
        match list.iter().position(|a| Some(&a.id) == self.selected.as_ref()) {
            Some(i) => self.wheel_to(i, false),
            None => {
                self.selected = list.first().map(|a| a.id.clone());
                self.wheel_to(0, true);
            }
        }
    }

    /// Elige una cuenta: la rueda va hacia ella y suena el pop.
    pub(crate) fn select(&mut self, id: String, cx: &mut Context<Self>) {
        let list = self.visible(cx);
        let Some(i) = list.iter().position(|a| a.id == id) else { return };
        self.wheel_to(i, false);
        if self.selected.as_ref() != Some(&id) {
            self.selected = Some(id);
            self.lp_hover = None;
            crate::sounds::pop();
        }
        cx.notify();
    }

    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let list = self.visible(cx);
        if list.is_empty() {
            return;
        }
        let i = list.iter().position(|a| Some(&a.id) == self.selected.as_ref()).unwrap_or(0) as isize;
        let next = (i + delta).clamp(0, list.len() as isize - 1) as usize;
        self.select(list[next].id.clone(), cx);
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

    fn sound_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let (enabled, volume) = crate::sounds::state();
        let on = enabled && volume > 0.;
        let icon = || Icon::empty().path(assets::icon(if on { "volume-2" } else { "volume-x" }));
        let label: SharedString = if enabled { format!("{}%", (volume * 100.).round()) } else { "Silencio".to_string() }.into();
        let slider = self.volume.clone();
        let view = cx.entity().downgrade();
        Popover::new("sound")
            .anchor(Anchor::TopRight)
            .trigger(Button::new("sound-btn").icon(icon().text_color(if on { p.text_2 } else { p.muted })).tooltip(if on {
                format!("Sonidos al {}% · clic para ajustar", (volume * 100.).round())
            } else {
                "Sonidos silenciados · clic para ajustar".to_string()
            }))
            .content(move |_, _, cx| {
                let p = palette(cx);
                let view = view.clone();
                h_flex()
                    .gap(px(10.))
                    .p(px(8.))
                    .w(px(230.))
                    .child(
                        Button::new("sound-mute")
                            .ghost()
                            .small()
                            .icon(Icon::empty().path(assets::icon(if on { "volume-2" } else { "volume-x" })))
                            .tooltip(if on { "Silenciar" } else { "Activar sonidos" })
                            .on_click(move |_, _, cx| {
                                let (enabled, volume) = crate::sounds::state();
                                let now_on = !(enabled && volume > 0.);
                                // Estaba en 0: vuelve a un volumen audible.
                                crate::sounds::configure(now_on, if now_on && volume == 0. { 0.5 } else { volume });
                                crate::sounds::pop();
                                view.update(cx, |this, cx| {
                                    this.save_prefs(cx);
                                    cx.notify();
                                })
                                .ok();
                            }),
                    )
                    .child(div().flex_1().child(Slider::new(&slider)))
                    .child(div().w(px(52.)).text_size(px(12.)).text_color(p.text_2).child(label.clone()))
            })
    }

    fn side(&mut self, side_w: f32, list: &[Account], cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let total = self.vault.read(cx).store.data().map(|d| d.accounts.len()).unwrap_or(0);
        let count = if list.is_empty() && self.friend_active() {
            let div = if self.friend_tier == "MASTER" { String::new() } else { format!(" {}", self.friend_div) };
            format!("Ninguna cuenta puede hacer dúo con {}{div}", tier_es(&self.friend_tier))
        } else if list.len() == total {
            format!("{} cuentas", list.len())
        } else {
            format!("{} de {} cuentas", list.len(), total)
        };
        let friend_on = self.friend_active();
        let wheel = self.wheel_view(list, side_w, cx);

        v_flex()
            .w(px(side_w))
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
                            .child(div().flex_1().child(
                                Input::new(&self.search).prefix(Icon::new(IconName::Search).text_color(p.muted)).suffix(kbd("Ctrl F", &p)),
                            ))
                            .child(div().w(px(150.)).child(Select::new(&self.sort_select)))
                            .child(self.sound_button(cx)),
                    )
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .child(div().flex_none().text_size(px(12.5)).text_color(if friend_on { p.gold_hi } else { p.text_2 }).child("Dúo con amigo"))
                            .child(div().flex_1().child(Select::new(&self.tier_select)))
                            .when(friend_on && self.friend_tier != "MASTER", |d| d.child(div().w(px(76.)).child(Select::new(&self.div_select))))
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
            .child(
                h_flex()
                    .justify_between()
                    .pt(px(2.))
                    .pb(px(4.))
                    .pl(px(20.))
                    .pr(px(30.))
                    .text_size(px(12.))
                    .text_color(p.muted)
                    .child(count)
                    .child(self.wheel_position_label(list.len())),
            )
            .child(wheel)
            .child(
                h_flex()
                    .gap(px(14.))
                    .pt(px(8.))
                    .pb(px(14.))
                    .px(px(20.))
                    .text_size(px(11.5))
                    .text_color(p.muted)
                    .child(h_flex().gap(px(4.)).child(kbd("↑", &p)).child(kbd("↓", &p)).child("elegir"))
                    .child(h_flex().gap(px(4.)).child(kbd("Enter", &p)).child("jugar"))
                    .child("escribe para buscar"),
            )
    }

    fn empty_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let (add, import) = (self.vault.clone(), self.vault.clone());
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
                )
                .child(
                    h_flex()
                        .gap(px(8.))
                        .mt(px(6.))
                        .child(
                            Button::new("empty-add")
                                .primary()
                                .icon(Icon::new(IconName::Plus))
                                .label("Nueva cuenta")
                                .on_click(move |_, window, cx| crate::dialogs::open_add(add.clone(), window, cx)),
                        )
                        .child(
                            Button::new("empty-import")
                                .icon(Icon::empty().path(assets::icon("upload")))
                                .label("Importar txt")
                                .on_click(move |_, window, cx| crate::dialogs::open_import(import.clone(), window, cx)),
                        ),
                ),
        )
    }

    /// Deja en la caché del disco, en segundo plano, las imágenes de todas las cuentas (en el orden de
    /// la rueda): el fondo desenfocado, el splash de su tarjeta y su ícono. Así aparecen al tiro al
    /// elegirlas. No ocupa RAM: solo se decodifica lo que se muestra.
    fn prefetch_images(&self, cx: &App) {
        let store = &self.vault.read(cx).store;
        let history = store.session().map(|s| &s.history);
        let mut urls = Vec::new();
        for a in self.visible(cx) {
            if let Some(splash) = ui::splash_for(&a, history, |_| String::new()).0 {
                urls.push(crate::http::blurred(&splash, 2));
                urls.push(splash.to_string());
            }
            if let Some(icon) = a.icon_id {
                urls.push(ui::profile_icon_url(icon).to_string());
            }
        }
        crate::http::prefetch(urls);
    }

    /// Lo que dibuja el fondo: el splash de la cuenta elegida, el color de su liga y si no hay cuentas.
    pub(crate) fn bg_scene(&self, cx: &App) -> (Option<SharedString>, Hsla, bool) {
        let empty = !self.vault.read(cx).store.data().is_some_and(|d| !d.accounts.is_empty());
        let account = self.selected_account(cx);
        let tier = account.as_ref().map(ui::account_tier_color).unwrap_or_else(|| ui::tier_color(None));
        let splash = account.as_ref().and_then(|a| {
            let store = &self.vault.read(cx).store;
            ui::splash_for(a, store.session().map(|s| &s.history), |_| String::new()).0
        });
        (splash, tier, empty)
    }
}

impl Focusable for MainScreen {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for MainScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(debug_assertions)]
        if std::env::var("SV_DEV_FPS").is_ok() {
            eprintln!("main");
        }
        let has_accounts = self.vault.read(cx).store.data().is_some_and(|d| !d.accounts.is_empty());
        if !has_accounts {
            self.side_w = 0.;
            return div()
                .relative()
                .size_full()
                .child(self.empty_view(cx))
                .children(self.client_overlays(cx))
                .into_any_element();
        }
        let list = self.visible(cx);
        if !list.iter().any(|a| Some(&a.id) == self.selected.as_ref()) {
            self.keep_selection_visible(cx);
        }
        self.advance_wheel(list.len(), window);
        let viewport = window.viewport_size();
        let (vw, vh) = (f32::from(viewport.width), f32::from(viewport.height) - 60.);
        let side_w = (vw * 0.44).max(400.);
        // Ancho útil del detalle (sin sus márgenes), para la grilla de tarjetas de rango.
        self.detail_w = vw - side_w - 22. - 36. - 6.;
        self.side_w = side_w;

        let account = self.selected_account(cx);
        let vh_ = vh;

        div()
            .id("stage")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            // El mouse solo mueve el fondo (otra vista): esta pantalla no se vuelve a dibujar.
            .on_mouse_move(cx.listener(move |this, ev: &MouseMoveEvent, _, cx| {
                if let Some(bg) = &this.bg {
                    let (fx, fy) = (f32::from(ev.position.x) / vw, (f32::from(ev.position.y) - 60.) / vh_);
                    bg.update(cx, |b, cx| b.follow(fx, fy, cx));
                }
            }))
            .relative()
            .size_full()
            .overflow_hidden()
            .child(h_flex().relative().size_full().items_start().child(self.side(side_w, &list, cx)).when_some(account, |d, a| d.child(self.detail(&a, cx))))
            .children(self.client_overlays(cx))
            .into_any_element()
    }
}
