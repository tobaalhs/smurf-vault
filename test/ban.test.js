// Prueba de la lectura de baneos: node test/ban.test.js
const assert = require('assert');
const { banFromSession } = require('../src/ban');

const future = Date.now() + 3 * 24 * 3600 * 1000;
const past = Date.now() - 1000;

assert.strictEqual(banFromSession(null), null);
assert.strictEqual(banFromSession({ state: 'SUCCEEDED' }), null, 'sin ban');
assert.strictEqual(banFromSession({ ban: { restrictions: [] } }), null);

// Restricción de chat: deja jugar.
assert.strictEqual(banFromSession({ ban: { restrictions: [{ type: 'TEXT_CHAT_RESTRICTION', scope: 'lol' }] } }), null);
// Restricción de ranked: deja jugar.
assert.strictEqual(banFromSession({ ban: { restrictions: [{ type: 'RANKED_RESTRICTION', scope: 'lol' }] } }), null);

// Baneo temporal de juego.
const b = banFromSession({ ban: { restrictions: [{ type: 'TIME_BAN', scope: 'lol', dat: { expirationMillis: future } }] } });
assert.deepStrictEqual(b, { permanent: false, until: new Date(future).toISOString() });

// Baneo ya vencido: no cuenta.
assert.strictEqual(banFromSession({ ban: { restrictions: [{ type: 'TIME_BAN', scope: 'lol', dat: { expirationMillis: past } }] } }), null);

// Permanente manda sobre temporal.
const perm = banFromSession({ ban: { restrictions: [{ type: 'TIME_BAN', scope: 'lol', dat: { expirationMillis: future } }, { type: 'PERMANENT_BAN', scope: 'lol' }] } });
assert.deepStrictEqual(perm, { permanent: true });

// Baneo de otro juego (valorant): no aplica.
assert.strictEqual(banFromSession({ ban: { restrictions: [{ type: 'TIME_BAN', scope: 'valorant', dat: { expirationMillis: future } }] } }), null);

// Dentro del idToken (JWT).
const jwt = ['e30', Buffer.from(JSON.stringify({ ban: { restrictions: [{ type: 'PERMANENT_BAN', scope: 'lol' }] } })).toString('base64url'), 'firma'].join('.');
assert.deepStrictEqual(banFromSession({ idToken: jwt }), { permanent: true });

console.log('ban OK');
