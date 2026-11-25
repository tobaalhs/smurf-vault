// Página del celular: muestra la cola y la selección de campeones del PC y manda las órdenes.
// El estado llega en vivo por Server-Sent Events; cada sección se redibuja solo si cambió.
const CDRAGON = 'https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/default/v1';
const $ = (s) => document.querySelector(s);
const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
const champIcon = (id) => `${CDRAGON}/champion-icons/${id}.png`;

// ---------- código (va en el QR; se guarda y se saca de la barra de direcciones) ----------
let token = '';
try {
  const fromUrl = new URLSearchParams(location.search).get('t');
  if (fromUrl) localStorage.setItem('svToken', fromUrl);
  token = fromUrl || localStorage.getItem('svToken') || '';
} catch {}
if (location.search) history.replaceState(null, '', location.pathname);

let statics = { champions: {}, spells: [] };
let state = null;
let clockOffset = 0; // reloj del PC - reloj del celular
let mode = ''; // qué esqueleto está dibujado
let search = '';
let setupTab = 'skin';
let spellSlot = 1;
let lastPhase = '';
let lastActionId = null;
let gridFor = null; // turno (acción) para el que está armada la grilla de campeones

function setHTML(el, html) {
  if (el && el._html !== html) {
    el.innerHTML = html;
    el._html = html;
  }
}

let toastTimer;
function toast(msg, error = false) {
  const t = $('#toast');
  t.textContent = msg;
  t.className = `toast${error ? ' error' : ''}`;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => t.classList.add('hidden'), 3000);
}

function setConn(kind, text) {
  $('#conn').className = `conn ${kind}`;
  $('#conn').lastElementChild.textContent = text;
}

function vibrate(pattern) {
  try {
    navigator.vibrate?.(pattern);
  } catch {}
}

async function api(path, body) {
  const res = await fetch(path, {
    method: body ? 'POST' : 'GET',
    headers: { 'X-Token': token, ...(body ? { 'Content-Type': 'application/json' } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  });
  const json = await res.json().catch(() => ({}));
  if (res.status === 401) throw Object.assign(new Error(json.error || 'Código inválido'), { auth: true });
  if (!res.ok) throw new Error(json.error || 'No se pudo');
  return json;
}

async function command(cmd) {
  try {
    await api('/api/command', cmd);
  } catch (e) {
    toast(e.message, true);
  }
}

// ---------- conexión ----------

function noToken() {
  mode = 'none';
  setConn('off', 'Sin código');
  setHTML($('#view'), `<div class="center"><h1>Escanea el QR</h1><p>Abre esta página escaneando el código QR de Smurf Vault (Herramientas → Control desde el celular).</p></div>`);
}

async function start() {
  if (!token) return noToken();
  try {
    statics = await api('/api/static');
  } catch (e) {
    if (e.auth) return noToken();
  }
  connect();
}

function connect() {
  const es = new EventSource(`/api/events?t=${encodeURIComponent(token)}`);
  es.onopen = () => setConn('on', 'Conectado');
  es.onmessage = (e) => {
    const msg = JSON.parse(e.data);
    clockOffset = msg.now - Date.now();
    state = msg.state;
    render();
  };
  es.onerror = () => {
    setConn('off', 'Reconectando…');
    // Si el código dejó de servir (se generó uno nuevo), no tiene sentido reintentar.
    api('/api/static').catch((err) => {
      if (err.auth) {
        es.close();
        noToken();
      }
    });
  };
}

// ---------- vistas ----------

function mmss(ms) {
  const s = Math.max(0, Math.round(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

function simple(title, text, extra = '') {
  mode = 'simple';
  setHTML($('#view'), `<div class="center">${extra}<h1>${title}</h1><p>${text}</p></div>`);
}

function render() {
  const phase = state?.phase || 'Offline';
  if (phase !== lastPhase) {
    if (phase === 'ReadyCheck') vibrate([250, 120, 250, 120, 250]);
    lastPhase = phase;
  }
  switch (phase) {
    case 'Offline':
      return simple('Cliente cerrado', 'Abre el LoL en tu PC y esto se conecta solo.');
    case 'None':
    case 'Lobby':
      return simple('En el lobby', 'Cuando busques partida, esta pantalla se actualiza sola.');
    case 'Matchmaking':
      return simple('Buscando partida', `En cola hace <span class="big-time">${mmss((state.search?.timeInQueue || 0) * 1000)}</span>`, '<div class="pulse"></div>');
    case 'ReadyCheck':
      return renderReady();
    case 'ChampSelect':
      return renderChampSelect();
    case 'GameStart':
    case 'InProgress':
    case 'Reconnect':
      return simple('En partida', '¡Suerte! Cuando termine, esto vuelve al lobby.');
    default:
      return simple('Terminando la partida', 'Esperando al cliente…');
  }
}

function renderReady() {
  mode = 'ready';
  const rc = state.readyCheck || {};
  const answered = rc.response === 'Accepted' ? 'Aceptaste · esperando a los demás' : rc.response === 'Declined' ? 'Rechazaste la partida' : '';
  setHTML(
    $('#view'),
    `<div class="center">
      <h1>¡Partida encontrada!</h1>
      <p>${answered || 'Tienes unos segundos para responder.'}</p>
      <div class="ready-actions">
        <button class="btn-big accept" data-cmd="accept" ${rc.response === 'Accepted' ? 'disabled' : ''}>ACEPTAR</button>
        <button class="btn-sub" data-cmd="decline" ${rc.response === 'Declined' ? 'disabled' : ''}>Rechazar</button>
      </div>
    </div>`
  );
}

// Esqueleto de la selección: cabecera (tiempo) + equipos + cuerpo (elegir, o skin/hechizos/runas).
function renderChampSelect() {
  const cs = state.champSelect;
  if (!cs) return simple('Selección de campeones', 'Cargando…');
  const choosing = !!cs.action;
  const want = choosing ? 'cs-choose' : 'cs-setup';
  if (cs.action && cs.action.id !== lastActionId) {
    vibrate([180, 80, 180]);
    search = '';
  }
  lastActionId = cs.action?.id ?? null;

  if (mode !== want) {
    mode = want;
    $('#view')._html = null;
    $('#view').innerHTML = `
      <div class="cs-head" id="csHead"></div>
      <div class="timebar"><i id="csBar"></i></div>
      <div class="team" id="csTeam"></div>
      <div class="bans" id="csBans"></div>
      ${
        choosing
          ? `<input class="search" id="csSearch" placeholder="Buscar campeón…" autocomplete="off" />
             <div class="champs" id="csChamps"></div>
             <p class="hint hidden" id="csNone">Ningún campeón coincide.</p>
             <div class="lockbar"><button class="btn-big" id="csLock"></button></div>`
          : `<div id="csSetup"></div>`
      }`;
    gridFor = null;
    if (choosing) {
      $('#csSearch').value = search;
      $('#csSearch').addEventListener('input', (e) => {
        search = e.target.value;
        renderChampSelect();
      });
    }
  }
  renderHead(cs);
  renderTeam(cs);
  if (choosing) renderChoose(cs);
  else renderSetup(cs);
}

function headText(cs) {
  if (cs.action?.type === 'ban') return ['Tu turno de banear', 'Elige a quién banear y confírmalo'];
  if (cs.action?.type === 'pick') return ['Tu turno de elegir', 'Elige tu campeón y confírmalo'];
  if (cs.timer.phase === 'PLANNING') return ['Antes de los baneos', 'Tu equipo está mostrando qué quiere jugar'];
  if (cs.timer.phase === 'FINALIZATION') return ['Preparación final', 'Skin, hechizos y runas'];
  return [cs.picked ? 'Ya elegiste' : 'Esperando', cs.picked ? 'Prepara tu skin, hechizos y runas' : 'Te avisamos cuando sea tu turno'];
}

function renderHead(cs) {
  const [title, sub] = headText(cs);
  setHTML($('#csHead'), `<span class="t" id="csTime"></span><div><h2>${title}</h2><small>${sub}</small></div>`);
  tickTimer();
}

// La cuenta regresiva se actualiza sola cada medio segundo, sin redibujar nada más.
function tickTimer() {
  const cs = state?.champSelect;
  const el = $('#csTime');
  if (!cs || !el) return;
  const left = cs.timer.endsAt - (Date.now() + clockOffset);
  el.textContent = Math.max(0, Math.ceil(left / 1000));
  el.classList.toggle('hurry', left < 8000);
  const bar = $('#csBar');
  if (bar) bar.style.width = `${cs.timer.totalMs ? Math.max(0, Math.min(100, (left / cs.timer.totalMs) * 100)) : 0}%`;
}
setInterval(tickTimer, 500);

function renderTeam(cs) {
  setHTML(
    $('#csTeam'),
    cs.myTeam
      .map((p) => {
        const id = p.championId || p.hoverId;
        return `<div class="slot${p.me ? ' me' : ''}">
          <div class="pic${!p.championId && p.hoverId ? ' hover' : ''}">${id ? `<img src="${champIcon(id)}" alt="" />` : ''}</div>
          <small>${p.me ? 'Tú' : esc(p.position || '—')}</small>
        </div>`;
      })
      .join('')
  );
  const banImgs = (list) => list.filter((id) => id > 0).map((id) => `<img src="${champIcon(id)}" alt="" />`).join('');
  const mine = banImgs(cs.bans.mine);
  const theirs = banImgs(cs.bans.theirs);
  setHTML($('#csBans'), mine || theirs ? `<span>Baneos</span>${mine}<span class="sep"></span>${theirs}` : '');
}

/**
 * Grilla de campeones: se arma una vez por turno y después solo se muestran, ocultan o marcan los
 * botones (buscar, alguien eligió uno), así no pestañea ni se vuelven a cargar las imágenes.
 */
function renderChoose(cs) {
  const grid = $('#csChamps');
  const available = new Set(cs.available);
  const missing = cs.available.some((id) => !grid.querySelector(`[data-champ="${id}"]`));
  if (gridFor !== cs.action.id || missing) {
    gridFor = cs.action.id;
    grid.innerHTML = cs.available
      .map((id) => ({ id, name: statics.champions[id] || `#${id}` }))
      .sort((a, b) => a.name.localeCompare(b.name))
      .map(
        (c) => `<button class="champ" data-champ="${c.id}" data-name="${esc(c.name.toLowerCase())}">
          <img src="${champIcon(c.id)}" alt="" loading="lazy" /><span>${esc(c.name)}</span>
        </button>`
      )
      .join('');
  }
  const q = search.trim().toLowerCase();
  const sel = cs.action.championId;
  const banning = cs.action.type === 'ban';
  let shown = 0;
  for (const b of grid.children) {
    const id = Number(b.dataset.champ);
    const show = available.has(id) && (!q || b.dataset.name.includes(q));
    b.classList.toggle('hidden', !show);
    b.classList.toggle('sel', id === sel);
    b.classList.toggle('banning', banning);
    if (show) shown++;
  }
  $('#csNone').classList.toggle('hidden', shown > 0);
  const name = sel ? statics.champions[sel] || 'campeón' : '';
  const lock = $('#csLock');
  lock.textContent = sel ? `${banning ? 'BANEAR' : 'ELEGIR'} ${name.toUpperCase()}` : banning ? 'Elige a quién banear' : 'Elige un campeón';
  lock.disabled = !sel;
  lock.className = `btn-big${banning ? ' ban' : ''}`;
}

function renderSetup(cs) {
  if (!cs.me.championId) {
    setHTML($('#csSetup'), '<p class="hint">Cuando elijas campeón vas a poder cambiar la skin, los hechizos y las runas desde aquí.</p>');
    return;
  }
  const tabs = `<div class="tabs">
    ${[['skin', 'Skin'], ['spells', 'Hechizos'], ['runes', 'Runas']]
      .map(([k, label]) => `<button data-tab="${k}" class="${setupTab === k ? 'on' : ''}">${label}</button>`)
      .join('')}
  </div>`;
  let body = '';
  if (setupTab === 'skin') {
    body = cs.skins.length
      ? `<div class="skins">${cs.skins
          .map(
            (s) => `<button class="skin${s.id === cs.me.skinId ? ' sel' : ''}" data-skin="${s.id}">
              ${s.image ? `<img src="${esc(s.image)}" alt="" loading="lazy" />` : ''}<span>${esc(s.name)}</span>
            </button>`
          )
          .join('')}</div>`
      : '<p class="hint">No hay skins disponibles para este campeón.</p>';
  } else if (setupTab === 'spells') {
    const byId = new Map(statics.spells.map((s) => [s.id, s]));
    const slot = (n, id) => {
      const sp = byId.get(id);
      return `<button class="spell-slot${spellSlot === n ? ' on' : ''}" data-slot="${n}">
        ${sp ? `<img src="${esc(sp.icon)}" alt="" />` : ''}<span><small>${n === 1 ? 'Tecla D' : 'Tecla F'}</small>${esc(sp?.name || '—')}</span>
      </button>`;
    };
    const current = spellSlot === 1 ? cs.me.spell1Id : cs.me.spell2Id;
    body = `<div class="spell-slots">${slot(1, cs.me.spell1Id)}${slot(2, cs.me.spell2Id)}</div>
      <div class="spells">${statics.spells
        .map((s) => `<button class="spell${s.id === current ? ' sel' : ''}" data-spell="${s.id}"><img src="${esc(s.icon)}" alt="" />${esc(s.name)}</button>`)
        .join('')}</div>`;
  } else {
    body = `<button class="btn-sub rec" data-cmd="recommendedRunes">Usar las runas recomendadas por Riot</button>
      <div class="pages">${cs.runes.pages
        .map(
          (p) => `<button class="page${p.id === cs.runes.currentId ? ' sel' : ''}" data-page="${p.id}">
            ${p.icon ? `<img src="${esc(p.icon)}" alt="" />` : ''}<span><b>${esc(p.name)}</b><small>${esc(p.style)}</small></span>
          </button>`
        )
        .join('')}</div>`;
  }
  setHTML($('#csSetup'), tabs + body);
}

// ---------- toques ----------

document.addEventListener('click', (e) => {
  const t = e.target.closest('button');
  if (!t || t.disabled) return;
  const cs = state?.champSelect;
  if (t.dataset.cmd) return command({ type: t.dataset.cmd });
  if (t.id === 'csLock' && cs?.action) {
    t.disabled = true;
    return command({ type: 'lock', championId: cs.action.championId });
  }
  if (t.dataset.champ) {
    // Se muestra al tiro como elegido; el cliente lo confirma en el siguiente estado.
    if (cs?.action) cs.action.championId = Number(t.dataset.champ);
    renderChoose(cs);
    return command({ type: 'hover', championId: Number(t.dataset.champ) });
  }
  if (t.dataset.tab) {
    setupTab = t.dataset.tab;
    return renderSetup(cs);
  }
  if (t.dataset.skin) {
    cs.me.skinId = Number(t.dataset.skin);
    renderSetup(cs);
    return command({ type: 'skin', skinId: Number(t.dataset.skin) });
  }
  if (t.dataset.slot) {
    spellSlot = Number(t.dataset.slot);
    return renderSetup(cs);
  }
  if (t.dataset.spell) {
    const id = Number(t.dataset.spell);
    let [s1, s2] = [cs.me.spell1Id, cs.me.spell2Id];
    // Elegir en un lado el hechizo que ya está en el otro los intercambia, como en el cliente.
    if (spellSlot === 1) [s1, s2] = id === s2 ? [id, s1] : [id, s2];
    else [s1, s2] = id === s1 ? [s2, id] : [s1, id];
    cs.me.spell1Id = s1;
    cs.me.spell2Id = s2;
    renderSetup(cs);
    return command({ type: 'spells', spell1Id: s1, spell2Id: s2 });
  }
  if (t.dataset.page) {
    cs.runes.currentId = Number(t.dataset.page);
    renderSetup(cs);
    return command({ type: 'runePage', pageId: Number(t.dataset.page) });
  }
});

start();
