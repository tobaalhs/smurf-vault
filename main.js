const { app, BrowserWindow, ipcMain, clipboard, Menu, Tray, nativeImage, shell, Notification, dialog } = require('electron');
const path = require('path');
const fs = require('fs');
const crypto = require('crypto');
const { createKey, encryptWithKey, decryptEnvelope, decryptWithKey, emptyVault } = require('./src/vault');
const { Drive } = require('./src/drive');
const lcu = require('./src/lcu');
const riot = require('./src/riot');
const { AutoAccept } = require('./src/autoaccept');
const { GameflowWatcher } = require('./src/gameflow');
const { OfflineMode } = require('./src/offline');
const switcher = require('./src/switcher');
const { LocalHistory, LpLog } = require('./src/history');
const { GameData } = require('./src/gamedata');
const { autoUpdater } = require('electron-updater');

const userData = app.getPath('userData');
const LOCAL_VAULT = path.join(userData, 'vault.dat');
const LOCAL_BACKUP = path.join(userData, 'vault.prev.dat');
// Preferencias que no son secretas y deben funcionar con la bóveda bloqueada.
const CONFIG_PATH = path.join(userData, 'config.json');
// Sesiones del Riot Client por cuenta ("Mantener sesión iniciada"). Solo viven en este PC: no se
// suben a Drive. Van cifradas con la clave de la bóveda, así que solo se usan con la bóveda abierta.
const SESSIONS_DIR = path.join(userData, 'sessions');
// Historial de partidas: solo en este PC, cifrado con la clave de la bóveda (no se sube a Drive).
const history = new LocalHistory(path.join(userData, 'history.dat'));
const lpLog = new LpLog(path.join(userData, 'lp.dat'));

let win;
let drive;
let autoAccept;
let config = {
  autoAccept: false,
  autoAcceptDelay: 0,
  closeToTray: true,
  trayHintShown: false,
  appearOffline: false, // herramienta: aparecer desconectado en el chat del LoL
};
const offlineMode = new OfflineMode();
let tray = null;
let gameData = null;
let quitting = false; // true cuando se sale de verdad (menú de la bandeja, actualización)

// Una sola instancia: si la abres de nuevo estando en la bandeja, se muestra la que ya existe.
const singleInstance = app.requestSingleInstanceLock();
if (!singleInstance) app.quit();
let updateReady = null; // versión descargada, lista para instalar

// Actualizaciones desde GitHub Releases. Solo en la versión instalada:
// en desarrollo (npm start) y en la portable no aplica.
function setupUpdates() {
  if (!app.isPackaged || process.env.PORTABLE_EXECUTABLE_DIR) return;
  autoUpdater.autoInstallOnAppQuit = true; // si no reinicias, se instala al cerrar la app
  autoUpdater.on('update-downloaded', (info) => {
    updateReady = info.version;
    win?.webContents.send('update', { version: info.version });
  });
  autoUpdater.on('error', () => {}); // sin internet o sin releases: se ignora
  const check = () => autoUpdater.checkForUpdates().catch(() => {});
  check();
  setInterval(check, 6 * 60 * 60 * 1000);
}
let session = null; // { key, salt, data } mientras la bóveda está desbloqueada
let uploadChain = Promise.resolve();

// Primero las credenciales que cargó el usuario; si no hay, las que vienen dentro del instalador.
function credentialsPath() {
  const candidates = [path.join(userData, 'credentials.json'), path.join(app.getAppPath(), 'credentials.json')];
  return candidates.find((p) => fs.existsSync(p)) || candidates[0];
}

function readLocal() {
  try {
    return JSON.parse(fs.readFileSync(LOCAL_VAULT, 'utf8'));
  } catch {
    return null;
  }
}

function writeLocal(envelope) {
  if (fs.existsSync(LOCAL_VAULT)) fs.copyFileSync(LOCAL_VAULT, LOCAL_BACKUP);
  const tmp = LOCAL_VAULT + '.tmp';
  fs.writeFileSync(tmp, JSON.stringify(envelope));
  fs.renameSync(tmp, LOCAL_VAULT);
}

function loadConfig() {
  try {
    config = { ...config, ...JSON.parse(fs.readFileSync(CONFIG_PATH, 'utf8')) };
  } catch {}
}

function saveConfig(patch) {
  config = { ...config, ...patch };
  fs.writeFileSync(CONFIG_PATH, JSON.stringify(config, null, 2));
  autoAccept.configure({ enabled: config.autoAccept, delay: config.autoAcceptDelay });
  offlineMode.configure({ enabled: config.appearOffline });
  syncTray();
  return toolsStatus();
}

// ---------- ventana y bandeja ----------

function showWindow() {
  if (!win) return;
  if (win.isMinimized()) win.restore();
  win.show();
  win.focus();
}

function quitApp() {
  quitting = true;
  app.quit();
}

function lockVault() {
  session = null;
  history.clear();
  lpLog.clear();
  win?.webContents.send('locked');
}

/** El ícono de la bandeja solo existe si al cerrar la app sigue en segundo plano. */
function syncTray() {
  if (!config.closeToTray) {
    tray?.destroy();
    tray = null;
    return;
  }
  if (tray) return;
  const image = nativeImage.createFromPath(path.join(__dirname, 'build', 'icon.png')).resize({ width: 16, height: 16 });
  tray = new Tray(image);
  tray.setToolTip('Smurf Vault');
  tray.setContextMenu(
    Menu.buildFromTemplate([
      { label: 'Abrir Smurf Vault', click: showWindow },
      { label: 'Bloquear bóveda', click: lockVault },
      { type: 'separator' },
      { label: 'Salir', click: quitApp },
    ])
  );
  tray.on('click', showWindow);
}

function onWindowClose(e) {
  if (quitting || !config.closeToTray) return;
  e.preventDefault();
  win.hide();
  // La primera vez avisamos dónde quedó, para que no parezca que se cerró.
  if (!config.trayHintShown && Notification.isSupported()) {
    new Notification({
      title: 'Smurf Vault sigue abierto',
      body: 'Quedó en la bandeja del sistema. Clic derecho en el ícono para salir. Puedes cambiarlo en Ajustes.',
      silent: true,
    }).show();
    saveConfig({ trayHintShown: true });
  }
}

function toolsStatus() {
  return { ...config, openAtLogin: openAtLogin(), clientConnected: autoAccept.connected };
}

// ---------- abrir al iniciar Windows ----------

// Con --hidden (al iniciar Windows) la app parte directo en la bandeja, sin mostrar la ventana.
const STARTUP_ARG = '--hidden';

// En la portable, process.execPath es la copia temporal: hay que registrar el .exe real.
function startupExe() {
  return process.env.PORTABLE_EXECUTABLE_FILE || process.execPath;
}

function openAtLogin() {
  return app.getLoginItemSettings({ path: startupExe(), args: [STARTUP_ARG] }).openAtLogin;
}

function setOpenAtLogin(enabled) {
  // En desarrollo se registraría electron.exe, no Smurf Vault.
  if (!app.isPackaged) throw new Error('Abrir al iniciar Windows solo funciona en la versión instalada');
  app.setLoginItemSettings({ openAtLogin: enabled, path: startupExe(), args: [STARTUP_ARG] });
}

function onMatchAccepted() {
  win?.webContents.send('tools', { event: 'accepted' });
  // Si estás en otra ventana, avisamos con una notificación de Windows.
  if (!win?.isFocused() && Notification.isSupported()) {
    new Notification({ title: 'Smurf Vault', body: 'Partida aceptada ✓', silent: true }).show();
  }
}

function emitSync(state, message = '') {
  win?.webContents.send('sync', { state, message, at: new Date().toISOString() });
}

function queueUpload(envelope) {
  if (!drive.status().linked) return;
  emitSync('syncing');
  uploadChain = uploadChain
    .then(() => drive.upload(envelope))
    .then(() => emitSync('ok'))
    .catch((e) => emitSync('error', e.message));
}

function persist() {
  const envelope = encryptWithKey(session.key, session.salt, session.data);
  writeLocal(envelope);
  queueUpload(envelope);
}

/** Lo que ve la interfaz: la bóveda más el historial local de cada cuenta y las sesiones guardadas. */
function publicData() {
  if (!session) return null;
  const accounts = session.data.accounts.map((a) => ({ ...a, matches: history.get(a.id), lp: lpLog.get(a.id) }));
  return { ...session.data, accounts, sessions: sessionIndex() };
}

function saveHistory() {
  history.save(session.key, session.salt);
}

function saveLp() {
  lpLog.save(session.key, session.salt);
}

/**
 * Splash de fondo de las cuentas, para la pantalla de bloqueo. Se guarda aparte y sin cifrar porque
 * se muestra antes de desbloquear; son solo números de skin, no dicen nada de las cuentas.
 */
function rememberLockSplashes() {
  const skins = [...new Set(session.data.accounts.map((a) => a.backgroundSkinId).filter(Boolean))];
  if (JSON.stringify(skins) !== JSON.stringify(config.lockSkins || [])) saveConfig({ lockSkins: skins });
}

// Firma de la bóveda sin la fecha de "datos actualizados", que cambia en cada detección:
// si solo cambió eso, no vale la pena volver a subir la bóveda a Drive.
function vaultSignature() {
  return JSON.stringify(session.data, (k, v) => (k === 'lastSyncedAt' ? undefined : v));
}

// ---------- sesiones del Riot Client (cambio de cuenta) ----------

const savedSessionHash = new Map(); // id -> hash de lo último guardado, para no reescribir lo mismo
let switching = false;

function sessionFile(id) {
  return path.join(SESSIONS_DIR, `${id}.dat`);
}

/** { idCuenta: fecha en que se guardó su sesión } */
function sessionIndex() {
  const out = {};
  try {
    for (const f of fs.readdirSync(SESSIONS_DIR)) {
      if (f.endsWith('.dat')) out[f.slice(0, -4)] = fs.statSync(path.join(SESSIONS_DIR, f)).mtime.toISOString();
    }
  } catch {}
  return out;
}

/** Guarda la sesión abierta en el Riot Client como la de `acc`, si tiene "Mantener sesión iniciada". */
function captureSession(acc) {
  const files = switcher.readSession();
  if (!switcher.isRemembered(files)) return false;
  const hash = crypto.createHash('sha256').update(JSON.stringify(files)).digest('hex');
  if (savedSessionHash.get(acc.id) === hash) return true;
  fs.mkdirSync(SESSIONS_DIR, { recursive: true });
  const tmp = sessionFile(acc.id) + '.tmp';
  fs.writeFileSync(tmp, JSON.stringify(encryptWithKey(session.key, session.salt, { files })));
  fs.renameSync(tmp, sessionFile(acc.id));
  savedSessionHash.set(acc.id, hash);
  return true;
}

function loadSession(id) {
  if (!fs.existsSync(sessionFile(id))) return null;
  try {
    return decryptWithKey(session.key, JSON.parse(fs.readFileSync(sessionFile(id), 'utf8'))).files;
  } catch {
    return null; // de otra bóveda o dañada: se inicia sesión de nuevo y se vuelve a guardar
  }
}

function deleteSession(id) {
  fs.rmSync(sessionFile(id), { force: true });
  savedSessionHash.delete(id);
}

// Cerrar el cliente en estas fases te saca de la cola o de la partida.
const BUSY_PHASES = { ReadyCheck: 'aceptando partida', ChampSelect: 'en selección de campeones', GameStart: 'entrando a la partida', InProgress: 'en partida', Reconnect: 'en partida' };

/**
 * Cierra el Riot Client, deja la sesión de `id` (o ninguna, para mostrar el login) y abre el LoL.
 * Antes guarda la sesión de la cuenta que estaba abierta, porque Riot renueva los tokens.
 */
async function switchTo(id) {
  if (switching) throw new Error('Ya se está cambiando de cuenta');
  switching = true;
  try {
    const phase = await lcu.get('/lol-gameflow/v1/gameflow-phase');
    if (BUSY_PHASES[phase]) throw new Error(`No se puede cambiar de cuenta ${BUSY_PHASES[phase]}`);
    if (await switcher.isInGame()) throw new Error('No se puede cambiar de cuenta en partida');
    await detectClient().catch(() => null);
    const files = id ? loadSession(id) : null;
    await switcher.closeRiot();
    switcher.writeSession(files);
    switcher.launchLeague();
    switcher.ensureLeagueStarts().catch(() => {});
    return { restored: !!files };
  } finally {
    switching = false;
  }
}

function copySecret(value) {
  clipboard.writeText(value);
  // Limpiamos el portapapeles a los 30 s si nadie copió otra cosa.
  setTimeout(() => {
    if (clipboard.readText() === value) clipboard.clear();
  }, 30_000);
}

function findAccount(id) {
  const acc = session.data.accounts.find((a) => a.id === id);
  if (!acc) throw new Error('Cuenta no encontrada');
  return acc;
}

function applySnapshot(acc, snap) {
  // Si cambió la cuenta de Riot vinculada, el PUUID de la API anterior y su historial ya no sirven.
  if (snap.puuid !== acc.puuid) {
    delete acc.apiPuuid;
    if (acc.puuid && history.remove(acc.id)) saveHistory();
    if (acc.puuid && lpLog.remove(acc.id)) saveLp();
  }
  if (snap.apiPuuid) acc.apiPuuid = snap.apiPuuid;
  Object.assign(acc, {
    puuid: snap.puuid,
    gameName: snap.gameName,
    tagLine: snap.tagLine,
    level: snap.level,
    iconId: snap.iconId,
    lastSyncedAt: new Date().toISOString(),
  });
  // null = sin rango; undefined = no se pudo consultar (se conserva el valor anterior).
  acc.ranks = { ...acc.ranks };
  for (const [queue, value] of Object.entries(snap.ranks || {})) {
    if (value !== undefined) acc.ranks[queue] = value;
  }
  if (snap.server) acc.server = snap.server;
  // Nos quedamos con la partida más reciente que conozcamos (cliente o API pueden venir sin dato).
  if (snap.lastPlayedAt && !(acc.lastPlayedAt > snap.lastPlayedAt)) acc.lastPlayedAt = snap.lastPlayedAt;
  if (snap.backgroundSkinId) acc.backgroundSkinId = snap.backgroundSkinId;
  if (snap.crest) acc.crest = snap.crest;
  if (snap.mastery) acc.mastery = snap.mastery;
  if (lpLog.record(acc.id, acc.ranks)) saveLp();
  if (snap.matches && history.add(acc.id, snap.matches)) saveHistory();
}

/**
 * Busca entre las cuentas guardadas la que está abierta en el cliente:
 * 1) por PUUID (ya vinculada), 2) por usuario de login, 3) por Riot ID.
 * En 2 y 3 solo cuenta si hay exactamente una coincidencia, para no vincular mal.
 */
function findDetectedAccount(snap) {
  const accounts = session.data.accounts;
  const byPuuid = accounts.find((a) => a.puuid === snap.puuid || a.apiPuuid === snap.puuid);
  if (byPuuid) return { acc: byPuuid };
  const norm = (s) => (s || '').trim().toLowerCase();
  const unique = (list) => (list.length === 1 ? { acc: list[0] } : null);
  if (snap.username) {
    const found = unique(accounts.filter((a) => norm(a.username) === norm(snap.username)));
    if (found) return found;
  }
  if (snap.gameName && snap.tagLine) {
    return unique(
      accounts.filter(
        (a) => !a.puuid && norm(a.gameName) === norm(snap.gameName) && norm(a.tagLine) === norm(snap.tagLine)
      )
    );
  }
  return null;
}

/** Lee la cuenta abierta en el cliente y, si está guardada, la actualiza. */
async function detectClient({ force = false } = {}) {
  const snap = await lcu.currentAccount({ force });
  if (!snap) return null;
  snap.server = riot.serverFromClient(snap.server);
  let matchedId = null;
  let autoLinked = false;
  if (session) {
    const match = findDetectedAccount(snap);
    if (match) {
      autoLinked = match.acc.puuid !== snap.puuid;
      const before = vaultSignature();
      // Una misma cuenta de Riot solo puede estar vinculada a una entrada.
      for (const a of session.data.accounts) if (a.puuid === snap.puuid && a !== match.acc) delete a.puuid;
      applySnapshot(match.acc, snap);
      // Con el cliente abierto esto corre cada 30 s: solo se guarda y sube si cambió algo de verdad.
      if (vaultSignature() !== before) {
        persist();
        rememberLockSplashes();
      }
      matchedId = match.acc.id;
      try {
        captureSession(match.acc);
      } catch {}
    }
  }
  return { snapshot: snap, matchedId, autoLinked, data: publicData() };
}

// Partidas terminadas mientras la bóveda estaba bloqueada: se aplican al desbloquear.
const pendingGameEnds = new Map(); // puuid -> fecha

function markPlayed(puuid, at) {
  const acc = session?.data.accounts.find((a) => a.puuid === puuid || a.apiPuuid === puuid);
  if (!acc || acc.lastPlayedAt > at) return null;
  acc.lastPlayedAt = at;
  return acc;
}

async function onGameEnd() {
  const puuid = (await lcu.get('/lol-summoner/v1/current-summoner'))?.puuid;
  if (!puuid) return;
  const at = new Date().toISOString();
  if (!session) {
    pendingGameEnds.set(puuid, at);
    return;
  }
  const acc = markPlayed(puuid, at);
  if (acc) {
    persist();
    win?.webContents.send('data', { reason: 'game-end', account: acc.gameName || acc.username, data: publicData() });
  }
  // El cliente tarda unos segundos en actualizar LP y rango después de la partida.
  for (const delay of [15_000, 60_000]) {
    setTimeout(async () => {
      const res = await detectClient().catch(() => null);
      if (res?.matchedId) win?.webContents.send('data', { reason: 'refresh', data: res.data });
    }, delay);
  }
}

function requireSession() {
  if (!session) throw new Error('La bóveda está bloqueada');
}

// IPC con manejo uniforme de errores: el renderer recibe { ok, value } o { ok:false, error }.
function handle(channel, fn) {
  ipcMain.handle(channel, async (_e, ...args) => {
    try {
      return { ok: true, value: await fn(...args) };
    } catch (e) {
      return { ok: false, error: e.message };
    }
  });
}

function registerIpc() {
  handle('app:status', () => ({
    hasLocal: fs.existsSync(LOCAL_VAULT),
    unlocked: !!session,
    google: drive.status(),
    credentialsPath: credentialsPath(),
    version: app.getVersion(),
    updateReady,
    lockSkins: config.lockSkins || [], // fondos para la pantalla de bloqueo
  }));

  handle('app:installUpdate', () => {
    if (updateReady) {
      quitting = true;
      autoUpdater.quitAndInstall();
    }
    return true;
  });

  handle('win:minimize', () => win.minimize());
  handle('win:toggleMaximize', () => (win.isMaximized() ? win.unmaximize() : win.maximize()));
  handle('win:close', () => win.close());
  handle('win:isMaximized', () => win.isMaximized());

  // Solo abrimos en el navegador links conocidos, nunca URLs arbitrarias.
  const EXTERNAL = ['https://developer.riotgames.com/', 'https://github.com/tobaalhs/smurf-vault'];
  handle('app:openExternal', (url) => {
    if (!EXTERNAL.some((u) => url.startsWith(u))) throw new Error('Link no permitido');
    return shell.openExternal(url);
  });

  handle('vault:remoteExists', async () => {
    if (!drive.status().linked) return false;
    return !!(await drive.download());
  });

  handle('vault:create', async (password) => {
    if (!password || password.length < 8) throw new Error('Usa al menos 8 caracteres');
    if (fs.existsSync(LOCAL_VAULT)) throw new Error('Ya existe una bóveda en este PC');
    if (drive.status().linked && (await drive.download())) {
      throw new Error('Ya tienes una bóveda en Drive: desbloquéala en vez de crear otra');
    }
    const { key, salt } = await createKey(password);
    session = { key, salt, data: emptyVault() };
    history.clear();
    lpLog.clear();
    persist();
    return publicData();
  });

  handle('vault:unlock', async (password) => {
    const local = readLocal();
    let remote = null;
    let remoteError = null;
    if (drive.status().linked) {
      try {
        remote = await drive.download();
      } catch (e) {
        remoteError = e.message;
      }
    }
    if (!local && !remote) throw new Error(remoteError || 'No hay bóveda para desbloquear');

    // Nos quedamos con la versión más nueva entre la local y la de Drive.
    const newest = !local ? remote : !remote ? local : remote.updatedAt > local.updatedAt ? remote : local;
    const { key, salt, data } = await decryptEnvelope(newest, password);
    session = { key, salt, data: { ...emptyVault(), ...data } };
    history.load(key);
    lpLog.load(key);
    // Primer punto del gráfico de LP para las cuentas que todavía no tienen: su rango guardado.
    let lpChanged = false;
    for (const a of session.data.accounts) {
      if (!lpLog.get(a.id)) lpChanged = lpLog.record(a.id, a.ranks, a.lastSyncedAt || a.updatedAt) || lpChanged;
    }
    if (lpChanged) saveLp();
    rememberLockSplashes();
    // Versiones anteriores guardaban el historial dentro de la bóveda: se pasa al archivo local.
    let migrated = false;
    for (const a of session.data.accounts) {
      if (!a.matches) continue;
      history.add(a.id, a.matches);
      delete a.matches;
      migrated = true;
    }
    if (migrated) {
      saveHistory();
      persist();
    }

    if (newest === remote && remote.updatedAt !== local?.updatedAt) writeLocal(remote);
    if (newest === local && remote?.updatedAt !== local.updatedAt) queueUpload(local);
    if (remoteError) emitSync('error', remoteError);
    else if (remote || drive.status().linked) emitSync('ok');

    // Partidas que terminaron con la bóveda bloqueada.
    let changed = false;
    for (const [puuid, at] of pendingGameEnds) changed = !!markPlayed(puuid, at) || changed;
    pendingGameEnds.clear();
    if (changed) persist();
    return publicData();
  });

  handle('vault:lock', () => {
    session = null;
    history.clear();
    lpLog.clear();
    return true;
  });

  handle('vault:get', () => publicData());

  handle('accounts:save', (input) => {
    requireSession();
    const now = new Date().toISOString();
    const fields = ['label', 'username', 'password', 'server', 'gameName', 'tagLine', 'notes'];
    let acc = input.id && session.data.accounts.find((a) => a.id === input.id);
    if (!acc) {
      acc = { id: crypto.randomUUID(), createdAt: now, ranks: { solo: null, flex: null } };
      session.data.accounts.push(acc);
    }
    // Si cambiaron el Riot ID a mano, es otra cuenta: olvidamos el PUUID y los datos de la anterior
    // para que "Actualizar rangos" la busque por el nombre nuevo.
    const norm = (s) => (s ?? '').toString().trim().toLowerCase();
    const riotIdChanged =
      ('gameName' in input && norm(input.gameName) !== norm(acc.gameName)) ||
      ('tagLine' in input && norm(input.tagLine) !== norm(acc.tagLine));
    if (riotIdChanged && acc.puuid) {
      for (const f of ['puuid', 'apiPuuid', 'level', 'iconId', 'lastPlayedAt', 'lastSyncedAt', 'backgroundSkinId', 'crest', 'mastery']) delete acc[f];
      acc.ranks = { solo: null, flex: null };
      if (history.remove(acc.id)) saveHistory();
      if (lpLog.remove(acc.id)) saveLp();
    }
    for (const f of fields) if (f in input) acc[f] = (input[f] ?? '').toString().trim();
    acc.updatedAt = now;
    persist();
    return publicData();
  });

  handle('accounts:import', (rows) => {
    requireSession();
    const now = new Date().toISOString();
    for (const r of rows) {
      session.data.accounts.push({
        id: crypto.randomUUID(),
        createdAt: now,
        updatedAt: now,
        label: '',
        username: r.username,
        password: r.password,
        server: r.server || '',
        gameName: '',
        tagLine: '',
        notes: '',
        ranks: { solo: null, flex: null },
      });
    }
    persist();
    return publicData();
  });

  handle('accounts:favorite', (id, favorite) => {
    requireSession();
    const acc = findAccount(id);
    if (favorite) acc.favorite = true;
    else delete acc.favorite;
    persist();
    return publicData();
  });

  handle('game:data', () => gameData.get());

  handle('accounts:delete', (id) => {
    requireSession();
    session.data.accounts = session.data.accounts.filter((a) => a.id !== id);
    deleteSession(id);
    if (history.remove(id)) saveHistory();
    if (lpLog.remove(id)) saveLp();
    persist();
    return publicData();
  });

  handle('settings:save', (settings) => {
    requireSession();
    session.data.settings = { ...session.data.settings, ...settings };
    persist();
    return publicData();
  });

  handle('clipboard:copy', (id, field) => {
    requireSession();
    copySecret(findAccount(id)[field] || '');
    return true;
  });

  handle('google:link', async () => {
    const status = await drive.link();
    if (session) persist(); // sube la bóveda actual a Drive
    return status;
  });

  handle('google:unlink', () => drive.unlink());

  // El instalador público no trae credentials.json: cada uno carga el suyo (o el que le pasen).
  handle('google:importCredentials', async () => {
    const { canceled, filePaths } = await dialog.showOpenDialog(win, {
      title: 'Elige el credentials.json de Google',
      filters: [{ name: 'JSON', extensions: ['json'] }],
      properties: ['openFile'],
    });
    if (canceled || !filePaths[0]) return drive.status();
    let raw;
    try {
      raw = JSON.parse(fs.readFileSync(filePaths[0], 'utf8'));
    } catch {
      throw new Error('Ese archivo no es un JSON válido');
    }
    if (!(raw.installed || raw).client_id) throw new Error('Ese JSON no parece un credentials.json de "App de escritorio"');
    const dest = path.join(userData, 'credentials.json');
    fs.copyFileSync(filePaths[0], dest);
    drive.credentialsPath = dest;
    return drive.status();
  });

  handle('google:syncNow', () => {
    requireSession();
    persist();
    return true;
  });

  handle('tools:get', () => toolsStatus());
  handle('tools:set', (patch) => {
    const allowed = {};
    if ('autoAccept' in patch) allowed.autoAccept = !!patch.autoAccept;
    if ('autoAcceptDelay' in patch) allowed.autoAcceptDelay = Number(patch.autoAcceptDelay) || 0;
    if ('closeToTray' in patch) allowed.closeToTray = !!patch.closeToTray;
    if ('appearOffline' in patch) allowed.appearOffline = !!patch.appearOffline;
    if ('openAtLogin' in patch) setOpenAtLogin(!!patch.openAtLogin);
    return saveConfig(allowed);
  });

  handle('lcu:detect', (manual) => detectClient({ force: !!manual }));

  handle('lcu:link', (id, snap) => {
    requireSession();
    // Una misma cuenta de Riot solo puede estar vinculada a una entrada.
    for (const a of session.data.accounts) if (a.puuid === snap.puuid && a.id !== id) delete a.puuid;
    const acc = findAccount(id);
    applySnapshot(acc, snap);
    persist();
    try {
      captureSession(acc);
    } catch {}
    return publicData();
  });

  // Jugar con una cuenta: restaura su sesión guardada, o abre el login con la contraseña copiada.
  handle('switch:play', async (id) => {
    requireSession();
    const acc = findAccount(id);
    const res = await switchTo(id);
    if (!res.restored && acc.password) copySecret(acc.password);
    return res;
  });

  // Abre el Riot Client en la pantalla de login para entrar con otra cuenta.
  handle('switch:login', () => {
    requireSession();
    return switchTo(null);
  });

  handle('switch:forget', (id) => {
    requireSession();
    deleteSession(id);
    return publicData();
  });

  handle('riot:refresh', async (ids) => {
    requireSession();
    const key = session.data.settings.riotApiKey;
    if (!key) throw new Error('Configura tu API key de Riot en Ajustes');
    const targets = ids?.length ? ids.map(findAccount) : session.data.accounts;
    const errors = [];
    for (const acc of targets) {
      try {
        applySnapshot(acc, await riot.lookup(key, acc));
      } catch (e) {
        errors.push(`${acc.label || acc.gameName || acc.username}: ${e.message}`);
      }
      await new Promise((r) => setTimeout(r, 150)); // no pasarse del rate limit
    }
    persist();
    return { data: publicData(), errors };
  });
}

function createWindow() {
  win = new BrowserWindow({
    width: 1180,
    height: 780,
    minWidth: 820,
    minHeight: 560,
    backgroundColor: '#0a0e13',
    title: 'Smurf Vault',
    icon: path.join(__dirname, 'build', 'icon.png'),
    frame: false, // la barra de arriba de la app hace de barra de título, con sus propios botones
    show: false,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });
  win.loadFile(path.join(__dirname, 'renderer', 'index.html'));
  // Al iniciar Windows parte en la bandeja; sin bandeja no tendría cómo abrirse, así que se muestra.
  const hidden = process.argv.includes(STARTUP_ARG) && config.closeToTray;
  win.once('ready-to-show', () => {
    if (!hidden) win.show();
  });
  // Links externos -> navegador del sistema, nunca dentro de la app.
  win.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
  win.webContents.on('will-navigate', (e) => e.preventDefault());
  win.on('close', onWindowClose);
  const sendState = () => win.webContents.send('win:state', { maximized: win.isMaximized() });
  win.on('maximize', sendState);
  win.on('unmaximize', sendState);
}

app.on('second-instance', showWindow);
app.on('before-quit', () => {
  quitting = true;
});

app.whenReady().then(() => {
  if (!singleInstance) return;
  Menu.setApplicationMenu(null);
  drive = new Drive({ credentialsPath: credentialsPath(), tokenPath: path.join(userData, 'google-token.bin') });
  app.setAppUserModelId('Smurf Vault'); // necesario para las notificaciones en Windows
  loadConfig();
  gameData = new GameData(path.join(userData, 'gamedata.json'));
  autoAccept = new AutoAccept({
    onAccepted: onMatchAccepted,
    onStatus: (s) => win?.webContents.send('tools', { event: 'status', ...s }),
  });
  autoAccept.configure({ enabled: config.autoAccept, delay: config.autoAcceptDelay });
  offlineMode.configure({ enabled: config.appearOffline });
  registerIpc();
  createWindow();
  syncTray();
  setupUpdates();
  new GameflowWatcher({ onGameEnd: () => onGameEnd().catch(() => {}) }).start();
});

app.on('window-all-closed', () => app.quit());
