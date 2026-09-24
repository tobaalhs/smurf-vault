const { app, BrowserWindow, ipcMain, clipboard, Menu, shell, Notification, dialog } = require('electron');
const path = require('path');
const fs = require('fs');
const crypto = require('crypto');
const { createKey, encryptWithKey, decryptEnvelope, emptyVault } = require('./src/vault');
const { Drive } = require('./src/drive');
const lcu = require('./src/lcu');
const riot = require('./src/riot');
const { AutoAccept } = require('./src/autoaccept');
const { GameflowWatcher } = require('./src/gameflow');
const { autoUpdater } = require('electron-updater');

const userData = app.getPath('userData');
const LOCAL_VAULT = path.join(userData, 'vault.dat');
const LOCAL_BACKUP = path.join(userData, 'vault.prev.dat');
// Preferencias que no son secretas y deben funcionar con la bóveda bloqueada.
const CONFIG_PATH = path.join(userData, 'config.json');

let win;
let drive;
let autoAccept;
let config = { autoAccept: false, autoAcceptDelay: 0 };
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
  return toolsStatus();
}

function toolsStatus() {
  return { ...config, clientConnected: autoAccept.connected };
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

function publicData() {
  return session ? session.data : null;
}

function findAccount(id) {
  const acc = session.data.accounts.find((a) => a.id === id);
  if (!acc) throw new Error('Cuenta no encontrada');
  return acc;
}

function applySnapshot(acc, snap) {
  // Si cambió la cuenta de Riot vinculada, el PUUID de la API anterior ya no sirve.
  if (snap.puuid !== acc.puuid) delete acc.apiPuuid;
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
      // Una misma cuenta de Riot solo puede estar vinculada a una entrada.
      for (const a of session.data.accounts) if (a.puuid === snap.puuid && a !== match.acc) delete a.puuid;
      applySnapshot(match.acc, snap);
      persist();
      matchedId = match.acc.id;
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
  }));

  handle('app:installUpdate', () => {
    if (updateReady) autoUpdater.quitAndInstall();
    return true;
  });

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
      for (const f of ['puuid', 'apiPuuid', 'level', 'iconId', 'lastPlayedAt', 'lastSyncedAt']) delete acc[f];
      acc.ranks = { solo: null, flex: null };
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

  handle('accounts:delete', (id) => {
    requireSession();
    session.data.accounts = session.data.accounts.filter((a) => a.id !== id);
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
    const value = findAccount(id)[field] || '';
    clipboard.writeText(value);
    // Limpiamos el portapapeles a los 30 s si nadie copió otra cosa.
    setTimeout(() => {
      if (clipboard.readText() === value) clipboard.clear();
    }, 30_000);
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
    return saveConfig(allowed);
  });

  handle('lcu:detect', (manual) => detectClient({ force: !!manual }));

  handle('lcu:link', (id, snap) => {
    requireSession();
    // Una misma cuenta de Riot solo puede estar vinculada a una entrada.
    for (const a of session.data.accounts) if (a.puuid === snap.puuid && a.id !== id) delete a.puuid;
    applySnapshot(findAccount(id), snap);
    persist();
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
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });
  win.loadFile(path.join(__dirname, 'renderer', 'index.html'));
  // Links externos -> navegador del sistema, nunca dentro de la app.
  win.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
  win.webContents.on('will-navigate', (e) => e.preventDefault());
}

app.whenReady().then(() => {
  Menu.setApplicationMenu(null);
  drive = new Drive({ credentialsPath: credentialsPath(), tokenPath: path.join(userData, 'google-token.bin') });
  app.setAppUserModelId('Smurf Vault'); // necesario para las notificaciones en Windows
  loadConfig();
  autoAccept = new AutoAccept({
    onAccepted: onMatchAccepted,
    onStatus: (s) => win?.webContents.send('tools', { event: 'status', ...s }),
  });
  autoAccept.configure({ enabled: config.autoAccept, delay: config.autoAcceptDelay });
  registerIpc();
  createWindow();
  setupUpdates();
  new GameflowWatcher({ onGameEnd: () => onGameEnd().catch(() => {}) }).start();
});

app.on('window-all-closed', () => app.quit());
