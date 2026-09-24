// Prueba del historial local cifrado: node test/history.test.js
const assert = require('assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { LocalHistory, LpLog } = require('../src/history');
const { masteryFrom } = require('../src/mastery');
const { createKey } = require('../src/vault');

(async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'smurf-history-'));
  const file = path.join(dir, 'history.dat');
  try {
    const { key, salt } = await createKey('maestra');
    const h = new LocalHistory(file);
    const m = { id: 'lol-1', mode: 'lol', at: '2026-09-20T10:00:00.000Z', champ: 103 };
    assert.strictEqual(h.add('acc', [m]), true);
    assert.strictEqual(h.add('acc', [m]), false, 'la misma partida no cuenta como cambio');
    h.save(key, salt);
    assert.ok(!fs.readFileSync(file, 'utf8').includes('lol-1'), 'el archivo va cifrado');

    const again = new LocalHistory(file);
    again.load(key);
    assert.deepStrictEqual(again.get('acc'), [m]);

    const other = await createKey('otra');
    again.load(other.key);
    assert.strictEqual(again.get('acc'), undefined, 'con otra clave parte vacío');

    // Registro de LP: solo anota cuando el rango cambia.
    const lp = new LpLog(path.join(dir, 'lp.dat'));
    const d4 = { tier: 'DIAMOND', division: 'IV', lp: 10 };
    assert.strictEqual(lp.record('acc', { solo: d4, flex: null }, '2026-09-01T00:00:00Z'), true);
    assert.strictEqual(lp.record('acc', { solo: { ...d4 } }, '2026-09-02T00:00:00Z'), false, 'sin cambios no anota');
    assert.strictEqual(lp.record('acc', { solo: { ...d4, lp: 28 } }, '2026-09-03T00:00:00Z'), true);
    assert.deepStrictEqual(lp.get('acc').solo.map((p) => p.lp), [10, 28]);
    assert.strictEqual(lp.get('acc').flex, undefined, 'sin rango no se anota');

    // Maestrías: las de más puntos primero y el puntaje total.
    const mastery = masteryFrom([{ championId: 1, championLevel: 5, championPoints: 10 }, { championId: 2, championLevel: 9, championPoints: 99 }], 14);
    assert.deepStrictEqual(mastery, { score: 14, top: [{ champ: 2, level: 9, points: 99 }, { champ: 1, level: 5, points: 10 }] });
    assert.strictEqual(masteryFrom(null, 3), undefined);

    assert.strictEqual(h.remove('acc'), true);
    assert.strictEqual(h.remove('acc'), false);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
  console.log('history OK');
})();
