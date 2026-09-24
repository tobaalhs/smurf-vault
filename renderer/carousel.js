// Carrusel de cuentas (estilo osu!): las cuentas en una rueda a la izquierda y la elegida en detalle a
// la derecha, con el splash de su fondo de perfil detrás. Usa las funciones de renderer.js (data, play,
// openAccount, refreshRanks…).

const CDRAGON = 'https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/default/v1';
const TIER_COLOR = { IRON: '#a39a95', BRONZE: '#cd8d63', SILVER: '#b7c3cf', GOLD: '#e6bb58', PLATINUM: '#4cc5b6', EMERALD: '#3fd68c', DIAMOND: '#8fa9ff', MASTER: '#c985ff', GRANDMASTER: '#ff6b6b', CHALLENGER: '#f5d97e' };
const NO_TIER_COLOR = '#5b6573';
const QUEUES = {
  0: 'Personalizada', 400: 'Normal', 420: 'Solo/Dúo', 430: 'Normal', 440: 'Flex', 450: 'ARAM', 490: 'Rápida',
  700: 'Clash', 830: 'Coop vs IA', 840: 'Coop vs IA', 850: 'Coop vs IA', 870: 'Coop vs IA', 880: 'Coop vs IA', 890: 'Coop vs IA',
  900: 'URF', 1020: 'Uno para todos', 1300: 'Nexus Blitz', 1700: 'Arena', 1710: 'Arena', 1900: 'URF', 2400: 'ARAM',
};
const TFT_QUEUES = { 1090: 'TFT Normal', 1100: 'TFT Clasificatoria', 1130: 'TFT Hiper Rápida', 1160: 'TFT Doble' };
const STEP = 84; // alto de cada cuenta en la rueda + separación
const ITEM_H = 74;

let wheelList = [];
let wheelSig = '';
const wheelEls = new Map(); // id -> elemento de la rueda
let selectedId = null;
let detailSig = '';
let detailTab = 'summary';
let historyMode = 'lol'; // qué historial se ve en la pestaña Historial: 'lol' o 'tft'
let progressQueue = 'solo'; // qué cola se ve en la pestaña Progreso
let wheelPos = 0; // posición actual, en cuentas (con decimales)
let wheelTarget = 0; // hacia dónde se mueve
let lastTickIndex = null;
let snapTimer = null;
let gameInfo = null; // { champions: { id: nombre }, ddragon: versión }
let bgFront = 0;
let bgUrl;
const parallax = { x: 0, y: 0, tx: 0, ty: 0 };

const clamp = (v, a, b) => Math.max(a, Math.min(b, v));

// ---------- datos de la cuenta ----------

/** El mejor rango entre Solo/Dúo, Flex y TFT: define el color y el marco de la cuenta. */
function topRank(a) {
  return ['solo', 'flex', 'tft'].map((q) => a.ranks?.[q]).filter(Boolean).sort((x, y) => rankScore(y) - rankScore(x))[0] || null;
}

function tierColor(a) {
  const r = topRank(a);
  return (r && TIER_COLOR[r.tier]) || NO_TIER_COLOR;
}

function champName(id) {
  return gameInfo?.champions?.[id] || '';
}

/** El campeón que más aparece en las últimas partidas de LoL. */
function mostPlayedChampion(a) {
  const count = new Map();
  for (const m of a.matches || []) if (m.mode === 'lol' && m.champ) count.set(m.champ, (count.get(m.champ) || 0) + 1);
  return [...count.entries()].sort((x, y) => y[1] - x[1])[0]?.[0] || null;
}

/** Splash del fondo de perfil; si no lo conocemos, el del campeón más jugado. */
function splashFor(a) {
  const skin = a.backgroundSkinId;
  if (skin) {
    return {
      url: `https://cdn.communitydragon.org/latest/champion/${Math.floor(skin / 1000)}/splash-art/skin/${skin % 1000}`,
      caption: champName(Math.floor(skin / 1000)) ? `Fondo de perfil · ${champName(Math.floor(skin / 1000))}` : 'Fondo de perfil',
    };
  }
  const champ = mostPlayedChampion(a);
  if (champ) return { url: `https://cdn.communitydragon.org/latest/champion/${champ}/splash-art`, caption: `Más jugado · ${champName(champ)}` };
  return { url: null, caption: '' };
}

function avatarHtml(a) {
  const initial = esc((a.gameName || a.username || '?')[0].toUpperCase());
  const img =
    a.iconId != null
      ? `<img class="av" src="${CDRAGON}/profile-icons/${a.iconId}.jpg" alt="" data-initial="${initial}" />`
      : `<div class="av ph">${initial}</div>`;
  return `<div class="avatar-c">${img}${a.level ? `<span class="lvl">${a.level}</span>` : ''}</div>`;
}

function displayName(a) {
  return a.gameName ? `${esc(a.gameName)}<span class="tag">#${esc(a.tagLine)}</span>` : esc(a.label || a.username);
}

function rankLabel(r) {
  return `${TIER_ES[r.tier] || r.tier}${r.division ? ' ' + r.division : ''}`;
}

function renderSoundBtn() {
  const b = $('#soundBtn');
  b.innerHTML = icon(Sounds.enabled ? 'volume' : 'volumeX');
  b.dataset.tip = Sounds.enabled ? 'Sonidos activados · clic para silenciar' : 'Sonidos silenciados · clic para activar';
  b.classList.toggle('muted', !Sounds.enabled);
}
$('#soundBtn').addEventListener('click', () => {
  Sounds.setEnabled(!Sounds.enabled);
  renderSoundBtn();
  Sounds.pop();
});
renderSoundBtn();

// ---------- rueda ----------

function renderCarousel(list) {
  wheelList = list;
  $('#wheelCount').textContent =
    list.length === data.accounts.length ? `${list.length} cuentas` : `${list.length} de ${data.accounts.length} cuentas`;
  const sig = JSON.stringify(list.map((a) => [a, data.sessions?.[a.id]]));
  if (sig !== wheelSig) {
    wheelSig = sig;
    buildWheel();
  }
  if (!list.length) return;
  const i = list.findIndex((a) => a.id === selectedId);
  if (i < 0) {
    selectAccount(list[0].id, { sound: false, jump: true });
  } else {
    wheelTarget = i;
    markSelected();
    renderDetail(false);
  }
}

function wheelItemHtml(a) {
  const r = topRank(a);
  const sub = [a.server, a.label, a.lastPlayedAt ? ago(a.lastPlayedAt) : ''].filter(Boolean).map(esc).join(' · ');
  return `<div class="plate"></div>
    <div class="w-row">
      ${avatarHtml(a)}
      <div class="w-who">
        <div class="w-name">${a.favorite ? `<span class="w-fav">${icon('star')}</span>` : ''}${displayName(a)}</div>
        <div class="w-sub"><span class="sdot${data.sessions?.[a.id] ? '' : ' off'}"></span>${sub}</div>
      </div>
      <div class="w-tier">
        ${
          r
            ? `<div><b class="t-${r.tier.toLowerCase()}">${rankLabel(r)}</b><small>${r.lp} LP</small></div><img src="assets/ranks/${r.tier.toLowerCase()}.png" alt="" />`
            : '<div><b class="t-none">Sin rango</b></div>'
        }
      </div>
    </div>`;
}

function buildWheel() {
  const wheel = $('#wheel');
  wheel.innerHTML = '';
  wheelEls.clear();
  if (!wheelList.length) {
    wheel.innerHTML = '<div class="wheel-empty">Ninguna cuenta coincide con la búsqueda</div>';
    return;
  }
  for (const a of wheelList) {
    const el = document.createElement('div');
    el.className = 'w-item';
    el.dataset.id = a.id;
    el.style.setProperty('--tier', tierColor(a));
    const splash = splashFor(a).url;
    if (splash) el.style.setProperty('--splash', `url("${splash}")`);
    el.innerHTML = wheelItemHtml(a);
    wheel.appendChild(el);
    wheelEls.set(a.id, el);
  }
  markSelected();
}

function markSelected() {
  for (const [id, el] of wheelEls) el.classList.toggle('sel', id === selectedId);
}

function layoutWheel() {
  const H = $('#wheel').clientHeight;
  if (!H) return;
  const center = H / 2 - ITEM_H / 2;
  const visible = H / STEP / 2 + 2;
  wheelList.forEach((a, i) => {
    const el = wheelEls.get(a.id);
    if (!el) return;
    const d = i - wheelPos;
    if (Math.abs(d) > visible) {
      el.style.visibility = 'hidden';
      return;
    }
    el.style.visibility = '';
    // Curva: la del centro sobresale hacia la derecha y las de los extremos se van hacia la izquierda.
    const x = -Math.min(d * d * 7.5, 170) + (a.id === selectedId ? 18 : 0);
    const s = 1 - Math.min(Math.abs(d) * 0.018, 0.08);
    el.style.transform = `translate3d(${x}px, ${center + d * STEP}px, 0) scale(${s})`;
    el.style.opacity = String(1 - Math.min(Math.abs(d) * 0.06, 0.45));
    el.style.zIndex = String(100 - Math.round(Math.abs(d) * 2));
  });
}

function frame() {
  if (!$('#stage').classList.contains('hidden')) {
    const diff = wheelTarget - wheelPos;
    wheelPos = Math.abs(diff) < 0.0005 ? wheelTarget : wheelPos + diff * 0.17;
    layoutWheel();
    const idx = Math.round(wheelPos);
    if (idx !== lastTickIndex) {
      if (lastTickIndex !== null) Sounds.tick();
      lastTickIndex = idx;
      $('#wheelPos').textContent = wheelList.length ? `${clamp(idx, 0, wheelList.length - 1) + 1} / ${wheelList.length}` : '';
    }
    // Parallax: el fondo sigue un poco al mouse.
    parallax.x += (parallax.tx - parallax.x) * 0.06;
    parallax.y += (parallax.ty - parallax.y) * 0.06;
    $('#stageBg').style.transform = `translate3d(${parallax.x.toFixed(2)}px, ${parallax.y.toFixed(2)}px, 0)`;
  }
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);

$('#stage').addEventListener('mousemove', (e) => {
  const r = $('#stage').getBoundingClientRect();
  parallax.tx = -((e.clientX - r.left) / r.width - 0.5) * 22;
  parallax.ty = -((e.clientY - r.top) / r.height - 0.5) * 14;
});

function scheduleSnap(delay = 140) {
  clearTimeout(snapTimer);
  snapTimer = setTimeout(() => (wheelTarget = clamp(Math.round(wheelTarget), 0, wheelList.length - 1)), delay);
}

// Rueda del mouse: recorre sin elegir, como en osu!.
$('#wheel').addEventListener(
  'wheel',
  (e) => {
    e.preventDefault();
    if (!wheelList.length) return;
    const delta = e.deltaMode === 1 ? e.deltaY * 33 : e.deltaY;
    wheelTarget = clamp(wheelTarget + delta / 95, -0.4, wheelList.length - 0.6);
    scheduleSnap();
  },
  { passive: false }
);

// Arrastrar con inercia; un clic sin arrastrar elige la cuenta.
let drag = null;
$('#wheel').addEventListener('pointerdown', (e) => {
  if (e.button !== 0 || !wheelList.length) return;
  clearTimeout(snapTimer);
  drag = { y: e.clientY, start: wheelTarget, moved: false, lastY: e.clientY, lastT: performance.now(), v: 0, item: e.target.closest('.w-item') };
  $('#wheel').setPointerCapture(e.pointerId);
});
$('#wheel').addEventListener('pointermove', (e) => {
  if (!drag) return;
  const dy = e.clientY - drag.y;
  if (Math.abs(dy) > 5) drag.moved = true;
  if (!drag.moved) return;
  $('#wheel').classList.add('dragging');
  const now = performance.now();
  const dt = Math.max(1, now - drag.lastT);
  drag.v = 0.8 * drag.v + 0.2 * (-(e.clientY - drag.lastY) / STEP / dt);
  drag.lastY = e.clientY;
  drag.lastT = now;
  wheelTarget = wheelPos = clamp(drag.start - dy / STEP, -0.4, wheelList.length - 0.6);
});
function endDrag() {
  if (!drag) return;
  $('#wheel').classList.remove('dragging');
  if (drag.moved) wheelTarget = clamp(Math.round(wheelTarget + drag.v * 220), 0, wheelList.length - 1);
  else if (drag.item) selectAccount(drag.item.dataset.id);
  drag = null;
}
$('#wheel').addEventListener('pointerup', endDrag);
$('#wheel').addEventListener('pointercancel', endDrag);

function selectAccount(id, { sound = true, jump = false } = {}) {
  const i = wheelList.findIndex((a) => a.id === id);
  if (i < 0) return;
  wheelTarget = i;
  if (jump) wheelPos = i;
  if (id === selectedId) return;
  selectedId = id;
  markSelected();
  renderDetail(true);
  if (sound) Sounds.pop();
}

// ---------- fondo ----------

function setBackground(a) {
  $('#stage').style.setProperty('--tier', tierColor(a));
  const { url } = splashFor(a);
  if (url === bgUrl) return;
  bgUrl = url;
  const layers = $('#stageBg').children;
  const show = () => {
    if (bgUrl !== url) return; // mientras cargaba se eligió otra cuenta
    bgFront = 1 - bgFront;
    layers[bgFront].style.backgroundImage = url ? `url("${url}")` : 'none';
    layers[bgFront].classList.add('show');
    layers[1 - bgFront].classList.remove('show');
  };
  if (!url) return show();
  const img = new Image();
  img.src = url;
  img.decode().then(show, show);
}

// ---------- detalle ----------

function rankCard(title, r, delta) {
  if (!r) {
    return `<div class="d-rank none"><div class="d-rank-body"><div class="q">${title}</div><div class="t t-none">Sin rango</div></div></div>`;
  }
  const games = r.wins + r.losses;
  const wr = games ? Math.round((r.wins / games) * 100) : 0;
  return `<div class="d-rank">
    <img src="assets/ranks/${r.tier.toLowerCase()}.png" alt="" />
    <div class="d-rank-body">
      <div class="q">${title}</div>
      <div class="t t-${r.tier.toLowerCase()}">${rankLabel(r)}<small>${r.lp} LP</small></div>
      <div class="wr">${games ? `${r.wins}V ${r.losses}D · ${wr}%` : 'Sin partidas'}${deltaBadge(delta)}</div>
      <div class="wrbar"><i class="${wr >= 50 ? 'good' : ''}" data-w="${wr}"></i></div>
    </div>
  </div>`;
}

function queueName(m) {
  return m.mode === 'tft' ? TFT_QUEUES[m.queueId] || 'TFT' : QUEUES[m.queueId] || 'Partida';
}

/** Clase de color: en LoL azul/rojo; en TFT oro, plata, bronce, gris para el 4.º y normal del 5.º al 8.º. */
function resultOf(m) {
  if (m.remake) return { cls: 'remake', text: 'Remake' };
  if (m.mode === 'tft') {
    const p = m.placement;
    return { cls: p >= 1 && p <= 4 ? `p${p}` : 'plow', text: p === 1 ? '¡Primer lugar!' : `Puesto ${p ?? '?'}` };
  }
  return { cls: m.win ? 'win' : 'loss', text: m.win ? 'Victoria' : 'Derrota' };
}

// Colas clasificatorias -> cola del registro de LP.
const RANKED_QUEUE = { 420: 'solo', 440: 'flex', 1100: 'tft' };

/**
 * LP que dio una partida clasificatoria: el cambio del registro de LP que se anotó justo después de
 * terminarla (la app revisa el rango a los pocos segundos de cada partida). null si no se sabe.
 */
function matchLp(m, a) {
  const list = a?.lp?.[RANKED_QUEUE[m.queueId]];
  if (!list || m.remake) return null;
  const end = Date.parse(m.at);
  const i = list.findIndex((p) => {
    const t = Date.parse(p.at);
    return t >= end - 60_000 && t <= end + 45 * 60_000;
  });
  return i > 0 ? lpValue(list[i]) - lpValue(list[i - 1]) : null;
}

function lpChip(diff) {
  if (diff == null || !diff) return '';
  return `<span class="m-lp ${diff > 0 ? 'up' : 'down'}">${diff > 0 ? '+' : '−'}${Math.abs(diff)} LP</span>`;
}

function matchTip(m, a) {
  const res = resultOf(m).text;
  const who = m.mode === 'tft' ? 'TFT' : champName(m.champ) || 'Campeón';
  const kda = m.mode === 'lol' ? ` · ${m.k}/${m.d}/${m.a}` : '';
  const lp = matchLp(m, a);
  return `${who} · ${res}${kda}${lp ? ` · ${lp > 0 ? '+' : '−'}${Math.abs(lp)} LP` : ''} · ${queueName(m)} · ${ago(m.at)}`;
}

/** Imagen de la minileyenda de una partida de TFT, si se conoce. */
function legendUrl(m) {
  const path = gameInfo?.companions?.[m.companion];
  return path ? `https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/default/${path}` : null;
}

function miniMatch(m, a) {
  const { cls } = resultOf(m);
  let inner = `<img src="${CDRAGON}/champion-icons/${m.champ}.png" alt="" />`;
  let extra = '';
  if (m.mode === 'tft') {
    // La minileyenda de fondo y encima un velo del color del puesto, con el número.
    const legend = legendUrl(m);
    inner = `${legend ? `<img src="${legend}" alt="" />` : ''}<b>${m.placement ?? '?'}</b>`;
    if (legend) extra = ' legend';
  }
  return `<span class="mm ${m.mode} ${cls}${extra}" data-tip="${esc(matchTip(m, a))}">${inner}</span>`;
}

function matchRow(m, a) {
  const { cls, text } = resultOf(m);
  const lp = lpChip(matchLp(m, a));
  const mins = Math.round((m.dur || 0) / 60);
  const when = `<div class="mwhen" data-tip="${new Date(m.at).toLocaleString()}">${ago(m.at)}</div>`;
  if (m.mode === 'tft') {
    const legend = legendUrl(m);
    const place = legend
      ? `<div class="mplace legend"><img src="${legend}" alt="" /><span>${m.placement ?? '?'}</span></div>`
      : `<div class="mplace">${m.placement ?? '?'}</div>`;
    return `<div class="mrow tft ${cls}">
      ${place}
      <div class="mres"><b>${text}${lp}</b><small>${queueName(m)} · ${mins} min</small></div>
      <div class="mkda">${m.lvl ? `<b>Nivel ${m.lvl}</b><small>al terminar</small>` : ''}</div>
      <div class="mitems"></div>
      ${when}
    </div>`;
  }
  const ratio = m.d ? ((m.k + m.a) / m.d).toFixed(1) : 'Perfecto';
  const items = gameInfo?.ddragon
    ? m.items
        .map((id) => (id ? `<img class="item" src="https://ddragon.leagueoflegends.com/cdn/${gameInfo.ddragon}/img/item/${id}.png" alt="" />` : '<span class="noitem"></span>'))
        .join('')
    : '';
  return `<div class="mrow lol ${cls}">
    <img class="mchamp" src="${CDRAGON}/champion-icons/${m.champ}.png" alt="" data-tip="${esc(champName(m.champ))}" />
    <div class="mres"><b>${text}${lp}</b><small>${queueName(m)} · ${mins} min</small></div>
    <div class="mkda"><b>${m.k} / ${m.d} / ${m.a}</b><small>${ratio} KDA · ${m.cs} CS</small></div>
    <div class="mitems">${items}</div>
    ${when}
  </div>`;
}

function lolSummary(matches) {
  const valid = matches.filter((m) => !m.remake);
  if (!valid.length) return '';
  const wins = valid.filter((m) => m.win).length;
  const wr = Math.round((wins / valid.length) * 100);
  let streak = 0;
  for (const m of valid) {
    if (m.win !== valid[0].win) break;
    streak++;
  }
  const champs = new Map();
  for (const m of valid) {
    const c = champs.get(m.champ) || { games: 0, wins: 0 };
    c.games++;
    if (m.win) c.wins++;
    champs.set(m.champ, c);
  }
  const top = [...champs.entries()].sort((x, y) => y[1].games - x[1].games).slice(0, 3);
  return `<div class="h-summary">
    <div class="h-stat"><b>${wins}V ${valid.length - wins}D</b><small>Últimas ${valid.length} · ${wr}%</small></div>
    ${streak >= 2 ? `<div class="h-stat ${valid[0].win ? 'win' : 'loss'}"><b>${streak} ${valid[0].win ? 'victorias' : 'derrotas'}</b><small>seguidas</small></div>` : ''}
    <div class="h-champs">
      ${top
        .map(
          ([id, c]) => `<div class="h-champ" data-tip="${esc(champName(id))}">
            <img src="${CDRAGON}/champion-icons/${id}.png" alt="" />
            <span><b>${c.games}</b> ${c.games === 1 ? 'partida' : 'partidas'}<small>${Math.round((c.wins / c.games) * 100)}% victorias</small></span>
          </div>`
        )
        .join('')}
    </div>
  </div>`;
}

function tftSummary(matches) {
  const placed = matches.filter((m) => m.placement);
  if (!placed.length) return '';
  const avg = placed.reduce((s, m) => s + m.placement, 0) / placed.length;
  const top4 = placed.filter((m) => m.placement <= 4).length;
  const firsts = placed.filter((m) => m.placement === 1).length;
  return `<div class="h-summary">
    <div class="h-stat"><b>${avg.toFixed(1)}</b><small>Puesto promedio · últimas ${placed.length}</small></div>
    <div class="h-stat"><b>${Math.round((top4 / placed.length) * 100)}%</b><small>Top 4 (${top4})</small></div>
    ${firsts ? `<div class="h-stat p1"><b>${firsts}</b><small>${firsts === 1 ? 'Primer lugar' : 'Primeros lugares'}</small></div>` : ''}
    <div class="h-dist">${[1, 2, 3, 4, 5, 6, 7, 8]
      .map((p) => {
        const n = placed.filter((m) => m.placement === p).length;
        return `<span class="${p <= 4 ? 'p' + p : 'plow'}" data-tip="${n} ${n === 1 ? 'vez' : 'veces'} ${p}.º"><i data-h="${Math.round((n / placed.length) * 100)}"></i><small>${p}</small></span>`;
      })
      .join('')}</div>
  </div>`;
}

/** Modos que tiene la cuenta en el historial, con el de la partida más reciente primero. */
function historyModes(a) {
  const modes = [...new Set((a.matches || []).map((m) => m.mode))];
  return modes;
}

function summaryHtml(a) {
  const matches = a.matches || [];
  const strip = (mode, label) => {
    const list = matches.filter((m) => m.mode === mode).slice(0, 10);
    return list.length
      ? `<div class="d-recent">
          <span class="d-label">${label}</span>
          <div class="recent-strip">${list.map((m) => miniMatch(m, a)).join('')}</div>
          <button type="button" class="link" data-tab="history" data-mode="${mode}">Ver historial</button>
        </div>`
      : '';
  };
  return `<div class="d-ranks">
      ${rankCard('Solo/Dúo', a.ranks?.solo, lpDelta(a, 'solo'))}
      ${rankCard('Flex', a.ranks?.flex, lpDelta(a, 'flex'))}
      ${a.ranks?.tft ? rankCard('TFT', a.ranks.tft, lpDelta(a, 'tft')) : ''}
      ${a.ranks?.doubleUp ? rankCard('Double Up', a.ranks.doubleUp) : ''}
    </div>
    ${strip('lol', 'Últimas de LoL')}
    ${strip('tft', 'Últimas de TFT')}
    <div class="d-meta">
      <span>${icon('gamepad')}${a.lastPlayedAt ? 'Jugó ' + ago(a.lastPlayedAt) : 'Sin partidas registradas'}</span>
      <span>${icon('refresh')}Datos ${a.lastSyncedAt ? 'de ' + ago(a.lastSyncedAt) : 'sin actualizar'}</span>
    </div>
    <div class="d-creds">
      <button type="button" class="cred" data-act="copy-user">
        <span class="cred-label">Usuario</span><span class="cred-value">${esc(a.username)}</span>${icon('copy')}
      </button>
      <button type="button" class="cred" data-act="copy-pass">
        <span class="cred-label">Contraseña</span>${a.password ? '<span class="cred-value secret">••••••••</span>' : '<span class="cred-value none">Sin guardar</span>'}${icon('copy')}
      </button>
    </div>
    ${a.notes ? `<p class="d-notes">${esc(a.notes)}</p>` : ''}`;
}

function historyHtml(a) {
  const modes = historyModes(a);
  if (!modes.length) {
    return `<div class="h-empty">${icon('clock')}<p>Todavía no hay partidas guardadas de esta cuenta.<br />Se cargan al detectarla en el cliente y se actualizan solas al terminar cada partida.</p></div>`;
  }
  // Si la cuenta no tiene partidas del modo elegido, se muestra el que sí tenga.
  const mode = modes.includes(historyMode) ? historyMode : modes[0];
  const matches = a.matches.filter((m) => m.mode === mode);
  const count = (m) => a.matches.filter((x) => x.mode === m).length;
  const switcher =
    modes.length > 1
      ? `<div class="h-modes">
          <button type="button" data-mode="lol" class="${mode === 'lol' ? 'on' : ''}">League of Legends<span class="count">${count('lol')}</span></button>
          <button type="button" data-mode="tft" class="${mode === 'tft' ? 'on' : ''}">TFT<span class="count">${count('tft')}</span></button>
        </div>`
      : '';
  return switcher + (mode === 'tft' ? tftSummary(matches) : lolSummary(matches)) + `<div class="h-list">${matches.map((m) => matchRow(m, a)).join('')}</div>`;
}

// ---------- progreso de LP ----------

const LP_QUEUES = [
  ['solo', 'Solo/Dúo'],
  ['flex', 'Flex'],
  ['tft', 'TFT'],
];
const APEX = ['MASTER', 'GRANDMASTER', 'CHALLENGER'];

/** Rango a un número continuo: 100 por división, 400 por liga; de Maestro para arriba, solo LP. */
function lpValue(p) {
  if (APEX.includes(p.tier)) return 2800 + p.lp;
  return TIERS.indexOf(p.tier) * 400 + (DIVS[p.division] ?? 0) * 100 + p.lp;
}

function lpPointLabel(p) {
  return `${rankLabel(p)} · ${p.lp} LP`;
}

/** Cambio del último partido en una cola: { diff, from, to } o null. */
function lpDelta(a, q) {
  const list = a.lp?.[q];
  if (!list || list.length < 2) return null;
  const to = list[list.length - 1];
  const from = list[list.length - 2];
  return { diff: lpValue(to) - lpValue(from), from, to };
}

function deltaBadge(d) {
  if (!d || !d.diff) return '';
  const up = d.diff > 0;
  const tierChange = d.from.tier !== d.to.tier || d.from.division !== d.to.division;
  const text = tierChange ? `${up ? 'Subió' : 'Bajó'} de ${rankLabel(d.from)}` : `${up ? '+' : '−'}${Math.abs(d.diff)} LP`;
  return `<span class="lp-delta ${up ? 'up' : 'down'}" data-tip="Último cambio · ${ago(d.to.at)}">${up ? '▲' : '▼'} ${text}</span>`;
}

function progressHtml(a) {
  const queues = LP_QUEUES.filter(([q]) => a.lp?.[q]?.length);
  if (!queues.length) {
    return `<div class="h-empty">${icon('chart')}<p>Todavía no hay registro de LP de esta cuenta.<br />Se anota solo cada vez que cambia su rango (al detectarla en el cliente o al actualizar).</p></div>`;
  }
  const q = queues.some(([k]) => k === progressQueue) ? progressQueue : queues[0][0];
  const list = a.lp[q];
  const first = list[0];
  const last = list[list.length - 1];
  const total = lpValue(last) - lpValue(first);
  const changes = list
    .map((p, i) => (i ? { p, diff: lpValue(p) - lpValue(list[i - 1]) } : null))
    .filter(Boolean)
    .reverse()
    .slice(0, 8);
  return `${
    queues.length > 1
      ? `<div class="h-modes">${queues.map(([k, label]) => `<button type="button" data-queue="${k}" class="${k === q ? 'on' : ''}">${label}</button>`).join('')}</div>`
      : ''
  }
    <div class="lp-head">
      <div class="h-stat"><b>${lpPointLabel(last)}</b><small>Ahora</small></div>
      ${
        list.length > 1
          ? `<div class="h-stat"><b class="${total > 0 ? 'up' : total < 0 ? 'down' : ''}">${total > 0 ? '+' : total < 0 ? '−' : ''}${Math.abs(total)} LP</b><small>desde ${new Date(first.at).toLocaleDateString()}</small></div>`
          : ''
      }
    </div>
    <div class="lp-chart" data-queue="${q}"></div>
    ${
      changes.length
        ? `<div class="lp-changes"><span class="d-label">Últimos cambios</span>${changes
            .map(
              ({ p, diff }) => `<div class="lp-change">
                <span class="${diff > 0 ? 'up' : 'down'}">${diff > 0 ? '▲ +' : '▼ −'}${Math.abs(diff)} LP</span>
                <span>${lpPointLabel(p)}</span>
                <span class="when" data-tip="${new Date(p.at).toLocaleString()}">${ago(p.at)}</span>
              </div>`
            )
            .join('')}</div>`
        : '<p class="hint lp-hint">El gráfico se va llenando con cada cambio de rango.</p>'
    }`;
}

/** Nombre corto de una línea guía del eje: la liga/división que empieza ahí. */
function tickLabel(v) {
  if (v >= 2800) return v === 2800 ? 'Maestro' : `${v - 2800} LP`;
  const tier = TIERS[Math.floor(v / 400)];
  const div = ['IV', 'III', 'II', 'I'][Math.floor((v % 400) / 100)];
  return `${TIER_ES[tier]} ${div}`;
}

/** Dibuja el gráfico escalonado de LP (el rango se mantiene hasta el siguiente cambio). */
function drawLpChart() {
  const box = $('#detail .lp-chart');
  const acc = data?.accounts.find((a) => a.id === selectedId);
  if (!box || !acc) return;
  const list = acc.lp?.[box.dataset.queue] || [];
  if (!list.length) return;
  // El color de la línea es el de la liga actual de esa cola, no el del mejor rango de la cuenta.
  box.style.setProperty('--tier', TIER_COLOR[list[list.length - 1].tier] || NO_TIER_COLOR);
  const W = box.clientWidth;
  const H = 200;
  const pad = { l: 92, r: 58, t: 14, b: 24 };
  const now = Date.now();
  const t0 = Date.parse(list[0].at);
  const t1 = Math.max(now, t0 + 60_000);
  const vals = list.map(lpValue);
  let lo = Math.min(...vals);
  let hi = Math.max(...vals);
  const span = Math.max(hi - lo, 100);
  lo = Math.max(0, lo - span * 0.25);
  hi = hi + span * 0.25;
  const x = (t) => pad.l + ((t - t0) / (t1 - t0)) * (W - pad.l - pad.r);
  const y = (v) => pad.t + (1 - (v - lo) / (hi - lo)) * (H - pad.t - pad.b);

  // Líneas guía en los cambios de liga (o de división si el rango es chico); en Maestro+, cada 100/200 LP.
  const step = hi - lo > 700 ? 400 : 100;
  const ticks = [];
  for (let v = Math.ceil(lo / step) * step; v <= hi; v += step) {
    ticks.push(v);
  }
  if (lo < 2800 && hi > 2800 && !ticks.includes(2800)) ticks.push(2800);

  // Escalones: horizontal hasta el siguiente punto, luego vertical; al final se estira hasta hoy.
  const pts = list.map((p) => [x(Date.parse(p.at)), y(lpValue(p))]);
  let d = `M${pts[0][0]},${pts[0][1]}`;
  for (let i = 1; i < pts.length; i++) d += `H${pts[i][0]}V${pts[i][1]}`;
  const endX = x(now);
  d += `H${endX}`;
  const area = `${d}V${H - pad.b}H${pts[0][0]}Z`;
  const last = pts[pts.length - 1];

  box.innerHTML = `<svg width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" role="img" aria-label="Evolución de LP">
      ${ticks
        .map(
          (v) => `<line class="grid" x1="${pad.l}" x2="${W - pad.r}" y1="${y(v)}" y2="${y(v)}" />
            <text class="tick" x="${pad.l - 10}" y="${y(v) + 4}" text-anchor="end">${esc(tickLabel(v))}</text>`
        )
        .join('')}
      <text class="tick" x="${pad.l}" y="${H - 6}">${new Date(t0).toLocaleDateString()}</text>
      <text class="tick" x="${W - pad.r}" y="${H - 6}" text-anchor="end">hoy</text>
      <path class="area" d="${area}" />
      <path class="line" d="${d}" />
      <circle class="end" cx="${endX}" cy="${last[1]}" r="5" />
      <text class="end-label" x="${endX + 10}" y="${last[1] + 4}">${list[list.length - 1].lp} LP</text>
      <line class="cross hidden" y1="${pad.t}" y2="${H - pad.b}" />
      <circle class="cross-dot hidden" r="5" />
      <rect class="hit" x="${pad.l}" y="0" width="${W - pad.l - pad.r}" height="${H}" />
    </svg>
    <div class="lp-tip hidden"></div>`;

  // Crosshair: la línea vertical se pega al cambio más cercano y muestra su rango.
  const svg = box.querySelector('svg');
  const tip = box.querySelector('.lp-tip');
  const cross = svg.querySelector('.cross');
  const dot = svg.querySelector('.cross-dot');
  const hide = () => [cross, dot, tip].forEach((el) => el.classList.add('hidden'));
  svg.querySelector('.hit').addEventListener('pointermove', (e) => {
    const mx = e.clientX - svg.getBoundingClientRect().left;
    let i = 0;
    for (let k = 1; k < pts.length; k++) if (Math.abs(pts[k][0] - mx) < Math.abs(pts[i][0] - mx)) i = k;
    const [px, py] = pts[i];
    cross.setAttribute('x1', px);
    cross.setAttribute('x2', px);
    dot.setAttribute('cx', px);
    dot.setAttribute('cy', py);
    const diff = i ? lpValue(list[i]) - lpValue(list[i - 1]) : 0;
    tip.innerHTML = `<b></b><span></span><small></small>`;
    tip.querySelector('b').textContent = lpPointLabel(list[i]);
    tip.querySelector('span').textContent = diff ? `${diff > 0 ? '+' : '−'}${Math.abs(diff)} LP` : 'Primer registro';
    tip.querySelector('span').className = diff > 0 ? 'up' : diff < 0 ? 'down' : '';
    tip.querySelector('small').textContent = new Date(list[i].at).toLocaleString();
    tip.style.left = `${Math.min(Math.max(px, 80), W - 80)}px`;
    tip.style.top = `${py}px`;
    [cross, dot, tip].forEach((el) => el.classList.remove('hidden'));
  });
  svg.querySelector('.hit').addEventListener('pointerleave', hide);
}

let lpResizeTimer = null;
addEventListener('resize', () => {
  clearTimeout(lpResizeTimer);
  lpResizeTimer = setTimeout(drawLpChart, 120);
});

// ---------- maestrías ----------

function formatPoints(n) {
  return n >= 1_000_000 ? `${(n / 1_000_000).toFixed(1)} M` : n >= 1000 ? `${Math.round(n / 1000)} mil` : String(n);
}

function championsHtml(a) {
  const m = a.mastery;
  if (!m?.top?.length) {
    return `<div class="h-empty">${icon('star')}<p>Todavía no hay maestrías de esta cuenta.<br />Se cargan al detectarla en el cliente, o con Actualizar si tienes API key de Riot.</p></div>`;
  }
  const max = m.top[0].points || 1;
  return `<div class="lp-head">
      <div class="h-stat"><b>${m.score}</b><small>Puntaje de maestría</small></div>
      <div class="h-stat"><b>${formatPoints(m.top.reduce((s, c) => s + c.points, 0))}</b><small>Puntos en su top ${m.top.length}</small></div>
    </div>
    <div class="mastery-list">
      ${m.top
        .map(
          (c, i) => `<div class="mastery-row${i < 3 ? ' podium' : ''}">
            <span class="m-rank">${i + 1}</span>
            <div class="m-champ"><img src="${CDRAGON}/champion-icons/${c.champ}.png" alt="" /><span class="m-level">${c.level}</span></div>
            <div class="m-body">
              <div class="m-name"><b>${esc(champName(c.champ) || 'Campeón')}</b><span>${c.points.toLocaleString()} pts</span></div>
              <div class="m-bar"><i data-w="${Math.round((c.points / max) * 100)}"></i></div>
            </div>
          </div>`
        )
        .join('')}
    </div>`;
}

/**
 * Marco del ícono: el de nivel si la cuenta eligió ese en el cliente; si no, las alas de su mejor
 * rango. `space` es la clase que reserva espacio arriba para lo que sobresale del marco.
 */
function crestOf(a, r) {
  if (a.crest?.type === 'level') {
    return { kind: 'level', src: `assets/crests/level-${a.crest.theme}.webp`, space: a.crest.theme >= 13 ? 'crest-tall' : 'crest-level' };
  }
  if (!r) return null;
  const tier = r.tier.toLowerCase();
  return { kind: 'wings', src: `assets/wings/${tier}.webp`, space: `tier-${tier}` };
}

function detailHtml(a) {
  const r = topRank(a);
  const saved = data.sessions?.[a.id];
  const count = a.matches?.length || 0;
  const crest = crestOf(a, r);
  const frame = crest
    ? `<img class="frame ${crest.kind}" src="${crest.src}" alt="" /><span class="frame-shine ${crest.kind}" data-mask="${crest.src}"></span>`
    : '';
  return `<div class="d-hero${crest ? ' ' + crest.space : ''}">
      <div class="d-avatar${crest ? ' framed' : ''}">${frame}${avatarHtml(a)}</div>
      <div class="d-id">
        <h1>${displayName(a)}</h1>
        <div class="d-chips">
          ${a.server ? `<span class="chip">${esc(a.server)}</span>` : ''}
          ${a.label ? `<span class="chip label">${esc(a.label)}</span>` : ''}
          ${saved ? `<span class="chip ok" data-tip="Guardada ${ago(saved)}"><span class="sdot"></span>Sesión guardada</span>` : '<span class="chip warn">Sin sesión guardada</span>'}
        </div>
      </div>
      <button type="button" class="fav-btn${a.favorite ? ' on' : ''}" data-act="fav" data-tip="${a.favorite ? 'Quitar de favoritas' : 'Favorita: queda siempre arriba'}">${icon('star')}</button>
    </div>
    <div class="d-tabs">
      <button type="button" data-tab="summary" class="${detailTab === 'summary' ? 'on' : ''}">Resumen</button>
      <button type="button" data-tab="history" class="${detailTab === 'history' ? 'on' : ''}">Historial${count ? `<span class="count">${count}</span>` : ''}</button>
      <button type="button" data-tab="progress" class="${detailTab === 'progress' ? 'on' : ''}">Progreso</button>
      <button type="button" data-tab="champions" class="${detailTab === 'champions' ? 'on' : ''}">Campeones</button>
    </div>
    <div class="d-body">${{ history: historyHtml, progress: progressHtml, champions: championsHtml }[detailTab]?.(a) ?? summaryHtml(a)}</div>
    <div class="d-actions">
      ${
        saved
          ? `<button type="button" class="play-big" data-act="play"><span>${icon('play')}JUGAR</span></button>`
          : `<button type="button" class="play-big login" data-act="play" data-tip="La primera vez, marca &quot;Mantener sesión iniciada&quot; y después entras con un clic"><span>${icon('logIn')}INICIAR SESIÓN</span></button>`
      }
      <button type="button" class="btn subtle" data-act="edit">${icon('pencil')}Editar</button>
      <button type="button" class="btn subtle" data-act="refresh" data-tip="Lee rangos, historial y maestrías desde el cliente de LoL (esta cuenta tiene que estar abierta ahí)">${icon('refresh')}Actualizar</button>
      <span class="d-caption">${esc(splashFor(a).caption)}</span>
    </div>`;
}

function renderDetail(animate) {
  const a = data.accounts.find((x) => x.id === selectedId);
  if (!a) return;
  const sig = JSON.stringify([a, data.sessions?.[a.id], detailTab, historyMode, progressQueue, !!gameInfo]);
  if (!animate && sig === detailSig) return;
  detailSig = sig;
  setBackground(a);
  const d = $('#detail');
  const scroll = d.querySelector('.d-body')?.scrollTop || 0;
  d.style.setProperty('--tier', tierColor(a));
  d.classList.remove('enter');
  d.innerHTML = detailHtml(a);
  // El CSP no permite style inline: anchos y máscaras se asignan aquí.
  d.querySelectorAll('i[data-w]').forEach((i) => (i.style.width = i.dataset.w + '%'));
  d.querySelectorAll('.h-dist i[data-h]').forEach((i) => (i.style.height = Math.max(4, i.dataset.h) + '%'));
  d.querySelectorAll('.frame-shine').forEach((s) => {
    const mask = `url("${s.dataset.mask}")`;
    s.style.webkitMaskImage = mask;
    s.style.maskImage = mask;
  });
  drawLpChart();
  if (animate) {
    void d.offsetWidth; // reinicia la animación de entrada
    d.classList.add('enter');
  } else {
    d.querySelector('.d-body').scrollTop = scroll;
  }
}

$('#detail').addEventListener('click', async (e) => {
  const acc = data?.accounts.find((a) => a.id === selectedId);
  if (!acc) return;
  const queue = e.target.closest('[data-queue]');
  if (queue && queue.tagName === 'BUTTON') {
    if (progressQueue === queue.dataset.queue) return;
    progressQueue = queue.dataset.queue;
    Sounds.tick();
    renderDetail(false);
    return;
  }
  const tab = e.target.closest('[data-tab], [data-mode]');
  if (tab) {
    const nextTab = tab.dataset.tab || detailTab;
    const nextMode = tab.dataset.mode || historyMode;
    if (nextTab === detailTab && nextMode === historyMode) return;
    const animate = nextTab !== detailTab;
    detailTab = nextTab;
    historyMode = nextMode;
    Sounds.tick();
    renderDetail(animate);
    return;
  }
  const btn = e.target.closest('[data-act]');
  if (!btn) return;
  switch (btn.dataset.act) {
    case 'play':
      Sounds.play();
      play(acc, btn);
      break;
    case 'edit':
      openAccount(acc);
      break;
    case 'refresh':
      updateFromClient(acc, btn);
      break;
    case 'fav':
      await run(null, async () => {
        data = await window.api.setFavorite(acc.id, !acc.favorite);
        Sounds.pop();
        render();
      });
      break;
    case 'copy-user':
    case 'copy-pass': {
      const pass = btn.dataset.act === 'copy-pass';
      if (pass && !acc.password) return toast('Esta cuenta no tiene contraseña guardada: agrégala en Editar', 'error');
      await run(null, async () => {
        await window.api.copy(acc.id, pass ? 'password' : 'username');
        flashCopied(btn);
        Sounds.pop();
        if (pass) toast('Contraseña copiada · se borra del portapapeles en 30 s', 'ok');
      });
      break;
    }
  }
});

// Imágenes que no cargan (sin internet, ícono o ítem nuevo): inicial en vez del ícono, hueco en vez del ítem.
$('#stage').addEventListener(
  'error',
  (e) => {
    const img = e.target;
    if (img.matches?.('img.av')) {
      const ph = document.createElement('div');
      ph.className = 'av ph';
      ph.textContent = img.dataset.initial || '?';
      img.replaceWith(ph);
    } else if (img.matches?.('img.item, img.mchamp, .mm img, .h-champ img')) {
      img.classList.add('broken');
    }
  },
  true
);

// ---------- teclado ----------

document.addEventListener('keydown', (e) => {
  if (!data || $('#stage').classList.contains('hidden') || document.querySelector('dialog[open]')) return;
  if (e.ctrlKey || e.altKey || e.metaKey) return;
  const active = document.activeElement;
  const inSearch = active === $('#search');
  if (!inSearch && active?.matches('input, textarea, select, button')) return;
  const i = wheelList.findIndex((a) => a.id === selectedId);
  const go = (n) => {
    e.preventDefault();
    if (wheelList.length) selectAccount(wheelList[clamp(n, 0, wheelList.length - 1)].id);
  };
  if (e.key === 'ArrowDown') return go(i + 1);
  if (e.key === 'ArrowUp') return go(i - 1);
  if (e.key === 'PageDown') return go(i + 5);
  if (e.key === 'PageUp') return go(i - 5);
  if (e.key === 'Home' && !inSearch) return go(0);
  if (e.key === 'End' && !inSearch) return go(wheelList.length - 1);
  if (e.key === 'Enter') {
    const acc = wheelList.find((a) => a.id === selectedId);
    if (!acc) return;
    e.preventDefault();
    const btn = $('#detail [data-act="play"]');
    Sounds.play();
    play(acc, btn);
    return;
  }
  if (e.key === 'Escape' && inSearch) {
    $('#search').value = '';
    $('#search').blur();
    render();
    return;
  }
  // Escribir cualquier letra busca, sin tener que hacer clic en el buscador.
  if (!inSearch && e.key.length === 1 && e.key !== ' ') $('#search').focus();
});

// Nombres de campeones y versión de ítems (se descargan una vez al día).
async function loadGameInfo() {
  if (gameInfo) return;
  gameInfo = await window.api.gameData().catch(() => null);
  if (gameInfo && data) {
    detailSig = '';
    renderDetail(false);
  }
}
