// API local del cliente de League (LCU). Solo funciona con el cliente abierto.
// La conexión (puerto + token) se guarda en memoria y solo se vuelve a buscar
// cuando falla, para no lanzar PowerShell en cada consulta.
const https = require('https');
const fs = require('fs');
const { execFile } = require('child_process');

const LOCKFILE_PATHS = [
  'C:\\Riot Games\\League of Legends\\lockfile',
  'D:\\Riot Games\\League of Legends\\lockfile',
];
const DISCOVER_EVERY_MS = 10_000;

// El cliente usa un certificado autofirmado: solo lo aceptamos para 127.0.0.1.
const agent = new https.Agent({ rejectUnauthorized: false });

let conn = null;
let lastDiscover = 0;

function fromProcess() {
  return new Promise((resolve) => {
    execFile(
      'powershell.exe',
      [
        '-NoProfile',
        '-Command',
        "Get-CimInstance Win32_Process -Filter \"Name='LeagueClientUx.exe'\" | Select-Object -ExpandProperty CommandLine",
      ],
      { windowsHide: true, timeout: 8000 },
      (err, stdout) => {
        if (err || !stdout) return resolve(null);
        const port = stdout.match(/--app-port=(\d+)/)?.[1];
        const password = stdout.match(/--remoting-auth-token=([\w-]+)/)?.[1];
        resolve(port && password ? { port, password } : null);
      }
    );
  });
}

function fromLockfile() {
  for (const p of LOCKFILE_PATHS) {
    try {
      const [, , port, password] = fs.readFileSync(p, 'utf8').split(':');
      if (port && password) return { port, password };
    } catch {}
  }
  return null;
}

async function connection({ force = false } = {}) {
  if (conn) return conn;
  if (!force && Date.now() - lastDiscover < DISCOVER_EVERY_MS) return null;
  lastDiscover = Date.now();
  conn = (await fromProcess()) || fromLockfile();
  return conn;
}

function rawRequest({ port, password }, method, path) {
  return new Promise((resolve, reject) => {
    const req = https.request(
      {
        host: '127.0.0.1',
        port,
        path,
        method,
        agent,
        timeout: 5000,
        headers: {
          Authorization: 'Basic ' + Buffer.from(`riot:${password}`).toString('base64'),
          Accept: 'application/json',
        },
      },
      (res) => {
        let body = '';
        res.on('data', (c) => (body += c));
        res.on('end', () => {
          let json = null;
          try {
            json = JSON.parse(body);
          } catch {}
          resolve({ status: res.statusCode, json });
        });
      }
    );
    req.on('timeout', () => req.destroy(new Error('LCU no responde')));
    req.on('error', reject);
    req.end();
  });
}

/**
 * Hace una petición al cliente. Devuelve { status, json }, o null si el cliente no está abierto.
 * Si la conexión guardada dejó de servir (cliente cerrado o reiniciado), se descarta.
 */
async function request(method, path, opts) {
  const c = await connection(opts);
  if (!c) return null;
  try {
    const res = await rawRequest(c, method, path);
    if (res.status === 401) throw new Error('token viejo');
    return res;
  } catch {
    conn = null;
    lastDiscover = 0; // la próxima consulta vuelve a buscar el cliente de inmediato
    return null;
  }
}

async function get(path, opts) {
  const res = await request('GET', path, opts);
  return res && res.status < 400 ? res.json : null;
}

function isConnected() {
  return !!conn;
}

function rankFrom(q) {
  if (!q || !q.tier || q.tier === 'NONE' || q.tier === '') return null;
  return {
    tier: q.tier,
    division: q.division && q.division !== 'NA' ? q.division : '',
    lp: q.leaguePoints ?? 0,
    wins: q.wins ?? 0,
    losses: q.losses ?? 0,
  };
}

/** Datos de la cuenta abierta en el cliente, o null si no hay cliente o sesión. */
async function currentAccount({ force = false } = {}) {
  const summoner = await get('/lol-summoner/v1/current-summoner', { force });
  if (!summoner?.puuid) return null;

  const [session, ranked, region, history, tftHistory] = await Promise.all([
    get('/lol-login/v1/session'), // trae el usuario de login (sin contraseña)
    get('/lol-ranked/v1/current-ranked-stats'),
    get('/riotclient/region-locale'),
    get('/lol-match-history/v1/products/lol/current-summoner/matches?begIndex=0&endIndex=1'),
    get(`/lol-match-history/v1/products/tft/${summoner.puuid}/matches?begin=0&count=1`),
  ]);
  // Última partida entre LoL y TFT.
  const last = history?.games?.games?.[0];
  const lolMs = last?.gameCreation ? last.gameCreation + (last.gameDuration || 0) * 1000 : 0;
  const tftMs = Number(tftHistory?.games?.[0]?.json?.game_datetime) || 0;
  const lastMs = Math.max(lolMs, tftMs);

  return {
    // Solo se usa para reconocer la cuenta guardada; no se guarda.
    username: typeof session?.username === 'string' ? session.username : '',
    puuid: summoner.puuid,
    gameName: summoner.gameName || summoner.displayName || '',
    tagLine: summoner.tagLine || '',
    level: summoner.summonerLevel,
    iconId: summoner.profileIconId,
    server: (region?.region || '').toUpperCase(),
    lastPlayedAt: lastMs ? new Date(lastMs).toISOString() : null,
    ranks: {
      solo: rankFrom(ranked?.queueMap?.RANKED_SOLO_5x5),
      flex: rankFrom(ranked?.queueMap?.RANKED_FLEX_SR),
      tft: rankFrom(ranked?.queueMap?.RANKED_TFT),
      doubleUp: rankFrom(ranked?.queueMap?.RANKED_TFT_DOUBLE_UP),
    },
  };
}

module.exports = { currentAccount, get, request, isConnected };
