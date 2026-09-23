// Acepta automáticamente la partida cuando aparece el "ready check" en el cliente.
// Hace lo mismo que apretar "Aceptar": no toca nada dentro de la partida.
const lcu = require('./lcu');

const POLL_MS = 1000;
const MAX_DELAY_S = 8; // el ready check dura ~10 s

class AutoAccept {
  constructor({ onAccepted, onStatus }) {
    this.onAccepted = onAccepted;
    this.onStatus = onStatus;
    this.enabled = false;
    this.delay = 0;
    this.timer = null;
    this.pending = null; // ready check que ya estamos esperando para aceptar
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

  async #readyCheck() {
    const res = await lcu.request('GET', '/lol-matchmaking/v1/ready-check');
    this.#setConnected(!!res);
    // 404 = no hay partida encontrada en este momento.
    return res && res.status === 200 ? res.json : null;
  }

  async tick() {
    const rc = await this.#readyCheck();
    const waiting = rc?.state === 'InProgress' && rc.playerResponse === 'None';
    if (!waiting) {
      clearTimeout(this.pending?.timeout);
      this.pending = null;
      return;
    }
    if (this.pending) return;

    this.pending = {
      timeout: setTimeout(async () => {
        // Revisamos de nuevo: puede que la hayas rechazado o aceptado a mano mientras tanto.
        const now = await this.#readyCheck();
        if (this.enabled && now?.state === 'InProgress' && now.playerResponse === 'None') {
          const res = await lcu.request('POST', '/lol-matchmaking/v1/ready-check/accept');
          if (res && res.status < 400) this.onAccepted?.();
        }
      }, this.delay * 1000),
    };
  }
}

module.exports = { AutoAccept };
