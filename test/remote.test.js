// Prueba del control desde el celular con un cliente de LoL falso: node test/remote.test.js
const assert = require('assert');
const path = require('path');
const { ChampSelect, myActiveAction } = require('../src/champselect');
const { RemoteServer } = require('../src/remote');

// Cliente de LoL falso: responde GETs según la ruta y anota las órdenes.
function fakeClient(routes) {
  const calls = [];
  return {
    calls,
    get: async (p) => {
      const hit = Object.entries(routes).find(([re]) => new RegExp(re).test(p));
      return hit ? hit[1] : null;
    },
    request: async (method, p, _opts, body) => {
      calls.push([method, p, body]);
      return { status: 204, json: null };
    },
  };
}

const session = {
  localPlayerCellId: 2,
  timer: { phase: 'BAN_PICK', adjustedTimeLeftInPhase: 25000, internalNowInEpoch: 1000, totalTimeInPhase: 30000 },
  myTeam: [
    { cellId: 1, championId: 0, championPickIntent: 157, assignedPosition: 'top' },
    { cellId: 2, championId: 0, championPickIntent: 0, assignedPosition: 'middle', spell1Id: 4, spell2Id: 14 },
  ],
  theirTeam: [{ cellId: 6, championId: 0 }],
  bans: { myTeamBans: [238], theirTeamBans: [] },
  actions: [
    [{ id: 10, actorCellId: 1, type: 'ban', completed: true, isInProgress: false, championId: 238 }],
    [{ id: 11, actorCellId: 2, type: 'pick', completed: false, isInProgress: true, championId: 103 }],
  ],
};

(async () => {
  // --- estado ---
  const client = fakeClient({
    'gameflow-phase': 'ChampSelect',
    'champ-select/v1/session$': session,
    'pickable-champion-ids': [103, 157, -1],
    'perks/v1/pages': [{ id: 5, name: 'Mid', isEditable: true, primaryStyleId: 8100 }],
    'perks/v1/currentpage': { id: 5 },
    'perks/v1/styles': [{ id: 8100, name: 'Dominación', iconPath: '/lol-game-data/assets/v1/perk-images/Styles/7200_Domination.png' }],
  });
  const cs = new ChampSelect({ client });
  assert.strictEqual(myActiveAction(session).id, 11);
  const st = await cs.state();
  assert.strictEqual(st.phase, 'ChampSelect');
  const c = st.champSelect;
  assert.deepStrictEqual(c.action, { id: 11, type: 'pick', championId: 103 });
  assert.deepStrictEqual(c.available, [103, 157], 'sin ids inválidos');
  // El tiempo restante cuenta desde que se vio ese valor; si el cliente no lo cambia, el término es el mismo.
  const firstEnd = c.timer.endsAt;
  assert.ok(Math.abs(firstEnd - (Date.now() + 25000)) < 1000);
  await new Promise((r) => setTimeout(r, 30));
  assert.strictEqual((await cs.state()).champSelect.timer.endsAt, firstEnd, 'no se corre mientras el cliente no avise');
  assert.strictEqual(c.me.position, 'Mid');
  assert.strictEqual(c.myTeam[0].hoverId, 157);
  assert.ok(c.runes.pages[0].icon.startsWith('https://raw.communitydragon.org/'), 'íconos por CommunityDragon');

  // Baneo con la lista del cliente vacía: se ofrecen todos menos los ya baneados o elegidos.
  const banClient = fakeClient({
    'bannable-champion-ids': [],
    'champion-summary': [{ id: -1 }, { id: 103 }, { id: 157 }, { id: 238 }, { id: 99 }, { id: 60001, alias: 'Jade_Annie' }],
    'disabled-champion-ids': [99],
  });
  const banSession = { ...session, actions: [[{ id: 9, actorCellId: 2, type: 'ban', completed: false, isInProgress: true }]] };
  const bans = await new ChampSelect({ client: banClient }).options('ban', banSession);
  assert.deepStrictEqual(bans, [103, 157], 'sin Zed (baneado), Lux (deshabilitada) ni campeones de LoL Classic');

  // --- órdenes ---
  // El cliente falso no marca la acción como confirmada: se prueba con la ruta de confirmar y, como
  // tampoco queda confirmada, se avisa en vez de quedarse callado.
  await assert.rejects(cs.run({ type: 'lock', championId: 157 }), /no confirmó/);
  assert.deepStrictEqual(client.calls.slice(-2), [
    ['PATCH', '/lol-champ-select/v1/session/actions/11', { championId: 157, completed: true }],
    ['POST', '/lol-champ-select/v1/session/actions/11/complete', undefined],
  ]);
  // Un cliente que sí confirma con la primera orden (como el real): no hace falta la segunda.
  const pick = { id: 11, actorCellId: 2, type: 'pick', completed: false, isInProgress: true, championId: 0 };
  const sent = [];
  const confirming = new ChampSelect({
    client: {
      get: async () => ({ localPlayerCellId: 2, actions: [[pick]] }),
      request: async (method, p, _opts, body) => {
        sent.push(method);
        if (body?.completed) pick.completed = true;
        return { status: 204, json: null };
      },
    },
  });
  await confirming.run({ type: 'lock', championId: 157 });
  assert.deepStrictEqual(sent, ['PATCH'], 'solo el PATCH');
  await cs.run({ type: 'spells', spell1Id: 14, spell2Id: 4 });
  assert.deepStrictEqual(client.calls.at(-1), ['PATCH', '/lol-champ-select/v1/session/my-selection', { spell1Id: 14, spell2Id: 4 }]);
  await cs.run({ type: 'runePage', pageId: 5 });
  assert.deepStrictEqual(client.calls.at(-1), ['PUT', '/lol-perks/v1/currentpage', 5]);
  await assert.rejects(cs.run({ type: 'borrarTodo' }), /desconocida/);

  // --- servidor: código obligatorio, solo sus archivos, órdenes ---
  const done = [];
  const server = new RemoteServer({
    champSelect: { state: async () => ({ phase: 'Lobby' }), run: async (cmd) => done.push(cmd) },
    gameData: { get: async () => ({ champions: { 103: 'Ahri' }, spells: [] }) },
    appDir: path.join(__dirname, '..'),
  });
  await server.start('secreto-de-prueba');
  const base = `http://127.0.0.1:${server.port}`;
  try {
    assert.strictEqual((await fetch(`${base}/`)).status, 200, 'la página se sirve sin código');
    assert.strictEqual((await fetch(`${base}/../main.js`)).status, 404, 'otros archivos no');
    assert.strictEqual((await fetch(`${base}/api/static`)).status, 401, 'la API pide código');
    assert.strictEqual((await fetch(`${base}/api/static`, { headers: { 'X-Token': 'otro' } })).status, 401);
    const ok = await fetch(`${base}/api/static`, { headers: { 'X-Token': 'secreto-de-prueba' } });
    assert.deepStrictEqual((await ok.json()).champions, { 103: 'Ahri' });
    const r = await fetch(`${base}/api/command`, {
      method: 'POST',
      headers: { 'X-Token': 'secreto-de-prueba', 'Content-Type': 'application/json' },
      body: JSON.stringify({ type: 'accept' }),
    });
    assert.strictEqual(r.status, 200);
    assert.deepStrictEqual(done, [{ type: 'accept' }]);
  } finally {
    server.stop();
  }
  console.log('remote OK');
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
