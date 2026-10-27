'use strict';
const fs = require('fs');
const path = require('path');
const { exists, normPath } = require('./util/common');

const DEFAULT_SETTINGS = {
  sources: { steam: true, epic: true, gog: true, ea: true, ubisoft: true, battlenet: true, battlestate: true, folder: true },
  scanFolders: [],
  sgdbKey: '',
  autoFetchMeta: true,
  closeToTray: true,
  autostart: false,
  startMinimized: false,
  minimizeOnLaunch: true,
  trackExternal: true,
  hotkey: 'CommandOrControl+Alt+G',
  cardSize: 190,
  sort: 'name',
  ignoredIds: [],
  scanOnStart: true,
  zapret: { dir: '', strategy: '', autostart: false, gameFilter: false, stopOnExit: false },
  vpn: { servers: [], subs: [], selected: 'auto', mode: 'proxy', directRu: true, gamesDirect: true, autostart: false, stopOnExit: true, port: 2080 },
  accounts: {},
  steamApiKey: '',
  showUninstalled: false,
  syncAccountsOnStart: true,
  browser: { economy: true, blockTrackers: true, autoEnable: false, unloadOnMinimizeSec: 60, unloadOnLeaveSec: 30 },
};

// Fields that come from a store account (owned games that may not be installed)
const OWNED_FIELDS = ['installUri', 'storeUrl', 'coverUrl', 'heroUrl'];

// Fields the scanner owns (refreshed every scan); everything else belongs to the user
const SCAN_FIELDS = ['source', 'name', 'installDir', 'exePath', 'launch', 'trackDir', 'steamAppId', 'importedPlaytime', 'importedLastPlayed', 'localArt'];

class Store {
  constructor(dir) {
    this.dir = dir;
    this.file = path.join(dir, 'library.json');
    this.mediaDir = path.join(dir, 'media');
    fs.mkdirSync(this.mediaDir, { recursive: true });
    this.games = {};
    this.collections = [];
    this.settings = { ...DEFAULT_SETTINGS };
    this.scanStatus = {};
    this._timer = null;
    this.load();
  }

  load() {
    for (const f of [this.file, this.file + '.bak']) {
      try {
        const d = JSON.parse(fs.readFileSync(f, 'utf8'));
        this.games = d.games || {};
        this.collections = d.collections || [];
        const s = d.settings || {};
        this.settings = {
          ...DEFAULT_SETTINGS, ...s,
          sources: { ...DEFAULT_SETTINGS.sources, ...(s.sources || {}) },
          zapret: { ...DEFAULT_SETTINGS.zapret, ...(s.zapret || {}) },
          vpn: { ...DEFAULT_SETTINGS.vpn, ...(s.vpn || {}) },
          accounts: { ...(s.accounts || {}) },
          browser: { ...DEFAULT_SETTINGS.browser, ...(s.browser || {}) },
        };
        return;
      } catch {}
    }
  }

  save() {
    clearTimeout(this._timer);
    this._timer = setTimeout(() => this.flush(), 400);
  }

  flush() {
    clearTimeout(this._timer);
    const data = JSON.stringify({ version: 1, games: this.games, collections: this.collections, settings: this.settings }, null, 1);
    try {
      if (exists(this.file)) fs.copyFileSync(this.file, this.file + '.bak');
      fs.writeFileSync(this.file + '.tmp', data);
      fs.renameSync(this.file + '.tmp', this.file);
    } catch (e) { console.error('save failed', e); }
  }

  get(id) { return this.games[id]; }

  update(id, patch) {
    const g = this.games[id];
    if (!g) return null;
    Object.assign(g, patch);
    this.save();
    return g;
  }

  addGame(g) {
    const old = this.games[g.id];
    this.games[g.id] = old ? { ...old, ...pickScan(g), missing: false } : newGame(g);
    this.settings.ignoredIds = this.settings.ignoredIds.filter((x) => x !== g.id);
    this.save();
    return this.games[g.id];
  }

  remove(id) {
    const g = this.games[id];
    if (!g) return;
    delete this.games[id];
    if (g.source !== 'manual' && !this.settings.ignoredIds.includes(id)) this.settings.ignoredIds.push(id);
    this.save();
  }

  mergeScan({ games, status }) {
    this.scanStatus = status || {};
    const seen = new Set();
    const ignored = new Set(this.settings.ignoredIds);
    let added = 0;
    for (const f of games) {
      if (ignored.has(f.id)) continue;
      seen.add(f.id);
      const old = this.games[f.id];
      if (old) {
        // an owned game that just got installed keeps its account data, but gets a real launch method
        this.games[f.id] = { ...old, ...pickScan(f), missing: false, installed: true };
      } else { this.games[f.id] = newGame(f); added++; }
    }
    for (const g of Object.values(this.games)) {
      if (seen.has(g.id)) continue;
      if (g.owned) {
        // bought on an account but not on disk (any more) — show it as "not installed", not as missing
        const st = this.scanStatus[g.source];
        if (!(st && st.error)) { g.installed = false; g.missing = false; }
        continue;
      }
      if (g.source === 'manual') {
        g.missing = g.launch && g.launch.type === 'exe' ? !exists(g.launch.path) : false;
        continue;
      }
      const st = this.scanStatus[g.source];
      if (st && st.enabled === false) continue; // source switched off: keep as is
      if (st && st.error) continue;
      g.missing = true;
    }
    this.save();
    return added;
  }

  // Owned games from a store account. Returns how many new games appeared.
  mergeOwned(source, list) {
    const ignored = new Set(this.settings.ignoredIds);
    const ids = new Set();
    let added = 0;
    for (const o of list) {
      if (ignored.has(o.id)) continue;
      ids.add(o.id);
      const g = this.games[o.id];
      const acc = {};
      for (const k of OWNED_FIELDS) if (o[k]) acc[k] = o[k];
      if (g) {
        Object.assign(g, acc, { owned: true });
        if (o.importedPlaytime > (g.importedPlaytime || 0)) g.importedPlaytime = o.importedPlaytime;
        if (o.importedLastPlayed > (g.importedLastPlayed || 0)) g.importedLastPlayed = o.importedLastPlayed;
        if (!g.steamAppId && o.steamAppId) g.steamAppId = o.steamAppId;
        if (!g.developer && o.developer) g.developer = o.developer;
      } else {
        const ng = newGame({ id: o.id, source: source, name: o.name, steamAppId: o.steamAppId, importedPlaytime: o.importedPlaytime || 0, importedLastPlayed: o.importedLastPlayed || 0 });
        Object.assign(ng, acc, { owned: true, installed: false, metaTried: true, developer: o.developer || '' });
        this.games[o.id] = ng;
        added++;
      }
    }
    // no longer on the account (refund, family sharing ended): drop if it isn't installed either
    for (const g of Object.values(this.games)) {
      if (g.source === source && g.owned && !ids.has(g.id)) {
        if (g.installed === false) delete this.games[g.id];
        else g.owned = false;
      }
    }
    this.save();
    return added;
  }

  removeOwned(source) {
    for (const g of Object.values(this.games)) {
      if (g.source !== source || !g.owned) continue;
      if (g.installed === false) delete this.games[g.id];
      else { g.owned = false; for (const k of OWNED_FIELDS) delete g[k]; }
    }
    this.save();
  }

  // Directories of manual games — so the folder scanner doesn't add them twice
  manualDirs() {
    return Object.values(this.games).filter((g) => g.source === 'manual' && g.installDir).map((g) => g.installDir);
  }

  addSession(id, start, end) {
    const g = this.games[id];
    if (!g) return;
    const secs = Math.max(0, Math.round((end - start) / 1000));
    if (secs < 20) return; // ignore accidental starts
    g.playtime = (g.playtime || 0) + secs;
    g.lastPlayed = end;
    g.sessions = [...(g.sessions || []), { start, end }].slice(-100);
    this.save();
  }

  createCollection(name) {
    const c = { id: 'c' + Date.now().toString(36) + Math.random().toString(36).slice(2, 5), name: String(name).slice(0, 60) };
    this.collections.push(c);
    this.save();
    return c;
  }
  renameCollection(id, name) {
    const c = this.collections.find((x) => x.id === id);
    if (c) { c.name = String(name).slice(0, 60); this.save(); }
  }
  deleteCollection(id) {
    this.collections = this.collections.filter((x) => x.id !== id);
    for (const g of Object.values(this.games)) if (g.collections) g.collections = g.collections.filter((x) => x !== id);
    this.save();
  }

  setSettings(patch) {
    this.settings = { ...this.settings, ...patch, sources: { ...this.settings.sources, ...(patch.sources || {}) } };
    this.save();
  }
}

function pickScan(f) {
  const o = {};
  for (const k of SCAN_FIELDS) if (f[k] !== undefined) o[k] = f[k];
  return o;
}

function newGame(f) {
  return {
    ...pickScan(f),
    id: f.id,
    customName: '',
    favorite: false,
    hidden: false,
    collections: [],
    playtime: 0,
    lastPlayed: 0,
    lastLaunched: 0,
    sessions: [],
    addedAt: Date.now(),
    coverPath: '',
    heroPath: '',
    iconPath: '',
    description: '',
    developer: '',
    releaseDate: '',
    genres: [],
    metaTried: false,
    missing: false,
  };
}

module.exports = { Store, DEFAULT_SETTINGS, normPath };
