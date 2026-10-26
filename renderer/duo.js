// Reglas de dúo de ranked Solo/Dúo de League ("Friend elo"): con qué rango de amigo puede jugar
// clasificatoria una cuenta. Portado de League Accounts (MIT, https://github.com/Tariolle/LeagueAccounts).
// Sirve en la interfaz (como <script>, define canDuo global) y en las pruebas (module.exports).
// Maestro+, Gran Maestro y Retador casi no pueden hacer dúo; por eso varias combinaciones dan false.
(function (root) {
  const TIERS = ['IRON', 'BRONZE', 'SILVER', 'GOLD', 'PLATINUM', 'EMERALD', 'DIAMOND', 'MASTER', 'GRANDMASTER', 'CHALLENGER'];
  const DIVS = { I: 1, II: 2, III: 3, IV: 4 };
  const divNum = (division) => DIVS[String(division || '').trim().toUpperCase()] ?? null;

  /** ¿La cuenta con `rank` (su Solo/Dúo, { tier, division } o null) puede hacer dúo con el amigo dado? */
  function canDuo(rank, friendTier, friendDivision) {
    if (!rank?.tier) return false;
    const tier = String(rank.tier).toLowerCase();
    const friend = String(friendTier || '').toLowerCase();
    const own = divNum(rank.division);
    const theirs = divNum(friendDivision);
    switch (friend) {
      case 'master':
        return tier === 'master' || (tier === 'diamond' && own === 1);
      case 'diamond':
        if (tier === 'diamond' && own !== null && theirs !== null && Math.abs(own - theirs) <= 2) return true;
        return (tier === 'emerald' && own === 1 && theirs === 4) || (tier === 'master' && theirs === 1);
      case 'emerald':
        return tier === 'emerald' || tier === 'platinum' || (theirs === 1 && tier === 'diamond' && own === 4);
      case 'platinum':
        return ['platinum', 'emerald', 'gold'].includes(tier);
      case 'iron':
        return ['iron', 'bronze', 'silver'].includes(tier);
      case 'bronze':
      case 'silver':
      case 'gold': {
        const i = TIERS.findIndex((t) => t.toLowerCase() === friend);
        const allowed = [TIERS[i - 1], TIERS[i]];
        if (i < TIERS.length - 3) allowed.push(TIERS[i + 1]);
        return allowed.some((t) => t?.toLowerCase() === tier);
      }
      default:
        return false; // apex (GM/Retador) no hace dúo
    }
  }

  const api = { canDuo, DUO_TIERS: TIERS.slice(0, 8) };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else {
    root.canDuo = canDuo;
    root.DUO_TIERS = api.DUO_TIERS;
  }
})(typeof window !== 'undefined' ? window : globalThis);
