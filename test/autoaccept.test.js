// Prueba del autoaceptar: node test/autoaccept.test.js
// Simula el cliente de LoL reemplazando src/lcu.js en el cache de require.
const path = require('path');
const assert = require('assert');
const root = path.join(__dirname, '..', 'src') + path.sep;
let rc = null; // estado del ready check simulado
const calls = [];
require.cache[require.resolve(root + 'lcu.js')] = {
  exports: {
    request: async (method, p) => {
      calls.push(`${method} ${p}`);
      if (method === 'POST') { rc = { ...rc, playerResponse: 'Accepted' }; return { status: 204 }; }
      return rc ? { status: 200, json: rc } : { status: 404, json: null };
    },
  },
};
const { AutoAccept } = require(root + 'autoaccept.js');
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

(async () => {
  let accepted = 0;
  const statuses = [];
  const aa = new AutoAccept({ onAccepted: () => accepted++, onStatus: (s) => statuses.push(s.connected) });

  // 1) sin partida: no acepta nada
  aa.configure({ enabled: true, delay: 0 });
  await wait(1200);
  assert.strictEqual(accepted, 0);
  assert.deepStrictEqual(statuses, [true]);

  // 2) aparece partida -> se acepta
  rc = { state: 'InProgress', playerResponse: 'None' };
  await wait(1500);
  assert.strictEqual(accepted, 1, 'debería aceptar');

  // 3) con espera de 2 s, rechazada a mano antes -> no acepta
  rc = null; await wait(1100);
  aa.configure({ enabled: true, delay: 2 });
  rc = { state: 'InProgress', playerResponse: 'None' };
  await wait(1200);
  rc = { state: 'InProgress', playerResponse: 'Declined' };
  await wait(2000);
  assert.strictEqual(accepted, 1, 'no debe aceptar una partida que rechazaste');

  // 4) apagado -> no acepta
  rc = null; await wait(1100);
  aa.configure({ enabled: false });
  rc = { state: 'InProgress', playerResponse: 'None' };
  await wait(1500);
  assert.strictEqual(accepted, 1, 'apagado no acepta');

  console.log('autoaccept OK', { accepted, posts: calls.filter((c) => c.startsWith('POST')).length });
  process.exit(0);
})().catch((e) => { console.error(e); process.exit(1); });
