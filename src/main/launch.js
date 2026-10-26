'use strict';
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const { shell } = require('electron');
const { parseArgs } = require('./util/common');

function launchSpec(g) {
  return g.launchOverride && (g.launchOverride.path || g.launchOverride.uri) ? g.launchOverride : g.launch;
}

async function launchGame(g) {
  const L = launchSpec(g);
  if (!L) throw new Error('Не указан способ запуска');
  if (L.type === 'uri') {
    await shell.openExternal(L.uri);
    return;
  }
  if (!L.path || !fs.existsSync(L.path)) throw new Error(`Файл не найден: ${L.path || '—'}`);
  const ext = path.extname(L.path).toLowerCase();
  if (['.lnk', '.url', '.bat', '.cmd'].includes(ext) && !L.args) {
    const err = await shell.openPath(L.path);
    if (err) throw new Error(err);
    return;
  }
  await new Promise((resolve, reject) => {
    let child;
    try {
      child = spawn(L.path, parseArgs(L.args), {
        cwd: L.cwd && fs.existsSync(L.cwd) ? L.cwd : path.dirname(L.path),
        detached: true,
        stdio: 'ignore',
        windowsHide: false,
        windowsVerbatimArguments: false,
      });
    } catch (e) { return reject(e); }
    child.once('spawn', () => { child.unref(); resolve(); });
    child.once('error', async (e) => {
      // Game needs admin rights (ERROR_ELEVATION_REQUIRED) — let Windows show the UAC prompt
      if (e.code === 'EACCES' || e.errno === -4092 || /740|elevation/i.test(e.message)) {
        const err = await shell.openPath(L.path);
        return err ? reject(new Error(err)) : resolve();
      }
      reject(e);
    });
  });
}

module.exports = { launchGame, launchSpec };
