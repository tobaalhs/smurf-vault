//! Jugar con una cuenta: inicio de sesión por inyección (si tiene usuario y contraseña), o restaurar
//! su sesión guardada del Riot Client; si nada de eso sirve, abre el login y escribe los datos (o deja
//! la contraseña copiada). Igual que play() de renderer.js y switchTo/injectLogin/assistLogin de main.js.
//!
//! Lo lento (cerrar y abrir el cliente, la inyección) va en hilos de fondo; la guía de arriba muestra
//! en qué paso va.
use crate::main_screen::MainScreen;
use crate::ui;
use futures::StreamExt;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::WindowExt;
use gpui_kit::*;
use std::time::Duration;
use sv_core::model::Account;
use sv_core::riotlogin::{auto_login, Step};
use sv_core::switcher;

/// Después de cambiar de cuenta se busca el cliente cada 5 s (no cada 30) hasta que abra el LoL.
const DETECT_SOON_EVERY: Duration = Duration::from_secs(5);
const DETECT_SOON_FOR: u32 = 36; // 3 minutos
const LEAGUE_START_TIMEOUT: Duration = Duration::from_secs(180);

fn riot_id_or_user(a: &Account) -> String {
    let id = a.riot_id();
    if id.is_empty() {
        a.username.clone()
    } else {
        id
    }
}

/// Deja que el Riot Client termine de abrir el LoL (puede tardar: espera a que haya sesión).
fn ensure_league_in_background() {
    std::thread::spawn(|| {
        switcher::ensure_league_starts(LEAGUE_START_TIMEOUT);
    });
}

impl MainScreen {
    /// Jugar con la cuenta elegida (botón JUGAR, Enter o doble clic en la rueda).
    pub(crate) fn play_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(acc) = self.selected_account(cx) else { return };
        if let Some(ban) = &acc.ban {
            let left = if ban["permanent"] == true {
                "Tiene un baneo permanente de LoL".to_string()
            } else {
                format!("Baneada: termina en {}", ui::until_text(ban["until"].as_str().unwrap_or("")))
            };
            let screen = cx.entity().downgrade();
            crate::dialogs::confirm(
                "Cuenta baneada",
                format!("{left}. Si entras a una partida te desconectará. ¿Abrir igual?"),
                "Abrir igual",
                window,
                cx,
                move |window, cx| {
                    let acc = acc.clone();
                    screen.update(cx, |s, cx| s.play(acc, window, cx)).ok();
                },
            );
            return;
        }
        self.play(acc, window, cx);
    }

    fn play(&mut self, acc: Account, window: &mut Window, cx: &mut Context<Self>) {
        if self.switching {
            window.push_notification(Notification::error("Ya se está cambiando de cuenta"), cx);
            return;
        }
        crate::sounds::play_chord();
        let auto = self.vault.read(cx).store.config.auto_type;
        if auto && !acc.username.is_empty() && !acc.password.is_empty() {
            self.inject_login(acc, window, cx);
        } else {
            self.switch_to(Some(acc), window, cx);
        }
    }

    /// Guarda la sesión que tiene ahora el Riot Client en su cuenta (Riot renueva los tokens) antes
    /// de tocar el cliente.
    fn capture_before_switch(&self, cx: &mut Context<Self>) {
        self.vault.update(cx, |v, _| {
            v.store.capture_current_session();
        });
    }

    /// Inicio de sesión automático por inyección. Si falla, cae al login normal con autoescrito.
    fn inject_login(&mut self, acc: Account, window: &mut Window, cx: &mut Context<Self>) {
        let name = riot_id_or_user(&acc);
        self.switching = true;
        self.guide = Some(format!("Iniciando sesión con {name} automáticamente…").into());
        cx.notify();
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<Step>();
        let (user, pass) = (acc.username.clone(), acc.password.clone());
        let work = cx.background_spawn(async move {
            switcher::check_not_busy()?;
            switcher::close_riot()?;
            let on_step = |s: Step| {
                let _ = tx.unbounded_send(s);
            };
            match auto_login(&user, &pass, &on_step) {
                Ok(_) => {
                    // Ya con sesión en el servicio del cliente: se abre el LoL como el botón "Jugar".
                    switcher::launch_league()?;
                    ensure_league_in_background();
                    Ok(Ok(()))
                }
                Err(e) => {
                    // La inyección no pudo: login normal, para escribirlo o pegarlo a mano.
                    let _ = switcher::write_session(None);
                    let _ = switcher::launch_league();
                    ensure_league_in_background();
                    Ok(Err(e))
                }
            }
        });
        self.capture_before_switch(cx);
        // Los pasos van actualizando la guía.
        cx.spawn_in(window, async move |this, cx| {
            while let Some(step) = rx.next().await {
                let alive = this.update(cx, |this, cx| {
                    if this.switching && step != Step::Done {
                        this.guide = Some(SharedString::from(step.text()));
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.spawn_in(window, async move |this, cx| {
            let result: Result<Result<(), String>, String> = work.await;
            this.update_in(cx, |this, window, cx| {
                this.switching = false;
                this.guide = None;
                match result {
                    Ok(Ok(())) => window.push_notification(Notification::success(format!("Sesión iniciada con {name}. Abriendo LoL…")), cx),
                    Ok(Err(e)) => {
                        window.push_notification(
                            Notification::error(format!("No pude iniciar sesión por inyección ({e}). Entra a mano en el Riot Client.")),
                            cx,
                        );
                        this.assist_login(acc.clone(), window, cx);
                    }
                    Err(e) => window.push_notification(Notification::error(e), cx),
                }
                this.detect_soon(window, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Cierra el Riot Client, deja la sesión guardada de `acc` (o ninguna, para mostrar el login) y
    /// abre el LoL. Sin sesión guardada, ayuda con el login.
    pub(crate) fn switch_to(&mut self, acc: Option<Account>, window: &mut Window, cx: &mut Context<Self>) {
        self.switching = true;
        cx.notify();
        self.capture_before_switch(cx);
        let files = acc.as_ref().and_then(|a| self.vault.read(cx).store.load_session(&a.id)).and_then(|v| v.as_object().cloned());
        let restored = files.is_some();
        let work = cx.background_spawn(async move {
            switcher::check_not_busy()?;
            switcher::close_riot()?;
            switcher::write_session(files.as_ref())?;
            switcher::launch_league()?;
            ensure_league_in_background();
            Ok::<_, String>(())
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = work.await;
            this.update_in(cx, |this, window, cx| {
                this.switching = false;
                match (result, acc) {
                    (Err(e), _) => window.push_notification(Notification::error(e), cx),
                    (Ok(()), Some(acc)) if restored => {
                        this.guide = None;
                        window.push_notification(Notification::success(format!("Abriendo LoL con {}…", riot_id_or_user(&acc))), cx);
                    }
                    (Ok(()), Some(acc)) => {
                        let copied = if acc.password.is_empty() { "" } else { " (la contraseña está copiada: pégala con Ctrl V)" };
                        this.guide = Some(
                            format!("Inicia sesión con {}{copied} y marca Mantener sesión iniciada. La próxima vez entras con un clic.", acc.username)
                                .into(),
                        );
                        this.assist_login(acc, window, cx);
                    }
                    (Ok(()), None) => {}
                }
                this.detect_soon(window, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// "Abrir login de Riot" (agregar cuenta): el cliente sin sesión, y se espera la cuenta nueva.
    pub(crate) fn open_riot_login(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.adding_account = true;
        self.guide = Some(
            "Inicia sesión en el Riot Client con la cuenta que quieres agregar y marca Mantener sesión iniciada. El LoL se abre solo y aquí te pedimos confirmarla."
                .into(),
        );
        self.switch_to(None, window, cx);
    }

    /// Login sin sesión guardada. Con el autoescrito activado escribe usuario y contraseña en el
    /// login de Riot (por el portapapeles, nunca como argumento); si no se puede, deja la contraseña
    /// copiada para pegarla.
    fn assist_login(&mut self, acc: Account, window: &mut Window, cx: &mut Context<Self>) {
        if acc.password.is_empty() {
            return;
        }
        if !self.vault.read(cx).store.config.auto_type {
            ui::copy_secret(acc.password.clone(), cx);
            return;
        }
        cx.spawn_in(window, async move |_, cx| {
            let bg = cx.background_executor().clone();
            let fallback = |msg: &'static str, cx: &mut AsyncWindowContext| {
                let pw = acc.password.clone();
                cx.update(|window, cx| {
                    ui::copy_secret(pw, cx);
                    window.push_notification(Notification::info(msg), cx);
                })
                .ok();
            };
            if !bg.spawn(async { switcher::wait_for_login_window(Duration::from_secs(90)) }).await {
                return fallback("No apareció la ventana de Riot; la contraseña quedó copiada para pegarla.", cx);
            }
            let user = acc.username.clone();
            cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string(user))).ok();
            if !bg.spawn(async { switcher::send_keys(&["^a", "^v", "{TAB}"]) }).await {
                return fallback("No pude enfocar el login de Riot; la contraseña quedó copiada.", cx);
            }
            let pw = acc.password.clone();
            cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string(pw))).ok();
            let typed = bg.spawn(async { switcher::send_keys(&["^a", "^v", "{ENTER}"]) }).await;
            let pw = acc.password.clone();
            cx.update(|window, cx| {
                if cx.read_from_clipboard().and_then(|c| c.text()).as_deref() == Some(pw.as_str()) {
                    cx.write_to_clipboard(ClipboardItem::new_string(String::new()));
                }
                let n = if typed {
                    Notification::success("Datos escritos en Riot. Si pide verificación o captcha, termínalo en la ventana.")
                } else {
                    Notification::error("No pude terminar de escribir; revisa el login de Riot.")
                };
                window.push_notification(n, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Busca el cliente cada 5 s hasta que aparezca una cuenta (o pasen 3 minutos).
    fn detect_soon(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let start = self.detect_hits;
        self.detect_soon_task = Some(cx.spawn_in(window, async move |this, cx| {
            for _ in 0..DETECT_SOON_FOR {
                cx.background_executor().timer(DETECT_SOON_EVERY).await;
                let done = this
                    .update_in(cx, |this, window, cx| {
                        if this.detect_hits != start {
                            return true;
                        }
                        this.detect(false, None, window, cx);
                        false
                    })
                    .unwrap_or(true);
                if done {
                    break;
                }
            }
        }));
    }
}
