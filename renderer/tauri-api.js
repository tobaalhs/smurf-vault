// Adaptador para la versión Tauri: expone el mismo window.api que preload.js en Electron, pero
// hablando con los comandos de Rust (src-tauri/src/lib.rs). Así la interfaz no cambia.
// En Electron no hace nada: ahí preload.js ya definió window.api y no existe window.__TAURI__.
(() => {
  if (window.api || !window.__TAURI__) return;
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const currentWindow = () => window.__TAURI__.window.getCurrentWindow();

  // Los errores de Rust llegan como texto: los pasamos a Error, igual que preload.js.
  const call = (cmd, args) =>
    invoke(cmd, args).catch((e) => {
      throw new Error(typeof e === 'string' ? e : e?.message || String(e));
    });
  const on = (event) => (cb) => {
    listen(event, (e) => cb(e.payload));
  };
  // Lo que todavía no se portó a Rust avisa en vez de romper la interfaz.
  const todo = (name) => () => Promise.reject(new Error(`Todavía no disponible en la versión Tauri (${name})`));

  window.api = {
    status: () => call('app_status'),
    openExternal: (url) => call('app_open_external', { url }),
    installUpdate: todo('actualizar'),
    onUpdate: on('update'),
    remoteExists: todo('Google Drive'),
    create: (password) => call('vault_create', { password }),
    unlock: (password) => call('vault_unlock', { password }),
    lock: () => call('vault_lock'),
    changePassword: todo('cambiar contraseña'),
    saveAccount: todo('guardar cuenta'),
    importAccounts: todo('importar cuentas'),
    deleteAccount: todo('borrar cuenta'),
    setFavorite: todo('favoritas'),
    gameData: todo('datos del juego'),
    listSkins: todo('skins'),
    setBackground: todo('fondo de perfil'),
    saveSettings: todo('ajustes'),
    copy: todo('copiar'),
    googleLink: todo('Google Drive'),
    googleUnlink: todo('Google Drive'),
    googleImportCredentials: todo('Google Drive'),
    syncNow: todo('Google Drive'),
    detect: todo('detectar cliente'),
    getTools: () => call('tools_get'),
    setTools: todo('herramientas'),
    onTools: on('tools'),
    linkClient: todo('vincular cuenta'),
    refreshRanks: todo('actualizar rangos'),
    onRiotProgress: on('riot-progress'),
    play: todo('jugar'),
    onLoginProgress: on('login-progress'),
    loginOther: todo('iniciar sesión'),
    forgetSession: todo('olvidar sesión'),
    onSync: on('sync'),
    onData: on('data'),
    onNotice: on('notice'),
    onLocked: on('locked'),
    setTheme: todo('colores'),
    remoteStatus: todo('control desde el celular'),
    remoteNewCode: todo('control desde el celular'),
    winMinimize: () => call('win_minimize'),
    winToggleMaximize: () => call('win_toggle_maximize'),
    winClose: () => call('win_close'),
    winIsMaximized: () => call('win_is_maximized'),
    onWinState: on('win:state'),
  };

  // Arrastrar la ventana desde la barra de arriba (en Electron lo hace -webkit-app-region en el CSS).
  const dragArea = (e) => e.target.closest('.topbar, .lock-drag') && !e.target.closest('button, input, select, a, .menu');
  document.addEventListener('mousedown', (e) => {
    if (e.button !== 0 || !dragArea(e)) return;
    // Con el arrastre activo no llega el dblclick: el doble clic (maximizar) se maneja aquí mismo.
    if (e.detail === 2) window.api.winToggleMaximize();
    else currentWindow().startDragging();
  });
  // Y evitamos que renderer.js también maximice con su propio dblclick (sería doble).
  window.addEventListener('dblclick', (e) => dragArea(e) && e.stopImmediatePropagation(), true);
})();
