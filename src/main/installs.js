'use strict';
// Download / install progress of Steam games, read from steamapps/appmanifest_*.acf.
// Steam writes BytesToDownload / BytesDownloaded / BytesToStage / BytesStaged and StateFlags there while it works.
// Only files whose modification time changed are re-read, so polling costs a directory listing.
const fs = require('fs');
const path = require('path');
const { parseVDF, ci } = require('./util/vdf');
const { getSteamPath, libraryFolders } = require('./scanners/steam');

// EAppState bits
const F = {
  FullyInstalled: 4, UpdateRunning: 256, UpdatePaused: 512, UpdateStarted: 1024, Uninstalling: 2048,
  Validating: 131072, AddingFiles: 262144, Preallocating: 524288, Downloading: 1048576, Staging: 2097152, Committing: 4194304,
};

// Pure: manifest fields -> progress or null when nothing is going on
function progressFromManifest(st) {
  const flags = parseInt(ci(st, 'StateFlags') || '0', 10);
  const num = (k) => Number(ci(st, k) || 0);
  const toDl = num('BytesToDownload'), dl = num('BytesDownloaded');
  const toStage = num('BytesToStage'), staged = num('BytesStaged');
  const idle = (flags & F.FullyInstalled) && !(flags & (F.UpdateRunning | F.UpdateStarted | F.UpdatePaused | F.Validating));
  if (idle || (flags & F.Uninstalling)) return null;
  const busyFlags = F.UpdateRunning | F.UpdateStarted | F.UpdatePaused | F.Downloading | F.Staging | F.Committing | F.Preallocating | F.AddingFiles | F.Validating;
  if (!(flags & busyFlags) && !(toDl && dl < toDl)) return null;
  let phase = 'download';
  if (flags & F.Validating) phase = 'validate';
  else if (flags & F.Preallocating) phase = 'allocate';
  else if ((flags & (F.Staging | F.Committing)) || (toDl && dl >= toDl && toStage)) phase = 'install';
  let done = dl, total = toDl;
  if (phase === 'install' && toStage) { done = staged; total = toStage; }
  // Steam keeps 1026 (UpdateRequired|UpdateStarted) during a normal download; only 512 means paused
  const paused = !!(flags & F.UpdatePaused);
  return {
    phase, paused, done, total, pct: total ? Math.min(100, Math.floor((done / total) * 1000) / 10) : 0,
  };
}

class Installs {
  constructor(store, { onChange, onFinished, log, isVisible }) {
    this.store = store;
    this.onChange = onChange || (() => {});
    this.onFinished = onFinished || (() => {});
    this.log = log || (() => {});
    this.isVisible = isVisible || (() => true);
    this.cache = new Map(); // file -> { mtime, appid, progress }
    this.active = {}; // id -> progress (+speed, eta)
    this.samples = {}; // id -> [{t, done}]
    this.timer = null;
  }

  start() { this.tick(); }
  stop() { clearTimeout(this.timer); }
  kick() { clearTimeout(this.timer); this.timer = setTimeout(() => this.tick(), 1500); }

  readAll() {
    const steamPath = getSteamPath();
    if (!steamPath) return {};
    const out = {};
    for (const lib of libraryFolders(steamPath)) {
      const dir = path.join(lib, 'steamapps');
      let files = [];
      try { files = fs.readdirSync(dir).filter((f) => /^appmanifest_\d+\.acf$/i.test(f)); } catch { continue; }
      for (const f of files) {
        const full = path.join(dir, f);
        let mtime = 0;
        try { mtime = fs.statSync(full).mtimeMs; } catch { continue; }
        let c = this.cache.get(full);
        if (!c || c.mtime !== mtime) {
          let progress = null, appid = f.replace(/\D/g, ''), name = '';
          try {
            const st = ci(parseVDF(fs.readFileSync(full, 'utf8')), 'AppState') || {};
            appid = String(ci(st, 'appid') || appid);
            name = ci(st, 'name') || '';
            progress = progressFromManifest(st);
          } catch {}
          c = { mtime, appid, name, progress };
          this.cache.set(full, c);
        }
        if (c.progress) out[`steam:${c.appid}`] = { ...c.progress, name: c.name };
      }
    }
    return out;
  }

  tick() {
    clearTimeout(this.timer);
    let changed = false;
    try {
      const now = Date.now();
      const cur = this.readAll();
      for (const [id, p] of Object.entries(cur)) {
        const sm = (this.samples[id] = (this.samples[id] || []).filter((x) => now - x.t < 20000 && x.phase === p.phase));
        if (!sm.length || sm[sm.length - 1].done !== p.done) sm.push({ t: now, done: p.done, phase: p.phase });
        let speed = 0;
        if (sm.length > 1 && !p.paused) {
          const a = sm[0], b = sm[sm.length - 1];
          speed = b.t > a.t ? Math.max(0, (b.done - a.done) / ((b.t - a.t) / 1000)) : 0;
        }
        const next = { ...p, speed: Math.round(speed), eta: speed > 0 && p.total ? Math.round((p.total - p.done) / speed) : 0 };
        const prev = this.active[id];
        if (!prev || prev.done !== next.done || prev.phase !== next.phase || prev.paused !== next.paused || prev.speed !== next.speed) changed = true;
        this.active[id] = next;
      }
      for (const id of Object.keys(this.active)) {
        if (!cur[id]) {
          delete this.active[id];
          delete this.samples[id];
          changed = true;
          this.onFinished(id);
        }
      }
    } catch (e) { this.log('installs', e.message); }
    if (changed) this.onChange();
    // fast while something downloads and the window is open; slow otherwise
    const busy = Object.keys(this.active).length > 0;
    this.timer = setTimeout(() => this.tick(), busy ? (this.isVisible() ? 2000 : 6000) : (this.isVisible() ? 8000 : 30000));
  }

  state() { return this.active; }
}

module.exports = { Installs, progressFromManifest, F };
