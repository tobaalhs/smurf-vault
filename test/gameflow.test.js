// Prueba del detector de fin de partida: node test/gameflow.test.js
const path = require('path');
const assert = require('assert');

// Simula las fases del cliente reemplazando src/lcu.js.
let phase = null;
require.cache[require.resolve(path.join(__dirname, '..', 'src', 'lcu.js'))] = {
  exports: { get: async () => phase },
};
const { GameflowWatcher } = require('../src/gameflow');

(async () => {
  let ends = 0;
  const w = new GameflowWatcher({ onGameEnd: () => ends++ });
  const step = async (p) => {
    phase = p;
    await w.tick();
  };

  // Partida normal: lobby -> jugando -> estadísticas -> fin
  for (const p of ['Lobby', 'ChampSelect', 'InProgress', 'InProgress', 'WaitingForStats', 'EndOfGame']) await step(p);
  assert.strictEqual(ends, 1, 'una partida = un aviso');

  // TFT: jugando -> directo a EndOfGame
  for (const p of ['InProgress', 'EndOfGame']) await step(p);
  assert.strictEqual(ends, 2);

  // Reconexión en medio de la partida no cuenta como fin
  for (const p of ['InProgress', 'Reconnect', 'InProgress']) await step(p);
  assert.strictEqual(ends, 2, 'reconectar no es terminar');
  await step('PreEndOfGame');
  assert.strictEqual(ends, 3);

  // Cliente cerrado durante la partida: no se cuenta
  for (const p of ['InProgress', null]) await step(p);
  assert.strictEqual(ends, 3, 'cliente cerrado no cuenta');

  // Dodge en selección de campeones: nunca estuvo jugando
  for (const p of ['ChampSelect', 'Lobby']) await step(p);
  assert.strictEqual(ends, 3, 'un dodge no cuenta');

  console.log('gameflow OK');
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
