// Prueba rápida del cifrado: node test/vault.test.js
const assert = require('assert');
const { createKey, checkPassword, encryptWithKey, decryptEnvelope } = require('../src/vault');

(async () => {
  const data = { accounts: [{ username: 'smurf1', password: 'ñandú#123' }], settings: {} };
  const { key, salt } = await createKey('contraseña-maestra');
  const env = encryptWithKey(key, salt, data);

  assert.ok(!JSON.stringify(env).includes('smurf1'), 'el envelope no debe tener texto plano');
  const out = await decryptEnvelope(env, 'contraseña-maestra');
  assert.deepStrictEqual(out.data, data);

  await assert.rejects(decryptEnvelope(env, 'otra'), /incorrecta/);

  // Verificar la contraseña de la bóveda abierta (para cambiarla).
  assert.strictEqual(await checkPassword('contraseña-maestra', salt, key), true);
  assert.strictEqual(await checkPassword('otra', salt, key), false);

  const tampered = { ...env, data: Buffer.from('x' + env.data).toString('base64') };
  await assert.rejects(decryptEnvelope(tampered, 'contraseña-maestra'));

  console.log('vault OK');
})();
