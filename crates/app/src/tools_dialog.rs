//! Diálogo de Herramientas: autoaceptar partidas, aparecer desconectado, inicio de sesión automático
//! y cambiar el fondo de perfil a cualquier skin. Igual que #toolsDialog de la versión Electron. El
//! control desde el celular llega en la fase siguiente.
use crate::assets;
use crate::dialogs::{below_bar, dialog_title};
use crate::state::Vault;
use crate::tabs::{segment, segmented};
use crate::theme::{palette, Palette};
use crate::ui::alpha;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{h_flex, v_flex, Icon, IconName, Sizable, WindowExt};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::time::Duration;
use sv_core::skins::{norm_text, set_profile_background, Skin, SkinCatalog};
use sv_core::tools::{tools, ClientStatus};

/// No se dibujan miles de tarjetas de una.
const MAX_SKINS: usize = 120;
const DELAYS: [u32; 4] = [0, 2, 4, 6];

/// Aplica a las herramientas en segundo plano lo que dice la configuración (al abrir la app y al
/// cambiar un interruptor).
pub fn apply_config(vault: &Entity<Vault>, cx: &App) {
    let c = &vault.read(cx).store.config;
    tools().configure_auto_accept(c.auto_accept, c.auto_accept_delay);
    tools().configure_offline(c.appear_offline);
}

pub struct ToolsPanel {
    vault: Entity<Vault>,
    search: Entity<InputState>,
    /// Skins con su texto de búsqueda (nombre + campeón, sin tildes). None mientras carga.
    skins: Option<Vec<(Skin, String)>>,
    skins_error: bool,
    applying: Option<i64>,
    /// Dirección, QR y celulares conectados del control desde el celular (se refresca cada segundo).
    remote: Option<sv_core::remote::RemoteStatus>,
    remote_qr: Option<(String, Vec<bool>, usize)>,
    remote_error: Option<SharedString>,
    scroll: ScrollHandle,
    _poll: Task<()>,
    _sub: Subscription,
}

impl ToolsPanel {
    fn new(vault: Entity<Vault>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Busca una skin o campeón…"));
        let sub = cx.subscribe(&search, |_, _, _: &InputEvent, cx| cx.notify());
        // El estado del cliente (conectado / esperando partida) se refresca mientras está abierto.
        let poll = cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            if this.update(cx, |this, cx| {
                this.refresh_remote(cx);
                cx.notify();
            })
            .is_err()
            {
                break;
            }
        });
        let paths = vault.read(cx).store.paths.clone();
        let champs = vault.read(cx).game.champions.clone();
        cx.spawn(async move |this, cx| {
            let loaded = cx.background_executor().spawn(async move { SkinCatalog::load(&paths) }).await;
            this.update(cx, |this, cx| {
                match loaded {
                    Ok(cat) => {
                        this.skins = Some(
                            cat.skins
                                .into_iter()
                                .map(|s| {
                                    let champ = champs.get(&s.champ.to_string()).cloned().unwrap_or_default();
                                    let q = norm_text(&format!("{} {champ}", s.name));
                                    (s, q)
                                })
                                .collect(),
                        )
                    }
                    Err(_) => this.skins_error = true,
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        let mut this = Self {
            vault,
            search,
            skins: None,
            skins_error: false,
            applying: None,
            remote: None,
            remote_qr: None,
            remote_error: None,
            scroll: ScrollHandle::new(),
            _poll: poll,
            _sub: sub,
        };
        this.refresh_remote(cx);
        // Solo en desarrollo: SV_DEV_SCROLL baja el diálogo (para revisar las secciones de abajo).
        #[cfg(debug_assertions)]
        if let Some(y) = std::env::var("SV_DEV_SCROLL").ok().and_then(|v| v.parse::<f32>().ok()) {
            this.scroll.set_offset(point(px(0.), px(-y)));
        }
        // Solo en desarrollo: SV_DEV_SKIN_SEARCH busca esa skin 1,5 s después de abrir (con la grilla
        // inicial ya pedida), para medir cuánto tarda en llenarse.
        #[cfg(debug_assertions)]
        if let Ok(q) = std::env::var("SV_DEV_SKIN_SEARCH") {
            let search = this.search.clone();
            window
                .spawn(cx, async move |cx| {
                    cx.background_executor().timer(std::time::Duration::from_millis(1500)).await;
                    let _ = cx.update(|window, cx| search.update(cx, |s, cx| s.set_value(q, window, cx)));
                })
                .detach();
        }
        this
    }

    fn refresh_remote(&mut self, cx: &App) {
        self.remote = self.vault.read(cx).remote.as_ref().map(|r| r.status());
        let url = self.remote.as_ref().and_then(|r| r.url.clone());
        if self.remote_qr.as_ref().map(|q| &q.0) != url.as_ref() {
            self.remote_qr = url.and_then(|u| sv_core::remote::qr_matrix(&u).map(|(m, w)| (u, m, w)));
        }
    }

    fn set_config(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut sv_core::config::Config)) {
        self.vault.update(cx, |v, cx| {
            f(&mut v.store.config);
            let _ = v.store.save_config();
            cx.notify();
        });
        apply_config(&self.vault, cx);
        self.remote_error = self.vault.update(cx, |v, _| v.sync_remote()).err().map(Into::into);
        self.refresh_remote(cx);
        cx.notify();
    }

    fn remote_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let Some(status) = &self.remote else {
            return note_text(self.remote_error.clone().unwrap_or_else(|| "Apagado".into()), &p).into_any_element();
        };
        let qr = self.remote_qr.clone();
        let clients = match status.clients {
            0 => "Ningún celular conectado todavía".to_string(),
            1 => "1 celular conectado".to_string(),
            n => format!("{n} celulares conectados"),
        };
        h_flex()
            .items_start()
            .gap(px(16.))
            .child(
                div().size(px(150.)).flex_none().rounded(px(10.)).bg(gpui_kit::white()).p(px(6.)).child(
                    canvas(
                        |_, _, _| (),
                        move |b, _, window, _| {
                            let Some((_, m, w)) = &qr else { return };
                            let cell = f32::from(b.size.width) / *w as f32;
                            let dark: Hsla = rgba(0x0b0f15ff).into();
                            for (i, on) in m.iter().enumerate() {
                                if *on {
                                    let (x, y) = ((i % w) as f32 * cell, (i / w) as f32 * cell);
                                    let origin = point(b.origin.x + px(x), b.origin.y + px(y));
                                    window.paint_quad(fill(Bounds::new(origin, size(px(cell + 0.5), px(cell + 0.5))), dark));
                                }
                            }
                        },
                    )
                    .size_full(),
                ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(8.))
                    .text_size(px(13.))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Escanea el código con la cámara del celular."))
                    .child(div().text_color(p.text_2).child("Se abre una página en el navegador; no hay que instalar nada."))
                    .child(note_text(format!("O abre en el celular: {}", status.address.clone().unwrap_or_else(|| "(sin red local)".into())), &p))
                    .child(note_text(clients, &p))
                    .child(note_text("Si Windows pregunta si Smurf Vault puede usar la red, acepta en redes privadas: si no, el celular no se puede conectar.", &p))
                    .child(
                        h_flex().child(
                            gpui_kit::component::button::Button::new("remote-new-code")
                                .small()
                                .label("Generar código nuevo")
                                .tooltip("Los celulares conectados con el código anterior quedan afuera")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.vault.update(cx, |v, _| v.new_remote_code());
                                    this.refresh_remote(cx);
                                    window.push_notification(Notification::success("Código nuevo: escanea el QR otra vez"), cx);
                                    cx.notify();
                                })),
                        ),
                    ),
            )
            .into_any_element()
    }

    fn apply_skin(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        if self.applying.is_some() {
            return;
        }
        self.applying = Some(id);
        cx.notify();
        let work = cx.background_spawn(async move { set_profile_background(id) });
        cx.spawn_in(window, async move |this, cx| {
            let res = work.await;
            this.update_in(cx, |this, window, cx| {
                this.applying = None;
                match res {
                    Ok(()) => window.push_notification(Notification::success("Fondo cambiado ✓ — reabre tu perfil en el cliente para verlo"), cx),
                    Err(e) => window.push_notification(Notification::error(e), cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn skin_grid(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let Some(skins) = &self.skins else {
            let note = if self.skins_error { "No pude cargar las skins (revisa tu internet)." } else { "Cargando skins…" };
            return v_flex().child(note_text(note, &p));
        };
        let q = norm_text(self.search.read(cx).value().trim());
        let matches: Vec<&Skin> = skins.iter().filter(|(_, text)| q.is_empty() || text.contains(&q)).map(|(s, _)| s).collect();
        let total = matches.len();
        let note = if q.is_empty() {
            format!("{total} skins — escribe para buscar")
        } else {
            format!("{total} resultado{}{}", if total == 1 { "" } else { "s" }, if total > MAX_SKINS { " (se muestran 120)" } else { "" })
        };
        let cards = matches.into_iter().take(MAX_SKINS).map(|s| {
            let id = s.id;
            let applying = self.applying == Some(id);
            v_flex()
                .id(SharedString::from(format!("skin-{id}")))
                .w(px(100.))
                .gap(px(4.))
                .p(px(4.))
                .rounded(px(8.))
                .border_1()
                .border_color(p.border)
                .bg(p.surface_2)
                .cursor_pointer()
                .hover(|st| st.border_color(p.cyan))
                .when(applying, |d| d.opacity(0.5))
                .child(match &s.tile {
                    Some(url) => img(SharedString::from(url.clone())).w_full().h(px(90.)).rounded(px(6.)).object_fit(ObjectFit::Cover).into_any_element(),
                    None => div().w_full().h(px(90.)).rounded(px(6.)).bg(p.surface_3).into_any_element(),
                })
                .child(div().text_size(px(11.)).text_color(p.text_2).truncate().child(s.name.clone()))
                .on_click(cx.listener(move |this, _, window, cx| this.apply_skin(id, window, cx)))
        });
        v_flex()
            .gap(px(8.))
            .child(h_flex().flex_wrap().gap(px(8.)).children(cards))
            .child(note_text(note, &p))
    }
}

fn note_text(text: impl Into<SharedString>, p: &Palette) -> Div {
    div().text_size(px(12.)).text_color(p.muted).child(text.into())
}

/// Una herramienta: ícono, título, explicación y su interruptor; opcionalmente un cuerpo abajo.
fn tool(icon: Icon, title: impl IntoElement, hint: &'static str, toggle: Option<Switch>, body: Option<AnyElement>, p: &Palette) -> Div {
    v_flex()
        .p(px(14.))
        .rounded(px(12.))
        .border_1()
        .border_color(p.border)
        .bg(p.bg)
        .child(
            h_flex()
                .items_start()
                .gap(px(12.))
                .child(div().size(px(36.)).flex_none().rounded(px(10.)).bg(alpha(p.cyan, 0.1)).flex().items_center().justify_center().child(icon.text_color(p.cyan)))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(3.))
                        .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child(title))
                        .child(div().text_size(px(12.5)).text_color(p.text_2).child(hint)),
                )
                .children(toggle),
        )
        .when_some(body, |d, b| d.child(div().mt(px(14.)).pt(px(14.)).border_t_1().border_color(p.border).child(b)))
}

impl Render for ToolsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let c = self.vault.read(cx).store.config.clone();
        let status = tools().client_status();
        let (dot, status_text) = match (c.auto_accept, status) {
            (false, _) => (p.muted, "Apagado"),
            (true, ClientStatus::Connected) => (p.ok, "Cliente de LoL conectado · esperando partida"),
            (true, ClientStatus::NotFound) => (p.muted, "Cliente de LoL no detectado · ábrelo y se conecta solo"),
            (true, ClientStatus::Searching) => (p.muted, "Buscando el cliente de LoL…"),
        };
        let delays = DELAYS.iter().map(|d| {
            let d = *d;
            let label = if d == 0 { "Al instante".to_string() } else { format!("{d} s") };
            segment(SharedString::from(format!("delay-{d}")), &label, None, c.auto_accept_delay == d, &p)
                .on_click(cx.listener(move |this, _, _, cx| this.set_config(cx, |c| c.auto_accept_delay = d)))
        });
        let accept_body = v_flex()
            .gap(px(8.))
            .when(!c.auto_accept, |d| d.opacity(0.45))
            .child(
                h_flex()
                    .gap(px(12.))
                    .child(div().flex_1().text_size(px(13.)).text_color(p.text_2).child("Esperar antes de aceptar"))
                    .child(segmented(&p).mb(px(0.)).children(delays)),
            )
            .child(note_text("Con espera alcanzas a rechazar la partida tú si no quieres jugarla. Solo hace lo mismo que apretar \"Aceptar\".", &p))
            .child(
                h_flex()
                    .gap(px(8.))
                    .child(div().size(px(7.)).rounded_full().bg(dot))
                    .child(div().text_size(px(12.)).text_color(if dot == p.ok { p.text_2 } else { p.muted }).child(status_text)),
            );
        let entity = cx.entity().downgrade();
        let switch = |id: &'static str, on: bool, f: fn(&mut sv_core::config::Config, bool)| {
            let entity = entity.clone();
            Switch::new(id).checked(on).color(p.cyan).on_click(move |v: &bool, _, cx| {
                let v = *v;
                entity.update(cx, |this, cx| this.set_config(cx, |c| f(c, v))).ok();
            })
        };
        let max_h = f32::from(window.viewport_size().height) - 190.;
        v_flex()
            .id("tools-scroll")
            .track_scroll(&self.scroll)
            .max_h(px(max_h.max(240.)))
            .overflow_y_scroll()
            .pr(px(4.))
            .gap(px(12.))
            .child(tool(
                Icon::empty().path(assets::icon("zap")),
                "Autoaceptar partidas",
                "Acepta la partida por ti cuando sale en el cliente de LoL o TFT. Sigue funcionando con la bóveda bloqueada, mientras Smurf Vault esté abierto.",
                Some(switch("tool-accept", c.auto_accept, |c, v| c.auto_accept = v)),
                Some(accept_body.into_any_element()),
                &p,
            ))
            .child(tool(
                Icon::empty().path(assets::icon("ghost")),
                "Aparecer desconectado",
                "Tus amigos te ven desconectado en el LoL aunque tengas el cliente abierto. Se mantiene al cambiar de cuenta, mientras Smurf Vault esté abierto. Al apagarlo vuelves a quedar en línea.",
                Some(switch("tool-offline", c.appear_offline, |c, v| c.appear_offline = v)),
                None,
                &p,
            ))
            .child(tool(
                Icon::empty().path(assets::icon("keyboard")),
                h_flex()
                    .gap(px(8.))
                    .child("Inicio de sesión automático")
                    .child(div().px(px(6.)).rounded(px(5.)).bg(p.surface_3).text_size(px(10.5)).text_color(p.text_2).child("Experimental")),
                "Al pulsar Jugar, Smurf Vault inicia sesión por ti: entra usuario y contraseña directamente en el login de Riot. Si Riot pide verificación o captcha, se abre su ventana para que lo termines. Si lo apagas, se vuelve al modo anterior (restaurar la sesión guardada o copiar la contraseña).",
                Some(switch("tool-autotype", c.auto_type, |c, v| c.auto_type = v)),
                None,
                &p,
            ))
            .child(tool(
                Icon::empty().path(assets::icon("smartphone")),
                "Control desde el celular",
                "Acepta la partida, banea, elige campeón, skin, hechizos y runas desde el celular, sin pararte del sillón. El celular tiene que estar en el mismo WiFi que el PC.",
                Some(switch("tool-remote", c.remote_enabled, |c, v| c.remote_enabled = v)),
                c.remote_enabled.then(|| self.remote_body(cx)),
                &p,
            ))
            .child(tool(
                Icon::empty().path(assets::icon("image")),
                "Fondo de perfil",
                "Cambia el fondo de tu perfil de LoL a cualquier skin, la tengas o no. Necesita el cliente de LoL abierto. Busca una skin o campeón y haz clic para aplicarla.",
                None,
                Some(v_flex().gap(px(10.)).child(Input::new(&self.search).prefix(Icon::new(IconName::Search))).child(self.skin_grid(cx)).into_any_element()),
                &p,
            ))
    }
}

pub fn open_tools(vault: Entity<Vault>, window: &mut Window, cx: &mut App) {
    let panel = cx.new(|cx| ToolsPanel::new(vault, window, cx));
    window.open_dialog(cx, move |dialog, _, _| below_bar(dialog).title(dialog_title("Herramientas")).w(px(616.)).child(panel.clone()));
}
