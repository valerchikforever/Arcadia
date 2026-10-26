'use strict';
const fs = require('fs');
const path = require('path');
const { parseVDF, ci, ciPath } = require('../util/vdf');
const { exists, fixPath } = require('../util/common');

const SKIP_APPIDS = new Set(['228980', '1070560', '1391110', '1628350', '1493710', '1826330', '2180100', '250820']);
const SKIP_NAME = /^(proton|steam linux runtime|steamworks common|steamvr$|steam controller|source sdk base)/i;

function findSteamPath(ctx) {
  const cands = [];
  const hkcu = ctx.reg['HKEY_CURRENT_USER\\SOFTWARE\\Valve\\Steam'];
  if (hkcu && hkcu.values) cands.push(hkcu.values.SteamPath, hkcu.values.steampath);
  const hklm = ctx.reg['HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Valve\\Steam'];
  if (hklm && hklm.values) cands.push(hklm.values.InstallPath);
  if (ctx.steamPath) cands.unshift(ctx.steamPath);
  cands.push('C:\\Program Files (x86)\\Steam', 'C:\\Program Files\\Steam');
  for (const c of cands) if (c && exists(path.join(fixPath(c), 'steamapps'))) return fixPath(c);
  return null;
}

function libraryFolders(steamPath) {
  const libs = new Set([steamPath]);
  const file = path.join(steamPath, 'steamapps', 'libraryfolders.vdf');
  try {
    const data = parseVDF(fs.readFileSync(file, 'utf8'));
    const root = ci(data, 'libraryfolders') || {};
    for (const [k, v] of Object.entries(root)) {
      if (!/^\d+$/.test(k)) continue;
      const p = typeof v === 'string' ? v : ci(v, 'path');
      if (p) libs.add(fixPath(p));
    }
  } catch {}
  return [...libs].filter((l) => exists(path.join(l, 'steamapps')));
}

// Steam's own playtime, from the most recently used local profile
function readPlaytime(steamPath) {
  const result = {};
  const ud = path.join(steamPath, 'userdata');
  let users = [];
  try {
    users = fs.readdirSync(ud).map((u) => {
      const f = path.join(ud, u, 'config', 'localconfig.vdf');
      try { return { f, t: fs.statSync(f).mtimeMs }; } catch { return null; }
    }).filter(Boolean).sort((a, b) => b.t - a.t);
  } catch {}
  if (!users.length) return result;
  try {
    const data = parseVDF(fs.readFileSync(users[0].f, 'utf8'));
    const apps = ciPath(data, 'UserLocalConfigStore', 'Software', 'Valve', 'Steam', 'apps') || {};
    for (const [id, v] of Object.entries(apps)) {
      const pt = parseInt(ci(v, 'Playtime') || '0', 10);
      const lp = parseInt(ci(v, 'LastPlayed') || '0', 10);
      if (pt || lp) result[id] = { seconds: pt * 60, lastPlayed: lp ? lp * 1000 : 0 };
    }
  } catch {}
  return result;
}

function findLocalArt(steamPath, appid) {
  const base = path.join(steamPath, 'appcache', 'librarycache');
  const art = {};
  const flat = (n) => path.join(base, `${appid}_${n}`);
  if (exists(flat('library_600x900.jpg'))) art.cover = flat('library_600x900.jpg');
  if (exists(flat('library_hero.jpg'))) art.hero = flat('library_hero.jpg');
  const dir = path.join(base, String(appid));
  if (exists(dir)) {
    const walk = (d, depth) => {
      let ents = [];
      try { ents = fs.readdirSync(d, { withFileTypes: true }); } catch { return; }
      for (const e of ents) {
        const full = path.join(d, e.name);
        if (e.isDirectory() && depth < 2) walk(full, depth + 1);
        else if (e.isFile()) {
          const n = e.name.toLowerCase();
          if (!art.cover && (n === 'library_600x900.jpg' || n === 'library_capsule.jpg')) art.cover = full;
          if (!art.hero && n === 'library_hero.jpg') art.hero = full;
        }
      }
    };
    walk(dir, 0);
  }
  return art;
}

async function scan(ctx) {
  const steamPath = findSteamPath(ctx);
  if (!steamPath) return [];
  const playtime = readPlaytime(steamPath);
  const games = [];
  const seen = new Set();
  for (const lib of libraryFolders(steamPath)) {
    const sa = path.join(lib, 'steamapps');
    let files = [];
    try { files = fs.readdirSync(sa).filter((f) => /^appmanifest_\d+\.acf$/i.test(f)); } catch {}
    for (const f of files) {
      try {
        const st = ci(parseVDF(fs.readFileSync(path.join(sa, f), 'utf8')), 'AppState');
        if (!st) continue;
        const appid = String(ci(st, 'appid') || '');
        const name = ci(st, 'name') || '';
        const flags = parseInt(ci(st, 'StateFlags') || '4', 10);
        if (!appid || seen.has(appid) || SKIP_APPIDS.has(appid) || SKIP_NAME.test(name)) continue;
        if (!(flags & 4)) continue; // not fully installed
        seen.add(appid);
        const installDir = path.join(sa, 'common', ci(st, 'installdir') || name);
        const pt = playtime[appid] || {};
        games.push({
          id: `steam:${appid}`,
          source: 'steam',
          name,
          installDir,
          steamAppId: appid,
          launch: { type: 'uri', uri: `steam://rungameid/${appid}` },
          importedPlaytime: pt.seconds || 0,
          importedLastPlayed: pt.lastPlayed || 0,
          localArt: findLocalArt(steamPath, appid),
        });
      } catch (e) { ctx.log && ctx.log('steam manifest', f, e.message); }
    }
  }
  return games;
}

module.exports = { scan, findSteamPath, libraryFolders, readPlaytime };
