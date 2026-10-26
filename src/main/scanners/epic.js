'use strict';
const fs = require('fs');
const path = require('path');
const { fixPath, exists } = require('../util/common');

async function scan(ctx) {
  const dir = ctx.epicManifestDir || path.join(ctx.programData, 'Epic', 'EpicGamesLauncher', 'Data', 'Manifests');
  let files = [];
  try { files = fs.readdirSync(dir).filter((f) => /\.item$/i.test(f)); } catch { return []; }
  const games = [];
  const seen = new Set();
  for (const f of files) {
    try {
      const m = JSON.parse(fs.readFileSync(path.join(dir, f), 'utf8').replace(/^﻿/, ''));
      if (m.bIsIncompleteInstall) continue;
      const cats = (m.AppCategories || []).map((c) => String(c).toLowerCase());
      if (cats.length && !cats.includes('games')) continue;
      if (m.MainGameAppName && m.AppName && m.MainGameAppName !== m.AppName) continue; // DLC
      if (/^UE_\d/.test(m.AppName || '')) continue; // Unreal Engine itself
      const installDir = fixPath(m.InstallLocation);
      if (!installDir || !exists(installDir)) continue;
      const key = m.CatalogItemId || m.AppName;
      if (seen.has(key)) continue;
      seen.add(key);
      const ns = encodeURIComponent(m.CatalogNamespace || '');
      const item = encodeURIComponent(m.CatalogItemId || '');
      const app = encodeURIComponent(m.AppName || '');
      const exePath = m.LaunchExecutable ? path.join(installDir, m.LaunchExecutable) : '';
      games.push({
        id: `epic:${m.AppName || key}`,
        source: 'epic',
        name: m.DisplayName || m.AppName,
        installDir,
        exePath,
        launch: { type: 'uri', uri: `com.epicgames.launcher://apps/${ns}%3A${item}%3A${app}?action=launch&silent=true` },
      });
    } catch (e) { ctx.log && ctx.log('epic manifest', f, e.message); }
  }
  return games;
}

module.exports = { scan };
