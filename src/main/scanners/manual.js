'use strict';
// Games added by hand: .exe, .lnk shortcuts, .url (e.g. steam:// links), .bat
const fs = require('fs');
const path = require('path');
const { normPath, cleanName, isDir } = require('../util/common');
const { findGameExe } = require('../util/exefinder');
const { nameForExe } = require('./folders');

function readUrlFile(p) {
  try {
    const m = /^URL=(.+)$/im.exec(fs.readFileSync(p, 'utf8'));
    return m ? m[1].trim() : null;
  } catch { return null; }
}

// shell: Electron's shell module (readShortcutLink is Windows only)
function makeManualGame(file, shell) {
  const ext = path.extname(file).toLowerCase();
  if (ext === '.url') {
    const uri = readUrlFile(file);
    if (!uri) throw new Error('Не удалось прочитать ярлык');
    const steam = /^steam:\/\/rungameid\/(\d+)/i.exec(uri);
    return {
      id: `manual:${normPath(file)}`,
      source: 'manual',
      name: cleanName(path.basename(file, ext)),
      installDir: '',
      steamAppId: steam ? steam[1] : undefined,
      launch: { type: 'uri', uri },
    };
  }
  if (isDir(file)) {
    const exe = findGameExe(file, cleanName(path.basename(file)), 3);
    if (!exe) throw new Error('В папке не найден исполняемый файл игры');
    return {
      id: `manual:${normPath(exe)}`,
      source: 'manual',
      name: cleanName(path.basename(file)),
      installDir: file,
      exePath: exe,
      launch: { type: 'exe', path: exe, args: '', cwd: path.dirname(exe) },
    };
  }
  let target = file, args = '', cwd = '';
  if (ext === '.lnk' && shell && shell.readShortcutLink) {
    try {
      const l = shell.readShortcutLink(file);
      if (l.target) { target = l.target; args = l.args || ''; cwd = l.cwd || ''; }
    } catch {}
  }
  if (!/\.(exe|bat|cmd|lnk)$/i.test(target)) throw new Error('Поддерживаются .exe, .lnk, .url и .bat');
  const { name, dir } = nameForExe(target);
  return {
    id: `manual:${normPath(target)}`,
    source: 'manual',
    name: ext === '.lnk' ? cleanName(path.basename(file, ext)) : name,
    installDir: dir,
    exePath: target,
    launch: { type: 'exe', path: target, args, cwd: cwd || path.dirname(target) },
  };
}

module.exports = { makeManualGame };
