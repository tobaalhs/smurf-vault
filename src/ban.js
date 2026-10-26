// Baneo de League leído de la sesión del cliente (/lol-login/v1/session). Las restricciones de chat,
// ranked o reputación dejan jugar; solo las de tipo baneo de juego impiden entrar a una partida.
// Técnica basada en League Accounts (MIT, https://github.com/Tariolle/LeagueAccounts).

// Tipos de restricción que impiden jugar (no chat/ranked/reputación).
const GAME_BANS = new Set(['PERMANENT_BAN', 'TIME_BAN', 'TIME_BAN_ACCOUNT', 'LEGACY_BAN']);

/** Decodifica el payload de un JWT sin validarlo (solo para leer el `ban`). */
function jwtPayload(token) {
  try {
    return JSON.parse(Buffer.from(String(token).split('.')[1], 'base64url').toString('utf8'));
  } catch {
    return null;
  }
}

/**
 * Baneo de juego a partir de la sesión de login. El `ban.restrictions` puede venir en el cuerpo o
 * dentro del idToken (un JWT). Devuelve:
 *   { permanent: true } · { permanent: false, until: ISO } · null (sin baneo de juego)
 */
function banFromSession(session) {
  const restrictions = session?.ban?.restrictions || jwtPayload(session?.idToken)?.ban?.restrictions;
  if (!Array.isArray(restrictions)) return null;
  let soonest = null;
  for (const r of restrictions) {
    const scope = (r?.scope || '').toLowerCase();
    if (scope && scope !== 'lol' && scope !== 'league_of_legends') continue;
    if (!GAME_BANS.has(r?.type)) continue;
    if (r.type === 'PERMANENT_BAN') return { permanent: true };
    const until = Number(r?.dat?.expirationMillis);
    if (!until || until <= Date.now()) continue; // ya venció
    if (soonest === null || until > soonest) soonest = until; // el que termina más tarde manda
  }
  return soonest ? { permanent: false, until: new Date(soonest).toISOString() } : null;
}

module.exports = { banFromSession };
