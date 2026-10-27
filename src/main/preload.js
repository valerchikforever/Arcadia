'use strict';
const { contextBridge, ipcRenderer, webUtils } = require('electron');
const { pathToFileURL } = require('url');

const call = (ch) => (...a) => ipcRenderer.invoke(ch, ...a);

contextBridge.exposeInMainWorld('api', {
  getState: call('state:get'),
  scan: call('scan'),
  launch: call('launch'),
  updateGame: call('game:update'),
  removeGame: call('game:remove'),
  addFiles: call('game:add-files'),
  openFolder: call('game:open-folder'),
  setImage: call('game:set-image'),
  refetch: call('game:refetch'),
  resetPlaytime: call('game:reset-playtime'),
  pickGames: call('dialog:pick-games'),
  pickFolder: call('dialog:pick-folder'),
  pickFile: call('dialog:pick-file'),
  createCollection: call('col:create'),
  renameCollection: call('col:rename'),
  deleteCollection: call('col:delete'),
  setSettings: call('settings:set'),
  unignore: call('settings:unignore'),
  fetchMissing: call('meta:fetch-missing'),
  refetchAll: call('meta:refetch-all'),
  openData: call('app:open-data'),
  openUrl: call('app:open-url'),
  quit: call('app:quit'),
  onState: (cb) => ipcRenderer.on('state', (_e, s) => cb(s)),
  onToast: (cb) => ipcRenderer.on('toast', (_e, t) => cb(t)),
  onOpenNet: (cb) => ipcRenderer.on('open-net', () => cb()),
  zapret: {
    set: call('zapret:set'), detect: call('zapret:detect'), pickDir: call('zapret:pick-dir'),
    start: call('zapret:start'), stop: call('zapret:stop'), open: call('zapret:open'),
  },
  vpn: {
    set: call('vpn:set'), add: call('vpn:add'), refreshSubs: call('vpn:refresh-subs'), removeServer: call('vpn:remove-server'),
    removeSub: call('vpn:remove-sub'), renameServer: call('vpn:rename-server'), ping: call('vpn:ping'),
    installCore: call('vpn:install-core'), installXray: call('vpn:install-xray'), pickXray: call('vpn:pick-xray'), pickCore: call('vpn:pick-core'), start: call('vpn:start'), stop: call('vpn:stop'), openLog: call('vpn:open-log'),
  },
  acc: {
    login: call('acc:login'), localSteam: call('acc:local-steam'), sync: call('acc:sync'), logout: call('acc:logout'),
    launcher: call('acc:launcher'), steamKey: call('acc:steam-key'),
  },
  br: {
    open: call('br:open'), bounds: call('br:bounds'), nav: call('br:nav'), unload: call('br:unload'),
    disable: call('br:disable'), external: call('br:external'), set: call('br:set'), full: call('br:full'),
  },
  fileUrl: (p, v) => {
    if (!p) return '';
    try { return pathToFileURL(p).href + (v ? `?v=${v}` : ''); } catch { return ''; }
  },
  pathForFile: (file) => {
    try { return webUtils.getPathForFile(file); } catch { return file.path || ''; }
  },
});
