'use strict';
const { app, BrowserWindow, ipcMain, dialog, shell, Tray, Menu, nativeImage, globalShortcut, net } = require('electron');
const path = require('path');
const fs = require('fs');
const { Store } = require('./store');
const { scanAll } = require('./scanners');
const { makeManualGame } = require('./scanners/manual');
const { Meta } = require('./meta');
const { Tracker } = require('./tracker');
const { launchGame } = require('./launch');
const { exists } = require('./util/common');
const { Zapret, detectDirs } = require('./net/zapret');
const { Vpn } = require('./net/vpn');
const { processNames } = require('./net/elevate');
const { Accounts } = require('./accounts');
const { StoreBrowser } = require('./browser');

const ASSETS = path.join(__dirname, '..', '..', 'assets');
const START_HIDDEN = process.argv.includes('--hidden');

if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  app.on('second-instance', () => showWindow());
  app.whenReady().then(init);
}

app.setAppUserModelId('com.arcadia.launcher');

let win = null, tray = null, store, meta, tracker, zapret, vpn, accounts, browser;
let netCleaned = false;
let quitting = false, scanning = false, lastError = '';
let pushTimer = null;

const logFile = () => path.join(app.getPath('userData'), 'arcadia.log');
function log(...a) {
  const line = `[${new Date().toISOString()}] ${a.map((x) => (typeof x === 'string' ? x : JSON.stringify(x))).join(' ')}\n`;
  try { fs.appendFileSync(logFile(), line); } catch {}
}

function snapshot() {
  return {
    games: store.games,
    collections: store.collections,
    settings: store.settings,
    scanStatus: store.scanStatus,
    running: tracker ? tracker.runningIds() : [],
    scanning,
    metaPending: meta ? meta.pending() : 0,
    lastError,
    net: zapret && vpn ? { zapret: zapret.state(), vpn: vpn.state() } : null,
    accounts: accounts ? accounts.state() : null,
    browser: browser ? browser.state() : null,
    platform: process.platform,
    version: app.getVersion(),
  };
}

function push() {
  if (pushTimer) return;
  pushTimer = setTimeout(() => {
    pushTimer = null;
    if (win && !win.isDestroyed()) win.webContents.send('state', snapshot());
    updateTray();
  }, 150);
}

// ---------------- window & tray ----------------
function createWindow() {
  win = new BrowserWindow({
    width: 1400,
    height: 880,
    minWidth: 1000,
    minHeight: 640,
    show: false,
    backgroundColor: '#0e0a12',
    title: 'Arcadia',
    icon: path.join(ASSETS, 'icon.png'),
    titleBarStyle: 'hidden',
    titleBarOverlay: { color: '#120d17', symbolColor: '#d9cbe0', height: 46 },
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false,
      spellcheck: false,
    },
  });
  win.loadFile(path.join(__dirname, '..', 'renderer', 'index.html'));
  win.once('ready-to-show', () => {
    if (!(START_HIDDEN || store.settings.startMinimized)) win.show();
  });
  win.on('close', (e) => {
    if (!quitting && store.settings.closeToTray) {
      e.preventDefault();
      win.hide();
    }
  });
  win.webContents.setWindowOpenHandler(({ url }) => {
    if (/^https:\/\//.test(url)) shell.openExternal(url);
    return { action: 'deny' };
  });
  win.webContents.on('will-navigate', (e) => e.preventDefault());
}

function showWindow() {
  if (!win || win.isDestroyed()) createWindow();
  if (win.isMinimized()) win.restore();
  win.show();
  win.focus();
}

function toggleWindow() {
  if (win && win.isVisible() && win.isFocused()) win.hide();
  else showWindow();
}

function displayName(g) { return g.customName || g.name; }

function updateTray() {
  if (!tray) return;
  const games = Object.values(store.games).filter((g) => !g.hidden && !g.missing && g.installed !== false);
  const recent = games
    .filter((g) => g.lastPlayed || g.lastLaunched || g.importedLastPlayed)
    .sort((a, b) => Math.max(b.lastPlayed || 0, b.lastLaunched || 0, b.importedLastPlayed || 0) - Math.max(a.lastPlayed || 0, a.lastLaunched || 0, a.importedLastPlayed || 0))
    .slice(0, 6);
  const favs = games.filter((g) => g.favorite && !recent.includes(g)).slice(0, 6);
  const item = (g) => ({ label: displayName(g).slice(0, 48), click: () => doLaunch(g.id) });
  const running = tracker ? tracker.runningIds().map((id) => store.get(id)).filter(Boolean) : [];
  const tpl = [
    { label: 'Открыть Arcadia', click: showWindow },
    { type: 'separator' },
  ];
  if (running.length) tpl.push({ label: 'Сейчас запущено', enabled: false }, ...running.map((g) => ({ label: '▶ ' + displayName(g), enabled: false })), { type: 'separator' });
  if (recent.length) tpl.push({ label: 'Недавние', enabled: false }, ...recent.map(item), { type: 'separator' });
  if (favs.length) tpl.push({ label: 'Избранное', enabled: false }, ...favs.map(item), { type: 'separator' });
  if (zapret && vpn) {
    const zs = zapret.state(), vs = vpn.state();
    tpl.push(
      { label: `Zapret${zs.running && zs.runningStrategy ? ' — ' + zs.runningStrategy.replace(/\.(bat|cmd)$/i, '') : ''}`, type: 'checkbox', checked: zs.running, enabled: zs.valid && !zs.busy, click: () => netAction(() => (zapret.running ? zapret.stop() : zapret.start())) },
      { label: `VPN${vs.running ? (vs.runningMode === 'tun' ? ' — весь трафик' : ' — прокси') : ''}`, type: 'checkbox', checked: vs.running, enabled: vs.core.installed && vs.servers.length > 0 && !vs.busy, click: () => netAction(() => (vpn.running ? vpn.stop() : vpn.start())) },
      { label: 'Обход блокировок…', click: () => { showWindow(); win.webContents.send('open-net'); } },
      { type: 'separator' },
    );
  }
  tpl.push({ label: scanning ? 'Сканирование…' : 'Обновить библиотеку', enabled: !scanning, click: runScan });
  tpl.push({ label: 'Выход', click: () => { quitting = true; app.quit(); } });
  tray.setContextMenu(Menu.buildFromTemplate(tpl));
  tray.setToolTip(running.length ? `Arcadia — ${displayName(running[0])}` : 'Arcadia');
}

function createTray() {
  const img = nativeImage.createFromPath(path.join(ASSETS, 'tray.png'));
  tray = new Tray(img.isEmpty() ? nativeImage.createEmpty() : img);
  tray.on('click', toggleWindow);
  tray.on('double-click', showWindow);
  updateTray();
}

function applyHotkey() {
  globalShortcut.unregisterAll();
  const hk = store.settings.hotkey;
  if (!hk) return true;
  try { return globalShortcut.register(hk, toggleWindow); } catch { return false; }
}

function applyAutostart() {
  if (process.platform !== 'win32') return;
  const args = ['--hidden'];
  const opts = { openAtLogin: !!store.settings.autostart, args };
  if (!app.isPackaged) { opts.path = process.execPath; opts.args = [path.resolve(__dirname, '..', '..'), ...args]; }
  else if (process.env.PORTABLE_EXECUTABLE_FILE) opts.path = process.env.PORTABLE_EXECUTABLE_FILE; // portable build unpacks to temp
  app.setLoginItemSettings(opts);
}

// ---------------- zapret / vpn ----------------
async function netAction(fn) {
  try { await fn(); } catch (e) { log('net', e.message); if (win && !win.isDestroyed()) win.webContents.send('toast', { type: 'err', msg: e.message }); }
  push();
}

let pollTimer = null, firstPoll = true;
async function pollNet() {
  clearTimeout(pollTimer);
  try {
    const names = await processNames();
    zapret.setStatus(names);
    vpn.setStatus(names);
    if (firstPoll) {
      firstPoll = false;
      if (!vpn.running && store.settings.vpn.proxySet) await vpn.clearProxy().catch(() => {}); // left over after a crash
      if (store.settings.zapret.autostart && !zapret.running && zapret.state().valid) netAction(() => zapret.start());
      if (store.settings.vpn.autostart && !vpn.running && vpn.corePath()) netAction(() => vpn.start());
    }
  } catch (e) { log('poll', e.message); }
  pollTimer = setTimeout(pollNet, win && win.isVisible() ? 4000 : 12000);
}

async function netShutdown() {
  if (vpn) {
    const v = store.settings.vpn;
    if (vpn.child || v.proxySet) { vpn.shutdownSync(); await vpn.clearProxy().catch(() => {}); }
    if (vpn.running && v.stopOnExit !== false) await vpn.stop().catch(() => {});
  }
  if (zapret && zapret.running && store.settings.zapret.stopOnExit) await zapret.stop().catch(() => {});
}

// ---------------- actions ----------------
async function runScan() {
  if (scanning) return;
  scanning = true;
  lastError = '';
  push();
  try {
    const result = await scanAll(store.settings, { extraClaimed: store.manualDirs(), log });
    const added = store.mergeScan(result);
    log('scan done', `${result.games.length} found, ${added} new`, result.status);
    if (store.settings.autoFetchMeta) meta.enqueueMissing();
  } catch (e) {
    lastError = 'Ошибка сканирования: ' + e.message;
    log('scan failed', e.stack || e.message);
  } finally {
    scanning = false;
    push();
  }
}

async function doLaunch(id) {
  const g = store.get(id);
  if (!g) return { ok: false, error: 'Игра не найдена' };
  if (g.installed === false) {
    // Bought but not installed: the official client downloads it
    if (!g.installUri) return { ok: false, error: 'Неизвестно, как установить эту игру' };
    try { await shell.openExternal(g.installUri); } catch (e) { return { ok: false, error: `Не удалось открыть лаунчер: ${e.message}` }; }
    return { ok: true, install: true };
  }
  try {
    await launchGame(g);
  } catch (e) {
    log('launch failed', id, e.message);
    return { ok: false, error: e.message };
  }
  store.update(id, { lastLaunched: Date.now() });
  tracker.expect(g);
  if (store.settings.minimizeOnLaunch && win && !win.isDestroyed()) win.minimize();
  push();
  return { ok: true };
}

async function getIcon(exe, dest) {
  try {
    if (!exists(exe)) return null;
    const img = await app.getFileIcon(exe, { size: 'large' });
    if (!img || img.isEmpty()) return null;
    fs.writeFileSync(dest, img.toPNG());
    return dest;
  } catch { return null; }
}

function addFiles(paths) {
  const added = [], errors = [];
  for (const p of paths || []) {
    try {
      const g = makeManualGame(p, shell);
      store.addGame(g);
      added.push(g.id);
      meta.enqueue(g.id);
    } catch (e) { errors.push(`${path.basename(p)}: ${e.message}`); }
  }
  push();
  return { added, errors };
}

// ---------------- IPC ----------------
function registerIpc() {
  const h = (ch, fn) => ipcMain.handle(ch, async (_e, ...a) => {
    try { return await fn(...a); } catch (e) { log(ch, e.stack || e.message); return { ok: false, error: e.message }; }
  });

  h('state:get', () => snapshot());
  h('scan', async () => { await runScan(); return { ok: true }; });
  h('launch', (id) => doLaunch(id));

  h('game:update', (id, patch) => {
    const allowed = ['customName', 'favorite', 'hidden', 'collections', 'launchOverride', 'notes'];
    const clean = {};
    for (const k of allowed) if (k in patch) clean[k] = patch[k];
    store.update(id, clean);
    push();
    return { ok: true };
  });
  h('game:remove', (id) => { store.remove(id); push(); return { ok: true }; });
  h('game:add-files', (paths) => addFiles(paths));
  h('game:open-folder', (id) => {
    const g = store.get(id);
    const p = g && (g.installDir || (g.launch && g.launch.path && path.dirname(g.launch.path)));
    if (p && exists(p)) shell.openPath(p);
    return { ok: !!p };
  });
  h('game:set-image', async (id, kind, src) => { await meta.setImage(id, kind, src); push(); return { ok: true }; });
  h('game:refetch', (id, term) => { meta.enqueue(id, { term: term || undefined, force: true }); return { ok: true }; });
  h('game:reset-playtime', (id) => { store.update(id, { playtime: 0, sessions: [] }); push(); return { ok: true }; });

  h('dialog:pick-games', async () => {
    const r = await dialog.showOpenDialog(win, {
      title: 'Добавить игру',
      properties: ['openFile', 'multiSelections'],
      filters: [{ name: 'Игры и ярлыки', extensions: ['exe', 'lnk', 'url', 'bat'] }],
    });
    if (r.canceled) return { added: [], errors: [] };
    return addFiles(r.filePaths);
  });
  h('dialog:pick-folder', async () => {
    const r = await dialog.showOpenDialog(win, { title: 'Папка с играми', properties: ['openDirectory'] });
    return r.canceled ? null : r.filePaths[0];
  });
  h('dialog:pick-file', async (kind) => {
    const filters = kind === 'image'
      ? [{ name: 'Изображения', extensions: ['jpg', 'jpeg', 'png', 'webp', 'gif'] }]
      : [{ name: 'Программы', extensions: ['exe', 'bat', 'cmd', 'lnk'] }];
    const r = await dialog.showOpenDialog(win, { properties: ['openFile'], filters });
    return r.canceled ? null : r.filePaths[0];
  });

  h('col:create', (name) => { const c = store.createCollection(name); push(); return c; });
  h('col:rename', (id, name) => { store.renameCollection(id, name); push(); return { ok: true }; });
  h('col:delete', (id) => { store.deleteCollection(id); push(); return { ok: true }; });

  h('settings:set', async (patch) => {
    const before = { ...store.settings };
    store.setSettings(patch);
    let hotkeyOk = true;
    if ('hotkey' in patch) hotkeyOk = applyHotkey();
    if ('autostart' in patch) applyAutostart();
    if ('trackExternal' in patch) tracker.schedule(1000);
    const rescan = 'scanFolders' in patch || ('sources' in patch && JSON.stringify(before.sources) !== JSON.stringify(store.settings.sources));
    if (rescan) runScan();
    push();
    return { ok: true, hotkeyOk };
  });
  h('settings:unignore', () => { store.setSettings({ ignoredIds: [] }); runScan(); return { ok: true }; });

  h('meta:fetch-missing', () => { meta.enqueueMissing(); return { ok: true }; });
  h('meta:refetch-all', () => {
    for (const g of Object.values(store.games)) if (!g.missing && g.installed !== false) meta.enqueue(g.id);
    return { ok: true };
  });
  h('app:open-data', () => { shell.openPath(app.getPath('userData')); return { ok: true }; });
  h('app:open-url', (url) => { if (/^https:\/\//.test(url)) shell.openExternal(url); return { ok: true }; });
  h('app:quit', () => { quitting = true; app.quit(); });

  // store accounts
  h('acc:login', async (key) => { const r = await accounts.login(key); push(); return { ok: true, ...(r || {}) }; });
  h('acc:local-steam', async () => { const r = await accounts.useLocalSteam(); push(); return r; });
  h('acc:sync', async (key) => { const r = await accounts.sync(key); push(); return r; });
  h('acc:logout', async (key) => { const r = await accounts.logout(key); push(); return r; });
  h('acc:launcher', async (key) => {
    const uri = accounts.launcherUri(key);
    if (!uri) return { ok: false, error: 'Для этого сервиса нет ссылки на лаунчер' };
    await shell.openExternal(uri);
    return { ok: true };
  });
  h('acc:steam-key', (key) => { store.settings.steamApiKey = String(key || '').trim(); store.save(); push(); return { ok: true }; });

  // built-in browser
  h('br:open', (key, url) => { browser.open(key, url); return { ok: true }; });
  h('br:bounds', (rect) => { browser.setBounds(rect); return { ok: true }; });
  h('br:nav', (action) => { browser.nav(action); return { ok: true }; });
  h('br:unload', () => { browser.unload('manual'); return { ok: true }; });
  h('br:disable', () => { browser.disable(); return { ok: true }; });
  h('br:external', () => { browser.openExternal(); return { ok: true }; });
  h('br:set', (patch) => {
    store.settings.browser = { ...store.settings.browser, ...patch };
    store.save();
    // economy / WebGL options apply to a newly created page
    if (('economy' in patch) && browser.view) { const k = browser.key, u = browser.url; browser.destroyView(); browser.open(k, u); }
    push();
    return { ok: true };
  });

  // zapret
  const zset = (patch) => { store.settings.zapret = { ...store.settings.zapret, ...patch }; store.save(); push(); return { ok: true }; };
  h('zapret:set', (patch) => zset(patch));
  h('zapret:detect', () => detectDirs());
  h('zapret:pick-dir', async () => {
    const r = await dialog.showOpenDialog(win, { title: 'Папка zapret', properties: ['openDirectory'] });
    if (r.canceled) return null;
    zset({ dir: r.filePaths[0], strategy: '' });
    return r.filePaths[0];
  });
  h('zapret:start', async (strategy) => { await zapret.start(strategy); push(); return { ok: true }; });
  h('zapret:stop', async () => { await zapret.stop(); push(); return { ok: true }; });
  h('zapret:open', (sub) => {
    const d = store.settings.zapret.dir;
    if (!d) return { ok: false };
    const p = sub ? path.join(d, sub) : d;
    shell.openPath(exists(p) ? p : d);
    return { ok: true };
  });

  // vpn
  h('vpn:set', (patch) => {
    const allowed = ['selected', 'mode', 'directRu', 'gamesDirect', 'autostart', 'stopOnExit', 'port'];
    const clean = {};
    for (const k of allowed) if (k in patch) clean[k] = patch[k];
    vpn.saveCfg(clean);
    push();
    return { ok: true };
  });
  h('vpn:add', async (text) => { const r = await vpn.addInput(text); push(); return { ok: true, ...r }; });
  h('vpn:refresh-subs', async () => { const r = await vpn.refreshSubs(); push(); return { ok: true, ...r }; });
  h('vpn:remove-server', (id) => { vpn.removeServer(id); push(); return { ok: true }; });
  h('vpn:remove-sub', (id) => { vpn.removeSub(id); push(); return { ok: true }; });
  h('vpn:rename-server', (id, name) => { vpn.renameServer(id, String(name).slice(0, 80)); push(); return { ok: true }; });
  h('vpn:ping', async () => { await vpn.pingAll(); push(); return { ok: true }; });
  h('vpn:install-core', async () => { await vpn.installCore(); push(); return { ok: true }; });
  h('vpn:install-xray', async () => { await vpn.installXray(); push(); return { ok: true }; });
  h('vpn:pick-xray', async () => {
    const r = await dialog.showOpenDialog(win, { title: 'Укажите xray.exe', properties: ['openFile'], filters: [{ name: 'Xray', extensions: ['exe'] }] });
    if (r.canceled) return { ok: false };
    vpn.setXrayFile(r.filePaths[0]);
    push();
    return { ok: true };
  });
  h('vpn:pick-core', async () => {
    const r = await dialog.showOpenDialog(win, { title: 'Укажите sing-box.exe', properties: ['openFile'], filters: [{ name: 'sing-box', extensions: ['exe'] }] });
    if (r.canceled) return { ok: false };
    vpn.setCoreFile(r.filePaths[0]);
    push();
    return { ok: true };
  });
  h('vpn:start', async () => { await vpn.start(); push(); return { ok: true }; });
  h('vpn:stop', async () => { await vpn.stop(); push(); return { ok: true }; });
  h('vpn:open-log', () => { shell.openPath(path.join(app.getPath('userData'), 'vpn')); return { ok: true }; });
}

// ---------------- lifecycle ----------------
async function init() {
  Menu.setApplicationMenu(null);
  store = new Store(app.getPath('userData'));
  meta = new Meta(store, { fetch: (u, o) => net.fetch(u, o), getIcon, onUpdate: push, log });
  tracker = new Tracker(store, { onChange: push, onSession: push, log });
  zapret = new Zapret(store, { onChange: push, log });
  accounts = new Accounts(store, { fetch: (u, o) => net.fetch(u, o), log, onChange: push, getWindow: () => win });
  browser = new StoreBrowser(store, { getWindow: () => win, onChange: push, log });
  vpn = new Vpn(store, {
    onChange: push, log, fetch: (u, o) => net.fetch(u, o), dataDir: app.getPath('userData'),
    getGameDirs: () => Object.values(store.games).filter((g) => !g.missing && g.installed !== false && g.installDir).map((g) => g.installDir),
  });
  registerIpc();
  createWindow();
  browser.attachWindow(win);
  createTray();
  applyHotkey();
  applyAutostart();
  tracker.start();
  pollNet();
  if (store.settings.scanOnStart || !Object.keys(store.games).length) setTimeout(runScan, 600);
  // owned libraries: refresh at most once in 12 hours, a bit after start
  if (store.settings.syncAccountsOnStart) {
    setTimeout(() => {
      const stale = Object.values(store.settings.accounts || {}).some((a) => a.id && Date.now() - (a.updatedAt || 0) > 12 * 3600e3);
      if (stale) accounts.syncAll().then(push);
    }, 15000);
  }
  else if (store.settings.autoFetchMeta) meta.enqueueMissing();
}

app.on('before-quit', (e) => {
  quitting = true;
  if (netCleaned) return;
  e.preventDefault();
  netCleaned = true;
  if (tracker) tracker.stop();
  if (store) store.flush();
  // restore the system proxy / stop VPN before exiting (max 8 s)
  Promise.race([netShutdown(), new Promise((r) => setTimeout(r, 8000))]).finally(() => {
    if (store) store.flush();
    app.quit();
  });
});
app.on('will-quit', () => globalShortcut.unregisterAll());
app.on('window-all-closed', () => {
  if (!store || !store.settings.closeToTray) { quitting = true; app.quit(); }
});
