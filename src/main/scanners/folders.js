'use strict';
// Games without a launcher (torrents, repacks, portable, DRM-free): every sub-folder of a watched folder is a game.
const fs = require('fs');
const path = require('path');
const { normPath, isInside, cleanName, exists, isDir } = require('../util/common');
const { findGameExe, listExes } = require('../util/exefinder');

// Folder names that are containers, not the game name
const GENERIC = /^(bin|bin32|bin64|binaries|win32|win64|x64|x86|game|games|release|retail|client|app|exe|build)$/i;

function nameForExe(exe) {
  let dir = path.dirname(exe);
  for (let i = 0; i < 4 && GENERIC.test(path.basename(dir)); i++) dir = path.dirname(dir);
  const n = cleanName(path.basename(dir));
  return { name: n || cleanName(path.basename(exe, path.extname(exe))), dir };
}

function makeFolderGame(dir, exe) {
  return {
    id: `folder:${normPath(dir)}`,
    source: 'folder',
    name: cleanName(path.basename(dir)),
    installDir: dir,
    exePath: exe,
    launch: { type: 'exe', path: exe, args: '', cwd: path.dirname(exe) },
  };
}

async function scan(ctx, settings, claimedDirs = []) {
  const games = [];
  const claimed = claimedDirs.map(normPath).filter(Boolean);
  const isClaimed = (d) => claimed.some((c) => isInside(d, c) || isInside(c, d));

  for (const root of settings.scanFolders || []) {
    if (!isDir(root)) continue;
    // The folder itself is a single game (exe right inside it)
    const direct = listExes(root, 0);
    if (direct.length && !isClaimed(root)) {
      const exe = findGameExe(root, path.basename(root), 0);
      if (exe) { games.push(makeFolderGame(root, exe)); continue; }
    }
    let ents = [];
    try { ents = fs.readdirSync(root, { withFileTypes: true }); } catch { continue; }
    for (const e of ents) {
      if (!e.isDirectory()) continue;
      const dir = path.join(root, e.name);
      if (isClaimed(dir)) continue;
      const exe = findGameExe(dir, cleanName(e.name), 3);
      if (!exe) continue; // e.g. an unpacked repack that still has only setup.exe
      games.push(makeFolderGame(dir, exe));
    }
  }
  return games;
}

module.exports = { scan, nameForExe, makeFolderGame };
