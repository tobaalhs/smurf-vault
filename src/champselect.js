// Estado de la cola y de la selección de campeones, y las órdenes que se pueden dar desde el celular.
// Todo pasa por la API local del cliente de LoL (la misma que usa la app para autoaceptar).
const lcu = require('./lcu');

const CDRAGON = 'https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/default';

/** Ruta de un recurso del cliente (/lol-game-data/assets/...) -> URL pública en CommunityDragon. */
function assetUrl(path) {
  if (!path) return null;
  return `${CDRAGON}/${path.replace(/^\/lol-game-data\/assets\//i, '').toLowerCase()}`;
}

const POSITIONS = { top: 'Top', jungle: 'Jungla', middle: 'Mid', bottom: 'ADC', utility: 'Support' };

/** Acción de ban o pick que te toca ahora (o null). */
function myActiveAction(session) {
  const me = session.localPlayerCellId;
  for (const group of session.actions || []) {
    for (const a of group) if (a.actorCellId === me && a.isInProgress && !a.completed) return a;
  }
  return null;
}

/** true si ya elegiste (pick confirmado). */
function myPickDone(session) {
  const me = session.localPlayerCellId;
  return (session.actions || []).flat().some((a) => a.actorCellId === me && a.type === 'pick' && a.completed);
}

class ChampSelect {
  constructor({ client = lcu } = {}) {
    this.client = client;
    this.styles = null; // estilos de runas del cliente (nombre e ícono), se piden una vez
    this.timerRef = null; // { phase, left, seenAt }: cuándo vimos cambiar el tiempo restante
  }

  /**
   * Hora (reloj del PC) en que termina la fase. El cliente solo actualiza el "tiempo restante" cuando
   * pasa algo en la selección (un pick, un ban), no en cada consulta: por eso se anota cuándo cambió
   * ese valor y se cuenta desde ahí. Así la cuenta regresiva corre sola entre eventos.
   */
  phaseEnd(timer) {
    const left = Math.max(0, timer.adjustedTimeLeftInPhase ?? 0);
    const ref = this.timerRef;
    if (!ref || ref.phase !== timer.phase || ref.left !== left) {
      this.timerRef = { phase: timer.phase, left, seenAt: Date.now() };
    }
    return this.timerRef.seenAt + this.timerRef.left;
  }

  async get(path) {
    return this.client.get(path);
  }

  async send(method, path, body) {
    const res = await this.client.request(method, path, {}, body);
    if (!res) throw new Error('El cliente de LoL no está abierto');
    if (res.status >= 400) throw new Error(res.json?.message || `El cliente respondió ${res.status}`);
    return res.json;
  }

  /** Foto del estado para el celular. */
  async state() {
    const phase = await this.get('/lol-gameflow/v1/gameflow-phase');
    if (typeof phase !== 'string') return { phase: 'Offline' };
    const out = { phase };
    if (phase === 'Matchmaking' || phase === 'ReadyCheck') {
      const search = await this.get('/lol-matchmaking/v1/search');
      out.search = search ? { timeInQueue: search.timeInQueue ?? 0, estimated: search.estimatedQueueTime ?? 0 } : null;
    }
    if (phase === 'ReadyCheck') {
      const rc = await this.get('/lol-matchmaking/v1/ready-check');
      out.readyCheck = rc ? { state: rc.state, response: rc.playerResponse, timer: rc.timer ?? 0 } : null;
    }
    if (phase === 'ChampSelect') out.champSelect = await this.champSelectState();
    return out;
  }

  async champSelectState() {
    const session = await this.get('/lol-champ-select/v1/session');
    if (!session) return null;
    const action = myActiveAction(session);
    const picked = myPickDone(session);
    const me = (session.myTeam || []).find((p) => p.cellId === session.localPlayerCellId) || {};
    const [available, skins, pages, current] = await Promise.all([
      action ? this.options(action.type, session) : null,
      me.championId ? this.get('/lol-champ-select/v1/skin-carousel-skins') : null,
      this.get('/lol-perks/v1/pages'),
      this.get('/lol-perks/v1/currentpage'),
    ]);
    if (!this.styles) this.styles = await this.get('/lol-perks/v1/styles');
    const styleById = new Map((this.styles || []).map((s) => [s.id, s]));
    const timer = session.timer || {};

    return {
      timer: {
        phase: timer.phase, // PLANNING, BAN_PICK, FINALIZATION
        endsAt: this.phaseEnd(timer), // el celular descuenta solo hasta esta hora
        totalMs: timer.totalTimeInPhase ?? 0,
      },
      action: action ? { id: action.id, type: action.type, championId: action.championId || 0 } : null,
      picked,
      me: {
        championId: me.championId || me.championPickIntent || 0,
        position: POSITIONS[me.assignedPosition] || '',
        spell1Id: me.spell1Id,
        spell2Id: me.spell2Id,
        skinId: me.selectedSkinId,
      },
      myTeam: (session.myTeam || []).map((p) => ({
        cellId: p.cellId,
        me: p.cellId === session.localPlayerCellId,
        championId: p.championId || 0,
        hoverId: p.championPickIntent || 0,
        position: POSITIONS[p.assignedPosition] || '',
      })),
      theirTeam: (session.theirTeam || []).map((p) => ({ cellId: p.cellId, championId: p.championId || 0 })),
      bans: {
        mine: session.bans?.myTeamBans || [],
        theirs: session.bans?.theirTeamBans || [],
      },
      available: available || [],
      skins: (skins || [])
        .filter((s) => s.unlocked && !s.disabled)
        .map((s) => ({ id: s.id, name: s.name, image: assetUrl(s.tilePath || s.splashPath) })),
      runes: {
        currentId: current?.id ?? null,
        pages: (pages || [])
          .filter((p) => p.isValid !== false)
          .map((p) => ({
            id: p.id,
            name: p.name,
            editable: !!p.isEditable,
            style: styleById.get(p.primaryStyleId)?.name || '',
            icon: assetUrl(styleById.get(p.primaryStyleId)?.iconPath),
          })),
      },
    };
  }

  /**
   * Campeones que se pueden banear o elegir. A veces el cliente entrega la lista vacía (pasa sobre todo
   * con los baneos): entonces se prueba la ruta alternativa y, si también viene vacía, todos los
   * campeones menos los ya baneados, elegidos o deshabilitados.
   */
  async options(type, session) {
    const kind = type === 'ban' ? 'bannable' : 'pickable';
    for (const base of ['/lol-champ-select/v1', '/lol-lobby-team-builder/champ-select/v1']) {
      const ids = await this.get(`${base}/${kind}-champion-ids`);
      if (Array.isArray(ids) && ids.some((id) => id > 0)) return ids.filter((id) => id > 0);
    }
    // Solo campeones de la Grieta: la lista del juego trae también los de LoL Classic (ids desde 60000,
    // alias "Jade_…"), que no se pueden elegir en una partida normal.
    this.allChampions ||= ((await this.get('/lol-game-data/assets/v1/champion-summary.json')) || [])
      .filter((c) => c.id > 0 && c.id < 10000 && !/^Jade_/.test(c.alias || ''))
      .map((c) => c.id);
    const disabled = new Set((await this.get('/lol-champ-select/v1/disabled-champion-ids')) || []);
    const taken = new Set([
      ...(session.bans?.myTeamBans || []),
      ...(session.bans?.theirTeamBans || []),
      ...(session.actions || []).flat().filter((a) => a.completed).map((a) => a.championId),
      ...[...(session.myTeam || []), ...(session.theirTeam || [])].map((p) => p.championId),
    ]);
    return this.allChampions.filter((id) => !taken.has(id) && !disabled.has(id));
  }

  // ---------- órdenes ----------

  async run(cmd) {
    switch (cmd?.type) {
      case 'accept':
        return this.send('POST', '/lol-matchmaking/v1/ready-check/accept');
      case 'decline':
        return this.send('POST', '/lol-matchmaking/v1/ready-check/decline');
      case 'hover':
        return this.hover(cmd.championId);
      case 'lock':
        return this.lock(cmd.championId);
      case 'skin':
        return this.send('PATCH', '/lol-champ-select/v1/session/my-selection', { selectedSkinId: Number(cmd.skinId) });
      case 'spells':
        return this.send('PATCH', '/lol-champ-select/v1/session/my-selection', {
          spell1Id: Number(cmd.spell1Id),
          spell2Id: Number(cmd.spell2Id),
        });
      case 'runePage':
        return this.send('PUT', '/lol-perks/v1/currentpage', Number(cmd.pageId));
      case 'recommendedRunes':
        return this.recommendedRunes();
      default:
        throw new Error('Orden desconocida');
    }
  }

  async currentAction() {
    const session = await this.get('/lol-champ-select/v1/session');
    const action = session && myActiveAction(session);
    if (!action) throw new Error('No es tu turno');
    return action;
  }

  /** Mostrar el campeón (sin confirmar), como pasar el mouse en el cliente. */
  async hover(championId) {
    const action = await this.currentAction();
    return this.send('PATCH', `/lol-champ-select/v1/session/actions/${action.id}`, { championId: Number(championId) });
  }

  /**
   * Confirmar el ban o el pick: el campeón y `completed: true` van en la misma orden, como lo hace el
   * cliente. Si aun así no quedó confirmado, se prueba con la ruta de confirmar; si tampoco, se avisa.
   */
  async lock(championId) {
    const action = await this.currentAction();
    const id = Number(championId || action.championId);
    if (!id) throw new Error('Elige un campeón primero');
    await this.send('PATCH', `/lol-champ-select/v1/session/actions/${action.id}`, { championId: id, completed: true });
    if (await this.isCompleted(action.id)) return null;
    await this.send('POST', `/lol-champ-select/v1/session/actions/${action.id}/complete`);
    if (!(await this.isCompleted(action.id))) throw new Error('El cliente no confirmó la selección: intenta de nuevo');
    return null;
  }

  async isCompleted(actionId) {
    const session = await this.get('/lol-champ-select/v1/session');
    const action = (session?.actions || []).flat().find((a) => a.id === actionId);
    return !action || !!action.completed;
  }

  /**
   * Crea (y deja activa) la página de runas que recomienda Riot para tu campeón y posición. Usa una
   * página propia "Smurf Vault": si ya existe se reemplaza, así no se llena la lista de páginas.
   */
  async recommendedRunes() {
    const session = await this.get('/lol-champ-select/v1/session');
    const me = session?.myTeam?.find((p) => p.cellId === session.localPlayerCellId);
    const champ = me?.championId || me?.championPickIntent;
    if (!champ) throw new Error('Elige un campeón primero');
    const position = me.assignedPosition || 'none';
    const recs = await this.get(`/lol-perks/v1/recommended-pages/champion/${champ}/position/${position}/map/11`);
    const rec = Array.isArray(recs) && recs[0];
    if (!rec) throw new Error('Riot no tiene runas recomendadas para este campeón');
    const pages = (await this.get('/lol-perks/v1/pages')) || [];
    const mine = pages.find((p) => p.isEditable && p.name?.startsWith('Smurf Vault'));
    if (mine) await this.send('DELETE', `/lol-perks/v1/pages/${mine.id}`);
    try {
      return await this.send('POST', '/lol-perks/v1/pages', {
        name: 'Smurf Vault · recomendadas',
        primaryStyleId: rec.primaryPerkStyleId,
        subStyleId: rec.secondaryPerkStyleId,
        selectedPerkIds: (rec.perks || []).map((p) => p.id),
        current: true,
      });
    } catch (e) {
      throw new Error(`No se pudo crear la página de runas (¿no tienes espacio para otra?): ${e.message}`);
    }
  }
}

module.exports = { ChampSelect, myActiveAction, myPickDone, assetUrl };
