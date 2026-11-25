// Acepta automáticamente la partida cuando aparece el "ready check" en el cliente.
// Hace lo mismo que apretar "Aceptar": no toca nada dentro de la partida.
// Revisa todos los clientes de juego abiertos (LoL y TFT), porque TFT tiene su propio cliente
// (TFTClient.exe) con su propia API local; así el autoaceptar también funciona en TFT.
const lcu = require('./lcu');

const READY_CHECK = '/lol-matchmaking/v1/ready-check';
const POLL_MS = 1000;
const MAX_DELAY_S = 8; // el ready check dura ~10 s

function isWaiting(json) {
  return json?.state === 'InProgress' && json.playerResponse === 'None';
}

class AutoAccept {
  constructor({ onAccepted, onStatus }) {
    this.onAccepted = onAccepted;
    this.onStatus = onStatus;
    this.enabled = false;
    this.delay = 0;
    this.timer = null;
    this.pending = null; // { conn, timeout } del ready check que ya estamos esperando para aceptar
    this.connected = null;
  }

  configure({ enabled, delay }) {
    this.delay = Math.min(Math.max(Number(delay) || 0, 0), MAX_DELAY_S);
    if (enabled && !this.enabled) {
      this.enabled = true;
      this.timer = setInterval(() => this.tick().catch(() => {}), POLL_MS);
      this.tick().catch(() => {});
    } else if (!enabled && this.enabled) {
      this.enabled = false;
      clearInterval(this.timer);
      clearTimeout(this.pending?.timeout);
      this.pending = null;
      this.#setConnected(null);
    }
  }

  #setConnected(value) {
    if (value === this.connected) return;
    this.connected = value;
    this.onStatus?.({ connected: value });
  }

  // Lee el ready check de una conexión concreta; null si no responde o no hay partida.
  async #readyCheckOn(conn) {
    try {
      const res = await lcu.rawRequest(conn, 'GET', READY_CHECK);
      // 404 = no hay partida encontrada en este momento (pero el cliente sí respondió).
      return res.status === 200 ? res.json : null;
    } catch {
      return null; // cliente cerrado o lockfile viejo
    }
  }

  // Busca un ready check pendiente en cualquier cliente abierto (LoL o TFT). Devuelve su conexión.
  async #pendingClient() {
    let connected = false;
    for (const conn of lcu.gameClients()) {
      let res;
      try {
        res = await lcu.rawRequest(conn, 'GET', READY_CHECK);
      } catch {
        continue; // este cliente no respondió
      }
      connected = true;
      if (res.status === 200 && isWaiting(res.json)) {
        this.#setConnected(true);
        return conn;
      }
    }
    this.#setConnected(connected);
    return null;
  }

  async tick() {
    const conn = await this.#pendingClient();
    if (!conn) {
      clearTimeout(this.pending?.timeout);
      this.pending = null;
      return;
    }
    if (this.pending) return;

    this.pending = {
      conn,
      timeout: setTimeout(async () => {
        // Revisamos de nuevo en ese mismo cliente: puede que la hayas rechazado o aceptado a mano.
        if (this.enabled && isWaiting(await this.#readyCheckOn(conn))) {
          try {
            const res = await lcu.rawRequest(conn, 'POST', `${READY_CHECK}/accept`);
            if (res.status < 400) this.onAccepted?.();
          } catch {}
        }
      }, this.delay * 1000),
    };
  }
}

module.exports = { AutoAccept };
