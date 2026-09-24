// Maestría de campeones: el puntaje total y los campeones con más puntos. Viene del cliente o de la
// API de Riot con el mismo formato (championId, championLevel, championPoints).
const TOP = 10;

function masteryFrom(list, score) {
  if (!Array.isArray(list)) return undefined;
  const top = [...list]
    .sort((a, b) => (b.championPoints || 0) - (a.championPoints || 0))
    .slice(0, TOP)
    .map((m) => ({ champ: m.championId, level: m.championLevel ?? 0, points: m.championPoints ?? 0 }));
  const total = Number.isFinite(Number(score)) ? Number(score) : list.reduce((s, m) => s + (m.championLevel || 0), 0);
  return { score: total, top };
}

module.exports = { masteryFrom, TOP };
