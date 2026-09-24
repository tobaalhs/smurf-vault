// Prueba del guardado/restaurado de sesiones con una carpeta temporal: node test/switcher.test.js
const assert = require('assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { readSession, writeSession, isRemembered, sessionPaths } = require('../src/switcher');
const { createKey, encryptWithKey, decryptWithKey } = require('../src/vault');

(async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'smurf-switcher-'));
  const [rc, lol] = sessionPaths().map((rel) => path.join(root, rel));
  try {
    assert.deepStrictEqual(readSession(root), {}, 'sin archivos no hay sesión');

    // Cuenta A con "Mantener sesión iniciada".
    fs.mkdirSync(path.dirname(lol), { recursive: true });
    fs.writeFileSync(lol, 'cookies:\n  - name: "ssid"\n    value: "cuenta-a"\n');
    const a = readSession(root);
    assert.ok(isRemembered(a));
    assert.ok(!isRemembered({ x: Buffer.from('name: "tdid"').toString('base64') }));
    // Formato actual del Riot Client: refresh_token en psl.authorization.
    const b64 = (s) => Buffer.from(s).toString('base64');
    assert.ok(isRemembered({ x: b64('psl:\n    authorization:\n        riot-client:\n            refresh_token: "abc"\n') }));
    assert.ok(!isRemembered({ x: b64('psl:\n    authorization:\n        riot-client:\n            refresh_token:\n            scopes: []\n') }));

    // Cuenta B guardada en la otra carpeta; al restaurar A se borra la de B.
    fs.mkdirSync(path.dirname(rc), { recursive: true });
    fs.writeFileSync(rc, 'cuenta-b');
    writeSession(a, root);
    assert.ok(!fs.existsSync(rc));
    assert.ok(fs.readFileSync(lol, 'utf8').includes('cuenta-a'));

    // Sin sesión: se borra todo para que el cliente muestre el login.
    writeSession(null, root);
    assert.deepStrictEqual(readSession(root), {});

    // Lo guardado va cifrado con la clave de la bóveda.
    const { key, salt } = await createKey('maestra');
    const env = encryptWithKey(key, salt, { files: a });
    assert.ok(!JSON.stringify(env).includes('cuenta-a'));
    assert.deepStrictEqual(decryptWithKey(key, env).files, a);
    const other = await createKey('otra');
    assert.throws(() => decryptWithKey(other.key, env));
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
  console.log('switcher OK');
})();
