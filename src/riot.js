// API oficial de Riot (necesita una API key de developer.riotgames.com).
// Servidor que se ve en el cliente -> "platform" de la API + "región" para account-v1 y match-v5.
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

async function call(apiKey, host, path, retries = 3) {
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
    return call(apiKey, host, path, retries - 1);
  }
  if (!res.ok) throw new Error(`Riot API ${res.status}`);
  return res.json();
}

// Para TFT: si la key no tiene acceso a TFT (p. ej. una Personal key solo de LoL), no rompemos
// la actualización de LoL; devolvemos undefined para no borrar lo que ya teníamos.
const optional = (p) => p.catch(() => undefined);

function rank(entries, queue) {
  const e = entries?.find((x) => x.queueType === queue);
  return e ? { tier: e.tier, division: ['MASTER', 'GRANDMASTER', 'CHALLENGER'].includes(e.tier) ? '' : e.rank, lp: e.leaguePoints, wins: e.wins, losses: e.losses } : null;
}

/**
 * Actualiza una cuenta usando su PUUID, o si no lo tiene, su Riot ID (Nombre#TAG).
 * Devuelve solo los campos a sobrescribir.
 */
async function lookup(apiKey, account) {
  const srv = SERVERS[account.server];
  if (!srv) throw new Error(`Servidor desconocido: ${account.server || '(vacío)'}`);

  let acc;
  if (account.puuid) {
    acc = await call(apiKey, srv.region, `/riot/account/v1/accounts/by-puuid/${account.puuid}`);
  } else if (account.gameName && account.tagLine) {
    acc = await call(
      apiKey,
      srv.region,
      `/riot/account/v1/accounts/by-riot-id/${encodeURIComponent(account.gameName)}/${encodeURIComponent(account.tagLine)}`
    );
  } else {
    throw new Error('Necesita Riot ID (Nombre#TAG) o haberla detectado con el cliente');
  }
  if (!acc) throw new Error('Riot no encontró la cuenta');

  const matchRegion = srv.matchRegion || srv.region;
  const [summoner, entries, matchIds, tftEntries, tftIds] = await Promise.all([
    call(apiKey, srv.platform, `/lol/summoner/v4/summoners/by-puuid/${acc.puuid}`),
    call(apiKey, srv.platform, `/lol/league/v4/entries/by-puuid/${acc.puuid}`),
    call(apiKey, matchRegion, `/lol/match/v5/matches/by-puuid/${acc.puuid}/ids?start=0&count=1`),
    optional(call(apiKey, srv.platform, `/tft/league/v1/by-puuid/${acc.puuid}`)),
    optional(call(apiKey, matchRegion, `/tft/match/v1/matches/by-puuid/${acc.puuid}/ids?start=0&count=1`)),
  ]);
  const [lastMatch, lastTft] = await Promise.all([
    matchIds?.[0] ? call(apiKey, matchRegion, `/lol/match/v5/matches/${matchIds[0]}`) : null,
    tftIds?.[0] ? optional(call(apiKey, matchRegion, `/tft/match/v1/matches/${tftIds[0]}`)) : null,
  ]);
  // Última partida entre LoL y TFT.
  const endMs = Math.max(
    lastMatch?.info?.gameEndTimestamp || lastMatch?.info?.gameCreation || 0,
    lastTft?.info?.game_datetime || 0
  );
  const tftRank = (queue) => (tftEntries === undefined ? undefined : rank(tftEntries, queue));

  return {
    puuid: acc.puuid,
    gameName: acc.gameName,
    tagLine: acc.tagLine,
    level: summoner?.summonerLevel ?? account.level,
    iconId: summoner?.profileIconId ?? account.iconId,
    lastPlayedAt: endMs ? new Date(endMs).toISOString() : null,
    ranks: {
      solo: rank(entries, 'RANKED_SOLO_5x5'),
      flex: rank(entries, 'RANKED_FLEX_SR'),
      tft: tftRank('RANKED_TFT'),
      doubleUp: tftRank('RANKED_TFT_DOUBLE_UP'),
    },
  };
}

module.exports = { SERVERS, serverFromClient, lookup };
