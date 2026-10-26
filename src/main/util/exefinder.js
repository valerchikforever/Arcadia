'use strict';
const fs = require('fs');
const path = require('path');
const { matchKey } = require('./common');

// Executables that are never the game itself
const BAD_EXE = new RegExp('^(' + [
  'unins\\d*', 'uninstall.*', 'setup.*', 'install(er)?', 'installer.*', 'vc_?redist.*', 'vcredist.*', 'dxsetup', 'dxwebsetup',
  'directx.*', 'dotnet.*', 'ndp\\d+.*', 'oalinst', 'physx.*', 'crash.*', '.*crashhandler.*', '.*crashreport.*',
  'unitycrashhandler(32|64)?', 'ue4prereqsetup.*', 'ueprereqsetup.*', 'prereq.*', 'redist.*', 'updater', 'update',
  'autoupdate.*', 'patcher', 'easyanticheat.*', 'eac.*', 'beservice.*', 'be_?setup.*', 'battleye.*',
  'quicksfv', 'notification_helper', 'cefprocess', 'cefsharp.*', 'zfgamebrowser', 'dowser', 'touchup', 'cleanup',
  'activation(ui)?', 'register', 'registration', 'report', 'bugreport.*', 'errorreporter', 'helper', 'service',
  'quickbms', 'cmd', 'python(w)?', 'java(w)?', 'node', '7z.*', 'unrar', 'winrar', 'unarc', 'isdone', 'dosbox.*',
  'gamelauncherhelper', 'link2ea', 'eadesktop', 'origin', 'gogbgsvc', 'galaxyclient.*', 'steam', 'steamservice',
  'steamerrorreporter.*', 'epicgameslauncher', 'upc', 'uplaywebcore', 'battle\\.net', 'agent', 'dxdiag', 'svc.*'
].join('|') + ')$', 'i');

const SKIP_DIRS = new Set([
  '_commonredist', 'commonredist', 'redist', '_redist', 'redistributables', 'directx', '__installer', 'support',
  'prerequisites', '_prerequisites', 'dotnet', 'vcredist', 'msvc', 'easyanticheat', 'battleye', 'eac', 'installers',
  'tools', 'sdk', 'docs', 'manual', 'soundtrack', 'ost', 'artbook', 'extras', 'bonus', 'crashreportclient',
  'thirdparty', 'third_party', '_temp', 'temp', 'logs', 'screenshots', 'saves', 'mods', 'mono', 'monobleedingedge',
  'webhelper', 'cef', 'node_modules', '.git', 'shadercache', 'cache', 'locales', 'swiftshader'
]);

function listExes(dir, maxDepth = 3) {
  const out = [];
  let budget = 4000;
  const walk = (d, depth) => {
    if (budget <= 0) return;
    let entries;
    try { entries = fs.readdirSync(d, { withFileTypes: true }); } catch { return; }
    for (const e of entries) {
      if (--budget <= 0) return;
      const full = path.join(d, e.name);
      if (e.isDirectory()) {
        if (depth < maxDepth && !SKIP_DIRS.has(e.name.toLowerCase())) walk(full, depth + 1);
      } else if (e.isFile() && /\.exe$/i.test(e.name)) {
        const base = e.name.replace(/\.exe$/i, '');
        if (BAD_EXE.test(base)) continue;
        let size = 0;
        try { size = fs.statSync(full).size; } catch {}
        out.push({ path: full, base, depth, size });
      }
    }
  };
  walk(dir, 0);
  return out;
}

function scoreExe(exe, hint) {
  const b = matchKey(exe.base.replace(/[-_](win)?(64|32|x64|x86|shipping|dx\d+|vulkan)\b/gi, ' '));
  const h = matchKey(hint);
  let s = 0;
  if (h) {
    const compactB = b.replace(/\s/g, ''), compactH = h.replace(/\s/g, '');
    if (compactB === compactH) s += 60;
    else if (compactH.startsWith(compactB) || compactB.startsWith(compactH)) s += 40;
    else {
      const tb = b.split(' '), th = new Set(h.split(' '));
      const common = tb.filter((t) => t.length > 1 && th.has(t)).length;
      s += common * 15;
      // acronym: "Grand Theft Auto V" -> "gtav"
      const acr = h.split(' ').map((w) => w[0]).join('');
      if (acr.length >= 2 && compactB.startsWith(acr)) s += 30;
    }
  }
  s -= exe.depth * 12;
  s += Math.min(25, Math.log2(Math.max(1, exe.size / 65536)) * 3);
  if (/launcher/i.test(exe.base)) s -= 6;
  if (/(config|settings|setup|editor|server|benchmark|tool|debug|test)/i.test(exe.base)) s -= 25;
  if (/shipping/i.test(exe.base)) s += 4;
  if (/(64|x64)/i.test(exe.base)) s += 2;
  return s;
}

// Best guess at the main game executable inside dir
function findGameExe(dir, hint, maxDepth = 3) {
  const exes = listExes(dir, maxDepth);
  if (!exes.length) return null;
  let best = null, bestScore = -Infinity;
  for (const e of exes) {
    const sc = scoreExe(e, hint);
    if (sc > bestScore) { bestScore = sc; best = e; }
  }
  return best ? best.path : null;
}

module.exports = { findGameExe, listExes, BAD_EXE };
