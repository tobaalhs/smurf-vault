// Prueba del modo "aparecer desconectado" con un cliente falso: node test/offline.test.js
const assert = require('assert');
const { OfflineMode } = require('../src/offline');

(async () => {
  let availability = 'chat';
  const puts = [];
  const client = {
    get: async () => ({ availability }),
    request: async (method, path, _opts, body) => {
      puts.push([method, path, body.availability]);
      availability = body.availability;
      return { status: 201 };
    },
  };
  const mode = new OfflineMode({ client });

  mode.configure({ enabled: true });
  await new Promise((r) => setTimeout(r, 20));
  assert.deepStrictEqual(puts, [['PUT', '/lol-chat/v1/me', 'offline']], 'al activarlo pasa a desconectado');

  await mode.tick();
  assert.strictEqual(puts.length, 1, 'si ya está desconectado no vuelve a pedirlo');

  availability = 'chat'; // el cliente se reinició y volvió a ponerte en línea
  await mode.tick();
  assert.strictEqual(puts.at(-1)[2], 'offline', 'lo vuelve a corregir');

  mode.configure({ enabled: false });
  await new Promise((r) => setTimeout(r, 20));
  assert.strictEqual(puts.at(-1)[2], 'chat', 'al apagarlo vuelve a en línea');
  assert.strictEqual(mode.timer, null);

  console.log('offline OK');
})();
