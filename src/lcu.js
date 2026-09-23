// Lectura de la API local del cliente de League (LCU).
// Solo funciona con el cliente abierto y con una sesión iniciada.
const https = require('https');
const fs = require('fs');
const { execFile } = require('child_process');

const LOCKFILE_PATHS = [
  'C:\\Riot Games\\League of Legends\\lockfile',
  'D:\\Riot Games\\League of Legends\\lockfile',
];

// El cliente usa un certificado autofirmado: solo lo aceptamos para 127.0.0.1.
const agent = new https.Agent({ rejectUnauthorized: false });

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

function get({ port, password }, path) {
  return new Promise((resolve, reject) => {
    const req = https.get(
      {
        host: '127.0.0.1',
        port,
        path,
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
          if (res.statusCode >= 400) return reject(new Error(`LCU ${res.statusCode} en ${path}`));
          try {
            resolve(JSON.parse(body));
          } catch {
            resolve(null);
          }
        });
      }
    );
    req.on('timeout', () => req.destroy(new Error('LCU no responde')));
    req.on('error', reject);
  });
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

/** Devuelve los datos de la cuenta abierta en el cliente, o null si no hay cliente/sesión. */
async function currentAccount() {
  const conn = (await fromProcess()) || fromLockfile();
  if (!conn) return null;
  let summoner;
  try {
    summoner = await get(conn, '/lol-summoner/v1/current-summoner');
  } catch {
    return null; // cliente abierto pero sin sesión todavía
  }
  if (!summoner?.puuid) return null;

  const [ranked, region] = await Promise.all([
    get(conn, '/lol-ranked/v1/current-ranked-stats').catch(() => null),
    get(conn, '/riotclient/region-locale').catch(() => null),
  ]);

  return {
    puuid: summoner.puuid,
    gameName: summoner.gameName || summoner.displayName || '',
    tagLine: summoner.tagLine || '',
    level: summoner.summonerLevel,
    iconId: summoner.profileIconId,
    server: (region?.region || '').toUpperCase(),
    ranks: {
      solo: rankFrom(ranked?.queueMap?.RANKED_SOLO_5x5),
      flex: rankFrom(ranked?.queueMap?.RANKED_FLEX_SR),
    },
  };
}

module.exports = { currentAccount };
