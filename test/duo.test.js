// Prueba de las reglas de dúo: node test/duo.test.js
const assert = require('assert');
const { canDuo } = require('../renderer/duo');
const r = (tier, division) => ({ tier, division });

assert.strictEqual(canDuo(null, 'GOLD', 'II'), false, 'sin rango no puede');
// Gold hace dúo con Silver, Gold y Platino (±1 tier).
assert.strictEqual(canDuo(r('GOLD', 'I'), 'GOLD', 'IV'), true);
assert.strictEqual(canDuo(r('SILVER', 'II'), 'GOLD', 'II'), true);
assert.strictEqual(canDuo(r('PLATINUM', 'IV'), 'GOLD', 'II'), true);
assert.strictEqual(canDuo(r('EMERALD', 'I'), 'GOLD', 'II'), false, 'muy arriba');
assert.strictEqual(canDuo(r('BRONZE', 'I'), 'GOLD', 'II'), false, 'muy abajo');
// Platino: Oro, Platino, Esmeralda.
assert.strictEqual(canDuo(r('EMERALD', 'III'), 'PLATINUM', 'II'), true);
assert.strictEqual(canDuo(r('DIAMOND', 'IV'), 'PLATINUM', 'I'), false);
// Esmeralda: Platino y Esmeralda; Diamante IV solo con Esmeralda I.
assert.strictEqual(canDuo(r('DIAMOND', 'IV'), 'EMERALD', 'I'), true);
assert.strictEqual(canDuo(r('DIAMOND', 'IV'), 'EMERALD', 'II'), false);
// Diamante: ±2 divisiones entre diamantes.
assert.strictEqual(canDuo(r('DIAMOND', 'IV'), 'DIAMOND', 'II'), true);
assert.strictEqual(canDuo(r('DIAMOND', 'I'), 'DIAMOND', 'IV'), false, 'más de 2 divisiones');
assert.strictEqual(canDuo(r('MASTER', ''), 'DIAMOND', 'I'), true);
// Maestro: solo Maestro o Diamante I.
assert.strictEqual(canDuo(r('DIAMOND', 'I'), 'MASTER', ''), true);
assert.strictEqual(canDuo(r('DIAMOND', 'II'), 'MASTER', ''), false);
assert.strictEqual(canDuo(r('MASTER', ''), 'MASTER', ''), true);
// Hierro: Hierro, Bronce, Plata.
assert.strictEqual(canDuo(r('SILVER', 'IV'), 'IRON', 'II'), true);
assert.strictEqual(canDuo(r('GOLD', 'IV'), 'IRON', 'II'), false);

console.log('duo OK');
