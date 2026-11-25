//! Vincular Google Drive desde la interfaz (pantalla de bloqueo y Ajustes): cargar credentials.json
//! si falta, abrir el login de Google en el navegador y esperar la respuesta.
use crate::state::Vault;
use futures::StreamExt;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::WindowExt;
use gpui_kit::*;

/// Pide el credentials.json de Google y lo copia a la carpeta de datos. true si quedó cargado.
pub fn import_credentials(vault: Entity<Vault>, window: &mut Window, cx: &mut App, on_done: impl FnOnce(&mut Window, &mut App) + 'static) {
    let paths = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some("Elige el credentials.json de Google".into()),
    });
    window
        .spawn(cx, async move |cx| {
            let Ok(Ok(Some(list))) = paths.await else { return };
            let Some(file) = list.into_iter().next() else { return };
            cx.update(|window, cx| {
                match vault.read(cx).drive.import_credentials(&file) {
                    Ok(()) => {
                        window.push_notification(Notification::success("Credenciales cargadas. Ahora vincula tu cuenta de Google."), cx);
                        vault.update(cx, |_, cx| cx.notify());
                        on_done(window, cx);
                    }
                    Err(e) => window.push_notification(Notification::error(e), cx),
                }
            })
            .ok();
        })
        .detach();
}

/// Vincula la cuenta de Google. Con la bóveda abierta, sube la versión actual. `on_done` corre al
/// terminar bien.
pub fn link(vault: Entity<Vault>, window: &mut Window, cx: &mut App, on_done: impl FnOnce(&mut Window, &mut App) + 'static) {
    if !vault.read(cx).drive.has_credentials() {
        let v = vault.clone();
        import_credentials(vault, window, cx, move |window, cx| link(v, window, cx, on_done));
        return;
    }
    window.push_notification(Notification::info("Se abrió el navegador para iniciar sesión con Google…"), cx);
    let drive = vault.read(cx).drive.clone();
    // El login avisa la URL a abrir; se abre desde la interfaz.
    let (tx, mut rx) = futures::channel::mpsc::unbounded::<String>();
    let work = cx.background_spawn(async move {
        drive.link(&|url| {
            let _ = tx.unbounded_send(url.to_string());
        })
    });
    window
        .spawn(cx, async move |cx| {
            if let Some(url) = rx.next().await {
                cx.update(|_, cx| cx.open_url(&url)).ok();
            }
        })
        .detach();
    window
        .spawn(cx, async move |cx| {
            let res = work.await;
            cx.update(|window, cx| match res {
                Ok(_) => {
                    window.push_notification(Notification::success("Google Drive vinculado"), cx);
                    // Con la bóveda abierta se sube la versión actual.
                    vault.update(cx, |v, cx| {
                        if v.store.is_unlocked() {
                            let _ = v.store.persist();
                            v.flush_upload();
                        }
                        cx.notify();
                    });
                    on_done(window, cx);
                }
                Err(e) => window.push_notification(Notification::error(e), cx),
            })
            .ok();
        })
        .detach();
}
