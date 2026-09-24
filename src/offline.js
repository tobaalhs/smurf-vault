// Aparecer desconectado en el chat del LoL: pone el estado "desconectado" del propio cliente y lo
// mantiene, porque el cliente vuelve a ponerte en línea al reiniciarse o cambiar de cuenta.
// Al apagarlo, vuelves a quedar en línea.
const lcu = require('./lcu');

const CHECK_EVERY_MS = 8000;

class OfflineMode {
  constructor({ client = lcu } = {}) {
    this.client = client;
    this.enabled = false;
    this.timer = null;
  }

  configure({ enabled }) {
    const was = this.enabled;
    this.enabled = !!enabled;
    clearInterval(this.timer);
    this.timer = null;
    if (this.enabled) {
      this.timer = setInterval(() => this.tick().catch(() => {}), CHECK_EVERY_MS);
      this.tick().catch(() => {});
    } else if (was) {
      this.setAvailability('chat').catch(() => {});
    }
  }

  /** Si el cliente está abierto y no apareces desconectado, lo corrige. */
  async tick() {
    if (!this.enabled) return;
    const me = await this.client.get('/lol-chat/v1/me');
    if (me && me.availability !== 'offline') await this.setAvailability('offline');
  }

  setAvailability(availability) {
    return this.client.request('PUT', '/lol-chat/v1/me', {}, { availability });
  }
}

module.exports = { OfflineMode };
