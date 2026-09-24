// Cambio de cuenta sin escribir la contraseña: el Riot Client guarda la sesión de "Mantener sesión
// iniciada" en archivos locales. Guardamos una copia por cuenta y, al cambiar, cerramos el cliente,
// restauramos la copia de esa cuenta y lo volvemos a abrir. Nunca se toca la contraseña.
const fs = require('fs');
const pathLib = require('path');
const { execFile, spawn } = require('child_process');
const { rawRequest } = require('./lcu');

const SESSION_FILE = 'RiotGamesPrivateSettings.yaml';
// Carpetas de %LOCALAPPDATA%\Riot Games con la sesión (versiones viejas y nuevas del cliente).
const SESSION_DIRS = ['Riot Client', 'League of Legends'];
const RIOT_PROCESSES = [
  'RiotClientServices.exe',
  'RiotClientUx.exe',
  'RiotClientUxRender.exe',
  'Riot Client.exe',
  'RiotClientCrashHandler.exe',
  'LeagueClient.exe',
  'LeagueClientUx.exe',
  'LeagueClientUxRender.exe',
];
const IN_GAME_PROCESS = 'League of Legends.exe';

function riotDataRoot() {
  return pathLib.join(process.env.LOCALAPPDATA || '', 'Riot Games');
}

/** Rutas relativas (al root) de los archivos de sesión, existan o no. */
function sessionPaths() {
  return SESSION_DIRS.map((d) => pathLib.join(d, 'Data', SESSION_FILE));
}

/** Lee los archivos de sesión actuales: { relPath: base64 }. */
function readSession(root = riotDataRoot()) {
  const files = {};
  for (const rel of sessionPaths()) {
    try {
      files[rel] = fs.readFileSync(pathLib.join(root, rel)).toString('base64');
    } catch {}
  }
  return files;
}

/**
 * true si la sesión tiene "Mantener sesión iniciada": en el cliente actual es el refresh_token de
 * psl.authorization; en versiones viejas era la cookie ssid. Sin eso el cliente pide la contraseña.
 */
function isRemembered(files) {
  return Object.values(files).some((b64) =>
    /^[ \t]*refresh_token:[ \t]*\S|\bssid\b/m.test(Buffer.from(b64, 'base64').toString('utf8'))
  );
}

/** Reemplaza la sesión del cliente por `files` (o la borra si es null, para mostrar el login). */
function writeSession(files, root = riotDataRoot()) {
  for (const rel of sessionPaths()) {
    const full = pathLib.join(root, rel);
    const content = files?.[rel];
    if (content) {
      fs.mkdirSync(pathLib.dirname(full), { recursive: true });
      const tmp = full + '.tmp';
      fs.writeFileSync(tmp, Buffer.from(content, 'base64'));
      fs.renameSync(tmp, full);
    } else {
      fs.rmSync(full, { force: true });
    }
  }
}

function runningProcesses() {
  return new Promise((resolve) => {
    execFile('tasklist', ['/FO', 'CSV', '/NH'], { windowsHide: true, timeout: 8000 }, (err, stdout) => {
      if (err) return resolve(new Set());
      const names = stdout.split(/\r?\n/).map((l) => l.match(/^"([^"]+)"/)?.[1]?.toLowerCase()).filter(Boolean);
      resolve(new Set(names));
    });
  });
}

async function isInGame() {
  return (await runningProcesses()).has(IN_GAME_PROCESS.toLowerCase());
}

async function closeRiot() {
  const running = await runningProcesses();
  const targets = RIOT_PROCESSES.filter((p) => running.has(p.toLowerCase()));
  if (!targets.length) return;
  await new Promise((resolve) =>
    execFile('taskkill', ['/F', ...targets.flatMap((p) => ['/IM', p])], { windowsHide: true }, () => resolve())
  );
  // Esperamos a que terminen de verdad: si no, pueden volver a escribir la sesión al cerrarse.
  for (let i = 0; i < 40; i++) {
    const now = await runningProcesses();
    if (!RIOT_PROCESSES.some((p) => now.has(p.toLowerCase()))) return;
    await new Promise((r) => setTimeout(r, 250));
  }
  throw new Error('No se pudo cerrar el cliente de Riot');
}

/** Ruta de RiotClientServices.exe según RiotClientInstalls.json, o la de por defecto. */
function riotClientPath() {
  const candidates = [];
  try {
    const installs = JSON.parse(
      fs.readFileSync(pathLib.join(process.env.PROGRAMDATA || 'C:\\ProgramData', 'Riot Games', 'RiotClientInstalls.json'), 'utf8')
    );
    candidates.push(installs.rc_default, installs.rc_live);
  } catch {}
  candidates.push('C:\\Riot Games\\Riot Client\\RiotClientServices.exe');
  return candidates.find((p) => p && fs.existsSync(p)) || null;
}

function launchLeague() {
  const exe = riotClientPath();
  if (!exe) throw new Error('No encontré el Riot Client instalado');
  spawn(exe, ['--launch-product=league_of_legends', '--launch-patchline=live'], {
    detached: true,
    stdio: 'ignore',
  }).unref();
}

/** Puerto y token de la API local del Riot Client (lockfile: nombre:pid:puerto:token:protocolo). */
function riotClientConnection(root = riotDataRoot()) {
  try {
    const [, , port, password] = fs.readFileSync(pathLib.join(root, 'Riot Client', 'Config', 'lockfile'), 'utf8').split(':');
    return port && password ? { port, password } : null;
  } catch {
    return null;
  }
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let launchRun = 0; // cada cambio de cuenta nuevo cancela la espera del anterior

/**
 * El Riot Client ignora --launch-product si la sesión todavía no está lista y se queda en su inicio.
 * Esperamos a que haya sesión y le pedimos el LoL por su API local, como el botón "Jugar".
 * Sin sesión guardada sigue esperando a que inicies sesión a mano (hasta `timeoutMs`).
 */
async function ensureLeagueStarts({ timeoutMs = 180_000 } = {}) {
  const run = ++launchRun;
  const until = Date.now() + timeoutMs;
  while (Date.now() < until && run === launchRun) {
    await sleep(2000);
    const running = await runningProcesses();
    if (running.has('leagueclient.exe') || running.has('leagueclientux.exe')) {
      await minimizeRiotClient();
      return true;
    }
    const conn = riotClientConnection();
    if (!conn) continue;
    try {
      const auth = await rawRequest(conn, 'GET', '/entitlements/v1/token');
      if (auth.status !== 200) continue; // todavía en el login
      await rawRequest(conn, 'POST', '/product-launcher/v1/products/league_of_legends/patchlines/live');
      await sleep(4000); // le damos tiempo a que aparezca el proceso antes de volver a pedirlo
    } catch {} // lockfile viejo o cliente arrancando
  }
  return false;
}

// Minimiza las ventanas del Riot Client con la API de Windows. No cierra nada: el LoL necesita que el
// Riot Client siga corriendo, y si quieres volver a verlo está en la barra de tareas.
// (Ojo: /riotclient/kill-ux de la API del LoL NO sirve para esto: cierra la ventana del propio LoL.)
const MINIMIZE_RIOT_CLIENT = `
Add-Type -Namespace SV -Name Win -MemberDefinition '[DllImport("user32.dll")] public static extern bool ShowWindowAsync(IntPtr h, int n);'
Get-Process -Name 'Riot Client' -ErrorAction SilentlyContinue |
  Where-Object { $_.MainWindowHandle -ne 0 } |
  ForEach-Object { [SV.Win]::ShowWindowAsync($_.MainWindowHandle, 6) | Out-Null }
`;

function runPowerShell(script) {
  return new Promise((resolve) =>
    execFile('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], { windowsHide: true, timeout: 15000 }, () =>
      resolve()
    )
  );
}

/** Con el LoL ya abierto, minimiza el Riot Client (varias veces: a veces vuelve al frente al cargar el LoL). */
async function minimizeRiotClient() {
  for (const wait of [1500, 3000, 5000]) {
    await sleep(wait);
    await runPowerShell(MINIMIZE_RIOT_CLIENT);
  }
}

module.exports = {
  readSession,
  writeSession,
  isRemembered,
  isInGame,
  closeRiot,
  launchLeague,
  ensureLeagueStarts,
  sessionPaths,
};
