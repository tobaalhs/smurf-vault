const { contextBridge, ipcRenderer } = require('electron');

async function call(channel, ...args) {
  const res = await ipcRenderer.invoke(channel, ...args);
  if (!res.ok) throw new Error(res.error);
  return res.value;
}

contextBridge.exposeInMainWorld('api', {
  status: () => call('app:status'),
  openExternal: (url) => call('app:openExternal', url),
  installUpdate: () => call('app:installUpdate'),
  onUpdate: (cb) => ipcRenderer.on('update', (_e, u) => cb(u)),
  remoteExists: () => call('vault:remoteExists'),
  create: (pw) => call('vault:create', pw),
  unlock: (pw) => call('vault:unlock', pw),
  lock: () => call('vault:lock'),
  saveAccount: (acc) => call('accounts:save', acc),
  importAccounts: (rows) => call('accounts:import', rows),
  deleteAccount: (id) => call('accounts:delete', id),
  setFavorite: (id, favorite) => call('accounts:favorite', id, favorite),
  gameData: () => call('game:data'),
  saveSettings: (s) => call('settings:save', s),
  copy: (id, field) => call('clipboard:copy', id, field),
  googleLink: () => call('google:link'),
  googleUnlink: () => call('google:unlink'),
  googleImportCredentials: () => call('google:importCredentials'),
  syncNow: () => call('google:syncNow'),
  detect: (manual) => call('lcu:detect', manual),
  getTools: () => call('tools:get'),
  setTools: (patch) => call('tools:set', patch),
  onTools: (cb) => ipcRenderer.on('tools', (_e, s) => cb(s)),
  linkClient: (id, snap) => call('lcu:link', id, snap),
  refreshRanks: (ids) => call('riot:refresh', ids),
  onRiotProgress: (cb) => ipcRenderer.on('riot-progress', (_e, p) => cb(p)),
  play: (id) => call('switch:play', id),
  loginOther: () => call('switch:login'),
  forgetSession: (id) => call('switch:forget', id),
  onSync: (cb) => ipcRenderer.on('sync', (_e, s) => cb(s)),
  onData: (cb) => ipcRenderer.on('data', (_e, d) => cb(d)),
  onLocked: (cb) => ipcRenderer.on('locked', () => cb()),
  setTheme: (theme) => call('theme:set', theme),
  winMinimize: () => call('win:minimize'),
  winToggleMaximize: () => call('win:toggleMaximize'),
  winClose: () => call('win:close'),
  winIsMaximized: () => call('win:isMaximized'),
  onWinState: (cb) => ipcRenderer.on('win:state', (_e, s) => cb(s)),
});
