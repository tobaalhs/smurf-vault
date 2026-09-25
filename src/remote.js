// Control desde el celular: un servidor HTTP chico en la red local que sirve la página del celular
// (carpeta remote/) y una API para ver la cola/selección de campeones y dar órdenes.
// Solo responde a quien tenga el código secreto (va en el QR), y solo consulta el cliente de LoL
// mientras hay un celular conectado.
const http = require('http');
const fs = require('fs');
const os = require('os');
const path = require('path');
const crypto = require('crypto');
const QRCode = require('qrcode');

const PORTS = [43210, 43211, 43212, 43213, 43214];
// Únicos archivos que se sirven (rutas relativas a la carpeta de la app).
const STATIC = {
  '/': ['remote/index.html', 'text/html; charset=utf-8'],
  '/remote.css': ['remote/remote.css', 'text/css; charset=utf-8'],
  '/remote.js': ['remote/remote.js', 'text/javascript; charset=utf-8'],
  '/icon.png': ['renderer/assets/logo.png', 'image/png'],
};
const CSP =
  "default-src 'self'; img-src 'self' data: https://raw.communitydragon.org https://cdn.communitydragon.org; " +
  "style-src 'self'; script-src 'self'; connect-src 'self'";
const MAX_BODY = 10_000;

function newToken() {
  return crypto.randomBytes(18).toString('base64url');
}

/** IPv4 de la red local, primero las de adaptadores reales (no VPN ni máquinas virtuales). */
function lanAddresses() {
  const virtual = /vethernet|virtualbox|vmware|wsl|hyper-v|loopback|bluetooth|tailscale|zerotier|hamachi/i;
  const out = [];
  for (const [name, list] of Object.entries(os.networkInterfaces())) {
    for (const a of list || []) {
      if (a.family !== 'IPv4' || a.internal) continue;
      const priv = /^(10\.|192\.168\.|172\.(1[6-9]|2\d|3[01])\.)/.test(a.address);
      out.push({ address: a.address, score: (virtual.test(name) ? 0 : 2) + (priv ? 1 : 0) });
    }
  }
  return out.sort((a, b) => b.score - a.score).map((a) => a.address);
}

class RemoteServer {
  constructor({ champSelect, gameData, appDir }) {
    this.champSelect = champSelect;
    this.gameData = gameData;
    this.appDir = appDir;
    this.server = null;
    this.port = null;
    this.token = null;
    this.clients = new Set();
    this.lastState = '';
    this.pollTimer = null;
    this.polling = false;
    this.pollAgain = false;
  }

  get running() {
    return !!this.server;
  }

  async start(token) {
    this.token = token;
    if (this.server) return;
    for (const port of PORTS) {
      try {
        await new Promise((resolve, reject) => {
          const srv = http.createServer((req, res) => this.handle(req, res).catch(() => this.reply(res, 500, { error: 'Error interno' })));
          srv.once('error', reject);
          srv.listen(port, '0.0.0.0', () => {
            this.server = srv;
            this.port = port;
            resolve();
          });
        });
        return;
      } catch (e) {
        if (e.code !== 'EADDRINUSE') throw e;
      }
    }
    throw new Error('No hay un puerto libre para el control desde el celular');
  }

  stop() {
    for (const c of this.clients) c.end();
    this.clients.clear();
    clearTimeout(this.pollTimer);
    this.pollTimer = null;
    this.server?.close();
    this.server = null;
  }

  /** Cambia el código: los celulares conectados con el anterior quedan afuera. */
  setToken(token) {
    this.token = token;
    for (const c of this.clients) c.end();
    this.clients.clear();
  }

  async status() {
    if (!this.server) return { running: false };
    const [ip] = lanAddresses();
    const url = ip ? `http://${ip}:${this.port}/?t=${this.token}` : null;
    return {
      running: true,
      url,
      address: ip ? `${ip}:${this.port}` : null,
      qr: url ? await QRCode.toString(url, { type: 'svg', margin: 1, color: { dark: '#0b0f15', light: '#ffffff' } }) : null,
      clients: this.clients.size,
    };
  }

  authorized(req, url) {
    const given = req.headers['x-token'] || url.searchParams.get('t') || '';
    const a = Buffer.from(String(given));
    const b = Buffer.from(this.token || '');
    return a.length === b.length && b.length > 0 && crypto.timingSafeEqual(a, b);
  }

  reply(res, status, body) {
    res.writeHead(status, { 'Content-Type': 'application/json; charset=utf-8', 'Cache-Control': 'no-store' });
    res.end(JSON.stringify(body));
  }

  async handle(req, res) {
    const url = new URL(req.url, 'http://local');
    const file = STATIC[url.pathname];
    if (req.method === 'GET' && file) {
      const [name, type] = file;
      res.writeHead(200, { 'Content-Type': type, 'Content-Security-Policy': CSP, 'Cache-Control': 'no-cache' });
      return fs.createReadStream(path.join(this.appDir, name)).pipe(res);
    }
    if (!url.pathname.startsWith('/api/')) return this.reply(res, 404, { error: 'No existe' });
    if (!this.authorized(req, url)) return this.reply(res, 401, { error: 'Código inválido: escanea el QR de nuevo' });

    if (req.method === 'GET' && url.pathname === '/api/static') {
      const data = (await this.gameData.get()) || {};
      return this.reply(res, 200, { champions: data.champions || {}, spells: data.spells || [] });
    }
    if (req.method === 'GET' && url.pathname === '/api/events') return this.subscribe(req, res);
    if (req.method === 'POST' && url.pathname === '/api/command') {
      const cmd = await readJson(req);
      try {
        await this.champSelect.run(cmd);
        this.reply(res, 200, { ok: true });
      } catch (e) {
        this.reply(res, 400, { error: e.message });
      }
      return this.poll(true); // que el celular vea el resultado al tiro
    }
    return this.reply(res, 404, { error: 'No existe' });
  }

  /** Estado en vivo (Server-Sent Events). */
  subscribe(req, res) {
    res.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-store', Connection: 'keep-alive' });
    res.write('retry: 2000\n\n');
    this.clients.add(res);
    const ping = setInterval(() => res.write(': ping\n\n'), 15_000);
    req.on('close', () => {
      clearInterval(ping);
      this.clients.delete(res);
    });
    if (this.lastState) res.write(message(this.lastState));
    this.poll(true);
  }

  /**
   * Consulta el cliente y avisa a los celulares si algo cambió. Más seguido en cola y selección.
   * Una sola consulta a la vez: si llega otra mientras corre (una orden del celular), se repite al final.
   */
  async poll(force = false) {
    if (this.polling) {
      this.pollAgain = true;
      return;
    }
    this.polling = true;
    clearTimeout(this.pollTimer);
    this.pollTimer = null;
    let state = { phase: 'Offline' };
    try {
      if (!this.clients.size) return;
      state = await this.champSelect.state().catch(() => ({ phase: 'Offline' }));
      const json = JSON.stringify(state);
      if (json !== this.lastState || force) {
        this.lastState = json;
        for (const c of this.clients) c.write(message(json));
      }
    } finally {
      this.polling = false;
    }
    if (this.pollAgain) {
      this.pollAgain = false;
      return this.poll(true);
    }
    const fast = ['ReadyCheck', 'ChampSelect', 'Matchmaking'].includes(state.phase);
    if (this.clients.size) this.pollTimer = setTimeout(() => this.poll(), fast ? 1000 : 3000);
  }
}

/** Evento SSE con el estado y la hora del PC (para que el celular calcule los tiempos con su reloj). */
function message(stateJson) {
  return `data: {"now":${Date.now()},"state":${stateJson}}\n\n`;
}

function readJson(req) {
  return new Promise((resolve) => {
    let body = '';
    req.on('data', (c) => {
      body += c;
      if (body.length > MAX_BODY) req.destroy();
    });
    req.on('end', () => {
      try {
        resolve(JSON.parse(body || '{}'));
      } catch {
        resolve({});
      }
    });
  });
}

module.exports = { RemoteServer, newToken, lanAddresses };
