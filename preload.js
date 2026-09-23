const { contextBridge, ipcRenderer } = require('electron');

async function call(channel, ...args) {
  const res = await ipcRenderer.invoke(channel, ...args);
  if (!res.ok) throw new Error(res.error);
  return res.value;
}

contextBridge.exposeInMainWorld('api', {
  status: () => call('app:status'),
  remoteExists: () => call('vault:remoteExists'),
  create: (pw) => call('vault:create', pw),
  unlock: (pw) => call('vault:unlock', pw),
  lock: () => call('vault:lock'),
  saveAccount: (acc) => call('accounts:save', acc),
  importAccounts: (rows) => call('accounts:import', rows),
  deleteAccount: (id) => call('accounts:delete', id),
  saveSettings: (s) => call('settings:save', s),
  copy: (id, field) => call('clipboard:copy', id, field),
  googleLink: () => call('google:link'),
  googleUnlink: () => call('google:unlink'),
  syncNow: () => call('google:syncNow'),
  detect: () => call('lcu:detect'),
  linkClient: (id, snap) => call('lcu:link', id, snap),
  refreshRanks: (ids) => call('riot:refresh', ids),
  onSync: (cb) => ipcRenderer.on('sync', (_e, s) => cb(s)),
});
