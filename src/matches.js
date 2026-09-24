// Historial de partidas: convierte lo que entrega el cliente (LCU) a un formato chico que se guarda
// en la bóveda, y mezcla lo nuevo con lo guardado.
const KEEP = 20;

/** Partida de LoL del endpoint current-summoner/matches (solo trae al jugador actual). */
function fromLolGame(g) {
  const p = g?.participants?.[0];
  const s = p?.stats;
  if (!g?.gameId || !s) return null;
  const dur = Number(g.gameDuration) || 0;
  return {
    id: `lol-${g.gameId}`,
    mode: 'lol',
    at: new Date((Number(g.gameCreation) || 0) + dur * 1000).toISOString(),
    dur,
    queueId: g.queueId ?? null,
    champ: p.championId ?? null,
    win: !!s.win,
    remake: dur > 0 && dur < 300, // menos de 5 min: no cuenta como victoria ni derrota
    k: s.kills ?? 0,
    d: s.deaths ?? 0,
    a: s.assists ?? 0,
    cs: (s.totalMinionsKilled ?? 0) + (s.neutralMinionsKilled ?? 0),
    lvl: s.champLevel ?? null,
    items: [0, 1, 2, 3, 4, 5, 6].map((i) => s[`item${i}`] || 0),
  };
}

/** Partida de TFT (formato de la API de match-v1 de TFT, dentro de `json`). */
function fromTftGame(g, puuid) {
  const j = g?.json;
  const me = j?.participants?.find((p) => p.puuid === puuid);
  if (!j?.game_datetime || !me) return null;
  const id = g.metadata?.match_id || `${j.game_datetime}`;
  return {
    id: `tft-${id}`,
    mode: 'tft',
    at: new Date(Number(j.game_datetime) + (Number(j.game_length) || 0) * 1000).toISOString(),
    dur: Math.round(Number(j.game_length) || 0),
    queueId: j.queue_id ?? null,
    placement: me.placement ?? null,
    win: (me.placement ?? 9) <= 4,
    lvl: me.level ?? null,
    companion: me.companion?.content_ID || null, // minileyenda usada en la partida
  };
}

/**
 * Junta partidas nuevas con las guardadas, sin repetir, y deja las `KEEP` más recientes de cada modo
 * (LoL y TFT por separado, para que jugar mucho TFT no borre el historial de LoL).
 */
function mergeMatches(saved = [], fresh = []) {
  const byId = new Map(saved.map((m) => [m.id, m]));
  for (const m of fresh) if (m) byId.set(m.id, m);
  const all = [...byId.values()].sort((a, b) => b.at.localeCompare(a.at));
  return ['lol', 'tft'].flatMap((mode) => all.filter((m) => m.mode === mode).slice(0, KEEP)).sort((a, b) => b.at.localeCompare(a.at));
}

module.exports = { fromLolGame, fromTftGame, mergeMatches, KEEP };
