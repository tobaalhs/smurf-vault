// Prueba del historial de partidas: node test/matches.test.js
const assert = require('assert');
const { fromLolGame, fromTftGame, mergeMatches, KEEP } = require('../src/matches');

const lol = fromLolGame({
  gameId: 1,
  gameCreation: Date.parse('2026-09-20T10:00:00Z'),
  gameDuration: 1800,
  queueId: 420,
  participants: [{ championId: 103, stats: { win: true, kills: 7, deaths: 2, assists: 9, totalMinionsKilled: 180, neutralMinionsKilled: 12, champLevel: 16, item0: 3285, item6: 3340 } }],
});
assert.strictEqual(lol.id, 'lol-1');
assert.strictEqual(lol.at, '2026-09-20T10:30:00.000Z', 'la fecha es el final de la partida');
assert.strictEqual(lol.cs, 192);
assert.deepStrictEqual(lol.items, [3285, 0, 0, 0, 0, 0, 3340]);
assert.strictEqual(lol.remake, false);
assert.strictEqual(fromLolGame({ gameId: 2, participants: [] }), null);

const tft = fromTftGame(
  { metadata: { match_id: 'LA2_9' }, json: { game_datetime: Date.parse('2026-09-21T10:00:00Z'), game_length: 2100.5, queue_id: 1100, participants: [{ puuid: 'otro', placement: 1 }, { puuid: 'yo', placement: 3, level: 8, companion: { content_ID: 'abc-123', item_ID: 1 } }] } },
  'yo'
);
assert.strictEqual(tft.placement, 3);
assert.strictEqual(tft.win, true, 'top 4 cuenta como victoria');
assert.strictEqual(tft.companion, 'abc-123');
assert.strictEqual(fromTftGame({ json: { game_datetime: 1, participants: [] } }, 'yo'), null);

// Mezcla: sin repetidos, más recientes primero y máximo KEEP por modo.
const old = Array.from({ length: KEEP }, (_, i) => ({ id: `o${i}`, mode: 'lol', at: new Date(Date.UTC(2026, 0, i + 1)).toISOString() }));
const merged = mergeMatches(old, [tft, lol, { ...old[0] }]);
assert.strictEqual(merged.length, KEEP + 1, '20 de LoL + 1 de TFT');
assert.deepStrictEqual(merged.slice(0, 2).map((m) => m.id), [tft.id, lol.id]);
assert.strictEqual(new Set(merged.map((m) => m.id)).size, KEEP + 1);
// Muchas partidas de TFT no desplazan a las de LoL.
const manyTft = Array.from({ length: 30 }, (_, i) => ({ id: `t${i}`, mode: 'tft', at: new Date(Date.UTC(2026, 5, i + 1)).toISOString() }));
const mixed = mergeMatches(old, manyTft);
assert.strictEqual(mixed.filter((m) => m.mode === 'lol').length, KEEP);
assert.strictEqual(mixed.filter((m) => m.mode === 'tft').length, KEEP);

console.log('matches OK');
