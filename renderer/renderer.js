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
let afterRiotKey = null; // qué actualizar después de guardar la API key
let tools = null; // { autoAccept, autoAcceptDelay, clientConnected }
const seenPuuids = new Set(); // cuentas del cliente ya avisadas en esta sesión

hydrateIcons();

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
  tip.style.top = `${r.bottom + 8}px`;
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

  const g = status.google;
  $('#lockGoogleText').textContent = g.linked
    ? `Sincronizada con ${g.email || 'Google Drive'}`
    : '¿Ya usabas Smurf Vault en otro PC?';
  $('#lockGoogleBtn').classList.toggle('hidden', g.linked);
  $('#pw1').focus();
}

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

async function lockVault() {
  await window.api.lock();
  showLock();
  toast('Bóveda bloqueada');
}

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
  return list.sort(cmp);
}

function rankRow(title, r) {
  if (!r) {
    return `<div class="rank"><span class="rank-q">${title}</span><span class="rank-name t-none">Sin rango</span><span></span></div>`;
  }
  const games = r.wins + r.losses;
  const wr = games ? Math.round((r.wins / games) * 100) : 0;
  return `<div class="rank">
    <span class="rank-q">${title}</span>
    <span class="rank-name t-${r.tier.toLowerCase()}"><span class="gem"></span>${TIER_ES[r.tier] || r.tier}${r.division ? ' ' + r.division : ''} <span class="lp">${r.lp} LP</span></span>
    <span class="rank-wr">${games ? `${r.wins}V ${r.losses}D · ${wr}%<div class="wrbar"><i class="${wr >= 50 ? 'good' : ''}" data-w="${wr}"></i></div>` : ''}</span>
  </div>`;
}

function card(a) {
  const initial = esc((a.gameName || a.username || '?')[0].toUpperCase());
  const avatar =
    a.iconId != null
      ? `<img class="avatar" src="https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/default/v1/profile-icons/${a.iconId}.jpg" alt="" data-initial="${initial}" />`
      : `<div class="avatar ph">${initial}</div>`;
  const name = a.gameName
    ? `<div class="riotid">${esc(a.gameName)}<span class="tag">#${esc(a.tagLine)}</span></div>`
    : `<div class="riotid unknown">Riot ID desconocido</div>`;
  return `<article class="card" data-id="${a.id}">
    <div class="card-head">
      <div class="avatar-wrap">${avatar}${a.level ? `<span class="lvl">${a.level}</span>` : ''}</div>
      <div class="who">
        ${name}
        <div class="meta">
          ${a.server ? `<span class="chip">${esc(a.server)}</span>` : ''}
          ${a.label ? `<span class="chip label">${esc(a.label)}</span>` : ''}
          ${a.puuid ? `<span class="chip" data-tip="Vinculada con el cliente: se actualiza sola al iniciar sesión">${icon('link')}Vinculada</span>` : ''}
        </div>
      </div>
      <div class="card-actions">
        <button class="btn ghost icon-only" data-act="refresh" data-tip="Actualizar esta cuenta">${icon('refresh')}</button>
        <button class="btn ghost icon-only" data-act="edit" data-tip="Editar">${icon('pencil')}</button>
      </div>
    </div>
    <div class="ranks">
      ${rankRow('Solo/Dúo', a.ranks?.solo)}
      ${rankRow('Flex', a.ranks?.flex)}
      ${a.ranks?.tft ? rankRow('TFT', a.ranks.tft) : ''}
      ${a.ranks?.doubleUp ? rankRow('Double Up', a.ranks.doubleUp) : ''}
    </div>
    <div class="creds">
      <button class="cred" data-act="copy-user">
        <span class="cred-label">Usuario</span><span class="cred-value">${esc(a.username)}</span>${icon('copy')}
      </button>
      <button class="cred" data-act="copy-pass">
        <span class="cred-label">Contraseña</span><span class="cred-value secret">••••••••</span>${icon('copy')}
      </button>
    </div>
    ${a.notes ? `<p class="notes">${esc(a.notes)}</p>` : ''}
    <div class="card-foot">
      <span class="played${a.lastPlayedAt ? '' : ' none'}" data-tip="${a.lastPlayedAt ? new Date(a.lastPlayedAt).toLocaleString() : 'Se obtiene al detectar la cuenta en el cliente o con Actualizar rangos'}">${icon('gamepad')}${a.lastPlayedAt ? 'Jugó ' + ago(a.lastPlayedAt) : 'Sin partidas registradas'}</span>
      <span class="spacer"></span>
      <span>Datos ${a.lastSyncedAt ? ago(a.lastSyncedAt) : 'sin actualizar'}</span>
    </div>
  </article>`;
}

function render() {
  const list = sortedAccounts();
  $('#grid').innerHTML = list.map(card).join('');
  // El CSP no permite style inline: el ancho de las barras de winrate se asigna aquí.
  document.querySelectorAll('.wrbar i[data-w]').forEach((i) => (i.style.width = i.dataset.w + '%'));
  $('#count').textContent = data.accounts.length;
  $('#empty').classList.toggle('hidden', data.accounts.length > 0);
  $('#noResults').classList.toggle('hidden', !(data.accounts.length && !list.length));
}

// Si el ícono no carga (sin internet, ícono nuevo), mostramos la inicial.
$('#grid').addEventListener(
  'error',
  (e) => {
    if (!e.target.matches('img.avatar')) return;
    const ph = document.createElement('div');
    ph.className = 'avatar ph';
    ph.textContent = e.target.dataset.initial || '?';
    e.target.replaceWith(ph);
  },
  true
);

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

$('#grid').addEventListener('click', async (e) => {
  const btn = e.target.closest('[data-act]');
  if (!btn) return;
  const id = btn.closest('.card').dataset.id;
  const acc = data.accounts.find((a) => a.id === id);
  switch (btn.dataset.act) {
    case 'edit':
      openAccount(acc);
      break;
    case 'copy-user':
      await run(null, async () => {
        await window.api.copy(id, 'username');
        flashCopied(btn);
      });
      break;
    case 'copy-pass':
      await run(null, async () => {
        await window.api.copy(id, 'password');
        flashCopied(btn);
        toast('Contraseña copiada · se borra del portapapeles en 30 s', 'ok');
      });
      break;
    case 'refresh':
      refreshRanks([id], btn);
      break;
  }
});

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
    toast(acc.id ? 'Cambios guardados' : 'Cuenta agregada', 'ok');
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
$('#btnAdd').addEventListener('click', newAccount);
$('#emptyAdd').addEventListener('click', newAccount);

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
  ({ tools: openTools, import: openImport, settings: openSettings, lock: lockVault })[item.dataset.menu]();
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

// ---------- detección del cliente ----------

async function detect({ manual = false } = {}) {
  const res = await window.api.detect(manual);
  if (!res) {
    if (manual) toast('No encontré el cliente de LoL abierto con una sesión iniciada', 'error');
    return;
  }
  const { snapshot, matchedId } = res;
  data = res.data;
  render();
  if (matchedId) {
    if (manual || !seenPuuids.has(snapshot.puuid)) toast(`${snapshot.gameName}#${snapshot.tagLine} actualizada desde el cliente`, 'ok');
    seenPuuids.add(snapshot.puuid);
    hideBanner();
    return;
  }
  if (!manual && seenPuuids.has(snapshot.puuid)) return;
  seenPuuids.add(snapshot.puuid);
  showBanner(snapshot);
}

function showBanner(snap) {
  const b = $('#detectBanner');
  b.innerHTML = `${icon('gamepad')}
    <span>En el cliente está abierta <b>${esc(snap.gameName)}#${esc(snap.tagLine)}</b> · ${esc(snap.server)} · nivel ${snap.level}. ¿La vinculamos a una de tus cuentas?</span>
    <div class="spacer"></div>
    <button class="btn primary sm" id="bannerLink">Vincular</button>
    <button class="btn ghost icon-only sm" id="bannerClose" aria-label="Cerrar">${icon('x')}</button>`;
  b.classList.remove('hidden');
  $('#bannerLink').onclick = () => openLinkDialog(snap);
  $('#bannerClose').onclick = hideBanner;
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
    hideBanner();
    pendingSnapshot = snap;
    openAccount({ gameName: snap.gameName, tagLine: snap.tagLine, server: snap.server });
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

$('#btnDetect').addEventListener('click', (e) => run(e.currentTarget, () => detect({ manual: true })));

// ---------- actualizar rangos (API de Riot) ----------

async function refreshRanks(ids, btn) {
  if (!data.settings.riotApiKey) {
    afterRiotKey = { ids, btn };
    $('#riotDialogKey').value = '';
    $('#riotDialog').showModal();
    $('#riotDialogKey').focus();
    return;
  }
  await run(btn, async () => {
    const res = await window.api.refreshRanks(ids);
    data = res.data;
    render();
    if (res.errors.length) res.errors.slice(0, 3).forEach((m) => toast(m, 'error'));
    else toast(ids ? 'Cuenta actualizada' : 'Rangos actualizados', 'ok');
  });
}

$('#btnRefresh').addEventListener('click', (e) => refreshRanks(undefined, e.currentTarget));

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
    ? 'Para sincronizar con Drive, primero carga el <code>credentials.json</code> de Google (lo creas siguiendo GOOGLE_SETUP.md, o te lo pasa quien te compartió la app).'
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

// ---------- herramientas (autoaceptar) ----------

function renderTools() {
  if (!tools) return;
  $('#autoChip').classList.toggle('hidden', !tools.autoAccept);
  $('#autoAcceptToggle').checked = tools.autoAccept;
  $('#autoAcceptBody').classList.toggle('off', !tools.autoAccept);
  document.querySelectorAll('#autoAcceptDelay button').forEach((b) =>
    b.classList.toggle('active', Number(b.dataset.delay) === tools.autoAcceptDelay)
  );
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
  if (k === 'n') { e.preventDefault(); newAccount(); }
});

showLock();
