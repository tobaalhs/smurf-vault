// Datos públicos del juego que la interfaz necesita para el historial: nombres de campeones e imágenes
// de minileyendas (CommunityDragon) y la versión actual de Data Dragon (para los íconos de ítems).
// Se guardan en un caché local y se renuevan una vez al día; sin internet se usa el caché.
const fs = require('fs');

const CHAMPIONS_URL =
  'https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/default/v1/champion-summary.json';
const VERSIONS_URL = 'https://ddragon.leagueoflegends.com/api/versions.json';
const COMPANIONS_URL = 'https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/default/v1/companions.json';
// Hechizos de invocador con nombre en español (para el control desde el celular).
const SPELLS_URL = 'https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/es_mx/v1/summoner-spells.json';
const CDRAGON_ASSETS = 'https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/default/';
const MAX_AGE_MS = 24 * 60 * 60 * 1000;

async function getJson(url) {
  const res = await fetch(url, { signal: AbortSignal.timeout(10_000) });
  if (!res.ok) throw new Error(`${url}: ${res.status}`);
  return res.json();
}

class GameData {
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
    const [champions, versions, companionList, spellList] = await Promise.all([
      getJson(CHAMPIONS_URL),
      getJson(VERSIONS_URL),
      getJson(COMPANIONS_URL),
      getJson(SPELLS_URL),
    ]);
    const names = {};
    for (const c of champions) if (c.id > 0) names[c.id] = c.name;
    // contentId de la minileyenda -> ruta de su imagen dentro de CommunityDragon.
    const companions = {};
    for (const c of companionList) {
      if (c.contentId && c.loadoutsIcon) companions[c.contentId] = c.loadoutsIcon.replace('/lol-game-data/assets/', '').toLowerCase();
    }
    // Solo los hechizos de Grieta y ARAM (el resto son de modos temporales).
    const spells = spellList
      .filter((sp) => sp.id > 0 && sp.gameModes?.some((m) => m === 'CLASSIC' || m === 'ARAM'))
      .map((sp) => ({
        id: sp.id,
        name: sp.name,
        icon: CDRAGON_ASSETS + sp.iconPath.replace('/lol-game-data/assets/', '').toLowerCase(),
        modes: sp.gameModes.filter((m) => m === 'CLASSIC' || m === 'ARAM'),
      }));
    const data = { champions: names, companions, spells, ddragon: versions[0], fetchedAt: Date.now() };
    fs.writeFileSync(this.cachePath, JSON.stringify(data));
    return data;
  }

  /** { champions: { id: nombre }, ddragon: '14.x.1' } o null si nunca se pudo descargar. */
  async get() {
    if (this.data && Date.now() - this.data.fetchedAt < MAX_AGE_MS) return this.data;
    if (!this.loading) {
      const cached = this.readCache();
      // Un caché de una versión anterior (sin minileyendas o sin hechizos) se renueva al tiro.
      const fresh = cached?.companions && cached?.spells && Date.now() - cached.fetchedAt < MAX_AGE_MS;
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

module.exports = { GameData };
