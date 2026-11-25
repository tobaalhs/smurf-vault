//! Diálogos de cuentas: crear/editar (con eliminar y olvidar sesión), agregar e importar desde txt,
//! más una confirmación genérica. Igual que los <dialog> de la versión Electron.
use crate::assets;
use crate::state::Vault;
use crate::theme::palette;
use crate::ui::alpha;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::select::{Select, SelectState};
use gpui_kit::component::dialog::Dialog;
use gpui_kit::component::{h_flex, v_flex, Icon, IndexPath, WindowExt};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use sv_core::import::{non_empty_lines, parse_import};
use sv_core::model::Account;
use sv_core::store::{AccountInput, ImportRow};

pub const SERVERS: [&str; 11] = ["LAS", "LAN", "NA", "BR", "EUW", "EUNE", "OCE", "TR", "RU", "KR", "JP"];

/// Los diálogos se abren bajo la barra de arriba, que sigue visible y usable. Sin el velo de la
/// librería: la vista raíz oscurece solo lo que queda bajo la barra (ver AppView).
pub fn below_bar(d: Dialog) -> Dialog {
    d.overlay(false).centered().p(px(22.))
}
const NO_SERVER: &str = "—";

type Choice = SelectState<Vec<SharedString>>;

fn server_select(value: &str, window: &mut Window, cx: &mut App) -> Entity<Choice> {
    let options: Vec<SharedString> = std::iter::once(NO_SERVER.into()).chain(SERVERS.iter().map(|s| SharedString::from(*s))).collect();
    let ix = SERVERS.iter().position(|s| *s == value).map(|i| i + 1).unwrap_or(0);
    cx.new(|cx| SelectState::new(options, Some(IndexPath::new(ix)), window, cx))
}

fn selected_server(select: &Entity<Choice>, cx: &App) -> String {
    select.read(cx).selected_value().map(|v| v.to_string()).filter(|v| v != NO_SERVER).unwrap_or_default()
}

fn field(label: &'static str, input: impl IntoElement, cx: &App) -> Div {
    let p = palette(cx);
    v_flex().flex_1().gap(px(5.)).child(div().text_size(px(12.)).text_color(p.text_2).child(label)).child(input)
}

/// Pregunta antes de algo que no se puede deshacer. `on_yes` corre si se confirma.
pub fn confirm(
    title: &'static str,
    text: String,
    yes: &'static str,
    window: &mut Window,
    cx: &mut App,
    on_yes: impl Fn(&mut Window, &mut App) + 'static,
) {
    let on_yes = std::rc::Rc::new(on_yes);
    window.open_dialog(cx, move |dialog, _, cx| {
        let p = palette(cx);
        let on_yes = on_yes.clone();
        below_bar(dialog)
            .title(dialog_title(title))
            .w(px(420.))
            .child(div().text_size(px(13.5)).text_color(p.text_2).child(text.clone()))
            .footer(
                h_flex()
                    .w_full()
                    .gap(px(8.))
                    .child(div().flex_1())
                    .child(Button::new("confirm-no").label("Cancelar").on_click(|_, window, cx| window.close_dialog(cx)))
                    .child(Button::new("confirm-yes").danger().label(yes).on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        on_yes(window, cx);
                    })),
            )
    });
}

// ---------- crear / editar cuenta ----------

pub struct AccountForm {
    vault: Entity<Vault>,
    id: Option<String>,
    label: Entity<InputState>,
    username: Entity<InputState>,
    password: Entity<InputState>,
    game_name: Entity<InputState>,
    tag_line: Entity<InputState>,
    notes: Entity<TextareaState>,
    server: Entity<Choice>,
    error: Option<SharedString>,
}

impl AccountForm {
    fn new(vault: Entity<Vault>, acc: Option<&Account>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let value = |f: fn(&Account) -> &String| acc.map(|a| f(a).clone()).unwrap_or_default();
        let input = |v: String, placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder).default_value(v))
        };
        let label = input(value(|a| &a.label), "ej: smurf mid, cuenta principal…", window, cx);
        let username = input(value(|a| &a.username), "", window, cx);
        let password = cx.new(|cx| {
            InputState::new(window, cx).masked(true).placeholder("Opcional con sesión guardada").default_value(value(|a| &a.password))
        });
        let game_name = input(value(|a| &a.game_name), "Nombre (o pega Nombre#TAG)", window, cx);
        let tag_line = input(value(|a| &a.tag_line), "LAS", window, cx);
        let notes = cx.new(|cx| {
            TextareaState::new(window, cx).rows(3).placeholder("Correo asociado, fecha de creación, ban…").default_value(value(|a| &a.notes))
        });
        let server = server_select(acc.map(|a| a.server.as_str()).unwrap_or("LAS"), window, cx);
        // Con la cuenta ya armada se empieza por la etiqueta; si es nueva, por el usuario.
        let first = if acc.is_some_and(|a| !a.username.is_empty()) { &label } else { &username };
        first.update(cx, |s, cx| s.focus(window, cx));
        Self { vault, id: acc.map(|a| a.id.clone()), label, username, password, game_name, tag_line, notes, server, error: None }
    }

    /// Guarda la cuenta. Devuelve true si se guardó (y el diálogo se puede cerrar).
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let v = |s: &Entity<InputState>, cx: &App| s.read(cx).value().to_string();
        let username = v(&self.username, cx);
        if username.trim().is_empty() {
            self.error = Some("Escribe el usuario de la cuenta".into());
            cx.notify();
            return false;
        }
        let (mut game_name, mut tag_line) = (v(&self.game_name, cx), v(&self.tag_line, cx));
        // Si pegaron "Nombre#TAG" en el campo de nombre, lo separamos.
        if let Some((g, t)) = game_name.clone().split_once('#') {
            game_name = g.to_string();
            tag_line = t.to_string();
        }
        let input = AccountInput {
            id: self.id.clone(),
            label: Some(v(&self.label, cx)),
            username: Some(username),
            password: Some(v(&self.password, cx)),
            server: Some(selected_server(&self.server, cx)),
            game_name: Some(game_name),
            tag_line: Some(tag_line),
            notes: Some(self.notes.read(cx).value().to_string()),
        };
        let editing = self.id.is_some();
        let before = self.id.as_ref().and_then(|id| self.vault.read(cx).store.data()?.account(id).map(|a| a.riot_id().to_lowercase()));
        let has_key = self.vault.read(cx).store.data().is_some_and(|d| !d.settings.riot_api_key.is_empty());
        match self.vault.update(cx, |vault, cx| vault.change(cx, |s| s.save_account(input))) {
            Ok(id) => {
                // Riot ID corregido: se busca al tiro la cuenta nueva si hay API key.
                let after = self.vault.read(cx).store.data().and_then(|d| d.account(&id)).filter(|a| !a.game_name.is_empty() && !a.tag_line.is_empty());
                let changed = after.is_some_and(|a| before.as_deref() != Some(a.riot_id().to_lowercase().as_str()));
                if changed && has_key {
                    window.push_notification(Notification::info("Riot ID cambiado, buscando la cuenta…"), cx);
                    let vault = self.vault.clone();
                    window.defer(cx, move |window, cx| crate::client::refresh_riot(vault, Some(vec![id]), window, cx));
                } else {
                    window.push_notification(Notification::success(if editing { "Cambios guardados" } else { "Cuenta agregada" }), cx);
                }
                true
            }
            Err(e) => {
                self.error = Some(e.into());
                cx.notify();
                false
            }
        }
    }
}

impl Render for AccountForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        v_flex()
            .gap(px(12.))
            .child(field("Etiqueta", Input::new(&self.label), cx))
            .child(
                h_flex()
                    .gap(px(10.))
                    .child(field("Usuario", Input::new(&self.username), cx))
                    .child(field("Contraseña", Input::new(&self.password).mask_toggle(), cx)),
            )
            .child(
                h_flex()
                    .gap(px(10.))
                    .child(div().flex_1().child(field("Riot ID", Input::new(&self.game_name), cx)))
                    .child(div().w(px(96.)).child(field("Tag", Input::new(&self.tag_line), cx)))
                    .child(div().w(px(96.)).child(field("Servidor", Select::new(&self.server), cx))),
            )
            .child(field("Notas", Textarea::new(&self.notes), cx))
            .when_some(self.error.clone(), |d, e| d.child(div().text_size(px(13.)).text_color(p.danger).child(e)))
    }
}

/// Abre el diálogo para crear (acc = None) o editar una cuenta.
pub fn open_account(vault: Entity<Vault>, acc: Option<Account>, window: &mut Window, cx: &mut App) {
    let form = cx.new(|cx| AccountForm::new(vault.clone(), acc.as_ref(), window, cx));
    let has_session = acc.as_ref().is_some_and(|a| vault.read(cx).store.has_session(&a.id));
    let (id, name) = acc.as_ref().map(|a| (a.id.clone(), if a.game_name.is_empty() { a.username.clone() } else { a.game_name.clone() })).unzip();
    let title = if acc.is_some() { "Editar cuenta" } else { "Nueva cuenta" };
    window.open_dialog(cx, move |dialog, _, cx| {
        let (form_save, vault_del, vault_forget) = (form.clone(), vault.clone(), vault.clone());
        let (id_del, id_forget, name) = (id.clone(), id.clone(), name.clone().unwrap_or_default());
        let danger = palette(cx).danger;
        below_bar(dialog).title(dialog_title(title)).w(px(580.)).child(form.clone()).footer(
            h_flex()
                .w_full()
                .gap(px(8.))
                .when_some(id_del, |d, id| {
                    d.child(Button::new("acc-delete").ghost().text_color(danger).label("Eliminar").on_click(move |_, window, cx| {
                        let (vault, id) = (vault_del.clone(), id.clone());
                        confirm(
                            "¿Eliminar cuenta?",
                            format!("Se eliminará \"{name}\" de tu bóveda. No se puede deshacer."),
                            "Eliminar",
                            window,
                            cx,
                            move |window, cx| {
                                match vault.update(cx, |v, cx| v.change(cx, |s| s.delete_account(&id))) {
                                    Ok(()) => {
                                        window.close_dialog(cx);
                                        window.push_notification(Notification::info("Cuenta eliminada"), cx);
                                    }
                                    Err(e) => window.push_notification(Notification::error(e), cx),
                                }
                            },
                        );
                    }))
                })
                .when(has_session, |d| {
                    let id = id_forget.clone().unwrap_or_default();
                    d.child(
                        Button::new("acc-forget")
                            .ghost()
                            .label("Olvidar sesión")
                            .tooltip("La próxima vez tendrás que escribir la contraseña en el cliente")
                            .on_click(move |_, window, cx| {
                                vault_forget.update(cx, |v, cx| v.change(cx, |s| s.delete_session(&id)));
                                window.close_dialog(cx);
                                window.push_notification(Notification::info("Sesión olvidada"), cx);
                            }),
                    )
                })
                .child(div().flex_1())
                .child(Button::new("acc-cancel").label("Cancelar").on_click(|_, window, cx| window.close_dialog(cx)))
                .child(Button::new("acc-save").primary().label("Guardar").on_click(move |_, window, cx| {
                    if form_save.update(cx, |f, cx| f.save(window, cx)) {
                        window.close_dialog(cx);
                    }
                })),
        )
    });
}

// ---------- agregar ----------

/// La forma principal de agregar: iniciar sesión en el cliente. La manual queda como alternativa.
pub fn open_add(vault: Entity<Vault>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _, cx| {
        let p = palette(cx);
        let vault = vault.clone();
        let step = |n: &'static str, text: &'static str| {
            h_flex()
                .gap(px(10.))
                .items_start()
                .child(div().size(px(22.)).flex_none().rounded_full().bg(alpha(p.gold, 0.15)).text_color(p.gold).text_size(px(12.)).font_weight(FontWeight::BOLD).flex().items_center().justify_center().child(n))
                .child(div().text_size(px(13.)).text_color(p.text_2).child(text))
        };
        below_bar(dialog)
            .title(dialog_title("Agregar cuenta"))
            .w(px(540.))
            .child(
                v_flex()
                    .gap(px(12.))
                    .child(div().text_size(px(13.)).text_color(p.text_2).child(
                        "Inicia sesión una vez en el Riot Client y Smurf Vault guarda la cuenta sola. Después entras con un clic en Jugar, sin escribir la contraseña.",
                    ))
                    .child(step("1", "Toca Abrir login de Riot. Si el cliente está abierto, se cierra."))
                    .child(step("2", "Inicia sesión con la cuenta y marca Mantener sesión iniciada (Stay signed in)."))
                    .child(step("3", "El LoL se abre solo y aquí te pedimos confirmar la cuenta para guardarla."))
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .p(px(10.))
                            .rounded(px(8.))
                            .bg(alpha(p.cyan, 0.08))
                            .child(Icon::empty().path(assets::icon("key-round")).text_color(p.cyan))
                            .child(div().text_size(px(12.5)).text_color(p.text_2).child(
                                "Sin Mantener sesión iniciada la cuenta se guarda igual, pero tendrás que escribir la contraseña cada vez.",
                            )),
                    ),
            )
            .footer(
                h_flex()
                    .w_full()
                    .gap(px(8.))
                    .child(Button::new("add-manual").ghost().label("Agregar a mano").on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        open_account(vault.clone(), None, window, cx);
                    }))
                    .child(div().flex_1())
                    .child(Button::new("add-cancel").label("Cancelar").on_click(|_, window, cx| window.close_dialog(cx)))
                    .child(
                        Button::new("add-login")
                            .primary()
                            .icon(Icon::empty().path(assets::icon("log-in")))
                            .label("Abrir login de Riot")
                            .on_click(|_, window, cx| {
                                window.push_notification(Notification::info("Abrir el login de Riot llega en la fase de cambio de cuenta."), cx)
                            }),
                    ),
            )
    });
}

// ---------- importar ----------

pub struct ImportForm {
    vault: Entity<Vault>,
    text: Entity<TextareaState>,
    server: Entity<Choice>,
    _sub: Subscription,
}

impl ImportForm {
    fn new(vault: Entity<Vault>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let text = cx.new(|cx| TextareaState::new(window, cx).rows(9).placeholder("micuenta1:clave123\nsmurf_2 otraClave"));
        let sub = cx.subscribe(&text, |_, _, _: &InputEvent, cx| cx.notify());
        text.update(cx, |s, cx| s.focus(window, cx));
        Self { vault, server: server_select("LAS", window, cx), text, _sub: sub }
    }

    fn import(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let server = selected_server(&self.server, cx);
        let rows: Vec<ImportRow> = parse_import(&self.text.read(cx).value())
            .into_iter()
            .map(|(username, password)| ImportRow { username, password, server: server.clone() })
            .collect();
        if rows.is_empty() {
            window.push_notification(Notification::error("No reconocí ninguna línea"), cx);
            return false;
        }
        match self.vault.update(cx, |v, cx| v.change(cx, |s| s.import_accounts(rows))) {
            Ok(n) => {
                window.push_notification(Notification::success(format!("{n} cuentas importadas")), cx);
                true
            }
            Err(e) => {
                window.push_notification(Notification::error(e), cx);
                false
            }
        }
    }
}

impl Render for ImportForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let text = self.text.read(cx).value().to_string();
        let (ok, lines) = (parse_import(&text).len(), non_empty_lines(&text));
        let preview = if lines == 0 {
            String::new()
        } else if lines > ok {
            format!("{ok} cuentas reconocidas · {} líneas ignoradas", lines - ok)
        } else {
            format!("{ok} cuentas reconocidas")
        };
        v_flex()
            .gap(px(12.))
            .child(div().text_size(px(13.)).text_color(p.text_2).child("Una cuenta por línea: usuario:contraseña. También sirve ; , tab, / o un espacio."))
            .child(Textarea::new(&self.text))
            .child(
                h_flex()
                    .gap(px(12.))
                    .child(div().w(px(110.)).child(field("Servidor", Select::new(&self.server), cx)))
                    .child(div().pt(px(18.)).text_size(px(12.5)).text_color(p.muted).child(preview)),
            )
    }
}

pub fn open_import(vault: Entity<Vault>, window: &mut Window, cx: &mut App) {
    let form = cx.new(|cx| ImportForm::new(vault, window, cx));
    window.open_dialog(cx, move |dialog, _, _| {
        let form_ok = form.clone();
        below_bar(dialog).title(dialog_title("Importar desde txt")).w(px(540.)).child(form.clone()).footer(
            h_flex()
                .w_full()
                .gap(px(8.))
                .child(div().flex_1())
                .child(Button::new("import-cancel").label("Cancelar").on_click(|_, window, cx| window.close_dialog(cx)))
                .child(Button::new("import-ok").primary().label("Importar").on_click(move |_, window, cx| {
                    if form_ok.update(cx, |f, cx| f.import(window, cx)) {
                        window.close_dialog(cx);
                    }
                })),
        )
    });
}

// ---------- API key de Riot ----------

/// Pide la API key de Riot y, al guardarla, sigue con la actualización que la necesitaba.
pub fn open_riot_key(vault: Entity<Vault>, ids: Option<Vec<String>>, window: &mut Window, cx: &mut App) {
    let key = cx.new(|cx| InputState::new(window, cx).masked(true).placeholder("RGAPI-xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"));
    key.update(cx, |s, cx| s.focus(window, cx));
    window.open_dialog(cx, move |dialog, _, cx| {
        let p = palette(cx);
        let (key_ok, vault, ids) = (key.clone(), vault.clone(), ids.clone());
        let step = |n: &'static str, text: Div| {
            h_flex()
                .gap(px(10.))
                .items_start()
                .child(div().size(px(22.)).flex_none().rounded_full().bg(alpha(p.gold, 0.15)).text_color(p.gold).text_size(px(12.)).font_weight(FontWeight::BOLD).flex().items_center().justify_center().child(n))
                .child(text.flex_1().min_w_0().text_size(px(13.)).text_color(p.text_2))
        };
        below_bar(dialog)
            .title(dialog_title("Actualizar rangos sin abrir el juego"))
            .w(px(540.))
            .child(
                v_flex()
                    .gap(px(12.))
                    .child(div().text_size(px(13.)).text_color(p.text_2).child(
                        "Para consultar todas tus cuentas a la vez, Smurf Vault usa la API oficial de Riot, que pide una API key gratuita.",
                    ))
                    .child(step(
                        "1",
                        h_flex().flex_wrap().child("Entra a ").child(
                            div()
                                .id("riot-portal")
                                .text_color(p.cyan)
                                .cursor_pointer()
                                .child("developer.riotgames.com")
                                .on_click(|_, _, cx| cx.open_url("https://developer.riotgames.com/")),
                        ).child(" con tu cuenta de Riot."),
                    ))
                    .child(step(
                        "2",
                        div().child("Copia la Development API Key y pégala aquí. Dura 24 h; para una permanente, pide una Personal API Key ahí mismo."),
                    ))
                    .child(Input::new(&key).mask_toggle())
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .p(px(10.))
                            .rounded(px(8.))
                            .bg(alpha(p.cyan, 0.08))
                            .child(Icon::empty().path(assets::icon("gamepad-2")).text_color(p.cyan))
                            .child(div().flex_1().min_w_0().text_size(px(12.5)).text_color(p.text_2).child(
                                "¿Sin key? Con el cliente de LoL abierto, la cuenta en la que inicias sesión se actualiza sola.",
                            )),
                    ),
            )
            .footer(
                h_flex()
                    .w_full()
                    .gap(px(8.))
                    .child(div().flex_1())
                    .child(Button::new("riot-later").label("Ahora no").on_click(|_, window, cx| window.close_dialog(cx)))
                    .child(Button::new("riot-save").primary().label("Guardar y actualizar").on_click(move |_, window, cx| {
                        let value = key_ok.read(cx).value().trim().to_string();
                        if !value.starts_with("RGAPI-") {
                            window.push_notification(Notification::error("La key debería empezar con RGAPI-"), cx);
                            return;
                        }
                        let mut patch = serde_json::Map::new();
                        patch.insert("riotApiKey".into(), value.into());
                        match vault.update(cx, |v, cx| v.change(cx, |s| s.save_settings(patch))) {
                            Ok(()) => {
                                window.close_dialog(cx);
                                crate::client::refresh_riot(vault.clone(), ids.clone(), window, cx);
                            }
                            Err(e) => window.push_notification(Notification::error(e), cx),
                        }
                    })),
            )
    });
}

/// Título de diálogo como en Electron (17 px, seminegrita).
pub fn dialog_title(text: &'static str) -> Div {
    div().pb(px(12.)).text_size(px(17.)).font_weight(FontWeight::SEMIBOLD).child(text)
}
