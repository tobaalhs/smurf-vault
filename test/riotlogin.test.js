// Pruebas de las funciones puras del inicio de sesión por inyección (el límite de confianza):
// cómo se interpreta el estado de la sesión del cliente y qué URL de depuración se acepta.
const assert = require('assert');
const { sessionState, validDebuggerUrl } = require('../src/riotlogin');

// --- estado de la sesión (/player-session-lifecycle/v1/session) ---

// Con identidad (puuid) siempre es "con sesión", aunque el loginState sea transitorio.
assert.strictEqual(sessionState({ status: 200, json: { loginState: 'PendingLoginStrategy', puuid: 'player' } }), 'signedIn');
assert.strictEqual(sessionState({ status: 200, json: { loginState: 'PendingProductContext', puuid: 'player' } }), 'signedIn');

// Sin identidad y en PendingLoginStrategy: pantalla de login.
assert.strictEqual(sessionState({ status: 200, json: { loginState: 'PendingLoginStrategy', puuid: '' } }), 'signedOut');

// Un paso interactivo (MFA/captcha) en curso.
assert.strictEqual(sessionState({ status: 200, json: { loginState: 'PendingAuthentication', actionRequired: true } }), 'action');

// Arrancando o sin respuesta válida: desconocido (no se puede escribir todavía).
for (const res of [null, { status: 404, json: {} }, { status: 503, json: {} }, { status: 200, json: {} }, { status: 200, json: { loginState: 'RestoreAuthorization', puuid: '' } }]) {
  assert.strictEqual(sessionState(res), 'unknown');
}

// --- URL de depuración (CDP): solo loopback, el puerto que abrimos y una página ---

assert.ok(validDebuggerUrl('ws://127.0.0.1:12345/devtools/page/abc', 12345));
for (const url of [
  'ws://example.com:12345/devtools/page/abc',
  'ws://127.0.0.1:23456/devtools/page/abc',
  'ws://user:secret@127.0.0.1:12345/devtools/page/abc',
  'ws://127.0.0.1:12345/devtools/browser/abc',
  'ws://127.0.0.1:12345/devtools/page/abc?token=secret',
  'http://127.0.0.1:12345/devtools/page/abc',
]) {
  assert.ok(!validDebuggerUrl(url, 12345), url);
}

console.log('riotlogin OK');
