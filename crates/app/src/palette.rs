//! Paleta de comandos (Ctrl K o el buscador de arriba): busca cuentas y acciones y se usa con el
//! teclado. ↑ ↓ eligen, Enter ejecuta, Esc cierra.
use crate::app::AppView;
use crate::assets;
use crate::main_screen::MainScreen;
use crate::state::{Page, Vault};
use crate::theme::palette;
use crate::ui::{account_tier_color, alpha};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex, Icon, IconName, WindowExt};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use sv_core::ranks::{rank_label, top_rank};
use sv_core::skins::norm_text;

/// Cuántas cuentas se muestran como máximo.
const MAX_ACCOUNTS: usize = 6;

#[derive(Clone)]
enum Action {
    Account(String),
    ToggleAccept,
    ToggleOffline,
    RefreshAll,
    Page(Page),
    NewAccount,
    Import,
    RiotKey,
    Lock,
}

#[derive(Clone)]
struct Item {
    group: &'static str,
    action: Action,
    title: SharedString,
    /// Texto chico a la derecha del título (Riot ID, rango…).
    detail: Option<SharedString>,
    /// A la derecha: estado o atajo.
    trailing: Option<SharedString>,
    icon: Option<&'static str>,
    color: Option<Hsla>,
    keywords: String,
}

pub struct CommandPalette {
    app: WeakEntity<AppView>,
    vault: Entity<Vault>,
    main: Option<Entity<MainScreen>>,
    query: Entity<InputState>,
    selected: usize,
    _sub: Subscription,
}

impl CommandPalette {
    fn new(app: WeakEntity<AppView>, vault: Entity<Vault>, main: Option<Entity<MainScreen>>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Busca una cuenta o una acción…"));
        query.update(cx, |s, cx| s.focus(window, cx));
        let sub = cx.subscribe_in(&query, window, |this, _, ev: &InputEvent, window, cx| match ev {
            InputEvent::Change => {
                this.selected = 0;
                cx.notify();
            }
            InputEvent::PressEnter { .. } => this.run_selected(window, cx),
            _ => {}
        });
        Self { app, vault, main, query, selected: 0, _sub: sub }
    }

    fn items(&self, cx: &App) -> Vec<Item> {
        let p = palette(cx);
        let q = norm_text(self.query.read(cx).value().trim());
        let v = self.vault.read(cx);
        let c = &v.store.config;
        let on_off = |on: bool| SharedString::from(if on { "Encendido" } else { "Apagado" });
        let count = v.store.data().map(|d| d.accounts.len()).unwrap_or(0);
        let mut out: Vec<Item> = Vec::new();
        if let Some(data) = v.store.data() {
            let accounts = data.accounts.iter().filter_map(|a| {
                let keywords = norm_text(&format!("{} {} {} {} {}", a.game_name, a.tag_line, a.username, a.label, a.server));
                if !q.is_empty() && !keywords.contains(&q) {
                    return None;
                }
                let title: SharedString = if a.game_name.is_empty() { a.display_name().into() } else { a.game_name.clone().into() };
                let tag = (!a.tag_line.is_empty()).then(|| format!("#{}", a.tag_line));
                let rank = top_rank(a).map(|r| rank_label(&r.tier, &r.division));
                let detail = [tag, rank].into_iter().flatten().collect::<Vec<_>>().join("  ");
                Some(Item {
                    group: "CUENTAS",
                    action: Action::Account(a.id.clone()),
                    title,
                    detail: (!detail.is_empty()).then(|| detail.into()),
                    trailing: Some("Enter jugar".into()),
                    icon: None,
                    color: Some(account_tier_color(a)),
                    keywords,
                })
            });
            out.extend(accounts.take(MAX_ACCOUNTS));
        }
        let tool = |action: Action, title: &'static str, icon: &'static str, trailing: Option<SharedString>, extra: &'static str| Item {
            group: "HERRAMIENTAS",
            keywords: norm_text(&format!("{title} {extra}")),
            action,
            title: title.into(),
            detail: None,
            trailing,
            icon: Some(icon),
            color: Some(p.cyan),
        };
        let vault = |action: Action, title: &'static str, icon: &'static str, trailing: Option<&'static str>, extra: &'static str| Item {
            group: "BÓVEDA",
            keywords: norm_text(&format!("{title} {extra}")),
            action,
            title: title.into(),
            detail: None,
            trailing: trailing.map(SharedString::from),
            icon: Some(icon),
            color: None,
        };
        let actions = [
            tool(Action::ToggleAccept, "Autoaceptar partidas", "zap", Some(on_off(c.auto_accept)), "aceptar partida"),
            tool(Action::ToggleOffline, "Aparecer desconectado", "ghost", Some(on_off(c.appear_offline)), "offline chat"),
            tool(Action::RefreshAll, "Actualizar todas con la API de Riot", "refresh", Some(format!("{count} cuentas").into()), "rangos api"),
            tool(Action::Page(Page::Tools), "Cambiar fondo de perfil", "image", None, "skin fondo perfil"),
            tool(Action::Page(Page::Tools), "Control desde el celular", "smartphone", None, "qr celular remoto"),
            tool(Action::Page(Page::Tools), "Herramientas", "wrench", None, "herramientas"),
            vault(Action::NewAccount, "Nueva cuenta", "plus", Some("Ctrl N"), "agregar cuenta"),
            vault(Action::Import, "Importar desde txt", "upload", None, "importar txt"),
            vault(Action::RiotKey, "API key de Riot", "key-round", None, "api key riot"),
            vault(Action::Page(Page::Settings), "Ajustes", "settings", None, "ajustes drive colores contraseña transparencia"),
            vault(Action::Lock, "Bloquear bóveda", "lock", Some("Ctrl L"), "bloquear"),
        ];
        out.extend(actions.into_iter().filter(|i| q.is_empty() || i.keywords.contains(&q)));
        out
    }

    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let len = self.items(cx).len();
        if len == 0 {
            return;
        }
        self.selected = (self.selected as isize + delta).rem_euclid(len as isize) as usize;
        cx.notify();
    }

    fn run_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.items(cx).get(self.selected).cloned() else { return };
        self.run(item.action, window, cx);
    }

    fn run(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        window.close_dialog(cx);
        let vault = self.vault.clone();
        match action {
            Action::Account(id) => {
                vault.update(cx, |v, cx| v.go(Page::Accounts, cx));
                if let Some(main) = &self.main {
                    main.update(cx, |m, cx| {
                        m.select(id, cx);
                        m.play_selected(window, cx);
                    });
                }
            }
            Action::ToggleAccept | Action::ToggleOffline => {
                let accept = matches!(action, Action::ToggleAccept);
                vault.update(cx, |v, cx| {
                    let c = &mut v.store.config;
                    if accept {
                        c.auto_accept = !c.auto_accept;
                    } else {
                        c.appear_offline = !c.appear_offline;
                    }
                    let _ = v.store.save_config();
                    cx.notify();
                });
                crate::tools_dialog::apply_config(&vault, cx);
            }
            Action::RefreshAll => crate::client::refresh_riot(vault, None, window, cx),
            Action::Page(page) => vault.update(cx, |v, cx| v.go(page, cx)),
            Action::NewAccount => crate::dialogs::open_add(vault, window, cx),
            Action::Import => crate::dialogs::open_import(vault, window, cx),
            Action::RiotKey => crate::dialogs::open_riot_key(vault, None, false, window, cx),
            Action::Lock => {
                self.app.update(cx, |app, cx| app.lock(window, cx)).ok();
            }
        }
    }
}

impl Render for CommandPalette {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let items = self.items(cx);
        self.selected = self.selected.min(items.len().saturating_sub(1));
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut last_group = "";
        for (ix, item) in items.iter().enumerate() {
            if item.group != last_group {
                last_group = item.group;
                rows.push(
                    div().px(px(10.)).pt(px(10.)).pb(px(4.)).text_size(px(11.)).font_weight(FontWeight::SEMIBOLD).text_color(p.muted).child(item.group).into_any_element(),
                );
            }
            let active = ix == self.selected;
            let leading = match (&item.action, item.icon) {
                (Action::Account(_), _) => {
                    let color = item.color.unwrap_or(p.text_2);
                    let initials: String = item.title.chars().filter(|c| c.is_alphanumeric()).take(2).collect::<String>().to_uppercase();
                    div()
                        .size(px(30.))
                        .flex_none()
                        .rounded_full()
                        .border_2()
                        .border_color(color)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(11.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(color)
                        .child(initials)
                        .into_any_element()
                }
                (_, Some(icon)) => {
                    let icon = match icon {
                        "refresh" => Icon::new(IconName::RefreshCw),
                        "plus" => Icon::new(IconName::Plus),
                        name => Icon::empty().path(assets::icon(name)),
                    };
                    div().w(px(30.)).flex_none().flex().justify_center().child(icon.size(px(17.)).text_color(item.color.unwrap_or(p.text_2))).into_any_element()
                }
                _ => div().w(px(30.)).into_any_element(),
            };
            let action = item.action.clone();
            rows.push(
                h_flex()
                    .id(("palette-item", ix))
                    .gap(px(10.))
                    .px(px(10.))
                    .py(px(7.))
                    .rounded(px(9.))
                    .border_1()
                    .border_color(if active { p.border_strong } else { transparent_black() })
                    .when(active, |d| d.bg(p.surface_2))
                    .cursor_pointer()
                    .hover(|s| s.bg(p.surface_2))
                    .child(leading)
                    .child(div().text_size(px(13.5)).font_weight(FontWeight::SEMIBOLD).text_color(p.text).child(item.title.clone()))
                    .when_some(item.detail.clone(), |d, t| d.child(div().text_size(px(12.5)).text_color(p.muted).child(t)))
                    .child(div().flex_1())
                    .when_some(item.trailing.clone().filter(|_| active || !matches!(item.action, Action::Account(_))), |d, t| {
                        let on = t.as_ref() == "Encendido";
                        d.child(
                            div()
                                .px(px(7.))
                                .py(px(1.))
                                .rounded(px(6.))
                                .when(on, |d| d.bg(alpha(p.cyan, 0.14)).text_color(p.cyan))
                                .when(!on, |d| d.text_color(p.muted))
                                .text_size(px(11.5))
                                .child(t),
                        )
                    })
                    .on_click(cx.listener(move |this, _, window, cx| this.run(action.clone(), window, cx)))
                    .into_any_element(),
            );
        }
        if rows.is_empty() {
            rows.push(div().p(px(16.)).text_size(px(13.)).text_color(p.muted).child("Nada coincide con la búsqueda").into_any_element());
        }
        v_flex()
            .key_context("CommandPalette")
            // ↑ ↓ antes que el campo de texto (que las usaría para mover el cursor).
            .capture_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| match ev.keystroke.key.as_str() {
                "up" => {
                    this.move_selection(-1, cx);
                    cx.stop_propagation();
                }
                "down" => {
                    this.move_selection(1, cx);
                    cx.stop_propagation();
                }
                _ => {}
            }))
            .child(
                div()
                    .px(px(6.))
                    .pb(px(8.))
                    .border_b_1()
                    .border_color(p.border)
                    .child(Input::new(&self.query).appearance(false).prefix(Icon::new(IconName::Search).text_color(p.muted))),
            )
            .child(v_flex().id("palette-list").max_h(px(440.)).overflow_y_scroll().p(px(4.)).children(rows))
            .child(
                h_flex()
                    .gap(px(14.))
                    .px(px(10.))
                    .pt(px(8.))
                    .border_t_1()
                    .border_color(p.border)
                    .text_size(px(11.5))
                    .text_color(p.muted)
                    .child("↑ ↓ moverse")
                    .child("Enter ejecutar")
                    .child("Esc cerrar"),
            )
    }
}

/// Abre la paleta, arriba al centro.
pub fn open(app: WeakEntity<AppView>, vault: Entity<Vault>, main: Option<Entity<MainScreen>>, window: &mut Window, cx: &mut App) {
    window.close_all_dialogs(cx);
    let view = cx.new(|cx| CommandPalette::new(app, vault, main, window, cx));
    window.open_dialog(cx, move |dialog, _, _| {
        dialog.overlay(false).close_button(false).top_inset(px(60. + 30.)).margin_top(px(0.)).w(px(640.)).p(px(10.)).child(view.clone())
    });
}
