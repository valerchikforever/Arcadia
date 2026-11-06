'use strict';
// Bridge between the interface and the Rust core: the same window.api the Electron version had.
(function () {
  const T = window.__TAURI__;
  const invoke = (ch, ...args) => T.core.invoke('call', { ch, args });
  const call = (ch) => (...a) => invoke(ch, ...a);
  const on = (name) => (cb) => T.event.listen(name, (e) => cb(e.payload));

  window.api = {
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
    hltb: call('game:hltb'),
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
    onState: on('state'),
    onToast: on('toast'),
    onOpenNet: on('open-net'),
    onDragDrop: on('dragdrop'),
    onLayout: on('layout'),
    onEvent: (name, cb) => T.event.listen(name, (e) => cb(e.payload)),
    win: { minimize: call('win:minimize'), maximize: call('win:maximize'), close: call('win:close'), drag: call('win:drag'), fullscreen: call('win:fullscreen') },
    zapret: {
      set: call('zapret:set'), detect: call('zapret:detect'), pickDir: call('zapret:pick-dir'),
      start: call('zapret:start'), stop: call('zapret:stop'), open: call('zapret:open'),
      autopick: call('zapret:autopick'), autopickStop: call('zapret:autopick-stop'),
    },
    vpn: {
      set: call('vpn:set'), add: call('vpn:add'), refreshSubs: call('vpn:refresh-subs'), removeServer: call('vpn:remove-server'),
      removeSub: call('vpn:remove-sub'), renameServer: call('vpn:rename-server'), ping: call('vpn:ping'),
      installCore: call('vpn:install-core'), installXray: call('vpn:install-xray'), pickXray: call('vpn:pick-xray'), pickCore: call('vpn:pick-core'),
      pickApp: call('vpn:pick-app'), start: call('vpn:start'), stop: call('vpn:stop'), openLog: call('vpn:open-log'),
    },
    acc: {
      login: call('acc:login'), localSteam: call('acc:local-steam'), sync: call('acc:sync'), logout: call('acc:logout'),
      launcher: call('acc:launcher'), steamKey: call('acc:steam-key'),
    },
    br: {
      open: call('br:open'), bounds: call('br:bounds'), nav: call('br:nav'), unload: call('br:unload'),
      disable: call('br:disable'), external: call('br:external'), set: call('br:set'), full: call('br:full'),
    },
    // extra features: api.x('name', ...args)
    x: (name, ...a) => invoke('x:' + name, ...a),
    fileUrl: (p, v) => (p ? T.core.convertFileSrc(p) + (v ? `?v=${v}` : '') : ''),
    pathForFile: () => '',
  };
})();
