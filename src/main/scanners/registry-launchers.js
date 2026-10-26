'use strict';
// GOG, Ubisoft Connect, EA app, Battle.net and BattleState (Tarkov) — all discovered via the Windows registry.
const fs = require('fs');
const path = require('path');
const { fixPath, exists, parseArgs } = require('../util/common');
const { findGameExe } = require('../util/exefinder');

const K = {
  GOG: ['HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\GOG.com\\Games', 'HKEY_LOCAL_MACHINE\\SOFTWARE\\GOG.com\\Games'],
  UBI: ['HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Ubisoft\\Launcher\\Installs', 'HKEY_LOCAL_MACHINE\\SOFTWARE\\Ubisoft\\Launcher\\Installs'],
};

function subkeys(reg, keys) {
  const out = [];
  for (const k of keys) if (reg[k] && reg[k].subkeys) out.push(...reg[k].subkeys);
  return out;
}

// ---------- GOG ----------
async function scanGog(ctx) {
  const games = [];
  const seen = new Set();
  for (const sk of subkeys(ctx.reg, K.GOG)) {
    const v = sk.values || {};
    const id = String(v.gameID || sk.name);
    if (seen.has(id)) continue;
    if (v.dependsOn && String(v.dependsOn).trim()) continue; // DLC
    const dir = fixPath(v.path || v.PATH);
    if (!dir || !exists(dir)) continue;
    let exe = fixPath(v.exe || v.EXE || v.launchCommand);
    if (!exists(exe)) exe = findGameExe(dir, v.gameName) || '';
    seen.add(id);
    games.push({
      id: `gog:${id}`,
      source: 'gog',
      name: v.gameName || v.GAMENAME || path.basename(dir),
      installDir: dir,
      exePath: exe,
      launch: { type: 'exe', path: exe, args: v.launchParam || '', cwd: fixPath(v.workingDir) || dir },
    });
  }
  return games;
}

// ---------- Ubisoft Connect ----------
async function scanUbisoft(ctx) {
  const names = {};
  for (const u of ctx.uninstall) {
    const m = /Uplay Install (\d+)$/i.exec(u.key);
    if (m && u.DisplayName) names[m[1]] = u.DisplayName;
  }
  const games = [];
  for (const sk of subkeys(ctx.reg, K.UBI)) {
    const id = sk.name;
    const dir = fixPath((sk.values || {}).InstallDir);
    if (!dir || !exists(dir)) continue;
    games.push({
      id: `ubisoft:${id}`,
      source: 'ubisoft',
      name: names[id] || path.basename(dir),
      installDir: dir,
      exePath: findGameExe(dir, names[id] || path.basename(dir)) || '',
      launch: { type: 'uri', uri: `uplay://launch/${id}/0` },
    });
  }
  return games;
}

// ---------- EA app ----------
const EA_PUB = /(electronic arts|^ea\b|ea swiss|ea digital illusions|respawn|criterion|bioware|maxis|codemasters|motive)/i;

function eaExeFromInstallerData(dir) {
  const f = path.join(dir, '__Installer', 'installerdata.xml');
  try {
    const xml = fs.readFileSync(f, 'utf8');
    const re = /<filePath>([^<]+)<\/filePath>/gi;
    let m;
    while ((m = re.exec(xml))) {
      const rel = m[1].replace(/^\[[^\]]*\]/, '').replace(/^[\\/]+/, '');
      if (!/\.exe$/i.test(rel)) continue;
      const full = path.join(dir, rel);
      if (exists(full)) return full;
    }
  } catch {}
  return null;
}

async function scanEA(ctx) {
  const games = [];
  const seen = new Set();
  for (const u of ctx.uninstall) {
    const name = u.DisplayName || '';
    if (!name || /^(ea app|ea|origin|ea desktop)$/i.test(name.trim())) continue;
    const dir = fixPath(u.InstallLocation);
    if (!dir || !exists(dir)) continue;
    const hasInstallerData = exists(path.join(dir, '__Installer', 'installerdata.xml'));
    const eaUninst = /(EAInstaller|EA Desktop|EADesktop|Origin)/i.test(u.UninstallString || '');
    if (!(hasInstallerData || (eaUninst && EA_PUB.test(u.Publisher || '')))) continue;
    const key = dir.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    const exe = eaExeFromInstallerData(dir) || findGameExe(dir, name) || '';
    if (!exe) continue;
    games.push({
      id: `ea:${(u.key.split('\\').pop() || name).toLowerCase()}`,
      source: 'ea',
      name,
      installDir: dir,
      exePath: exe,
      launch: { type: 'exe', path: exe, args: '', cwd: path.dirname(exe) },
    });
  }
  return games;
}

// ---------- Battle.net ----------
// uid (from uninstall string) -> Battle.net product code used by --exec="launch CODE"
const BNET_CODES = {
  wow: 'WoW', wow_classic: 'WoWC', wow_classic_era: 'WoWC', d3: 'D3', diablo3: 'D3', hs_beta: 'WTCG',
  hearthstone: 'WTCG', heroes: 'Hero', prometheus: 'Pro', overwatch: 'Pro', s2: 'S2', s1: 'S1', w3: 'W3',
  osi: 'OSI', fenris: 'Fen', anbs: 'ANBS', viper: 'VIPR', odin: 'ODIN', zeus: 'ZEUS', fore: 'FORE',
  lazarus: 'LAZR', rtro: 'RTRO', wlby: 'WLBY',
};

function findBnetExe(ctx) {
  for (const u of ctx.uninstall) {
    if (/^battle\.net$/i.test((u.DisplayName || '').trim()) && u.InstallLocation) {
      for (const n of ['Battle.net.exe', 'Battle.net Launcher.exe']) {
        const p = path.join(fixPath(u.InstallLocation), n);
        if (exists(p)) return p;
      }
    }
  }
  for (const p of ['C:\\Program Files (x86)\\Battle.net\\Battle.net.exe', 'C:\\Program Files\\Battle.net\\Battle.net.exe']) if (exists(p)) return p;
  return null;
}

async function scanBattleNet(ctx) {
  const bnet = findBnetExe(ctx);
  const games = [];
  for (const u of ctx.uninstall) {
    const us = u.UninstallString || '';
    if (!/battle\.net/i.test(us) || !/--uid=/i.test(us)) continue;
    const name = u.DisplayName || '';
    if (!name || /^battle\.net$/i.test(name.trim())) continue;
    const dir = fixPath(u.InstallLocation);
    if (!dir || !exists(dir)) continue;
    let uid = (/--uid=([^\s"]+)/i.exec(us) || [])[1] || '';
    uid = uid.toLowerCase().replace(/_[a-z]{2}[a-z]{2}$/, '');
    const code = BNET_CODES[uid];
    const exe = findGameExe(dir, name) || '';
    let launch;
    if (bnet && code) launch = { type: 'exe', path: bnet, args: `--exec="launch ${code}"`, cwd: path.dirname(bnet) };
    else if (exe) launch = { type: 'exe', path: exe, args: '', cwd: path.dirname(exe) };
    else continue;
    games.push({ id: `battlenet:${uid || name.toLowerCase()}`, source: 'battlenet', name, installDir: dir, exePath: exe, launch });
  }
  return games;
}

// ---------- BattleState Games (Escape from Tarkov / Arena) ----------
async function scanBattleState(ctx) {
  const bsg = ctx.uninstall.filter((u) => /battlestate/i.test(u.Publisher || '') || /bsglauncher/i.test(u.DisplayName || ''));
  let launcher = null;
  for (const u of bsg) {
    if (/launcher/i.test(u.DisplayName || '') && u.InstallLocation) {
      const p = path.join(fixPath(u.InstallLocation), 'BsgLauncher.exe');
      if (exists(p)) launcher = p;
    }
  }
  if (!launcher) {
    for (const p of ['C:\\Battlestate Games\\BsgLauncher\\BsgLauncher.exe', 'C:\\Program Files\\Battlestate Games\\BsgLauncher\\BsgLauncher.exe']) if (exists(p)) launcher = p;
  }
  const games = [];
  for (const u of bsg) {
    const name = u.DisplayName || '';
    if (!name || /launcher/i.test(name)) continue;
    const dir = fixPath(u.InstallLocation);
    if (!dir || !exists(dir)) continue;
    const exe = findGameExe(dir, name) || '';
    // Tarkov cannot start without its launcher (auth), so we open BsgLauncher and track the game folder.
    const launch = launcher
      ? { type: 'exe', path: launcher, args: '', cwd: path.dirname(launcher) }
      : { type: 'exe', path: exe, args: '', cwd: dir };
    if (!launch.path) continue;
    games.push({ id: `battlestate:${name.toLowerCase().replace(/\s+/g, '-')}`, source: 'battlestate', name, installDir: dir, exePath: exe, trackDir: dir, launch });
  }
  return games;
}

module.exports = { scanGog, scanUbisoft, scanEA, scanBattleNet, scanBattleState, BNET_CODES, parseArgs };
