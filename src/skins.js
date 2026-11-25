// Catálogo de skins de LoL para el cambiador de fondo de perfil. Una sola descarga de CommunityDragon
// (skins.json) trae todas las skins con su id (= backgroundSkinId), su nombre y su miniatura.
// Se guarda en un caché local y se renueva cada semana; sin internet se usa el caché.
const fs = require('fs');

// Nombres en español; si falla, en inglés (default).
const SKINS_URL_ES = 'https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/es_mx/v1/skins.json';
const SKINS_URL_EN = 'https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/default/v1/skins.json';
const CDRAGON = 'https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/default/';
const MAX_AGE_MS = 7 * 24 * 60 * 60 * 1000; // las skins cambian poco

async function getJson(url) {
  const res = await fetch(url, { signal: AbortSignal.timeout(20_000) });
  if (!res.ok) throw new Error(`${url}: ${res.status}`);
  return res.json();
}

// Las rutas de CommunityDragon vienen como /lol-game-data/assets/...: se pasan a URL absoluta.
function assetUrl(p) {
  return p ? CDRAGON + p.replace('/lol-game-data/assets/', '').toLowerCase() : null;
}

class Skins {
  constructor(cachePath) {
    this.cachePath = cachePath;
    this.data = null;
    this.loading = null;
  }

  readCache() {
    try {
      return JSON.parse(fs.readFileSync(this.cachePath, 'utf8'));
    } catch {
      return null;
    }
  }

  async fetchFresh() {
    let raw;
    try {
      raw = await getJson(SKINS_URL_ES);
    } catch {
      raw = await getJson(SKINS_URL_EN);
    }
    const skins = [];
    for (const key of Object.keys(raw)) {
      const s = raw[key];
      if (!s || !Number.isFinite(s.id)) continue;
      // id = championKey*1000 + número de skin (así lo interpreta el cliente).
      skins.push({ id: s.id, name: s.name || '', champ: Math.floor(s.id / 1000), tile: assetUrl(s.tilePath) });
    }
    skins.sort((a, b) => a.id - b.id);
    const data = { skins, fetchedAt: Date.now() };
    try {
      fs.writeFileSync(this.cachePath, JSON.stringify(data));
    } catch {}
    return data;
  }

  /** { skins: [{ id, name, champ, tile }], fetchedAt } o el caché si no se pudo descargar. */
  async get() {
    if (this.data && Date.now() - this.data.fetchedAt < MAX_AGE_MS) return this.data;
    if (!this.loading) {
      const cached = this.readCache();
      const fresh = cached?.skins?.length && Date.now() - cached.fetchedAt < MAX_AGE_MS;
      this.loading = (fresh ? Promise.resolve(cached) : this.fetchFresh())
        .catch(() => cached)
        .then((d) => {
          this.data = d;
          this.loading = null;
          return d;
        });
    }
    return this.loading;
  }
}

module.exports = { Skins };
