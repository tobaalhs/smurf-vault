const { app, BrowserWindow, ipcMain, clipboard, Menu, shell } = require('electron');
const path = require('path');
const fs = require('fs');
const crypto = require('crypto');
const { createKey, encryptWithKey, decryptEnvelope, emptyVault } = require('./src/vault');
const { Drive } = require('./src/drive');
const lcu = require('./src/lcu');
const riot = require('./src/riot');

const userData = app.getPath('userData');
const LOCAL_VAULT = path.join(userData, 'vault.dat');
const LOCAL_BACKUP = path.join(userData, 'vault.prev.dat');

let win;
let drive;
let session = null; // { key, salt, data } mientras la bóveda está desbloqueada
let uploadChain = Promise.resolve();

function credentialsPath() {
  const candidates = [path.join(app.getAppPath(), 'credentials.json'), path.join(userData, 'credentials.json')];
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
  Object.assign(acc, {
    puuid: snap.puuid,
    gameName: snap.gameName,
    tagLine: snap.tagLine,
    level: snap.level,
    iconId: snap.iconId,
    ranks: snap.ranks,
    lastSyncedAt: new Date().toISOString(),
  });
  if (snap.server) acc.server = snap.server;
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
  }));

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

  handle('google:syncNow', () => {
    requireSession();
    persist();
    return true;
  });

  handle('lcu:detect', async () => {
    const snap = await lcu.currentAccount();
    if (!snap) return null;
    snap.server = riot.serverFromClient(snap.server);
    let matchedId = null;
    if (session) {
      const acc = session.data.accounts.find((a) => a.puuid === snap.puuid);
      if (acc) {
        applySnapshot(acc, snap);
        persist();
        matchedId = acc.id;
      }
    }
    return { snapshot: snap, matchedId, data: publicData() };
  });

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
  registerIpc();
  createWindow();
});

app.on('window-all-closed', () => app.quit());
