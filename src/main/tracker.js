'use strict';
// Playtime tracking: a game is "running" while any process lives inside its folder.
// Works for launcher-started games too (Steam/Epic/Battle.net spawn the game themselves).
const path = require('path');
const { normPath } = require('./util/common');
const { listProcessPaths } = require('./util/winsys');

const GENERIC_DIRS = /\/(program files( \(x86\))?|programdata|windows|users|games|steamapps\/common|steamapps|steam|epic games|gog games|ubisoft|origin games|ea games)$/i;

function trackDirOf(g) {
  const d = g.trackDir || g.installDir || (g.launch && g.launch.type === 'exe' && g.launch.path ? path.dirname(g.launch.path) : '');
  const n = normPath(d);
  if (!n || /^[a-z]:$/.test(n) || n.split('/').length < 2 || GENERIC_DIRS.test(n)) return null;
  return n;
}

class Tracker {
  constructor(store, { onChange, onSession, log }) {
    this.store = store;
    this.onChange = onChange || (() => {});
    this.onSession = onSession || (() => {});
    this.log = log || (() => {});
    this.pendingLaunch = new Map(); // id -> deadline
    this.active = new Map(); // id -> { start, lastSeen }
    this.timer = null;
    this.busy = false;
  }

  runningIds() { return [...this.active.keys()]; }

  start() { this.schedule(4000); }
  stop() {
    clearTimeout(this.timer);
    const now = Date.now();
    for (const [id, s] of this.active) this.store.addSession(id, s.start, now);
    this.active.clear();
  }

  schedule(ms) {
    clearTimeout(this.timer);
    this.timer = setTimeout(() => this.tick(), ms);
  }

  expect(game) {
    if (!trackDirOf(game)) return;
    this.pendingLaunch.set(game.id, Date.now() + 5 * 60 * 1000);
    this.schedule(5000);
  }

  async tick() {
    if (this.busy) return;
    this.busy = true;
    try {
      const watchAll = this.store.settings.trackExternal;
      const now = Date.now();
      for (const [id, dl] of this.pendingLaunch) if (dl < now) this.pendingLaunch.delete(id);
      if (watchAll || this.pendingLaunch.size || this.active.size) {
        const procs = (await listProcessPaths()).map(normPath);
        let changed = false;
        for (const g of Object.values(this.store.games)) {
          if (g.missing || g.installed === false) continue;
          const watched = watchAll || this.pendingLaunch.has(g.id) || this.active.has(g.id);
          if (!watched) continue;
          const dir = trackDirOf(g);
          if (!dir) continue;
          const running = procs.some((p) => p.startsWith(dir + '/'));
          const cur = this.active.get(g.id);
          if (running && !cur) {
            this.active.set(g.id, { start: now, lastSeen: now });
            this.pendingLaunch.delete(g.id);
            this.store.update(g.id, { lastPlayed: now });
            changed = true;
          } else if (running && cur) {
            cur.lastSeen = now;
          } else if (!running && cur) {
            this.active.delete(g.id);
            this.store.addSession(g.id, cur.start, now);
            this.onSession(g.id);
            changed = true;
          }
        }
        if (changed) this.onChange();
      }
    } catch (e) { this.log('tracker', e.message); }
    this.busy = false;
    const fast = this.pendingLaunch.size || this.active.size;
    this.schedule(fast ? 10000 : 30000);
  }
}

module.exports = { Tracker, trackDirOf };
