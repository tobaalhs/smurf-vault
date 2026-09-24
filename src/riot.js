// API oficial de Riot (necesita una API key de developer.riotgames.com).
// Servidor que se ve en el cliente -> "platform" de la API + "región" para account-v1 y match-v5.
const { masteryFrom, TOP: MASTERY_TOP } = require('./mastery');

const SERVERS = {
  LAS: { platform: 'la2', region: 'americas' },
  LAN: { platform: 'la1', region: 'americas' },
  NA: { platform: 'na1', region: 'americas' },
  BR: { platform: 'br1', region: 'americas' },
  OCE: { platform: 'oc1', region: 'americas', matchRegion: 'sea' },
  EUW: { platform: 'euw1', region: 'europe' },
  EUNE: { platform: 'eun1', region: 'europe' },
  TR: { platform: 'tr1', region: 'europe' },
  RU: { platform: 'ru', region: 'europe' },
  KR: { platform: 'kr', region: 'asia' },
  JP: { platform: 'jp1', region: 'asia' },
};

// Lo que devuelve el cliente (/riotclient/region-locale) -> nuestro código de servidor.
const CLIENT_REGION = { LA2: 'LAS', LA1: 'LAN', NA: 'NA', NA1: 'NA', BR: 'BR', BR1: 'BR', OC1: 'OCE', EUW: 'EUW', EUW1: 'EUW', EUNE: 'EUNE', EUN1: 'EUNE', TR: 'TR', RU: 'RU', KR: 'KR', JP: 'JP' };

function serverFromClient(region) {
  return CLIENT_REGION[region] || region || '';
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

class RiotError extends Error {
  constructor(status, step, detail) {
    super(`Riot respondió ${status} al consultar ${step}${detail ? ` (${detail})` : ''}`);
    this.status = status;
  }
}

async function call(apiKey, host, path, step, retries = 3) {
  const res = await fetch(`https://${host}.api.riotgames.com${path}`, {
    headers: { 'X-Riot-Token': apiKey },
  });
  if (res.status === 404) return null;
  if (res.status === 401 || res.status === 403) {
    throw new Error('API key de Riot inválida o caducada (las de desarrollo duran 24 h)');
  }
  if (res.status === 429) {
    // Límite de consultas: Riot dice cuántos segundos esperar.
    if (!retries) throw new Error('Demasiadas consultas a Riot, espera un poco');
    await sleep((Number(res.headers.get('retry-after')) || 5) * 1000);
    return call(apiKey, host, path, step, retries - 1);
  }
  if (!res.ok) {
    let detail = '';
    try {
      detail = (await res.json())?.status?.message || '';
    } catch {}
    throw new RiotError(res.status, step, detail);
  }
  return res.json();
}

// Para TFT: si la key no tiene acceso a TFT (p. ej. una Personal key solo de LoL), no rompemos
// la actualización de LoL; devolvemos undefined para no borrar lo que ya teníamos.
const optional = (p) => p.catch(() => undefined);

function rank(entries, queue) {
  const e = entries?.find((x) => x.queueType === queue);
  return e ? { tier: e.tier, division: ['MASTER', 'GRANDMASTER', 'CHALLENGER'].includes(e.tier) ? '' : e.rank, lp: e.leaguePoints, wins: e.wins, losses: e.losses } : null;
}

function byRiotId(apiKey, srv, account) {
  return call(
    apiKey,
    srv.region,
    `/riot/account/v1/accounts/by-riot-id/${encodeURIComponent(account.gameName)}/${encodeURIComponent(account.tagLine)}`,
    'la cuenta por Riot ID'
  );
}

/** Nick, nivel, rangos y última partida de un PUUID válido para esta API key. */
async function details(apiKey, srv, puuid) {
  const matchRegion = srv.matchRegion || srv.region;
  const [summoner, entries, matchIds, tftEntries, tftIds, masteryTop, masteryScore] = await Promise.all([
    call(apiKey, srv.platform, `/lol/summoner/v4/summoners/by-puuid/${puuid}`, 'el invocador'),
    call(apiKey, srv.platform, `/lol/league/v4/entries/by-puuid/${puuid}`, 'el rango'),
    call(apiKey, matchRegion, `/lol/match/v5/matches/by-puuid/${puuid}/ids?start=0&count=1`, 'el historial'),
    optional(call(apiKey, srv.platform, `/tft/league/v1/by-puuid/${puuid}`, 'el rango de TFT')),
    optional(call(apiKey, matchRegion, `/tft/match/v1/matches/by-puuid/${puuid}/ids?start=0&count=1`, 'el historial de TFT')),
    optional(call(apiKey, srv.platform, `/lol/champion-mastery/v4/champion-masteries/by-puuid/${puuid}/top?count=${MASTERY_TOP}`, 'las maestrías')),
    optional(call(apiKey, srv.platform, `/lol/champion-mastery/v4/scores/by-puuid/${puuid}`, 'el puntaje de maestría')),
  ]);
  const [lastMatch, lastTft] = await Promise.all([
    matchIds?.[0] ? optional(call(apiKey, matchRegion, `/lol/match/v5/matches/${matchIds[0]}`, 'la última partida')) : null,
    tftIds?.[0] ? optional(call(apiKey, matchRegion, `/tft/match/v1/matches/${tftIds[0]}`, 'la última partida de TFT')) : null,
  ]);
  return { summoner, entries, tftEntries, lastMatch, lastTft, mastery: masteryFrom(masteryTop, masteryScore) };
}

/**
 * Actualiza una cuenta usando su PUUID, o si no lo tiene, su Riot ID (Nombre#TAG).
 * Devuelve solo los campos a sobrescribir.
 */
async function lookup(apiKey, account) {
  const srv = SERVERS[account.server];
  if (!srv) throw new Error(`Servidor desconocido: ${account.server || '(vacío)'}`);
  const hasRiotId = account.gameName && account.tagLine;
  if (!account.puuid && !hasRiotId) throw new Error('Necesita Riot ID (Nombre#TAG) o haberla detectado con el cliente');

  // Preferimos el PUUID que ya funcionó con la API; si no, el que vino del cliente.
  const knownPuuid = account.apiPuuid || account.puuid;
  let acc;
  let info;
  try {
    acc = knownPuuid
      ? await call(apiKey, srv.region, `/riot/account/v1/accounts/by-puuid/${knownPuuid}`, 'la cuenta')
      : await byRiotId(apiKey, srv, account);
    if (!acc) throw new Error('Riot no encontró la cuenta');
    info = await details(apiKey, srv, acc.puuid);
  } catch (e) {
    // 400 con un PUUID = Riot no lo acepta para esta key. Si hay Riot ID, la buscamos por nombre.
    if (!(e instanceof RiotError && e.status === 400 && knownPuuid)) throw e;
    if (!hasRiotId) throw new Error(`${e.message}. Agrégale su Riot ID (Nombre#TAG) en Editar para buscarla por nombre`);
    acc = await byRiotId(apiKey, srv, account);
    if (!acc) throw new Error(`Riot no encontró ${account.gameName}#${account.tagLine}`);
    info = await details(apiKey, srv, acc.puuid);
  }

  const { summoner, entries, tftEntries, lastMatch, lastTft, mastery } = info;
  // Última partida entre LoL y TFT.
  const endMs = Math.max(
    lastMatch?.info?.gameEndTimestamp || lastMatch?.info?.gameCreation || 0,
    lastTft?.info?.game_datetime || 0
  );
  const tftRank = (queue) => (tftEntries === undefined ? undefined : rank(tftEntries, queue));

  return {
    // El PUUID del cliente se conserva para reconocer la cuenta al detectarla.
    puuid: account.puuid || acc.puuid,
    apiPuuid: acc.puuid,
    gameName: acc.gameName,
    tagLine: acc.tagLine,
    level: summoner?.summonerLevel ?? account.level,
    iconId: summoner?.profileIconId ?? account.iconId,
    lastPlayedAt: endMs ? new Date(endMs).toISOString() : null,
    mastery,
    ranks: {
      solo: rank(entries, 'RANKED_SOLO_5x5'),
      flex: rank(entries, 'RANKED_FLEX_SR'),
      tft: tftRank('RANKED_TFT'),
      doubleUp: tftRank('RANKED_TFT_DOUBLE_UP'),
    },
  };
}

module.exports = { SERVERS, serverFromClient, lookup };
