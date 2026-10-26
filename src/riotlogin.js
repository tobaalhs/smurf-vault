// Inicio de sesión automático en el Riot Client, sin escribir la contraseña a mano.
//
// Portado del proceso de inyección de LeagueAccounts (src/riot_client.rs + riot_renderer.rs):
// 1) Cierra el Riot Client y lo arranca en modo headless para llegar a la pantalla de login.
// 2) Lanza el render del cliente (Riot Client.exe) con depuración remota (CDP) en un puerto local.
// 3) Se conecta por WebSocket al CDP e inyecta riot_renderer.js, que rellena usuario y contraseña
//    en el formulario nativo de Riot y lo envía (nunca salen credenciales del proceso del cliente).
// 4) Espera a que la API local del cliente confirme la sesión iniciada con esa cuenta.
//
// La contraseña solo viaja hacia el propio Riot Client (127.0.0.1): ni al portapapeles ni a teclado.
const fs = require('fs');
const net = require('net');
const http = require('http');
const pathLib = require('path');
const { spawn, execFile } = require('child_process');
const WebSocket = require('ws');
const { rawRequest } = require('./lcu');

const RIOT_PROCESSES = [
  'RiotClientServices.exe',
  'RiotClientUx.exe',
  'RiotClientUxRender.exe',
  'Riot Client.exe',
  'RiotClientCrashHandler.exe',
];

// Tiempos (ms). Igual que en el original: arranque frío del cliente y pasos que pide Riot (MFA/captcha).
const LOGIN_TIMEOUT = 120_000; // esperar a que el cliente llegue a "sin sesión" o "con sesión"
const SIGNED_OUT_TIMEOUT = 45_000; // tras reiniciar headless, esperar a la pantalla de login
const RENDERER_TIMEOUT = 30_000; // esperar a que aparezca el objetivo de depuración
const CONFIRM_TIMEOUT = 300_000; // el jugador tiene este tiempo para MFA/captcha
const SIGN_OUT_TIMEOUT = 15_000;
const POLL = 500;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

class LoginError extends Error {
  constructor(code, message) {
    super(message || code);
    this.code = code;
  }
}

// ---------- rutas del Riot Client ----------

function programData() {
  return process.env.PROGRAMDATA || 'C:\\ProgramData';
}

function localAppData() {
  return process.env.LOCALAPPDATA || '';
}

/** Ruta de RiotClientServices.exe según RiotClientInstalls.json, o la de por defecto. */
function clientServicesPath() {
  const candidates = [];
  try {
    const installs = JSON.parse(
      fs.readFileSync(pathLib.join(programData(), 'Riot Games', 'RiotClientInstalls.json'), 'utf8')
    );
    candidates.push(installs.rc_default, installs.rc_live);
  } catch {}
  candidates.push('C:\\Riot Games\\Riot Client\\RiotClientServices.exe');
  return candidates.find((p) => p && fs.existsSync(p)) || null;
}

// ---------- procesos ----------

function runningProcesses() {
  return new Promise((resolve) => {
    execFile('tasklist', ['/FO', 'CSV', '/NH'], { windowsHide: true, timeout: 8000 }, (err, stdout) => {
      if (err) return resolve(new Set());
      const names = (stdout || '').split(/\r?\n/).map((l) => l.match(/^"([^"]+)"/)?.[1]?.toLowerCase()).filter(Boolean);
      resolve(new Set(names));
    });
  });
}

function killProcesses(names) {
  return new Promise((resolve) =>
    execFile('taskkill', ['/F', ...names.flatMap((p) => ['/IM', p])], { windowsHide: true }, () => resolve())
  );
}

/** Cierra solo los procesos del Riot Client (no el LoL en partida) y espera a que terminen. */
async function stopRiotClient() {
  const running = await runningProcesses();
  const targets = RIOT_PROCESSES.filter((p) => running.has(p.toLowerCase()));
  if (targets.length) await killProcesses(targets);
  for (let i = 0; i < 40; i++) {
    const now = await runningProcesses();
    if (!RIOT_PROCESSES.some((p) => now.has(p.toLowerCase()))) return;
    await sleep(250);
  }
  throw new LoginError('riot_close_failed', 'No se pudo cerrar el cliente de Riot');
}

function clientRunning(running) {
  return running.has('riotclientservices.exe') && running.has('riot client.exe');
}

/** Arranca RiotClientServices.exe (headless o con ventana) sin bloquear este proceso. */
function startService(exe, headless) {
  const args = headless ? ['--headless'] : [];
  spawn(exe, args, { detached: true, stdio: 'ignore', windowsHide: true }).unref();
}

// ---------- API local del Riot Client (lockfile + HTTPS con token) ----------

function readLockfile() {
  try {
    const raw = fs.readFileSync(pathLib.join(localAppData(), 'Riot Games', 'Riot Client', 'Config', 'lockfile'), 'utf8');
    const [, pid, port, password] = raw.trim().split(':');
    if (port && password) return { pid, port, password };
  } catch {}
  return null;
}

/** GET a la API local; devuelve { status, json } o null si el cliente no responde todavía. */
async function apiGet(lock, path) {
  try {
    return await rawRequest({ port: lock.port, password: lock.password }, 'GET', path);
  } catch {
    return null;
  }
}

/**
 * Estado de la sesión del cliente a partir de /player-session-lifecycle/v1/session.
 * 'signedIn' (hay identidad), 'signedOut' (pantalla de login), 'action' (MFA/captcha en curso),
 * o 'unknown' (el cliente aún está arrancando). Igual que session_state() del original.
 */
function sessionState(res) {
  if (!res || res.status !== 200 || !res.json) return 'unknown';
  const body = res.json;
  if (typeof body.puuid === 'string' && body.puuid) return 'signedIn';
  if (body.loginState === 'PendingLoginStrategy') return 'signedOut';
  if (body.loginState && body.actionRequired === true) return 'action';
  return 'unknown';
}

async function session(lock) {
  return sessionState(await apiGet(lock, '/player-session-lifecycle/v1/session'));
}

/** Usuario de login de la cuenta con sesión iniciada (userInfo viene como JSON dentro de un string). */
async function signedInUser(lock) {
  const res = await apiGet(lock, '/rso-auth/v1/authorization/userinfo');
  if (!res || res.status !== 200 || !res.json) return null;
  let info = res.json.userInfo ?? res.json;
  if (typeof info === 'string') {
    try {
      info = JSON.parse(info);
    } catch {
      return null;
    }
  }
  const name = info?.username;
  return typeof name === 'string' && name ? name : null;
}

/** Cierra la sesión actual (para entrar con otra cuenta). Comprueba que el cliente lo permite. */
async function signOut(lock) {
  const help = await apiGet(lock, '/help');
  const functions = help?.json?.functions || {};
  if (!('DeletePlayerSessionLifecycleV1Session' in functions)) {
    throw new LoginError('riot_sign_out_failed', 'El cliente no permite cerrar sesión');
  }
  let res;
  try {
    res = await rawRequest({ port: lock.port, password: lock.password }, 'DELETE', '/player-session-lifecycle/v1/session');
  } catch {
    throw new LoginError('riot_sign_out_failed', 'No se pudo cerrar la sesión actual');
  }
  if (!res || res.status >= 400) throw new LoginError('riot_sign_out_failed', 'No se pudo cerrar la sesión actual');
}

// ---------- conexión CDP (Chrome DevTools Protocol) ----------

function freePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.unref();
    server.on('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });
}

/** Solo aceptamos la URL de depuración del puerto que abrimos, en loopback, para una página. */
function validDebuggerUrl(value, port) {
  try {
    const url = new URL(value);
    return (
      url.protocol === 'ws:' &&
      url.hostname === '127.0.0.1' &&
      url.port === String(port) &&
      !url.username &&
      !url.password &&
      url.pathname.startsWith('/devtools/page/') &&
      !url.search &&
      !url.hash
    );
  } catch {
    return false;
  }
}

function httpGetJson(url, timeoutMs = 2000) {
  return new Promise((resolve) => {
    const req = http.get(url, { timeout: timeoutMs }, (res) => {
      let body = '';
      res.on('data', (c) => (body += c));
      res.on('end', () => {
        try {
          resolve(JSON.parse(body));
        } catch {
          resolve(null);
        }
      });
    });
    req.on('timeout', () => req.destroy());
    req.on('error', () => resolve(null));
  });
}

/** Cliente CDP mínimo: evalúa expresiones en el render del Riot Client y espera el resultado. */
class Cdp {
  constructor(socket) {
    this.socket = socket;
    this.sequence = 0;
    this.pending = new Map();
    socket.on('message', (data) => {
      let msg;
      try {
        msg = JSON.parse(data.toString());
      } catch {
        return;
      }
      if (msg.id == null) return; // evento, no respuesta
      const waiter = this.pending.get(msg.id);
      if (!waiter) return;
      this.pending.delete(msg.id);
      if (msg.error || msg.result?.exceptionDetails) {
        waiter.reject(new LoginError('riot_renderer_failed', 'El render del cliente rechazó la inyección'));
      } else {
        waiter.resolve(msg.result?.result?.value);
      }
    });
  }

  static connect(url, port) {
    if (!validDebuggerUrl(url, port)) return Promise.reject(new LoginError('riot_renderer_failed', 'URL de depuración no válida'));
    return new Promise((resolve, reject) => {
      const socket = new WebSocket(url, { handshakeTimeout: 2000 });
      const onError = () => reject(new LoginError('riot_renderer_failed', 'No se pudo conectar al render del cliente'));
      socket.once('error', onError);
      socket.once('open', () => {
        socket.off('error', onError);
        socket.on('error', () => {}); // errores posteriores se ven como evals fallidas
        resolve(new Cdp(socket));
      });
    });
  }

  // Las expresiones pueden llevar credenciales: no se registran ni se reintentan.
  eval(expression, timeoutMs = 15_000) {
    if (this.socket.readyState !== WebSocket.OPEN) {
      return Promise.reject(new LoginError('riot_renderer_failed', 'El render del cliente se cerró'));
    }
    const id = ++this.sequence;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new LoginError('riot_renderer_failed', 'El render del cliente no respondió'));
      }, timeoutMs);
      this.pending.set(id, {
        resolve: (v) => {
          clearTimeout(timer);
          resolve(v);
        },
        reject: (e) => {
          clearTimeout(timer);
          reject(e);
        },
      });
      try {
        this.socket.send(
          JSON.stringify({ id, method: 'Runtime.evaluate', params: { expression, returnByValue: true, awaitPromise: true } })
        );
      } catch {
        clearTimeout(timer);
        this.pending.delete(id);
        reject(new LoginError('riot_renderer_failed', 'No se pudo enviar la inyección'));
      }
    });
  }

  close() {
    try {
      this.socket.send(JSON.stringify({ id: 0, method: 'Browser.close' }));
    } catch {}
    try {
      this.socket.close();
    } catch {}
  }
}

// El JS que se inyecta en el render del Riot Client (rellena el formulario nativo y lo envía).
const INJECTION = fs.readFileSync(pathLib.join(__dirname, 'riot_renderer.js'), 'utf8');

/** Mata el render de depuración y toda su descendencia (por si el job de Windows no basta). */
function killTree(child) {
  if (!child || child.exitCode !== null) return;
  try {
    execFile('taskkill', ['/F', '/T', '/PID', String(child.pid)], { windowsHide: true }, () => {});
  } catch {}
}

/**
 * Inicia sesión con `username`/`password` en el Riot Client e inyecta las credenciales.
 * Llama a `onProgress(step)` con los pasos ('closeClient','waitAuth','signOut','connect',
 * 'findWindow','type','verification','confirm','done'). Lanza LoginError con `.code` si falla.
 * Devuelve 'alreadySignedIn' si esa cuenta ya estaba abierta, o 'success' tras iniciar sesión.
 */
async function autoLogin({ username, password, onProgress = () => {} }) {
  if (!password) throw new LoginError('riot_auth_rejected', 'La cuenta no tiene contraseña guardada');
  const exe = clientServicesPath();
  if (!exe) throw new LoginError('riot_client_missing', 'No encontré el Riot Client instalado');
  const root = pathLib.dirname(exe);

  let child = null; // render de depuración
  let cdp = null;
  try {
    // 1) Asegurar que el servicio del cliente esté corriendo para poder consultarlo.
    onProgress('waitAuth');
    if (!clientRunning(await runningProcesses())) startService(exe, false);

    // 2) Esperar a conocer el estado: ya con sesión (misma u otra cuenta) o sin sesión.
    const until = Date.now() + LOGIN_TIMEOUT;
    let signOutSentAt = 0;
    for (;;) {
      if (Date.now() > until) throw new LoginError('riot_timeout', 'El cliente de Riot no respondió a tiempo');
      const lock = readLockfile();
      if (lock) {
        const state = await session(lock);
        if (state === 'signedIn') {
          const user = await signedInUser(lock);
          if (user && user.toLowerCase() === username.toLowerCase()) {
            onProgress('done');
            return 'alreadySignedIn';
          }
          if (user) {
            if (!signOutSentAt) {
              onProgress('signOut');
              await signOut(lock);
              signOutSentAt = Date.now();
            } else if (Date.now() - signOutSentAt > SIGN_OUT_TIMEOUT) {
              throw new LoginError('riot_sign_out_failed', 'No se pudo cerrar la sesión de la otra cuenta');
            }
          }
        } else if (state === 'signedOut') {
          break;
        }
      }
      await sleep(POLL);
    }

    // 3) Reiniciar el cliente en headless para tener un render limpio de login bajo nuestro control.
    onProgress('connect');
    await stopRiotClient();
    startService(exe, true);

    // Esperar a que la API local vuelva y reporte "sin sesión".
    const signedOutUntil = Date.now() + SIGNED_OUT_TIMEOUT;
    let lock = null;
    for (;;) {
      if (Date.now() > signedOutUntil) throw new LoginError('riot_timeout', 'El cliente no llegó a la pantalla de login');
      lock = readLockfile();
      if (lock) {
        const state = await session(lock);
        if (state === 'signedOut') break;
        if (state === 'signedIn') {
          const user = await signedInUser(lock);
          if (user && user.toLowerCase() === username.toLowerCase()) {
            onProgress('done');
            return 'alreadySignedIn';
          }
          throw new LoginError('game_account_mismatch', 'El cliente quedó con otra cuenta');
        }
      }
      await sleep(300);
    }

    // 4) Lanzar el render del cliente (Riot Client.exe) con depuración remota en un puerto local.
    const machine = (await apiGet(lock, '/riotclient/machine-id'))?.json;
    const hostSession = (await apiGet(lock, '/product-session/v1/host-session/id'))?.json;
    if (typeof machine !== 'string' || typeof hostSession !== 'string') {
      throw new LoginError('riot_renderer_failed', 'No pude leer los datos del cliente');
    }
    const ux = pathLib.join(root, 'RiotClientElectron', 'Riot Client.exe');
    const dataRoot = pathLib.join(localAppData(), 'Riot Games', 'Riot Client');
    const port = await freePort();
    const args = [
      `--app-port=${lock.port}`,
      `--remoting-auth-token=${lock.password}`,
      `--app-pid=${lock.pid}`,
      `--machine-id=${machine}`,
      `--session-id=${hostSession}`,
      `--app-root=${root}`,
      `--user-data-root=${dataRoot}`,
      `--log-dir=${pathLib.join(dataRoot, 'Logs', 'Riot Client UX Logs')}`,
      '--crashpad-environment=prod',
      '--enable-hardware-acceleration',
      '--allow-chrome-dev-tools',
      '--remote-debugging-address=127.0.0.1',
      `--remote-debugging-port=${port}`,
    ];
    child = spawn(ux, args, { detached: false, stdio: 'ignore', windowsHide: true });
    child.on('error', () => {});

    // 5) Buscar el objetivo de depuración de la ventana "Riot Client" y conectarse por WebSocket.
    onProgress('findWindow');
    const rendererUntil = Date.now() + RENDERER_TIMEOUT;
    let wsUrl = null;
    for (;;) {
      if (Date.now() > rendererUntil) throw new LoginError('riot_renderer_failed', 'No apareció la ventana de login');
      if (child.exitCode !== null) throw new LoginError('riot_renderer_failed', 'El render del cliente se cerró');
      const targets = await httpGetJson(`http://127.0.0.1:${port}/json/list`);
      const page = Array.isArray(targets)
        ? targets.find((t) => t.type === 'page' && t.title === 'Riot Client' && validDebuggerUrl(t.webSocketDebuggerUrl, port))
        : null;
      if (page) {
        wsUrl = page.webSocketDebuggerUrl;
        break;
      }
      await sleep(200);
    }
    cdp = await Cdp.connect(wsUrl, port);

    // 6) Inyectar el script y esperar a que el formulario esté listo.
    await cdp.eval(INJECTION);
    const readyUntil = Date.now() + RENDERER_TIMEOUT;
    while ((await cdp.eval('globalThis.__leagueAccountsLogin.ready()')) !== true) {
      if (Date.now() > readyUntil) throw new LoginError('riot_renderer_failed', 'El formulario de login no estuvo listo');
      await sleep(200);
    }
    // Dejar que la inicialización nativa de autenticación se asiente antes de rellenar.
    await sleep(1500);

    // 7) Rellenar usuario y contraseña, y enviar el formulario.
    onProgress('type');
    const filled = await cdp.eval(`globalThis.__leagueAccountsLogin.fill(...${JSON.stringify([username, password])})`);
    if (filled !== true) throw new LoginError('riot_renderer_failed', 'No se pudieron rellenar las credenciales');
    if ((await cdp.eval('globalThis.__leagueAccountsLogin.submit()')) !== true) {
      throw new LoginError('riot_renderer_failed', 'No se pudo enviar el formulario');
    }

    // 8) Esperar la confirmación. Si Riot pide MFA/captcha, se muestra la ventana para que lo resuelvas.
    onProgress('confirm');
    const confirmUntil = Date.now() + CONFIRM_TIMEOUT;
    let verification = false;
    for (;;) {
      if (Date.now() > confirmUntil) throw new LoginError('riot_timeout', 'No se confirmó el inicio de sesión a tiempo');
      const current = readLockfile();
      if (!current) throw new LoginError('riot_renderer_failed', 'Se perdió la conexión con el cliente');
      if ((await session(current)) === 'signedIn') {
        const user = await signedInUser(current);
        if (user) {
          if (user.toLowerCase() === username.toLowerCase()) {
            onProgress('done');
            return 'success';
          }
          throw new LoginError('game_account_mismatch', 'Se inició sesión con otra cuenta');
        }
      }
      const state = await cdp.eval('globalThis.__leagueAccountsLogin.status()');
      if (state === 'rejected') throw new LoginError('riot_auth_rejected', 'Usuario o contraseña incorrectos');
      if (state === 'captcha_rejected') throw new LoginError('riot_captcha_rejected', 'Riot pidió un captcha y lo rechazó');
      if (state === 'rate_limited') throw new LoginError('riot_rate_limited', 'Demasiados intentos: espera un momento');
      if (state === 'changed') throw new LoginError('riot_renderer_failed', 'Cambió la cuenta durante el login');
      if (state === 'verification' && !verification) {
        await cdp.eval('globalThis.__leagueAccountsLogin.showVerification()');
        onProgress('verification');
        verification = true;
      }
      await sleep(300);
    }
  } finally {
    // Cerrar el render de depuración: el inicio de sesión ya quedó en el servicio del cliente.
    if (cdp) cdp.close();
    await sleep(300);
    killTree(child);
  }
}

module.exports = { autoLogin, LoginError, sessionState, validDebuggerUrl };
