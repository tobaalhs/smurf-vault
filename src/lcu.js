// API local del cliente de League (LCU). Solo funciona con el cliente abierto.
// La conexión (puerto + token) se guarda en memoria y solo se vuelve a buscar
// cuando falla. Se busca primero el lockfile y solo de vez en cuando el proceso.
const https = require('https');
const fs = require('fs');
const pathLib = require('path');
const { execFile } = require('child_process');
const { fromLolGame, fromTftGame, KEEP } = require('./matches');
const { masteryFrom } = require('./mastery');

// Carpetas donde buscar el lockfile del cliente. Si el cliente está instalado en otra parte,
// la carpeta se aprende al encontrar el proceso.
const LOCKFILE_DIRS = new Set(['C:\\Riot Games\\League of Legends', 'D:\\Riot Games\\League of Legends']);
// Leer el lockfile es casi gratis; buscar el proceso lanza PowerShell, así que se limita.
const PROCESS_LOOKUP_EVERY_MS = 60_000;

// El cliente usa un certificado autofirmado: solo lo aceptamos para 127.0.0.1.
const agent = new https.Agent({ rejectUnauthorized: false });

let conn = null;
let lastProcessLookup = 0;

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
        const dir = stdout.match(/--install-directory=([^"]+?)(?:"|\s--|$)/)?.[1];
        if (dir) LOCKFILE_DIRS.add(dir.trim().replace(/[\\/]+$/, ''));
        resolve(port && password ? { port, password } : null);
      }
    );
  });
}

function fromLockfile() {
  for (const dir of LOCKFILE_DIRS) {
    try {
      const [, , port, password] = fs.readFileSync(pathLib.join(dir, 'lockfile'), 'utf8').split(':');
      if (port && password) return { port, password };
    } catch {}
  }
  return null;
}

async function connection({ force = false } = {}) {
  if (conn) return conn;
  conn = fromLockfile();
  if (conn) return conn;
  if (!force && Date.now() - lastProcessLookup < PROCESS_LOOKUP_EVERY_MS) return null;
  lastProcessLookup = Date.now();
  conn = await fromProcess();
  return conn;
}

function rawRequest({ port, password }, method, path, body) {
  const payload = body === undefined ? null : JSON.stringify(body);
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
          ...(payload ? { 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(payload) } : {}),
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
    req.end(payload ?? undefined);
  });
}

/**
 * Hace una petición al cliente. Devuelve { status, json }, o null si el cliente no está abierto.
 * Si la conexión guardada dejó de servir (cliente cerrado o reiniciado), se descarta.
 */
async function request(method, path, opts, body) {
  const c = await connection(opts);
  if (!c) return null;
  try {
    const res = await rawRequest(c, method, path, body);
    if (res.status === 401) throw new Error('token viejo');
    return res;
  } catch {
    conn = null;
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

/**
 * Marco del ícono que eligió la cuenta: { type: 'level', theme: 1..21 } para los marcos de nivel,
 * { type: 'ranked' } para las alas de rango, o undefined si el cliente no respondió.
 */
function crestFrom(regalia) {
  if (!regalia?.crestType) return undefined;
  const theme = Number(regalia.selectedPrestigeCrest);
  if (regalia.crestType === 'prestige' && theme >= 1 && theme <= 21) return { type: 'level', theme };
  return { type: 'ranked' };
}

/** Datos de la cuenta abierta en el cliente, o null si no hay cliente o sesión. */
async function currentAccount({ force = false } = {}) {
  const summoner = await get('/lol-summoner/v1/current-summoner', { force });
  if (!summoner?.puuid) return null;

  const [session, ranked, region, history, tftHistory, profile, regalia, mastery, masteryScore] = await Promise.all([
    get('/lol-login/v1/session'), // trae el usuario de login (sin contraseña)
    get('/lol-ranked/v1/current-ranked-stats'),
    get('/riotclient/region-locale'),
    get(`/lol-match-history/v1/products/lol/current-summoner/matches?begIndex=0&endIndex=${KEEP}`),
    get(`/lol-match-history/v1/products/tft/${summoner.puuid}/matches?begin=0&count=${KEEP}`),
    get('/lol-summoner/v1/current-summoner/summoner-profile'), // skin elegida de fondo del perfil
    get('/lol-regalia/v2/current-summoner/regalia'), // marco elegido: de nivel o de rango
    get('/lol-champion-mastery/v1/local-player/champion-mastery'),
    get('/lol-champion-mastery/v1/local-player/champion-mastery-score'),
  ]);
  const matches = [
    ...(history?.games?.games || []).map(fromLolGame),
    ...(tftHistory?.games || []).map((g) => fromTftGame(g, summoner.puuid)),
  ].filter(Boolean);
  // Última partida entre LoL y TFT.
  const lastMs = Math.max(0, ...matches.map((m) => Date.parse(m.at)));

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
    // undefined si el cliente no respondió: así no se borra lo que ya estaba guardado.
    backgroundSkinId: Number(profile?.backgroundSkinId) || undefined,
    matches: history || tftHistory ? matches : undefined,
    crest: crestFrom(regalia),
    mastery: masteryFrom(mastery, masteryScore),
    ranks: {
      solo: rankFrom(ranked?.queueMap?.RANKED_SOLO_5x5),
      flex: rankFrom(ranked?.queueMap?.RANKED_FLEX_SR),
      tft: rankFrom(ranked?.queueMap?.RANKED_TFT),
      doubleUp: rankFrom(ranked?.queueMap?.RANKED_TFT_DOUBLE_UP),
    },
  };
}

module.exports = { currentAccount, get, request, rawRequest, isConnected };
