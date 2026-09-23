// Cifrado de la bóveda: scrypt (contraseña maestra -> clave) + AES-256-GCM.
// El "sobre" (envelope) es JSON con todo lo necesario para descifrar menos la contraseña.
const crypto = require('crypto');

const FORMAT = 'smurf-vault';
const SCRYPT = { N: 2 ** 17, r: 8, p: 1, keyLen: 32 };

function deriveKey(password, salt, params = SCRYPT) {
  return new Promise((resolve, reject) => {
    crypto.scrypt(
      password,
      salt,
      params.keyLen,
      { N: params.N, r: params.r, p: params.p, maxmem: 256 * 1024 * 1024 },
      (err, key) => (err ? reject(err) : resolve(key))
    );
  });
}

function encryptWithKey(key, salt, data) {
  const iv = crypto.randomBytes(12);
  const cipher = crypto.createCipheriv('aes-256-gcm', key, iv);
  const plain = Buffer.from(JSON.stringify(data), 'utf8');
  const enc = Buffer.concat([cipher.update(plain), cipher.final()]);
  return {
    format: FORMAT,
    v: 1,
    kdf: { name: 'scrypt', N: SCRYPT.N, r: SCRYPT.r, p: SCRYPT.p, salt: salt.toString('base64') },
    iv: iv.toString('base64'),
    tag: cipher.getAuthTag().toString('base64'),
    data: enc.toString('base64'),
    updatedAt: new Date().toISOString(),
  };
}

async function decryptEnvelope(envelope, password) {
  if (!envelope || envelope.format !== FORMAT) throw new Error('Archivo de bóveda inválido');
  const salt = Buffer.from(envelope.kdf.salt, 'base64');
  const key = await deriveKey(password, salt, { ...envelope.kdf, keyLen: 32 });
  try {
    const decipher = crypto.createDecipheriv('aes-256-gcm', key, Buffer.from(envelope.iv, 'base64'));
    decipher.setAuthTag(Buffer.from(envelope.tag, 'base64'));
    const plain = Buffer.concat([decipher.update(Buffer.from(envelope.data, 'base64')), decipher.final()]);
    return { key, salt, data: JSON.parse(plain.toString('utf8')) };
  } catch {
    throw new Error('Contraseña maestra incorrecta');
  }
}

async function createKey(password) {
  const salt = crypto.randomBytes(16);
  const key = await deriveKey(password, salt);
  return { key, salt };
}

function emptyVault() {
  return { accounts: [], settings: { riotApiKey: '' } };
}

module.exports = { createKey, encryptWithKey, decryptEnvelope, emptyVault };
