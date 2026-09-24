// Prueba de la API de Riot con respuestas simuladas: node test/riot.test.js
const assert = require('assert');
const { lookup } = require('../src/riot');

const calls = [];
function mockFetch(routes) {
  global.fetch = async (url) => {
    const path = new URL(url).pathname;
    calls.push(path);
    const hit = Object.entries(routes).find(([re]) => new RegExp(re).test(path));
    const [status, body] = hit ? hit[1] : [404, null];
    return { status, ok: status < 400, headers: new Map(), json: async () => body };
  };
}

const decryptError = [400, { status: { message: 'Bad Request - Exception decrypting CLIENT', status_code: 400 } }];
const common = {
  'summoners/by-puuid/API': [200, { summonerLevel: 300, profileIconId: 7 }],
  'entries/by-puuid/API': [200, [{ queueType: 'RANKED_SOLO_5x5', tier: 'DIAMOND', rank: 'I', leaguePoints: 25, wins: 58, losses: 54 }]],
  'matches/by-puuid/API/ids': [200, []],
};

(async () => {
  // 1) El PUUID del cliente da 400 y la cuenta tiene Riot ID: se busca por nombre y funciona.
  mockFetch({
    'accounts/by-puuid/CLIENT': decryptError,
    'accounts/by-riot-id/Main/LAS': [200, { puuid: 'API', gameName: 'Main', tagLine: 'LAS' }],
    ...common,
  });
  const res = await lookup('RGAPI-x', { server: 'LAS', puuid: 'CLIENT', gameName: 'Main', tagLine: 'LAS' });
  assert.strictEqual(res.puuid, 'CLIENT', 'conserva el PUUID del cliente para reconocer la cuenta');
  assert.strictEqual(res.apiPuuid, 'API');
  assert.strictEqual(res.ranks.solo.tier, 'DIAMOND');
  assert.strictEqual(res.level, 300);

  // 2) Mismo 400 pero sin Riot ID: error que explica qué hacer y trae el mensaje de Riot.
  await assert.rejects(
    lookup('RGAPI-x', { server: 'LAS', puuid: 'CLIENT' }),
    (e) => /400/.test(e.message) && /Exception decrypting/.test(e.message) && /Riot ID/.test(e.message)
  );

  // 3) La próxima vez usa directo el PUUID que ya funcionó (sin pasar por el 400).
  calls.length = 0;
  mockFetch({ 'accounts/by-puuid/API': [200, { puuid: 'API', gameName: 'Main', tagLine: 'LAS' }], ...common });
  await lookup('RGAPI-x', { server: 'LAS', puuid: 'CLIENT', apiPuuid: 'API', gameName: 'Main', tagLine: 'LAS' });
  assert.ok(!calls.some((c) => c.includes('CLIENT')), 'no vuelve a usar el PUUID rechazado');

  console.log('riot OK');
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
