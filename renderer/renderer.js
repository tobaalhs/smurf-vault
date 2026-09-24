const $ = (s) => document.querySelector(s);
const SERVERS = ['LAS', 'LAN', 'NA', 'BR', 'EUW', 'EUNE', 'OCE', 'TR', 'RU', 'KR', 'JP'];
const TIERS = ['IRON', 'BRONZE', 'SILVER', 'GOLD', 'PLATINUM', 'EMERALD', 'DIAMOND', 'MASTER', 'GRANDMASTER', 'CHALLENGER'];
const TIER_ES = { IRON: 'Hierro', BRONZE: 'Bronce', SILVER: 'Plata', GOLD: 'Oro', PLATINUM: 'Platino', EMERALD: 'Esmeralda', DIAMOND: 'Diamante', MASTER: 'Maestro', GRANDMASTER: 'Gran Maestro', CHALLENGER: 'Retador' };
const DIVS = { IV: 0, III: 1, II: 2, I: 3 };
const DETECT_EVERY_MS = 30_000;

let data = null;
let status = null;
let lockMode = 'unlock';
let detectTimer = null;
let lastSyncError = null;
let pendingSnapshot = null; // cuenta detectada que se está creando como nueva
let addingAccount = false; // se abrió el login para agregar una cuenta: la próxima desconocida se ofrece guardar
let afterRiotKey = null; // qué actualizar después de guardar la API key
let tools = null; // { autoAccept, autoAcceptDelay, clientConnected }
const seenPuuids = new Set(); // cuentas del cliente ya avisadas en esta sesión

hydrateIcons();

// ---------- botones de ventana ----------

function setMaximized(maximized) {
  const b = $('#winMax');
  b.innerHTML = icon(maximized ? 'restore' : 'maximize');
  b.setAttribute('aria-label', maximized ? 'Restaurar' : 'Maximizar');
}
$('#winMin').addEventListener('click', () => window.api.winMinimize());
$('#winMax').addEventListener('click', () => window.api.winToggleMaximize());
$('#winClose').addEventListener('click', () => window.api.winClose());
window.api.onWinState((s) => setMaximized(s.maximized));
window.api.winIsMaximized().then(setMaximized).catch(() => {});
// Doble clic en la barra de arriba maximiza, como en una ventana normal.
document.addEventListener('dblclick', (e) => {
  if (e.target.closest('.topbar, .lock-drag') && !e.target.closest('button, input, select, .menu')) window.api.winToggleMaximize();
});

// ---------- utilidades ----------

function esc(s) {
  return String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
}

function toast(msg, kind = 'info') {
  const el = document.createElement('div');
  el.className = `toast ${kind}`;
  el.textContent = msg;
  $('#toasts').appendChild(el);
  setTimeout(() => el.remove(), kind === 'error' ? 7000 : 3200);
}

/** Ejecuta fn mostrando un spinner en el botón y un toast si falla. */
async function run(btn, fn) {
  btn?.classList.add('loading');
  if (btn) btn.disabled = true;
  try {
    return await fn();
  } catch (e) {
    toast(e.message, 'error');
  } finally {
    btn?.classList.remove('loading');
    if (btn) btn.disabled = false;
  }
}

function rankScore(r) {
  if (!r) return -1;
  return TIERS.indexOf(r.tier) * 10000 + (DIVS[r.division] ?? 0) * 1000 + (r.lp || 0);
}

function bestRank(a) {
  return Math.max(rankScore(a.ranks?.solo), rankScore(a.ranks?.tft));
}

function ago(iso) {
  if (!iso) return 'nunca';
  const m = Math.round((Date.now() - new Date(iso)) / 60000);
  if (m < 1) return 'recién';
  if (m < 60) return `hace ${m} min`;
  const h = Math.round(m / 60);
  if (h < 48) return `hace ${h} h`;
  const d = Math.round(h / 24);
  if (d < 60) return `hace ${d} días`;
  return `hace ${Math.round(d / 30)} meses`;
}

function riotId(a) {
  return a.gameName ? `${a.gameName}${a.tagLine ? '#' + a.tagLine : ''}` : '';
}

function fillServers(select, value = 'LAS') {
  select.innerHTML = `<option value="">—</option>` + SERVERS.map((s) => `<option>${s}</option>`).join('');
  select.value = value;
}

// ---------- tooltips ----------

document.addEventListener('mouseover', (e) => {
  const el = e.target.closest('[data-tip]');
  const tip = $('#tooltip');
  if (!el) return tip.classList.add('hidden');
  tip.textContent = el.dataset.tip;
  tip.classList.remove('hidden');
  const r = el.getBoundingClientRect();
  const t = tip.getBoundingClientRect();
  tip.style.left = `${Math.min(Math.max(8, r.left + r.width / 2 - t.width / 2), innerWidth - t.width - 8)}px`;
  // Debajo del elemento; si no cabe (botones de abajo de la ventana), arriba.
  const below = r.bottom + 8;
  tip.style.top = `${below + t.height > innerHeight - 8 ? Math.max(8, r.top - t.height - 8) : below}px`;
});
document.addEventListener('mousedown', () => $('#tooltip').classList.add('hidden'));

document.addEventListener('click', (e) => {
  const a = e.target.closest('[data-external]');
  if (!a) return;
  e.preventDefault();
  window.api.openExternal(a.dataset.external).catch((err) => toast(err.message, 'error'));
});

// ---------- pantalla de bloqueo ----------

async function showLock() {
  status = await window.api.status();
  loadTheme(status.theme);
  stopDetect();
  data = null;
  closeMenu();
  document.querySelectorAll('dialog[open]').forEach((d) => d.close());
  $('#appScreen').classList.add('hidden');
  $('#lockScreen').classList.remove('hidden');
  $('#lockError').textContent = '';
  $('#pw1').value = $('#pw2').value = '';

  let hasRemote = false;
  if (!status.hasLocal && status.google.linked) {
    $('#lockHint').textContent = 'Buscando tu bóveda en Google Drive…';
    hasRemote = await window.api.remoteExists().catch((e) => {
      $('#lockError').textContent = e.message;
      return false;
    });
  }
  lockMode = status.hasLocal || hasRemote ? 'unlock' : 'create';

  $('#pw2wrap').classList.toggle('hidden', lockMode !== 'create');
  $('#pw2').required = lockMode === 'create';
  $('#lockSubmit').textContent = lockMode === 'create' ? 'Crear bóveda' : 'Desbloquear';
  $('#lockHint').textContent =
    lockMode === 'create'
      ? 'Crea tu contraseña maestra. No se puede recuperar: si la olvidas, pierdes la bóveda.'
      : hasRemote
        ? 'Encontramos tu bóveda en Google Drive. Ingresa tu contraseña maestra.'
        : 'Bóveda bloqueada. Ingresa tu contraseña maestra.';

  setLockBackground(status.lockSkins);

  const g = status.google;
  $('#lockGoogleText').textContent = g.linked
    ? `Sincronizada con ${g.email || 'Google Drive'}`
    : '¿Ya usabas Smurf Vault en otro PC?';
  $('#lockGoogleBtn').classList.toggle('hidden', g.linked);
  $('#pw1').focus();
}

// Fondo de la pantalla de bloqueo: el fondo de perfil de alguna de tus cuentas, al azar.
function setLockBackground(skins = []) {
  const bg = $('#lockBg');
  bg.classList.remove('show');
  if (!skins.length) return;
  const skin = skins[Math.floor(Math.random() * skins.length)];
  const url = `https://cdn.communitydragon.org/latest/champion/${Math.floor(skin / 1000)}/splash-art/skin/${skin % 1000}`;
  const img = new Image();
  img.src = url;
  img.decode().then(
    () => {
      bg.style.backgroundImage = `url("${url}")`;
      bg.classList.add('show');
    },
    () => {} // sin internet: queda el fondo de siempre
  );
}

$('#lockScreen').addEventListener('mousemove', (e) => {
  const x = -(e.clientX / innerWidth - 0.5) * 20;
  const y = -(e.clientY / innerHeight - 0.5) * 12;
  $('#lockBg').style.translate = `${x.toFixed(1)}px ${y.toFixed(1)}px`;
});

$('#lockForm').addEventListener('submit', async (e) => {
  e.preventDefault();
  const pw = $('#pw1').value;
  $('#lockError').textContent = '';
  if (lockMode === 'create' && pw !== $('#pw2').value) {
    $('#lockError').textContent = 'Las contraseñas no coinciden';
    return;
  }
  const btn = $('#lockSubmit');
  btn.classList.add('loading');
  btn.disabled = true;
  try {
    data = lockMode === 'create' ? await window.api.create(pw) : await window.api.unlock(pw);
    showApp();
  } catch (err) {
    $('#lockError').textContent = err.message;
    $('#pw1').select();
  } finally {
    btn.classList.remove('loading');
    btn.disabled = false;
  }
});

$('#lockGoogleBtn').addEventListener('click', (e) =>
  run(e.currentTarget, async () => {
    toast('Se abrió el navegador para iniciar sesión con Google…');
    await window.api.googleLink();
    toast('Google Drive vinculado', 'ok');
    showLock();
  })
);

$('#btnLock').addEventListener('click', () => lockVault());

async function lockVault() {
  await window.api.lock();
  showLock();
  toast('Bóveda bloqueada');
}

// Bloqueada desde el menú de la bandeja.
window.api.onLocked(() => {
  if (!data) return;
  showLock();
});

// ---------- app principal ----------

async function showApp() {
  status = await window.api.status();
  $('#lockScreen').classList.add('hidden');
  $('#appScreen').classList.remove('hidden');
  if (!status.google.linked) updateSync({ state: 'off' });
  tools = await window.api.getTools();
  renderTools();
  if (status.updateReady) showUpdate(status.updateReady);
  render();
  startDetect();
  loadGameInfo();
}

function showUpdate(version) {
  $('#updateText').textContent = `Actualizar a v${version}`;
  $('#updateChip').classList.remove('hidden');
}

window.api.onUpdate((u) => {
  showUpdate(u.version);
  toast(`Hay una versión nueva (v${u.version}) lista para instalar`, 'ok');
});

$('#updateChip').addEventListener('click', () => window.api.installUpdate());

function sortedAccounts() {
  const q = $('#search').value.trim().toLowerCase();
  const list = data.accounts.filter(
    (a) => !q || [a.label, a.username, a.gameName, a.tagLine, a.notes, a.server].some((v) => (v || '').toLowerCase().includes(q))
  );
  const cmp = {
    // Mejor rango entre LoL Solo/Dúo y TFT.
    rank: (a, b) => bestRank(b) - bestRank(a) || (b.level || 0) - (a.level || 0),
    level: (a, b) => (b.level || 0) - (a.level || 0),
    name: (a, b) => (riotId(a) || a.username).localeCompare(riotId(b) || b.username),
    // Las cuentas sin partidas registradas quedan al final.
    played: (a, b) => (b.lastPlayedAt || '').localeCompare(a.lastPlayedAt || ''),
  }[$('#sort').value];
  // Las favoritas siempre arriba.
  return list.sort((a, b) => !!b.favorite - !!a.favorite || cmp(a, b));
}

function render() {
  const has = data.accounts.length > 0;
  $('#emptyView').classList.toggle('hidden', has);
  $('#stage').classList.toggle('hidden', !has);
  if (has) renderCarousel(sortedAccounts());
}

$('#search').addEventListener('input', render);
// El orden elegido se recuerda en este PC.
try {
  const saved = localStorage.getItem('sort');
  if (saved && [...$('#sort').options].some((o) => o.value === saved)) $('#sort').value = saved;
} catch {}
$('#sort').addEventListener('change', () => {
  try {
    localStorage.setItem('sort', $('#sort').value);
  } catch {}
  render();
});

function flashCopied(btn) {
  btn.classList.add('copied');
  btn.querySelector('.ic').outerHTML = icon('check');
  setTimeout(() => {
    btn.classList.remove('copied');
    btn.querySelector('.ic').outerHTML = icon('copy');
  }, 1400);
}

// ---------- diálogos ----------

document.querySelectorAll('dialog').forEach((d) =>
  d.querySelectorAll('[data-close]').forEach((b) => b.addEventListener('click', () => d.close()))
);

function openAccount(acc = {}) {
  const f = $('#accountForm');
  f.reset();
  fillServers(f.server, acc.server ?? 'LAS');
  for (const k of ['id', 'label', 'username', 'password', 'gameName', 'tagLine', 'notes']) f[k].value = acc[k] || '';
  f.password.type = 'password';
  $('#accountTitle').textContent = acc.id ? 'Editar cuenta' : 'Nueva cuenta';
  $('#accountDelete').classList.toggle('hidden', !acc.id);
  $('#accountForget').classList.toggle('hidden', !data.sessions?.[acc.id]);
  $('#accountDialog').showModal();
  (acc.username ? f.label : f.username).focus();
}

$('[data-toggle-pw]').addEventListener('click', () => {
  const i = $('#accountForm').password;
  i.type = i.type === 'password' ? 'text' : 'password';
});

$('#accountForm').addEventListener('submit', async (e) => {
  e.preventDefault();
  const f = e.currentTarget;
  const acc = Object.fromEntries(new FormData(f));
  // Si pegaron "Nombre#TAG" en el campo de nombre, lo separamos.
  if (acc.gameName.includes('#')) [acc.gameName, acc.tagLine] = acc.gameName.split('#');
  const before = acc.id && data.accounts.find((a) => a.id === acc.id);
  const riotIdChanged =
    before && riotId(before).toLowerCase() !== riotId({ gameName: acc.gameName.trim(), tagLine: acc.tagLine.trim() }).toLowerCase();
  await run(f.querySelector('[type=submit]'), async () => {
    data = await window.api.saveAccount(acc);
    const snap = pendingSnapshot;
    pendingSnapshot = null;
    if (snap && !acc.id) {
      const created = data.accounts[data.accounts.length - 1];
      data = await window.api.linkClient(created.id, snap);
    }
    $('#accountDialog').close();
    render();
    // Riot ID corregido: buscamos al tiro la cuenta nueva si hay API key.
    if (riotIdChanged && acc.gameName.trim() && acc.tagLine.trim() && data.settings.riotApiKey) {
      toast('Riot ID cambiado, buscando la cuenta…');
      refreshRanks([acc.id]);
    } else {
      toast(acc.id ? 'Cambios guardados' : 'Cuenta agregada', 'ok');
    }
  });
});

$('#accountDelete').addEventListener('click', async () => {
  const f = $('#accountForm');
  if (!(await confirmDialog(`Se eliminará "${f.gameName.value || f.username.value}" de tu bóveda. No se puede deshacer.`))) return;
  await run(null, async () => {
    data = await window.api.deleteAccount(f.id.value);
    $('#accountDialog').close();
    render();
    toast('Cuenta eliminada');
  });
});

$('#accountForget').addEventListener('click', (e) =>
  run(e.currentTarget, async () => {
    data = await window.api.forgetSession($('#accountForm').id.value);
    $('#accountForget').classList.add('hidden');
    render();
    toast('Sesión olvidada');
  })
);

function confirmDialog(text) {
  const d = $('#confirmDialog');
  $('#confirmText').textContent = text;
  d.returnValue = '';
  d.showModal();
  return new Promise((res) => d.addEventListener('close', () => res(d.returnValue === 'yes'), { once: true }));
}

function newAccount() {
  pendingSnapshot = null;
  openAccount();
}

// La forma principal de agregar: iniciar sesión en el cliente. La manual queda en el menú.
function openAdd() {
  $('#addDialog').showModal();
}
$('#btnAdd').addEventListener('click', openAdd);
$('#emptyAdd').addEventListener('click', openAdd);
$('#addManual').addEventListener('click', () => {
  $('#addDialog').close();
  newAccount();
});
$('#addOpenLogin').addEventListener('click', (e) =>
  run(e.currentTarget, async () => {
    await window.api.loginOther();
    $('#addDialog').close();
    addingAccount = true;
    detectSoon();
    showGuide('Inicia sesión en el Riot Client con la cuenta que quieres agregar y marca <b>Mantener sesión iniciada</b>. El LoL se abre solo y aquí te pedimos confirmarla.');
  })
);

// ---------- menú ⋯ ----------

function closeMenu() {
  $('#menu').classList.add('hidden');
}

$('#btnMenu').addEventListener('click', (e) => {
  e.stopPropagation();
  $('#menu').classList.toggle('hidden');
});
document.addEventListener('click', (e) => {
  if (!e.target.closest('.menu-wrap')) closeMenu();
});

$('#menu').addEventListener('click', (e) => {
  const item = e.target.closest('[data-menu]');
  if (!item) return;
  closeMenu();
  ({
    riotRefresh: () => refreshRanks(undefined, null),
    manual: newAccount,
    tools: openTools,
    import: openImport,
    settings: openSettings,
    lock: lockVault,
  })[item.dataset.menu]();
});

// ---------- importar ----------

function parseImport(text) {
  return text
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter(Boolean)
    .map((l) => l.match(/^(\S+?)\s*(?::|;|\t|\s\/\s|\s\|\s|\s)\s*(\S+)$/))
    .filter(Boolean)
    .map((m) => ({ username: m[1], password: m[2] }));
}

function openImport() {
  $('#importText').value = '';
  $('#importPreview').textContent = '';
  fillServers($('#importServer'));
  $('#importDialog').showModal();
}
$('#emptyImport').addEventListener('click', openImport);

$('#importText').addEventListener('input', () => {
  const lines = $('#importText').value.split(/\r?\n/).filter((l) => l.trim()).length;
  const ok = parseImport($('#importText').value).length;
  $('#importPreview').textContent = `${ok} cuentas reconocidas${lines > ok ? ` · ${lines - ok} líneas ignoradas` : ''}`;
});

$('#importForm').addEventListener('submit', async (e) => {
  e.preventDefault();
  const rows = parseImport($('#importText').value).map((r) => ({ ...r, server: $('#importServer').value }));
  if (!rows.length) return toast('No reconocí ninguna línea', 'error');
  await run(e.currentTarget.querySelector('[type=submit]'), async () => {
    data = await window.api.importAccounts(rows);
    $('#importDialog').close();
    render();
    toast(`${rows.length} cuentas importadas`, 'ok');
  });
});

// ---------- cambiar de cuenta ----------

// Después de cambiar de cuenta buscamos el cliente cada 5 s (no cada 30) hasta que el LoL abra.
function detectSoon(timeoutMs = 180_000) {
  const until = Date.now() + timeoutMs;
  const timer = setInterval(async () => {
    const found = await detect().catch(() => null);
    if (found || Date.now() > until || !data) clearInterval(timer);
  }, 5000);
}

async function play(acc, btn) {
  await run(btn, async () => {
    const res = await window.api.play(acc.id);
    detectSoon();
    if (res.restored) {
      hideGuide();
      toast(`Abriendo LoL con ${riotId(acc) || acc.username}…`, 'ok');
    } else {
      showGuide(
        `Inicia sesión con <b>${esc(acc.username)}</b>${acc.password ? ' (la contraseña está copiada: pégala con Ctrl V)' : ''} y marca <b>Mantener sesión iniciada</b>. La próxima vez entras con un clic.`
      );
    }
  });
}

// Instrucciones mientras se inicia sesión en el cliente: quedan hasta que se detecte la cuenta o se cierren.
function showGuide(html) {
  const b = $('#guideBanner');
  b.innerHTML = `${icon('logIn')}<span>${html}</span><div class="spacer"></div>
    <button class="btn ghost icon-only sm" id="guideClose" aria-label="Cerrar">${icon('x')}</button>`;
  b.classList.remove('hidden');
  $('#guideClose').onclick = () => {
    hideGuide();
    addingAccount = false;
  };
}

function hideGuide() {
  $('#guideBanner').classList.add('hidden');
}

// ---------- detección del cliente ----------

async function detect({ manual = false } = {}) {
  const res = await window.api.detect(manual);
  if (!res) {
    if (manual) toast('No encontré el cliente de LoL abierto con una sesión iniciada', 'error');
    return null;
  }
  const { snapshot, matchedId, autoLinked } = res;
  hideGuide();
  const adding = addingAccount;
  addingAccount = false;
  const hadSession = !!data.sessions?.[matchedId];
  data = res.data;
  render();
  if (matchedId && !hadSession && data.sessions?.[matchedId]) {
    toast(`Sesión de ${snapshot.gameName} guardada: la próxima vez entras con un clic`, 'ok');
  }
  if (matchedId) {
    const name = `${snapshot.gameName}#${snapshot.tagLine}`;
    const acc = data.accounts.find((a) => a.id === matchedId);
    if (autoLinked) toast(`${name} reconocida y vinculada a "${acc?.label || acc?.username}"`, 'ok');
    else if (manual || !seenPuuids.has(snapshot.puuid)) toast(`${name} actualizada desde el cliente`, 'ok');
    seenPuuids.add(snapshot.puuid);
    hideBanner();
    return res;
  }
  if (!manual && !adding && seenPuuids.has(snapshot.puuid)) return res;
  seenPuuids.add(snapshot.puuid);
  showBanner(snapshot, adding);
  return res;
}

// `adding`: la cuenta viene de "Agregar cuenta", así que lo normal es guardarla como nueva.
function showBanner(snap, adding = false) {
  const b = $('#detectBanner');
  const who = `<b>${esc(snap.gameName)}#${esc(snap.tagLine)}</b> · ${esc(snap.server)} · nivel ${snap.level}`;
  b.innerHTML = adding
    ? `${icon('gamepad')}
      <span>Iniciaste sesión con ${who}. ¿La guardamos en tu bóveda?</span>
      <div class="spacer"></div>
      <button class="btn subtle sm" id="bannerLink">Es una que ya tengo</button>
      <button class="btn primary sm" id="bannerNew">Guardar cuenta</button>
      <button class="btn ghost icon-only sm" id="bannerClose" aria-label="Cerrar">${icon('x')}</button>`
    : `${icon('gamepad')}
      <span>En el cliente está abierta ${who}. ¿La vinculamos a una de tus cuentas?</span>
      <div class="spacer"></div>
      <button class="btn primary sm" id="bannerLink">Vincular</button>
      <button class="btn ghost icon-only sm" id="bannerClose" aria-label="Cerrar">${icon('x')}</button>`;
  b.classList.remove('hidden');
  $('#bannerLink').onclick = () => openLinkDialog(snap);
  $('#bannerClose').onclick = hideBanner;
  if (adding) $('#bannerNew').onclick = () => saveDetectedAsNew(snap);
}

function saveDetectedAsNew(snap) {
  hideBanner();
  pendingSnapshot = snap;
  openAccount({ username: snap.username, gameName: snap.gameName, tagLine: snap.tagLine, server: snap.server });
}

function hideBanner() {
  $('#detectBanner').classList.add('hidden');
}

function openLinkDialog(snap) {
  $('#linkInfo').innerHTML = `En el cliente está <b>${esc(snap.gameName)}#${esc(snap.tagLine)}</b> (${esc(snap.server)}, nivel ${snap.level}). Elige con qué cuenta guardada iniciaste sesión:`;
  const candidates = data.accounts.filter((a) => !a.puuid).concat(data.accounts.filter((a) => a.puuid));
  $('#linkList').innerHTML = candidates.length
    ? candidates
        .map(
          (a) => `<button type="button" class="linkitem" data-id="${a.id}">
            ${icon('user')}
            <span><b>${esc(a.username)}</b><br><span class="sub">${esc(riotId(a) || a.label || 'Sin Riot ID')}${a.server ? ' · ' + esc(a.server) : ''}</span></span>
            ${a.puuid ? '<span class="chip">Ya vinculada</span>' : ''}
          </button>`
        )
        .join('')
    : '<p class="hint">Todavía no tienes cuentas guardadas.</p>';
  $('#linkList').onclick = async (e) => {
    const item = e.target.closest('.linkitem');
    if (!item) return;
    await run(null, async () => {
      data = await window.api.linkClient(item.dataset.id, snap);
      $('#linkDialog').close();
      hideBanner();
      render();
      toast('Cuenta vinculada', 'ok');
    });
  };
  $('#linkNew').onclick = () => {
    $('#linkDialog').close();
    saveDetectedAsNew(snap);
  };
  $('#linkDialog').showModal();
}

function startDetect() {
  stopDetect();
  detect().catch(() => {});
  detectTimer = setInterval(() => detect().catch(() => {}), DETECT_EVERY_MS);
}

function stopDetect() {
  clearInterval(detectTimer);
  detectTimer = null;
}

/**
 * Actualiza una cuenta leyéndola del cliente de LoL (al instante, sin API key). Solo sirve si es la
 * cuenta abierta en el cliente; si hay otra, lo avisa.
 */
async function updateFromClient(acc, btn) {
  await run(btn, async () => {
    const res = await detect({ manual: true }); // ya avisa si no hay cliente o qué cuenta se actualizó
    if (res && res.matchedId !== acc.id) {
      toast(`En el cliente está abierta ${res.snapshot.gameName}#${res.snapshot.tagLine}, no esta cuenta. Entra con Jugar para actualizarla.`, 'error');
    }
  });
}

// ---------- actualizar rangos (API de Riot) ----------

async function refreshRanks(ids, btn) {
  if (!data.settings.riotApiKey) {
    afterRiotKey = { ids, btn };
    $('#riotDialogKey').value = '';
    $('#riotDialog').showModal();
    $('#riotDialogKey').focus();
    return;
  }
  const all = !ids?.length;
  if (all) showRiotProgress({ done: 0, total: data.accounts.length, name: '' });
  try {
    await run(btn, async () => {
      const res = await window.api.refreshRanks(ids);
      data = res.data;
      render();
      const ok = res.total - res.errors.length;
      if (all) toast(`${ok} de ${res.total} cuentas actualizadas con la API de Riot`, res.errors.length ? 'info' : 'ok');
      else if (!res.errors.length) toast('Cuenta actualizada', 'ok');
      res.errors.slice(0, 3).forEach((m) => toast(m, 'error'));
      if (res.errors.length > 3) toast(`…y ${res.errors.length - 3} errores más`, 'error');
    });
  } finally {
    hideRiotProgress();
  }
}

// Panel de avance mientras se actualiza con la API de Riot (puede tardar: son varias consultas por cuenta).
function showRiotProgress({ done, total, name }) {
  let el = $('#riotProgress');
  if (!el) {
    el = document.createElement('div');
    el.id = 'riotProgress';
    el.className = 'toast progress';
    el.innerHTML = `<span class="spin"></span><div class="p-body"><b>Actualizando con la API de Riot</b><small></small><div class="p-bar"><i></i></div></div>`;
    $('#toasts').prepend(el);
  }
  el.querySelector('small').textContent = `${Math.min(done + 1, total)} de ${total}${name ? ' · ' + name : ''}`;
  el.querySelector('.p-bar i').style.width = `${total ? Math.round((done / total) * 100) : 0}%`;
}

function hideRiotProgress() {
  $('#riotProgress')?.remove();
}

window.api.onRiotProgress((p) => {
  if ($('#riotProgress')) showRiotProgress(p);
});


$('#riotForm').addEventListener('submit', async (e) => {
  e.preventDefault();
  const key = $('#riotDialogKey').value.trim();
  if (!key.startsWith('RGAPI-')) return toast('La key debería empezar con RGAPI-', 'error');
  await run(e.currentTarget.querySelector('[type=submit]'), async () => {
    data = await window.api.saveSettings({ riotApiKey: key });
    $('#riotDialog').close();
    const next = afterRiotKey;
    afterRiotKey = null;
    if (next) refreshRanks(next.ids, next.btn);
  });
});

// ---------- ajustes / Google ----------

async function openSettings() {
  status = await window.api.status();
  renderThemeGrid();
  tools = await window.api.getTools();
  renderWindowSettings();
  renderGoogle();
  $('#riotKey').value = data.settings.riotApiKey || '';
  $('#appVersion').textContent = `Smurf Vault v${status.version}`;
  $('#settingsDialog').showModal();
}

function renderGoogle() {
  const g = status.google;
  $('#googleBadge').textContent = g.linked ? 'Conectado' : 'No conectado';
  $('#googleBadge').className = `badge ${g.linked ? 'ok' : ''}`;
  $('#googleStatus').innerHTML = !g.hasCredentials
    ? 'Para sincronizar con Drive, primero carga el <code>credentials.json</code> de Google (esta versión no lo trae incluido).'
    : g.linked
      ? `Tu bóveda cifrada se guarda en una carpeta oculta del Drive de <b>${esc(g.email || 'tu cuenta de Google')}</b>. Cada cambio se sube solo.`
      : 'Tu bóveda solo está en este PC. Vincula Google para tener un respaldo y usarla en otros PCs.';
  $('#googleCredsBtn').classList.toggle('hidden', g.hasCredentials);
  $('#googleLinkBtn').classList.toggle('hidden', g.linked || !g.hasCredentials);
  $('#googleSyncBtn').classList.toggle('hidden', !g.linked);
  $('#googleUnlinkBtn').classList.toggle('hidden', !g.linked);
}

$('#syncChip').addEventListener('click', openSettings);

$('#googleLinkBtn').addEventListener('click', (e) =>
  run(e.currentTarget, async () => {
    toast('Se abrió el navegador para iniciar sesión con Google…');
    await window.api.googleLink();
    status = await window.api.status();
    renderGoogle();
    toast('Google Drive vinculado, subiendo bóveda…', 'ok');
  })
);

$('#googleCredsBtn').addEventListener('click', (e) =>
  run(e.currentTarget, async () => {
    const g = await window.api.googleImportCredentials();
    status = await window.api.status();
    renderGoogle();
    if (g.hasCredentials) toast('Credenciales cargadas. Ahora vincula tu cuenta de Google.', 'ok');
  })
);

$('#googleSyncBtn').addEventListener('click', (e) => run(e.currentTarget, () => window.api.syncNow()));

$('#googleUnlinkBtn').addEventListener('click', (e) =>
  run(e.currentTarget, async () => {
    await window.api.googleUnlink();
    status = await window.api.status();
    renderGoogle();
    updateSync({ state: 'off' });
    toast('Google desvinculado. El archivo en Drive no se borra.');
  })
);

$('#settingsForm').addEventListener('submit', async (e) => {
  e.preventDefault();
  await run(e.currentTarget.querySelector('[type=submit]'), async () => {
    data = await window.api.saveSettings({ riotApiKey: $('#riotKey').value.trim() });
    $('#settingsDialog').close();
    toast('Ajustes guardados', 'ok');
  });
});

function updateSync(s) {
  const chip = $('#syncChip');
  const map = {
    off: ['Solo local', 'Tu bóveda solo está en este PC. Clic para vincular Google Drive.'],
    syncing: ['Sincronizando…', 'Subiendo cambios a Google Drive'],
    ok: ['Drive al día', `Guardado en Google Drive${s.at ? ' · ' + new Date(s.at).toLocaleTimeString() : ''}`],
    error: ['Error de Drive', s.message],
  };
  const [text, tip] = map[s.state];
  $('#syncText').textContent = text;
  chip.className = `sync-chip ${s.state}`;
  chip.dataset.tip = tip;
  // Un mismo error se avisa una sola vez, no en cada guardado.
  if (s.state === 'error' && s.message !== lastSyncError) toast(`Google Drive: ${s.message}`, 'error');
  lastSyncError = s.state === 'error' ? s.message : null;
}

window.api.onSync(updateSync);

// Datos que cambian solos (fin de partida, LP actualizados después de la partida).
window.api.onData((d) => {
  if (!data || !d.data) return; // bóveda bloqueada
  data = d.data;
  render();
  if (d.reason === 'game-end') toast(`Partida terminada · ${d.account} actualizada`, 'ok');
});

// ---------- herramientas (autoaceptar) ----------

function renderTools() {
  if (!tools) return;
  $('#autoChip').classList.toggle('hidden', !tools.autoAccept);
  $('#offlineChip').classList.toggle('hidden', !tools.appearOffline);
  $('#autoAcceptToggle').checked = tools.autoAccept;
  $('#autoAcceptBody').classList.toggle('off', !tools.autoAccept);
  document.querySelectorAll('#autoAcceptDelay button').forEach((b) =>
    b.classList.toggle('active', Number(b.dataset.delay) === tools.autoAcceptDelay)
  );
  $('#offlineToggle').checked = !!tools.appearOffline;
  const st = $('#clientStatus');
  st.classList.toggle('on', tools.autoAccept && tools.clientConnected === true);
  st.lastElementChild.textContent = !tools.autoAccept
    ? 'Apagado'
    : tools.clientConnected === true
      ? 'Cliente de LoL conectado · esperando partida'
      : tools.clientConnected === false
        ? 'Cliente de LoL no detectado · ábrelo y se conecta solo'
        : 'Buscando el cliente de LoL…';
}

async function openTools() {
  tools = await window.api.getTools();
  renderTools();
  $('#toolsDialog').showModal();
}

async function setTools(patch) {
  await run(null, async () => {
    tools = await window.api.setTools(patch);
    renderTools();
  });
}

$('#autoChip').addEventListener('click', openTools);
$('#offlineChip').addEventListener('click', openTools);
$('#offlineToggle').addEventListener('change', async (e) => {
  await setTools({ appearOffline: e.target.checked }); // renderTools muestra u oculta el chip
  toast(e.target.checked ? 'Apareces desconectado en el LoL' : 'Vuelves a aparecer en línea', 'ok');
});

function renderWindowSettings() {
  $('#closeToTray').checked = tools.closeToTray;
  $('#openAtLogin').checked = tools.openAtLogin;
  $('#openAtLoginHint').textContent = tools.closeToTray
    ? 'Parte en la bandeja, sin abrir la ventana.'
    : 'Se abre la ventana al prender el PC (con la bandeja activada partiría oculto).';
}

$('#closeToTray').addEventListener('change', async (e) => {
  await setTools({ closeToTray: e.target.checked });
  renderWindowSettings();
  toast(e.target.checked ? 'Al cerrar, Smurf Vault seguirá en la bandeja' : 'Al cerrar, Smurf Vault se cerrará del todo');
});

$('#openAtLogin').addEventListener('change', async (e) => {
  const on = e.target.checked;
  await setTools({ openAtLogin: on });
  renderWindowSettings(); // si falló, el interruptor vuelve a como estaba
  if (tools.openAtLogin === on) toast(on ? 'Smurf Vault se abrirá al iniciar Windows' : 'Ya no se abrirá al iniciar Windows');
});
$('#autoAcceptToggle').addEventListener('change', (e) => {
  setTools({ autoAccept: e.target.checked });
  toast(e.target.checked ? 'Autoaceptar activado' : 'Autoaceptar desactivado', e.target.checked ? 'ok' : 'info');
});
$('#autoAcceptDelay').addEventListener('click', (e) => {
  const b = e.target.closest('[data-delay]');
  if (b) setTools({ autoAcceptDelay: Number(b.dataset.delay) });
});

window.api.onTools((s) => {
  if (s.event === 'accepted') toast('Partida aceptada ✓', 'ok');
  if (s.event === 'status' && tools) {
    tools.clientConnected = s.connected;
    renderTools();
  }
});

// ---------- atajos de teclado ----------

document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') closeMenu();
  if (!data || !e.ctrlKey || document.querySelector('dialog[open]')) return;
  const k = e.key.toLowerCase();
  if (k === 'l') { e.preventDefault(); lockVault(); }
  if (k === 'f') { e.preventDefault(); $('#search').focus(); }
  if (k === 'n') { e.preventDefault(); openAdd(); }
});

showLock();
