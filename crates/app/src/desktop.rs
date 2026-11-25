//! Integración con Windows: una sola instancia, ocultar/mostrar la ventana (bandeja), abrir al
//! iniciar Windows y el ícono de la bandeja. Igual que requestSingleInstanceLock, Tray y
//! setLoginItemSettings de la versión Electron.
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

/// Con este argumento (al iniciar Windows) la app parte en la bandeja, sin mostrar la ventana.
pub const STARTUP_ARG: &str = "--hidden";
/// Nombre de la entrada en "Ejecutar al iniciar" (el mismo que usaba Electron, así se reemplaza).
const RUN_VALUE: &str = "Smurf Vault";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Ventana principal (HWND), para ocultarla y mostrarla desde la bandeja.
static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);
/// true cuando se sale de verdad (Salir en la bandeja): cerrar ya no la manda a la bandeja.
static QUITTING: AtomicBool = AtomicBool::new(false);

pub fn quitting() -> bool {
    QUITTING.load(Ordering::SeqCst)
}

pub fn set_quitting() {
    QUITTING.store(true, Ordering::SeqCst);
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Guarda el HWND de la ventana principal.
pub fn remember_window(window: &gpui_kit::Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    if let Ok(h) = HasWindowHandle::window_handle(window) {
        if let RawWindowHandle::Win32(w) = h.as_raw() {
            MAIN_HWND.store(w.hwnd.get(), Ordering::SeqCst);
        }
    }
}

#[cfg(windows)]
mod win {
    use super::*;
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, BOOL};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE,
        REG_SZ,
    };
    use windows_sys::Win32::System::Threading::CreateMutexW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsIconic, SetForegroundWindow, ShowWindow, SW_HIDE, SW_RESTORE, SW_SHOW,
    };

    pub fn show_hwnd(hwnd: HWND) {
        // SAFETY: llamadas de Win32 sobre una ventana; si ya no existe, simplemente fallan.
        unsafe {
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            } else {
                ShowWindow(hwnd, SW_SHOW);
            }
            SetForegroundWindow(hwnd);
        }
    }

    pub fn hide_hwnd(hwnd: HWND) {
        // SAFETY: igual que arriba.
        unsafe { ShowWindow(hwnd, SW_HIDE) };
    }

    /// Muestra las ventanas de otro proceso (la instancia que ya estaba abierta).
    pub fn show_process_windows(pid: u32) {
        unsafe extern "system" fn each(hwnd: HWND, data: LPARAM) -> BOOL {
            let mut owner = 0u32;
            // SAFETY: hwnd viene de EnumWindows.
            unsafe { GetWindowThreadProcessId(hwnd, &mut owner) };
            if owner == data as u32 {
                let mut title = [0u16; 64];
                // SAFETY: búfer local de 64 u16.
                let n = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetWindowTextW(hwnd, title.as_mut_ptr(), 64) };
                if String::from_utf16_lossy(&title[..n.max(0) as usize]) == "Smurf Vault" {
                    show_hwnd(hwnd);
                    return 0;
                }
            }
            1
        }
        // SAFETY: la función de arriba respeta el contrato de EnumWindows.
        unsafe { EnumWindows(Some(each), pid as LPARAM) };
    }

    /// true si esta es la única instancia. El mutex vive mientras corra el proceso.
    pub fn take_instance_lock() -> bool {
        let name = wide("Local\\SmurfVault.Rust.Instance");
        // SAFETY: nombre terminado en 0; el handle se deja abierto a propósito hasta que salga.
        unsafe {
            let h = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            !(h.is_null() || GetLastError() == ERROR_ALREADY_EXISTS)
        }
    }

    fn open_run_key(access: u32) -> Option<HKEY> {
        let key = wide(RUN_KEY);
        let mut h: HKEY = std::ptr::null_mut();
        // SAFETY: parámetros válidos; se cierra con RegCloseKey.
        let r = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, access, &mut h) };
        (r == 0).then_some(h)
    }

    pub fn run_value() -> Option<String> {
        let h = open_run_key(KEY_READ)?;
        let name = wide(RUN_VALUE);
        let mut buf = vec![0u16; 1024];
        let mut len = (buf.len() * 2) as u32;
        // SAFETY: búfer del tamaño indicado en bytes.
        let r = unsafe { RegQueryValueExW(h, name.as_ptr(), std::ptr::null(), std::ptr::null_mut(), buf.as_mut_ptr() as *mut u8, &mut len) };
        unsafe { RegCloseKey(h) };
        (r == 0).then(|| String::from_utf16_lossy(&buf[..(len as usize / 2)]).trim_end_matches('\0').to_string())
    }

    pub fn set_run_value(value: Option<&str>) -> Result<(), String> {
        let h = open_run_key(KEY_SET_VALUE).ok_or("No pude abrir el registro de inicio de Windows")?;
        let name = wide(RUN_VALUE);
        // SAFETY: datos terminados en 0, tamaño en bytes.
        let r = unsafe {
            match value {
                Some(v) => {
                    let data = wide(v);
                    RegSetValueExW(h, name.as_ptr(), 0, REG_SZ, data.as_ptr() as *const u8, (data.len() * 2) as u32)
                }
                None => RegDeleteValueW(h, name.as_ptr()),
            }
        };
        unsafe { RegCloseKey(h) };
        // 2 = no existía (al borrar): da igual.
        if r == 0 || (value.is_none() && r == 2) {
            Ok(())
        } else {
            Err(format!("Windows no dejó cambiar el inicio automático (error {r})"))
        }
    }
}

/// Una sola instancia: si ya había una abierta, la muestra y devuelve false (esta debe salir).
pub fn single_instance(data_dir: &std::path::Path) -> bool {
    let pid_file = data_dir.join("instance.pid");
    #[cfg(windows)]
    {
        if !win::take_instance_lock() {
            if let Some(pid) = std::fs::read_to_string(&pid_file).ok().and_then(|s| s.trim().parse().ok()) {
                win::show_process_windows(pid);
            }
            return false;
        }
    }
    let _ = std::fs::create_dir_all(data_dir);
    let _ = std::fs::write(pid_file, std::process::id().to_string());
    true
}

pub fn show_main_window() {
    #[cfg(windows)]
    {
        let h = MAIN_HWND.load(Ordering::SeqCst);
        if h != 0 {
            win::show_hwnd(h as _);
        }
    }
}

pub fn hide_main_window() {
    #[cfg(windows)]
    {
        let h = MAIN_HWND.load(Ordering::SeqCst);
        if h != 0 {
            win::hide_hwnd(h as _);
        }
    }
}

fn startup_command() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(format!("\"{}\" {STARTUP_ARG}", exe.display()))
}

/// ¿Está registrada para abrir al iniciar Windows (con este mismo .exe)?
pub fn open_at_login() -> bool {
    #[cfg(windows)]
    {
        return win::run_value().is_some_and(|v| Some(v) == startup_command());
    }
    #[allow(unreachable_code)]
    false
}

pub fn set_open_at_login(enabled: bool) -> Result<(), String> {
    // En desarrollo se registraría el ejecutable de prueba, no Smurf Vault.
    if cfg!(debug_assertions) {
        return Err("Abrir al iniciar Windows solo funciona en la versión instalada".into());
    }
    #[cfg(windows)]
    {
        return win::set_run_value(if enabled { startup_command() } else { None }.as_deref());
    }
    #[allow(unreachable_code)]
    Err("Solo en Windows".into())
}

// ---------- bandeja ----------

pub enum TrayAction {
    Open,
    Lock,
    Quit,
}

pub struct Tray {
    _icon: tray_icon::TrayIcon,
    open: tray_icon::menu::MenuId,
    lock: tray_icon::menu::MenuId,
    quit: tray_icon::menu::MenuId,
}

impl Tray {
    pub fn new() -> Result<Self, String> {
        use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};
        let png = image::load_from_memory(crate::assets::ICON_PNG).map_err(|e| e.to_string())?;
        let small = png.resize_exact(32, 32, image::imageops::FilterType::Lanczos3).to_rgba8();
        let icon = tray_icon::Icon::from_rgba(small.into_raw(), 32, 32).map_err(|e| e.to_string())?;
        let open = MenuItem::new("Abrir Smurf Vault", true, None);
        let lock = MenuItem::new("Bloquear bóveda", true, None);
        let quit = MenuItem::new("Salir", true, None);
        let menu = Menu::new();
        menu.append_items(&[&open, &lock, &PredefinedMenuItem::separator(), &quit]).map_err(|e| e.to_string())?;
        let tray = tray_icon::TrayIconBuilder::new()
            .with_tooltip("Smurf Vault")
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { _icon: tray, open: open.id().clone(), lock: lock.id().clone(), quit: quit.id().clone() })
    }

    /// Lo que se pidió desde la bandeja desde la última vez (clic en el ícono o en el menú).
    pub fn poll(&self) -> Vec<TrayAction> {
        use tray_icon::{MouseButton, MouseButtonState, TrayIconEvent};
        let mut out = Vec::new();
        while let Ok(ev) = tray_icon::menu::MenuEvent::receiver().try_recv() {
            if ev.id == self.open {
                out.push(TrayAction::Open);
            } else if ev.id == self.lock {
                out.push(TrayAction::Lock);
            } else if ev.id == self.quit {
                out.push(TrayAction::Quit);
            }
        }
        while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                out.push(TrayAction::Open);
            }
        }
        out
    }
}

// ---------- actualizaciones ----------

/// Actualización ya descargada y verificada, esperando instalarse.
static PENDING_UPDATE: std::sync::Mutex<Option<sv_core::update::ReadyUpdate>> = std::sync::Mutex::new(None);

pub fn set_pending_update(update: sv_core::update::ReadyUpdate) {
    *PENDING_UPDATE.lock().unwrap() = Some(update);
}

pub fn pending_update_version() -> Option<String> {
    PENDING_UPDATE.lock().unwrap().as_ref().map(|u| u.version.clone())
}

/// "Actualizar": instala ya, sin ventanas, y vuelve a abrir la app.
pub fn install_update_now(cx: &mut gpui_kit::App) -> Result<(), String> {
    let update = PENDING_UPDATE.lock().unwrap().take().ok_or("No hay una actualización lista")?;
    sv_core::update::launch_installer(&update, true, true)?;
    set_quitting();
    cx.quit();
    Ok(())
}

/// Al salir de la app: si hay una actualización lista, se instala sola (como electron-updater).
pub fn install_pending_on_exit() {
    if let Some(update) = PENDING_UPDATE.lock().unwrap().take() {
        let _ = sv_core::update::launch_installer(&update, true, false);
    }
}

// ---------- avisos de Windows ----------

/// Id de la app en Windows: el mismo de los accesos directos que crea el instalador (y Electron).
pub const APP_USER_MODEL_ID: &str = "com.tobaalhs.smurfvault";

/// Declara el id de la app, para que los avisos salgan a nombre de Smurf Vault.
pub fn set_app_id() {
    #[cfg(windows)]
    {
        let id = wide(APP_USER_MODEL_ID);
        // SAFETY: texto terminado en 0.
        unsafe { windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(id.as_ptr()) };
    }
}

/// Aviso de Windows (silencioso), como `new Notification(...)` en Electron.
pub fn notify(title: &str, body: &str) {
    #[cfg(windows)]
    {
        let (title, body) = (title.to_string(), body.to_string());
        std::thread::spawn(move || {
            let _ = tauri_winrt_notification::Toast::new(APP_USER_MODEL_ID)
                .title(&title)
                .text1(&body)
                .sound(None)
                .show();
        });
    }
}
