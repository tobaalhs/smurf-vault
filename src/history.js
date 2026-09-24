// Datos de cada cuenta que solo viven en este PC (no se suben a Drive, así la bóveda sincronizada queda
// chica) y van cifrados con la misma clave de la bóveda: el historial de partidas y la evolución de LP.
// Si se pierden, se vuelven a armar solos desde el cliente de LoL.
const fs = require('fs');
const { encryptWithKey, decryptWithKey } = require('./vault');
const { mergeMatches } = require('./matches');

/** Archivo cifrado con un objeto { idCuenta: datos }. */
class LocalStore {
  constructor(filePath) {
    this.filePath = filePath;
    this.byAccount = {};
  }

  /** Lee el archivo con la clave de la bóveda recién abierta. Si no existe o es de otra bóveda, parte vacío. */
  load(key) {
    try {
      this.byAccount = decryptWithKey(key, JSON.parse(fs.readFileSync(this.filePath, 'utf8'))) || {};
    } catch {
      this.byAccount = {};
    }
  }

  clear() {
    this.byAccount = {};
  }

  save(key, salt) {
    const tmp = this.filePath + '.tmp';
    fs.writeFileSync(tmp, JSON.stringify(encryptWithKey(key, salt, this.byAccount)));
    fs.renameSync(tmp, this.filePath);
  }

  get(id) {
    return this.byAccount[id];
  }

  /** Borra lo de una cuenta. Devuelve true si tenía algo. */
  remove(id) {
    if (!(id in this.byAccount)) return false;
    delete this.byAccount[id];
    return true;
  }
}

/** Últimas partidas de LoL y TFT de cada cuenta. */
class LocalHistory extends LocalStore {
  /** Junta partidas nuevas con las guardadas. Devuelve true si cambió algo. */
  add(id, matches) {
    const before = JSON.stringify(this.byAccount[id] || []);
    const merged = mergeMatches(this.byAccount[id], matches);
    if (JSON.stringify(merged) === before) return false;
    this.byAccount[id] = merged;
    return true;
  }
}

const LP_QUEUES = ['solo', 'flex', 'tft'];
const LP_KEEP = 300; // puntos por cola: sobra para una temporada completa

/** Evolución del rango: { solo: [{ at, tier, division, lp }], flex: [...], tft: [...] } por cuenta. */
class LpLog extends LocalStore {
  /** Anota los rangos si cambiaron desde el último punto. Devuelve true si anotó algo. */
  record(id, ranks, at = new Date().toISOString()) {
    let changed = false;
    for (const q of LP_QUEUES) {
      const r = ranks?.[q];
      if (!r?.tier) continue;
      const log = (this.byAccount[id] ||= {});
      const list = (log[q] ||= []);
      const last = list[list.length - 1];
      if (last && last.tier === r.tier && last.division === r.division && last.lp === r.lp) continue;
      list.push({ at, tier: r.tier, division: r.division || '', lp: r.lp ?? 0 });
      if (list.length > LP_KEEP) list.splice(0, list.length - LP_KEEP);
      changed = true;
    }
    return changed;
  }
}

module.exports = { LocalHistory, LpLog, LP_QUEUES };
