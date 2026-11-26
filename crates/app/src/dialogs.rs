//! Diálogos de cuentas: crear/editar (con eliminar y detectar del cliente) e importar desde txt,
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
use gpui_kit::component::{h_flex, v_flex, Icon, IconName, IndexPath, Sizable, WindowExt};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use sv_core::import::{non_empty_lines, parse_import};
use sv_core::model::Account;
use sv_core::snapshot::Snapshot;
use sv_core::store::{AccountInput, ImportRow};

pub const SERVERS: [&str; 11] = ["LAS", "LAN", "NA", "BR", "EUW", "EUNE", "OCE", "TR", "RU", "KR", "JP"];

/// Los diálogos se abren bajo la barra de arriba (60 px), que sigue visible y usable: se puede
/// mover la ventana y tocar sus botones. Quedan centrados con el mismo espacio arriba y abajo
/// (DIALOG_GAP + el margen de la librería). Sin el velo de la librería: la vista raíz oscurece solo
/// lo que queda bajo la barra (ver AppView).
const DIALOG_GAP: f32 = 20.;
pub fn below_bar(d: Dialog) -> Dialog {
    d.overlay(false).centered().top_inset(px(60. + DIALOG_GAP)).bottom_inset(px(DIALOG_GAP)).p(px(22.))
}
/// Alto máximo del contenido con scroll propio de un diálogo (Herramientas, Ajustes) para que quepa
/// entero: el diálogo mide como mucho la ventana menos la barra, los espacios de arriba y abajo y el
/// margen de la librería (16 px por lado); adentro van el relleno (22 + 22), el título con su
/// separación y, si hay, el pie con sus botones.
pub fn body_max_h(window: &Window, footer: bool) -> Pixels {
    let dialog = f32::from(window.viewport_size().height) - (60. + DIALOG_GAP) - DIALOG_GAP - 2. * 16.;
    let chrome = 22. + 28. + 8. + 22. + if footer { 22. + 32. } else { 0. };
    px((dialog - chrome).max(160.))
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
    /// Cuenta leída del cliente con "Detectar del cliente" (o desde el aviso de cuenta detectada):
    /// al guardar queda vinculada a ella, para reconocerla después.
    detected: Option<Snapshot>,
    detecting: bool,
    detect_note: Option<SharedString>,
}

impl AccountForm {
    fn new(vault: Entity<Vault>, acc: Option<&Account>, prefill: Option<Snapshot>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let value = |f: fn(&Account) -> &String| acc.map(|a| f(a).clone()).unwrap_or_default();
        let input = |v: String, placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder).default_value(v))
        };
        let label = input(value(|a| &a.label), "ej: smurf mid, cuenta principal…", window, cx);
        let username = input(value(|a| &a.username), "", window, cx);
        let password = cx.new(|cx| {
            InputState::new(window, cx).masked(true).default_value(value(|a| &a.password))
        });
        let game_name = input(value(|a| &a.game_name), "Nombre (o pega Nombre#TAG)", window, cx);
        let tag_line = input(value(|a| &a.tag_line), "LAS", window, cx);
        let notes = cx.new(|cx| {
            TextareaState::new(window, cx).rows(3).placeholder("Correo asociado, fecha de creación, ban…").default_value(value(|a| &a.notes))
        });
        let server = server_select(acc.map(|a| a.server.as_str()).unwrap_or("LAS"), window, cx);
        let mut this = Self {
            vault,
            id: acc.map(|a| a.id.clone()),
            label,
            username,
            password,
            game_name,
            tag_line,
            notes,
            server,
            error: None,
            detected: None,
            detecting: false,
            detect_note: None,
        };
        if let Some(snap) = prefill {
            this.fill_from(snap, window, cx);
        }
        // Se empieza por lo primero que falta: el usuario, o la contraseña si el usuario ya vino del cliente.
        let first = if this.value(&this.username, cx).is_empty() { &this.username } else { &this.password };
        first.update(cx, |s, cx| s.focus(window, cx));
        this
    }

    fn value(&self, s: &Entity<InputState>, cx: &App) -> String {
        s.read(cx).value().to_string()
    }

    /// Lee la cuenta abierta en el cliente de LoL y rellena usuario, Riot ID y servidor.
    fn detect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.detecting {
            return;
        }
        self.detecting = true;
        self.error = None;
        cx.notify();
        let work = cx.background_spawn(async move { sv_core::lcu::lcu().current_account(true) });
        cx.spawn_in(window, async move |this, cx| {
            let snap = work.await;
            this.update_in(cx, |this, window, cx| {
                this.detecting = false;
                match snap {
                    None => this.error = Some("No encontré el cliente de LoL abierto con una sesión iniciada.".into()),
                    Some(snap) => {
                        let existing = this.vault.read(cx).store.find_detected(&snap).filter(|id| this.id.as_ref() != Some(id));
                        let existing = existing.and_then(|id| this.vault.read(cx).store.data()?.account(&id).cloned());
                        if let Some(a) = existing {
                            let name = if a.label.is_empty() { a.username.clone() } else { a.label.clone() };
                            this.error = Some(format!("{} ya está guardada en tu bóveda (\"{name}\").", snap.riot_id()).into());
                        } else {
                            this.fill_from(snap, window, cx);
                            this.password.update(cx, |s, cx| s.focus(window, cx));
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn fill_from(&mut self, snap: Snapshot, window: &mut Window, cx: &mut Context<Self>) {
        let set = |s: &Entity<InputState>, v: &str, window: &mut Window, cx: &mut Context<Self>| {
            if !v.is_empty() {
                let v = v.to_string();
                s.update(cx, |s, cx| s.set_value(v, window, cx));
            }
        };
        set(&self.username, &snap.username, window, cx);
        set(&self.game_name, &snap.game_name, window, cx);
        set(&self.tag_line, &snap.tag_line, window, cx);
        if let Some(ix) = SERVERS.iter().position(|s| *s == snap.server) {
            self.server.update(cx, |s, cx| s.set_selected_index(Some(IndexPath::new(ix + 1)), window, cx));
        }
        let what = if snap.username.is_empty() { "Riot ID y servidor" } else { "usuario, Riot ID y servidor" };
        self.detect_note = Some(format!("{} · {what} tomados del cliente. Falta la contraseña.", snap.riot_id()).into());
        self.detected = Some(snap);
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
        // Sin contraseña no se puede entrar con Jugar (se escribe en el login de Riot).
        if self.id.is_none() && v(&self.password, cx).is_empty() {
            self.error = Some("Escribe la contraseña: se usa para iniciar sesión con Jugar".into());
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
        // Se vincula a la cuenta detectada solo si el Riot ID sigue siendo el del cliente.
        let link = self.detected.clone().filter(|snap| {
            input.game_name.as_deref().unwrap_or("").eq_ignore_ascii_case(&snap.game_name)
                && input.tag_line.as_deref().unwrap_or("").eq_ignore_ascii_case(&snap.tag_line)
        });
        let saved = self.vault.update(cx, |vault, cx| {
            vault.change(cx, |s| {
                let id = s.save_account(input)?;
                if let Some(snap) = &link {
                    s.link_snapshot(&id, snap)?;
                }
                Ok::<_, String>(id)
            })
        });
        match saved {
            Ok(id) => {
                // Riot ID corregido: se busca al tiro la cuenta nueva si hay API key.
                let after = self.vault.read(cx).store.data().and_then(|d| d.account(&id)).filter(|a| !a.game_name.is_empty() && !a.tag_line.is_empty());
                let changed = after.is_some_and(|a| before.as_deref() != Some(a.riot_id().to_lowercase().as_str()));
                if changed && has_key {
                    window.push_notification(Notification::info("Riot ID cambiado, buscando la cuenta…"), cx);
                    let vault = self.vault.clone();
                    window.defer(cx, move |window, cx| crate::client::refresh_riot_quiet(vault, id, window, cx));
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
        // Al agregar, lo primero es poder traer los datos de la cuenta abierta en el cliente.
        let detect_bar = self.id.is_none().then(|| {
            let text: SharedString = self
                .detect_note
                .clone()
                .unwrap_or_else(|| "¿Tienes la cuenta abierta en el LoL? Trae su usuario, Riot ID y servidor.".into());
            let done = self.detect_note.is_some();
            h_flex()
                .gap(px(10.))
                .py(px(8.))
                .pl(px(12.))
                .pr(px(8.))
                .rounded(px(8.))
                .border_1()
                .border_color(alpha(p.cyan, 0.25))
                .bg(alpha(p.cyan, 0.07))
                .child(if done { Icon::new(IconName::Check) } else { Icon::empty().path(assets::icon("gamepad-2")) }.text_color(p.cyan))
                .child(div().flex_1().min_w_0().text_size(px(12.5)).text_color(p.text_2).child(text))
                .child(
                    Button::new("acc-detect")
                        .small()
                        .label(if done { "Volver a detectar" } else { "Detectar del cliente" })
                        .loading(self.detecting)
                        .on_click(cx.listener(|this, _, window, cx| this.detect(window, cx))),
                )
        });
        v_flex()
            .gap(px(12.))
            .children(detect_bar)
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
            .child(field("Etiqueta", Input::new(&self.label), cx))
            .child(field("Notas", Textarea::new(&self.notes), cx))
            .when_some(self.error.clone(), |d, e| d.child(div().text_size(px(13.)).text_color(p.danger).child(e)))
    }
}

/// Abre el diálogo para crear (acc = None) o editar una cuenta.
pub fn open_account(vault: Entity<Vault>, acc: Option<Account>, window: &mut Window, cx: &mut App) {
    open_account_with(vault, acc, None, window, cx);
}

fn open_account_with(vault: Entity<Vault>, acc: Option<Account>, prefill: Option<Snapshot>, window: &mut Window, cx: &mut App) {
    let form = cx.new(|cx| AccountForm::new(vault.clone(), acc.as_ref(), prefill, window, cx));
    let (id, name) = acc.as_ref().map(|a| (a.id.clone(), if a.game_name.is_empty() { a.username.clone() } else { a.game_name.clone() })).unzip();
    let title = if acc.is_some() { "Editar cuenta" } else { "Nueva cuenta" };
    window.open_dialog(cx, move |dialog, _, cx| {
        let (form_save, vault_del) = (form.clone(), vault.clone());
        let (id_del, name) = (id.clone(), name.clone().unwrap_or_default());
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

/// Nueva cuenta: el formulario vacío, con "Detectar del cliente" arriba para traer los datos de la
/// cuenta abierta en el LoL.
pub fn open_add(vault: Entity<Vault>, window: &mut Window, cx: &mut App) {
    open_account_with(vault, None, None, window, cx);
}

/// Nueva cuenta ya rellenada con la cuenta detectada en el cliente (desde su aviso).
pub fn open_add_detected(vault: Entity<Vault>, snap: Snapshot, window: &mut Window, cx: &mut App) {
    open_account_with(vault, None, Some(snap), window, cx);
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
/// Pide (o cambia) la API key de Riot y después actualiza `ids` (todas si es None). `expired`: la key
/// guardada fue rechazada por Riot.
pub fn open_riot_key(vault: Entity<Vault>, ids: Option<Vec<String>>, expired: bool, window: &mut Window, cx: &mut App) {
    let current = vault.read(cx).store.data().map(|d| d.settings.riot_api_key.clone()).unwrap_or_default();
    let key = cx.new(|cx| InputState::new(window, cx).masked(true).placeholder("RGAPI-xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx").default_value(current));
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
            .title(dialog_title(if expired { "Tu API key de Riot caducó" } else { "API key de Riot" }))
            .w(px(540.))
            .child(
                v_flex()
                    .gap(px(12.))
                    .when(expired, |d| {
                        d.child(
                            h_flex()
                                .gap(px(8.))
                                .p(px(10.))
                                .rounded(px(8.))
                                .bg(alpha(p.danger, 0.1))
                                .border_1()
                                .border_color(alpha(p.danger, 0.3))
                                .child(Icon::new(IconName::CircleX).text_color(p.danger))
                                .child(div().flex_1().min_w_0().text_size(px(12.5)).text_color(p.text).child(
                                    "Riot rechazó la key guardada, así que no se actualizó ninguna cuenta. Pega una nueva para seguir.",
                                )),
                        )
                    })
                    .child(div().text_size(px(13.)).text_color(p.text_2).child(
                        "Para consultar todas tus cuentas a la vez sin abrir el juego, Smurf Vault usa la API oficial de Riot, que pide una API key gratuita.",
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
