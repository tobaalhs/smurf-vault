// Vigila el estado del cliente (gameflow) para saber cuándo termina una partida de LoL o TFT.
const lcu = require('./lcu');

const POLL_MS = 5000;
const PLAYING = new Set(['InProgress', 'Reconnect']);

class GameflowWatcher {
  constructor({ onGameEnd }) {
    this.onGameEnd = onGameEnd;
    this.phase = null;
    this.timer = null;
  }

  start() {
    this.timer = setInterval(() => this.tick().catch(() => {}), POLL_MS);
    this.tick().catch(() => {});
  }

  async tick() {
    const phase = await lcu.get('/lol-gameflow/v1/gameflow-phase');
    const prev = this.phase;
    this.phase = typeof phase === 'string' ? phase : null;
    // Estaba jugando y ahora el cliente está en otra fase (estadísticas, fin de partida, lobby…).
    // Si el cliente se cerró (phase null) no contamos nada.
    if (PLAYING.has(prev) && this.phase && !PLAYING.has(this.phase)) this.onGameEnd();
  }
}

module.exports = { GameflowWatcher };
