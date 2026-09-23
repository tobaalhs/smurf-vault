// Vinculación con Google (OAuth 2.0 para apps de escritorio, loopback + PKCE)
// y lectura/escritura de la bóveda en la carpeta oculta "appDataFolder" de Drive.
const http = require('http');
const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const { shell, safeStorage } = require('electron');

const SCOPES = 'https://www.googleapis.com/auth/drive.appdata openid email';
const VAULT_NAME = 'vault.dat';
const LOGIN_TIMEOUT_MS = 5 * 60 * 1000;

class Drive {
  constructor({ credentialsPath, tokenPath }) {
    this.credentialsPath = credentialsPath;
    this.tokenPath = tokenPath;
    this.accessToken = null;
    this.accessExpiresAt = 0;
    this.fileId = null;
    this.token = this.#loadToken();
  }

  // ---------- credenciales / token ----------

  hasCredentials() {
    return fs.existsSync(this.credentialsPath);
  }

  #client() {
    if (!this.hasCredentials()) {
      throw new Error('Falta credentials.json (revisa GOOGLE_SETUP.md)');
    }
    const raw = JSON.parse(fs.readFileSync(this.credentialsPath, 'utf8'));
    const c = raw.installed || raw.web || raw;
    if (!c.client_id) throw new Error('credentials.json no tiene client_id');
    return { id: c.client_id, secret: c.client_secret };
  }

  #loadToken() {
    try {
      if (!fs.existsSync(this.tokenPath)) return null;
      const buf = fs.readFileSync(this.tokenPath);
      return JSON.parse(safeStorage.decryptString(buf));
    } catch {
      return null;
    }
  }

  #saveToken(token) {
    this.token = token;
    if (!token) {
      fs.rmSync(this.tokenPath, { force: true });
      return;
    }
    // safeStorage usa DPAPI en Windows: solo tu usuario de Windows puede leerlo.
    fs.writeFileSync(this.tokenPath, safeStorage.encryptString(JSON.stringify(token)));
  }

  status() {
    return {
      hasCredentials: this.hasCredentials(),
      linked: !!this.token?.refresh_token,
      email: this.token?.email || null,
    };
  }

  // ---------- login ----------

  async link() {
    const client = this.#client();
    const verifier = crypto.randomBytes(32).toString('base64url');
    const challenge = crypto.createHash('sha256').update(verifier).digest('base64url');
    const state = crypto.randomBytes(16).toString('hex');

    const { code, redirectUri } = await new Promise((resolve, reject) => {
      const server = http.createServer((req, res) => {
        const url = new URL(req.url, 'http://127.0.0.1');
        if (url.pathname !== '/') {
          res.writeHead(404).end();
          return;
        }
        const err = url.searchParams.get('error');
        const ok = !err && url.searchParams.get('state') === state && url.searchParams.get('code');
        res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
        res.end(
          `<html><body style="font-family:sans-serif;background:#0b1015;color:#e8e2d0;text-align:center;padding-top:80px">
           <h2>${ok ? '✅ Cuenta de Google vinculada' : '❌ No se pudo vincular'}</h2>
           <p>Ya puedes cerrar esta pestaña y volver a Smurf Vault.</p></body></html>`
        );
        clearTimeout(timer);
        server.close();
        if (ok) resolve({ code: url.searchParams.get('code'), redirectUri });
        else reject(new Error(err ? `Google respondió: ${err}` : 'Respuesta inválida de Google'));
      });
      let redirectUri;
      const timer = setTimeout(() => {
        server.close();
        reject(new Error('Se acabó el tiempo para iniciar sesión'));
      }, LOGIN_TIMEOUT_MS);
      server.listen(0, '127.0.0.1', () => {
        redirectUri = `http://127.0.0.1:${server.address().port}`;
        const auth = new URL('https://accounts.google.com/o/oauth2/v2/auth');
        auth.search = new URLSearchParams({
          client_id: client.id,
          redirect_uri: redirectUri,
          response_type: 'code',
          scope: SCOPES,
          code_challenge: challenge,
          code_challenge_method: 'S256',
          state,
          access_type: 'offline',
          prompt: 'consent',
        }).toString();
        shell.openExternal(auth.toString());
      });
    });

    const tok = await this.#tokenRequest({
      grant_type: 'authorization_code',
      code,
      redirect_uri: redirectUri,
      code_verifier: verifier,
      client_id: client.id,
      client_secret: client.secret,
    });
    if (!tok.refresh_token) throw new Error('Google no entregó refresh_token');

    let email = null;
    if (tok.id_token) {
      try {
        email = JSON.parse(Buffer.from(tok.id_token.split('.')[1], 'base64url').toString()).email;
      } catch {}
    }
    this.#useAccess(tok);
    this.fileId = null;
    this.#saveToken({ refresh_token: tok.refresh_token, email });
    return this.status();
  }

  async unlink() {
    const refresh = this.token?.refresh_token;
    this.#saveToken(null);
    this.accessToken = null;
    this.fileId = null;
    if (refresh) {
      await fetch(`https://oauth2.googleapis.com/revoke?token=${encodeURIComponent(refresh)}`, {
        method: 'POST',
      }).catch(() => {});
    }
    return this.status();
  }

  async #tokenRequest(params) {
    const res = await fetch('https://oauth2.googleapis.com/token', {
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
      body: new URLSearchParams(params),
    });
    const json = await res.json();
    if (!res.ok) {
      if (json.error === 'invalid_grant') {
        // Token revocado o caducado (típico si la app sigue en modo "Testing": dura 7 días).
        this.#saveToken(null);
        throw new Error('La vinculación con Google caducó, vuelve a vincular la cuenta');
      }
      throw new Error(`Error de Google: ${json.error_description || json.error}`);
    }
    return json;
  }

  #useAccess(tok) {
    this.accessToken = tok.access_token;
    this.accessExpiresAt = Date.now() + (tok.expires_in - 60) * 1000;
  }

  async #access() {
    if (!this.token?.refresh_token) throw new Error('Google no está vinculado');
    if (this.accessToken && Date.now() < this.accessExpiresAt) return this.accessToken;
    const client = this.#client();
    const tok = await this.#tokenRequest({
      grant_type: 'refresh_token',
      refresh_token: this.token.refresh_token,
      client_id: client.id,
      client_secret: client.secret,
    });
    this.#useAccess(tok);
    return this.accessToken;
  }

  async #api(url, opts = {}) {
    const token = await this.#access();
    const res = await fetch(url, {
      ...opts,
      headers: { ...(opts.headers || {}), Authorization: `Bearer ${token}` },
    });
    if (!res.ok) {
      const text = await res.text();
      throw new Error(`Drive ${res.status}: ${text.slice(0, 200)}`);
    }
    return res;
  }

  // ---------- archivo de la bóveda ----------

  async #findFile() {
    if (this.fileId) return this.fileId;
    const q = encodeURIComponent(`name='${VAULT_NAME}'`);
    const res = await this.#api(
      `https://www.googleapis.com/drive/v3/files?spaces=appDataFolder&q=${q}&fields=files(id,modifiedTime)&orderBy=modifiedTime desc`
    );
    const { files } = await res.json();
    this.fileId = files[0]?.id || null;
    return this.fileId;
  }

  /** Devuelve el envelope guardado en Drive, o null si no existe. */
  async download() {
    const id = await this.#findFile();
    if (!id) return null;
    const res = await this.#api(`https://www.googleapis.com/drive/v3/files/${id}?alt=media`);
    return JSON.parse(await res.text());
  }

  async upload(envelope) {
    const body = JSON.stringify(envelope);
    const id = await this.#findFile();
    if (id) {
      await this.#api(`https://www.googleapis.com/upload/drive/v3/files/${id}?uploadType=media`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body,
      });
      return;
    }
    const boundary = 'smurf-vault' + crypto.randomBytes(8).toString('hex');
    const multipart =
      `--${boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n` +
      JSON.stringify({ name: VAULT_NAME, parents: ['appDataFolder'] }) +
      `\r\n--${boundary}\r\nContent-Type: application/json\r\n\r\n` +
      body +
      `\r\n--${boundary}--`;
    const res = await this.#api('https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart&fields=id', {
      method: 'POST',
      headers: { 'Content-Type': `multipart/related; boundary=${boundary}` },
      body: multipart,
    });
    this.fileId = (await res.json()).id;
  }
}

module.exports = { Drive };
