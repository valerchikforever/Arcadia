'use strict';
const { readRegistry } = require('../util/winsys');
const steam = require('./steam');
const epic = require('./epic');
const rl = require('./registry-launchers');
const folders = require('./folders');

const REG_KEYS = [
  'HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall',
  'HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall',
  'HKEY_CURRENT_USER\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall',
  'HKEY_CURRENT_USER\\SOFTWARE\\Valve\\Steam',
  'HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Valve\\Steam',
  'HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\GOG.com\\Games',
  'HKEY_LOCAL_MACHINE\\SOFTWARE\\GOG.com\\Games',
  'HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Ubisoft\\Launcher\\Installs',
  'HKEY_LOCAL_MACHINE\\SOFTWARE\\Ubisoft\\Launcher\\Installs',
];

const UNINSTALL_KEYS = REG_KEYS.slice(0, 3);

const SOURCES = {
  steam: (ctx) => steam.scan(ctx),
  epic: (ctx) => epic.scan(ctx),
  gog: (ctx) => rl.scanGog(ctx),
  ea: (ctx) => rl.scanEA(ctx),
  ubisoft: (ctx) => rl.scanUbisoft(ctx),
  battlenet: (ctx) => rl.scanBattleNet(ctx),
  battlestate: (ctx) => rl.scanBattleState(ctx),
};

function collectUninstall(reg) {
  const out = [];
  for (const k of UNINSTALL_KEYS) {
    const e = reg[k];
    if (!e || !e.subkeys) continue;
    for (const sk of e.subkeys) out.push({ key: `${k}\\${sk.name}`, ...(sk.values || {}) });
  }
  return out;
}

async function scanAll(settings, { extraClaimed = [], log = () => {}, reg: regOverride, ctxOverride = {} } = {}) {
  let reg = regOverride;
  if (!reg) {
    try { reg = await readRegistry(REG_KEYS); } catch (e) { log('registry', e.message); reg = {}; }
  }
  const ctx = {
    reg,
    uninstall: collectUninstall(reg),
    programData: process.env.ProgramData || 'C:\\ProgramData',
    log,
    ...ctxOverride,
  };
  const games = [];
  const status = {};
  const ids = new Set();
  for (const [key, fn] of Object.entries(SOURCES)) {
    if (settings.sources && settings.sources[key] === false) { status[key] = { enabled: false }; continue; }
    try {
      const found = (await fn(ctx)).filter((g) => !ids.has(g.id));
      found.forEach((g) => ids.add(g.id));
      games.push(...found);
      status[key] = { count: found.length };
    } catch (e) {
      status[key] = { error: e.message };
      log(key, e.stack || e.message);
    }
  }
  if (!settings.sources || settings.sources.folder !== false) {
    const claimed = games.map((g) => g.installDir).filter(Boolean).concat(extraClaimed);
    try {
      const found = await folders.scan(ctx, settings, claimed);
      games.push(...found);
      status.folder = { count: found.length };
    } catch (e) { status.folder = { error: e.message }; }
  }
  return { games, status };
}

module.exports = { scanAll, collectUninstall, REG_KEYS };
