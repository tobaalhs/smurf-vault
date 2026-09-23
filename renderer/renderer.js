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
const seenPuuids = new Set(); // cuentas del cliente ya avisadas en esta sesión
let pendingSnapshot = null;

// ---------- utilidades ----------

function esc(s) {
  return String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
}

function toast(msg, kind = 'info') {
  const el = document.createElement('div');
  el.className = `toast ${kind}`;
  el.textContent = msg;
  $('#toasts').appendChild(el);
  setTimeout(() => el.remove(), kind === 'error' ? 7000 : 3500);
}

async function run(btn, fn) {
  const label = btn?.textContent;
  if (btn) {
    btn.disabled = true;
    btn.textContent = '…';
  }
  try {
    return await fn();
  } catch (e) {
    toast(e.message, 'error');
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.textContent = label;
    }
  }
}

function rankScore(r) {
  if (!r) return -1;
  return TIERS.indexOf(r.tier) * 10000 + (DIVS[r.division] ?? 0) * 1000 + (r.lp || 0);
}

function rankText(r) {
  if (!r) return 'Sin rango';
  return `${TIER_ES[r.tier] || r.tier} ${r.division || ''} · ${r.lp} LP`.replace('  ', ' ');
}

function winrate(r) {
  if (!r || !(r.wins + r.losses)) return '';
  return `${r.wins}V ${r.losses}D · ${Math.round((r.wins / (r.wins + r.losses)) * 100)}%`;
}

function ago(iso) {
  if (!iso) return 'nunca';
  const m = Math.round((Date.now() - new Date(iso)) / 60000);
  if (m < 1) return 'recién';
  if (m < 60) return `hace ${m} min`;
  const h = Math.round(m / 60);
  if (h < 48) return `hace ${h} h`;
  return `hace ${Math.round(h / 24)} días`;
}

function riotId(a) {
  return a.gameName ? `${a.gameName}${a.tagLine ? '#' + a.tagLine : ''}` : '';
}

function fillServers(select, value = 'LAS') {
  select.innerHTML = `<option value="">—</option>` + SERVERS.map((s) => `<option>${s}</option>`).join('');
  select.value = value;
}

// ---------- pantalla de bloqueo ----------

async function showLock() {
  status = await window.api.status();
  stopDetect();
  data = null;
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

  $('#pw2').classList.toggle('hidden', lockMode !== 'create');
  $('#pw2').required = lockMode === 'create';
  $('#lockSubmit').textContent = lockMode === 'create' ? 'Crear bóveda' : 'Desbloquear';
  $('#lockHint').textContent =
    lockMode === 'create'
      ? 'Crea tu contraseña maestra. No se puede recuperar: si la olvidas, pierdes la bóveda.'
      : hasRemote
        ? 'Se encontró tu bóveda en Google Drive. Ingresa tu contraseña maestra.'
        : 'Ingresa tu contraseña maestra.';

  const g = status.google;
  $('#lockGoogleText').textContent = g.linked
    ? `Drive vinculado: ${g.email || 'cuenta de Google'}`
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
  $('#lockSubmit').disabled = true;
  $('#lockSubmit').textContent = lockMode === 'create' ? 'Creando…' : 'Desbloqueando…';
  try {
    data = lockMode === 'create' ? await window.api.create(pw) : await window.api.unlock(pw);
    showApp();
  } catch (err) {
    $('#lockError').textContent = err.message;
  } finally {
    $('#lockSubmit').disabled = false;
    $('#lockSubmit').textContent = lockMode === 'create' ? 'Crear bóveda' : 'Desbloquear';
  }
});

$('#lockGoogleBtn').addEventListener('click', (e) =>
  run(e.currentTarget, async () => {
    toast('Se abrió el navegador para iniciar sesión con Google…');
    await window.api.googleLink();
    toast('Google vinculado', 'ok');
    showLock();
  })
);

// ---------- app principal ----------

async function showApp() {
  status = await window.api.status();
  $('#lockScreen').classList.add('hidden');
  $('#appScreen').classList.remove('hidden');
  updateSyncPill(status.google.linked ? null : { state: 'off' });
  render();
  startDetect();
}

function sortedAccounts() {
  const q = $('#search').value.trim().toLowerCase();
  let list = data.accounts.filter(
    (a) => !q || [a.label, a.username, a.gameName, a.tagLine, a.notes, a.server].some((v) => (v || '').toLowerCase().includes(q))
  );
  const by = $('#sort').value;
  const cmp = {
    rank: (a, b) => rankScore(b.ranks?.solo) - rankScore(a.ranks?.solo) || (b.level || 0) - (a.level || 0),
    level: (a, b) => (b.level || 0) - (a.level || 0),
    name: (a, b) => (riotId(a) || a.username).localeCompare(riotId(b) || b.username),
    recent: (a, b) => (b.updatedAt || '').localeCompare(a.updatedAt || ''),
  }[by];
  return list.sort(cmp);
}

function rankBlock(title, r) {
  const tier = r ? r.tier.toLowerCase() : 'none';
  return `<div class="rank">
    <span class="rank-q">${title}</span>
    <span class="rank-badge t-${tier}">${esc(rankText(r))}</span>
    <span class="rank-wr muted">${esc(winrate(r))}</span>
  </div>`;
}

function card(a) {
  const icon = a.iconId != null
    ? `<img class="avatar" src="https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/default/v1/profile-icons/${a.iconId}.jpg" alt="" />`
    : `<div class="avatar ph">${esc((a.gameName || a.username || '?')[0].toUpperCase())}</div>`;
  const id = riotId(a);
  return `<article class="card" data-id="${a.id}">
    <div class="card-head">
      <div class="avatar-wrap">${icon}${a.level ? `<span class="lvl">${a.level}</span>` : ''}</div>
      <div class="who">
        <div class="riotid">${id ? esc(a.gameName) + `<span class="tagline">#${esc(a.tagLine)}</span>` : '<span class="muted">Nick desconocido</span>'}</div>
        <div class="muted small">${esc(a.label || 'Sin etiqueta')} ${a.server ? `· <b>${esc(a.server)}</b>` : ''} ${a.puuid ? '· <span title="Vinculada por PUUID">🔗</span>' : ''}</div>
      </div>
      <button class="btn icon ghost" data-act="edit" title="Editar">✎</button>
    </div>
    ${rankBlock('Solo/Dúo', a.ranks?.solo)}
    ${rankBlock('Flex', a.ranks?.flex)}
    <div class="creds">
      <button class="cred" data-act="copy-user" title="Copiar usuario"><span class="muted small">Usuario</span><span>${esc(a.username)}</span></button>
      <button class="cred" data-act="copy-pass" title="Copiar contraseña"><span class="muted small">Contraseña</span><span class="secret">••••••••</span></button>
    </div>
    ${a.notes ? `<p class="notes small">${esc(a.notes)}</p>` : ''}
    <div class="card-foot muted small">
      <span>Actualizada ${ago(a.lastSyncedAt)}</span>
      <button class="btn ghost small" data-act="refresh">↻</button>
    </div>
  </article>`;
}

function render() {
  const list = sortedAccounts();
  $('#grid').innerHTML = list.map(card).join('');
  $('#empty').classList.toggle('hidden', data.accounts.length > 0);
}

// Si el ícono no carga (sin internet, ícono nuevo), mostramos un placeholder.
$('#grid').addEventListener(
  'error',
  (e) => {
    if (!e.target.matches('img.avatar')) return;
    const ph = document.createElement('div');
    ph.className = 'avatar ph';
    ph.textContent = '?';
    e.target.replaceWith(ph);
  },
  true
);

$('#search').addEventListener('input', render);
$('#sort').addEventListener('change', render);

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
        toast('Usuario copiado');
      });
      break;
    case 'copy-pass':
      await run(null, async () => {
        await window.api.copy(id, 'password');
        toast('Contraseña copiada (se borra en 30 s)');
      });
      break;
    case 'refresh':
      await run(btn, async () => {
        const res = await window.api.refreshRanks([id]);
        data = res.data;
        render();
        res.errors.length ? toast(res.errors[0], 'error') : toast('Cuenta actualizada', 'ok');
      });
      break;
  }
});

// ---------- diálogo de cuenta ----------

function closeOnButtons(dialog) {
  dialog.querySelectorAll('[data-close]').forEach((b) => b.addEventListener('click', () => dialog.close()));
}
document.querySelectorAll('dialog').forEach(closeOnButtons);

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
    toast('Cuenta guardada', 'ok');
  });
});

$('#accountDelete').addEventListener('click', async () => {
  const f = $('#accountForm');
  if (!(await confirmDialog(`¿Eliminar la cuenta "${f.gameName.value || f.username.value}"?`))) return;
  await run(null, async () => {
    data = await window.api.deleteAccount(f.id.value);
    $('#accountDialog').close();
    render();
  });
});

function confirmDialog(text) {
  const d = $('#confirmDialog');
  $('#confirmText').textContent = text;
  d.returnValue = '';
  d.showModal();
  return new Promise((res) => d.addEventListener('close', () => res(d.returnValue === 'yes'), { once: true }));
}

$('#btnAdd').addEventListener('click', () => {
  pendingSnapshot = null;
  openAccount();
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

$('#btnImport').addEventListener('click', () => {
  $('#importText').value = '';
  $('#importPreview').textContent = '';
  fillServers($('#importServer'));
  $('#importDialog').showModal();
});

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
  const res = await window.api.detect();
  if (!res) {
    if (manual) toast('No encontré el cliente de LoL abierto con sesión iniciada', 'error');
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
  b.innerHTML = `<span>🎮 En el cliente está abierta <b>${esc(snap.gameName)}#${esc(snap.tagLine)}</b> (${esc(snap.server)}, nivel ${snap.level}), que no está vinculada.</span>
    <div class="spacer"></div>
    <button class="btn primary small" id="bannerLink">Vincular</button>
    <button class="btn ghost small" id="bannerClose">✕</button>`;
  b.classList.remove('hidden');
  $('#bannerLink').onclick = () => openLinkDialog(snap);
  $('#bannerClose').onclick = hideBanner;
}

function hideBanner() {
  $('#detectBanner').classList.add('hidden');
}

function openLinkDialog(snap) {
  $('#linkInfo').innerHTML = `Detectada <b>${esc(snap.gameName)}#${esc(snap.tagLine)}</b> · ${esc(snap.server)} · nivel ${snap.level}. Elige la cuenta guardada con la que iniciaste sesión:`;
  const candidates = data.accounts.filter((a) => !a.puuid).concat(data.accounts.filter((a) => a.puuid));
  $('#linkList').innerHTML = candidates.length
    ? candidates
        .map(
          (a) => `<button type="button" class="linkitem" data-id="${a.id}">
            <b>${esc(a.username)}</b> <span class="muted">${esc(riotId(a) || a.label || '')} ${a.server ? '· ' + esc(a.server) : ''}</span>
            ${a.puuid ? '<span class="muted small">(ya vinculada a otra)</span>' : ''}
          </button>`
        )
        .join('')
    : '<p class="muted">No tienes cuentas guardadas todavía.</p>';
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

$('#btnRefresh').addEventListener('click', (e) =>
  run(e.currentTarget, async () => {
    if (!data.settings.riotApiKey) {
      toast('Primero pon tu API key de Riot en Ajustes', 'error');
      return openSettings();
    }
    const res = await window.api.refreshRanks();
    data = res.data;
    render();
    if (res.errors.length) res.errors.slice(0, 3).forEach((m) => toast(m, 'error'));
    else toast('Rangos actualizados', 'ok');
  })
);

// ---------- ajustes / Google ----------

async function openSettings() {
  status = await window.api.status();
  renderGoogle();
  $('#riotKey').value = data.settings.riotApiKey || '';
  $('#settingsDialog').showModal();
}

function renderGoogle() {
  const g = status.google;
  $('#googleStatus').innerHTML = !g.hasCredentials
    ? `Falta <code>credentials.json</code>. Sigue los pasos de <b>GOOGLE_SETUP.md</b> y déjalo en:<br><code>${esc(status.credentialsPath)}</code>`
    : g.linked
      ? `Vinculado con <b>${esc(g.email || 'tu cuenta de Google')}</b>. Tu bóveda cifrada se guarda en una carpeta oculta de tu Drive.`
      : 'No vinculado. Tu bóveda solo está en este PC.';
  $('#googleLinkBtn').classList.toggle('hidden', g.linked);
  $('#googleLinkBtn').disabled = !g.hasCredentials;
  $('#googleSyncBtn').classList.toggle('hidden', !g.linked);
  $('#googleUnlinkBtn').classList.toggle('hidden', !g.linked);
}

$('#btnSettings').addEventListener('click', openSettings);

$('#googleLinkBtn').addEventListener('click', (e) =>
  run(e.currentTarget, async () => {
    toast('Se abrió el navegador para iniciar sesión con Google…');
    await window.api.googleLink();
    status = await window.api.status();
    renderGoogle();
    toast('Google vinculado, subiendo bóveda…', 'ok');
  })
);

$('#googleSyncBtn').addEventListener('click', (e) => run(e.currentTarget, () => window.api.syncNow()));

$('#googleUnlinkBtn').addEventListener('click', (e) =>
  run(e.currentTarget, async () => {
    await window.api.googleUnlink();
    status = await window.api.status();
    renderGoogle();
    updateSyncPill({ state: 'off' });
    toast('Google desvinculado (el archivo en Drive no se borra)');
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

$('#btnLock').addEventListener('click', async () => {
  await window.api.lock();
  showLock();
});

function updateSyncPill(s) {
  const p = $('#syncPill');
  if (!s) return;
  const map = {
    off: ['Drive: no vinculado', 'off'],
    syncing: ['Drive: subiendo…', 'syncing'],
    ok: ['Drive: sincronizado ✓', 'ok'],
    error: ['Drive: error ⚠', 'error'],
  };
  const [text, cls] = map[s.state];
  p.textContent = text;
  p.className = `pill ${cls}`;
  p.title = s.message || (s.at ? `Última vez: ${new Date(s.at).toLocaleString()}` : '');
  if (s.state === 'error') toast(`Google Drive: ${s.message}`, 'error');
}

window.api.onSync(updateSyncPill);

showLock();
