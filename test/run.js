'use strict';
// Scanner tests on a fake Windows-like layout. Run: node test/run.js
const fs = require('fs');
const os = require('os');
const path = require('path');
const assert = require('assert');
const { parseVDF } = require('../src/main/util/vdf');
const { cleanName, similarity } = require('../src/main/util/common');
const { findGameExe } = require('../src/main/util/exefinder');
const { scanAll } = require('../src/main/scanners');

let passed = 0;
const t = (name, fn) => Promise.resolve().then(fn).then(() => { passed++; console.log('  ✓', name); }, (e) => { console.error('  ✗', name, '\n', e); process.exitCode = 1; });

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'arcadia-'));
const mk = (p, content = '') => { fs.mkdirSync(path.dirname(p), { recursive: true }); fs.writeFileSync(p, content); return p; };
const big = (p, kb) => mk(p, Buffer.alloc(kb * 1024));

(async () => {
  console.log('vdf / names');
  await t('parse vdf with escapes and nesting', () => {
    const d = parseVDF('"libraryfolders"\n{\n "0"\n {\n  "path"  "D:\\\\SteamLibrary"\n  "apps" { "570" "123" }\n }\n}');
    assert.strictEqual(d.libraryfolders['0'].path, 'D:\\SteamLibrary');
    assert.strictEqual(d.libraryfolders['0'].apps['570'], '123');
  });
  await t('cleanName strips repack noise', () => {
    assert.strictEqual(cleanName('Cyberpunk.2077.v2.1-FitGirl'), 'Cyberpunk 2077');
    assert.strictEqual(cleanName('Hollow Knight [GOG] (v1.5.78)'), 'Hollow Knight');
    assert.strictEqual(cleanName('Ведьмак 3 [R.G. Механики]'), 'Ведьмак 3');
    assert.strictEqual(cleanName('Elden Ring - DODI Repack'), 'Elden Ring');
    assert.strictEqual(cleanName('S.T.A.L.K.E.R. 2'), 'S.T.A.L.K.E.R. 2');
    assert.strictEqual(cleanName('Red_Dead_Redemption_2'), 'Red Dead Redemption 2');
  });
  await t('similarity', () => assert.ok(similarity('The Witcher 3: Wild Hunt', 'The Witcher® 3: Wild Hunt') > 0.9));

  console.log('exe finder');
  await t('picks the real game exe, not redist/uninstaller', () => {
    const d = path.join(tmp, 'G', 'Hollow Knight');
    big(path.join(d, 'unins000.exe'), 900);
    big(path.join(d, 'UnityCrashHandler64.exe'), 1200);
    big(path.join(d, '_CommonRedist', 'vcredist_x64.exe'), 5000);
    big(path.join(d, 'hollow_knight.exe'), 650);
    assert.strictEqual(path.basename(findGameExe(d, 'Hollow Knight')), 'hollow_knight.exe');
  });
  await t('prefers top-level UE exe over shipping binary deep inside', () => {
    const d = path.join(tmp, 'G', 'Stray');
    big(path.join(d, 'Stray.exe'), 300);
    big(path.join(d, 'Hk_project', 'Binaries', 'Win64', 'Stray-Win64-Shipping.exe'), 80000);
    assert.strictEqual(path.basename(findGameExe(d, 'Stray')), 'Stray.exe');
  });

  console.log('full scan');
  const pd = path.join(tmp, 'ProgramData');
  // Steam
  const steamDir = path.join(tmp, 'Steam');
  const lib2 = path.join(tmp, 'SteamLibrary');
  mk(path.join(steamDir, 'steamapps', 'libraryfolders.vdf'), `"libraryfolders"{"0"{"path" "${steamDir.replace(/\\/g, '\\\\')}"} "1"{"path" "${lib2.replace(/\\/g, '\\\\')}"}}`);
  mk(path.join(steamDir, 'steamapps', 'appmanifest_570.acf'), '"AppState"{"appid" "570" "name" "Dota 2" "installdir" "dota 2 beta" "StateFlags" "4"}');
  mk(path.join(steamDir, 'steamapps', 'appmanifest_228980.acf'), '"AppState"{"appid" "228980" "name" "Steamworks Common Redistributables" "installdir" "x" "StateFlags" "4"}');
  mk(path.join(lib2, 'steamapps', 'appmanifest_1091500.acf'), '"AppState"{"appid" "1091500" "name" "Cyberpunk 2077" "installdir" "Cyberpunk 2077" "StateFlags" "4"}');
  mk(path.join(lib2, 'steamapps', 'appmanifest_999.acf'), '"AppState"{"appid" "999" "name" "Half Downloaded" "installdir" "hd" "StateFlags" "1026"}');
  mk(path.join(steamDir, 'userdata', '42', 'config', 'localconfig.vdf'), '"UserLocalConfigStore"{"Software"{"Valve"{"Steam"{"apps"{"570"{"Playtime" "600" "LastPlayed" "1700000000"}}}}}}');
  mk(path.join(steamDir, 'appcache', 'librarycache', '570', 'abc', 'library_600x900.jpg'), 'img');
  // Epic
  const epicGame = path.join(tmp, 'Epic', 'Fortnite');
  big(path.join(epicGame, 'FortniteGame', 'Binaries', 'Win64', 'FortniteLauncher.exe'), 100);
  mk(path.join(pd, 'Epic', 'EpicGamesLauncher', 'Data', 'Manifests', 'A.item'), JSON.stringify({ DisplayName: 'Fortnite', AppName: 'Fortnite', MainGameAppName: 'Fortnite', CatalogNamespace: 'fn', CatalogItemId: '4fe7', InstallLocation: epicGame, LaunchExecutable: 'FortniteGame/Binaries/Win64/FortniteLauncher.exe', AppCategories: ['public', 'games'] }));
  mk(path.join(pd, 'Epic', 'EpicGamesLauncher', 'Data', 'Manifests', 'B.item'), JSON.stringify({ DisplayName: 'Some DLC', AppName: 'Dlc1', MainGameAppName: 'Fortnite', InstallLocation: epicGame, AppCategories: ['addons'] }));
  // GOG / Ubisoft / EA / Battle.net / BattleState via fake registry
  const gogDir = path.join(tmp, 'GOG Games', 'Witcher 3'); big(path.join(gogDir, 'bin', 'x64', 'witcher3.exe'), 2000);
  const ubiDir = path.join(tmp, 'Ubisoft', 'Far Cry 5'); big(path.join(ubiDir, 'bin', 'FarCry5.exe'), 2000);
  const eaDir = path.join(tmp, 'EA Games', 'Apex'); big(path.join(eaDir, 'r5apex.exe'), 3000);
  mk(path.join(eaDir, '__Installer', 'installerdata.xml'), '<DiPManifest><runtime><launcher><filePath>[HKEY_LOCAL_MACHINE\\SOFTWARE\\Respawn\\Apex\\Install Dir]r5apex.exe</filePath></launcher></runtime></DiPManifest>');
  const bnet = path.join(tmp, 'Battle.net'); big(path.join(bnet, 'Battle.net.exe'), 100);
  const owDir = path.join(tmp, 'Overwatch'); big(path.join(owDir, '_retail_', 'Overwatch.exe'), 3000);
  const bsgL = path.join(tmp, 'BsgLauncher'); big(path.join(bsgL, 'BsgLauncher.exe'), 100);
  const eft = path.join(tmp, 'EFT'); big(path.join(eft, 'EscapeFromTarkov.exe'), 600);
  const U = 'HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall';
  const reg = {
    'HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\GOG.com\\Games': { values: {}, subkeys: [
      { name: '1207664663', values: { gameID: '1207664663', gameName: 'The Witcher 3: Wild Hunt', path: gogDir, exe: path.join(gogDir, 'bin', 'x64', 'witcher3.exe'), workingDir: path.join(gogDir, 'bin', 'x64') } },
      { name: '1640424747', values: { gameID: '1640424747', gameName: 'W3 DLC', path: gogDir, dependsOn: '1207664663' } },
    ] },
    'HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Ubisoft\\Launcher\\Installs': { values: {}, subkeys: [{ name: '4311', values: { InstallDir: ubiDir.replace(/\\/g, '/') + '/' } }] },
    [U]: { values: {}, subkeys: [
      { name: 'Uplay Install 4311', values: { DisplayName: 'Far Cry 5' } },
      { name: '{EA-APEX}', values: { DisplayName: 'Apex Legends', Publisher: 'Electronic Arts', InstallLocation: eaDir, UninstallString: 'EAInstaller\\Cleanup.exe' } },
      { name: 'Battle.net', values: { DisplayName: 'Battle.net', InstallLocation: bnet, UninstallString: `"${bnet}\\Battle.net.exe" --uninstall` } },
      { name: 'Overwatch', values: { DisplayName: 'Overwatch', Publisher: 'Blizzard Entertainment', InstallLocation: owDir, UninstallString: '"C:\\ProgramData\\Battle.net\\Agent\\Blizzard Uninstaller.exe" --lang=enUS --uid=prometheus --displayname="Overwatch"' } },
      { name: 'BsgLauncher', values: { DisplayName: 'BsgLauncher', Publisher: 'Battlestate Games', InstallLocation: bsgL } },
      { name: 'EscapeFromTarkov', values: { DisplayName: 'Escape from Tarkov', Publisher: 'Battlestate Games', InstallLocation: eft } },
      { name: 'Notepad++', values: { DisplayName: 'Notepad++', Publisher: 'Don Ho', InstallLocation: path.join(tmp, 'npp') } },
    ] },
  };
  // Torrent / repack folder (contains a duplicate of a Steam game dir which must be skipped)
  const torr = path.join(tmp, 'Torrents');
  big(path.join(torr, 'Hades.v1.38-GOG', 'Hades.exe'), 400);
  big(path.join(torr, 'Hades.v1.38-GOG', 'x64', 'Hades.exe'), 9000);
  big(path.join(torr, 'Elden Ring [FitGirl Repack]', 'setup.exe'), 4000); // not unpacked yet -> skipped
  mk(path.join(torr, 'Elden Ring [FitGirl Repack]', 'fg-01.bin'), 'x');
  big(path.join(torr, 'Lies of P (DODI Repack)', 'LOP.exe'), 300);
  big(path.join(torr, 'Lies of P (DODI Repack)', 'LiesofP', 'Binaries', 'Win64', 'LOP-Win64-Shipping.exe'), 90000);
  big(path.join(lib2, 'steamapps', 'common', 'Cyberpunk 2077', 'bin', 'x64', 'Cyberpunk2077.exe'), 50000);

  const settings = { sources: {}, scanFolders: [torr, path.join(lib2, 'steamapps', 'common')] };
  const { games, status } = await scanAll(settings, { reg, ctxOverride: { programData: pd, steamPath: steamDir } });
  const byId = Object.fromEntries(games.map((g) => [g.id, g]));
  console.log('   found:', games.map((g) => `${g.source}:${g.name}`).join(' | '));

  await t('steam: 2 games across 2 libraries, skips redist & partial', () => {
    assert.ok(byId['steam:570'] && byId['steam:1091500']);
    assert.ok(!byId['steam:228980'] && !byId['steam:999']);
    assert.strictEqual(byId['steam:570'].importedPlaytime, 36000);
    assert.ok(/library_600x900/.test(byId['steam:570'].localArt.cover));
  });
  await t('epic: game with launcher URI, DLC skipped', () => {
    assert.ok(byId['epic:Fortnite'].launch.uri.startsWith('com.epicgames.launcher://apps/fn%3A4fe7%3AFortnite'));
    assert.strictEqual(status.epic.count, 1);
  });
  await t('gog: direct exe, DLC skipped', () => { assert.ok(byId['gog:1207664663'].launch.path.endsWith('witcher3.exe')); assert.strictEqual(status.gog.count, 1); });
  await t('ubisoft: uplay uri + name from uninstall', () => { assert.strictEqual(byId['ubisoft:4311'].name, 'Far Cry 5'); assert.strictEqual(byId['ubisoft:4311'].launch.uri, 'uplay://launch/4311/0'); });
  await t('ea: exe from installerdata.xml', () => assert.ok(games.find((g) => g.source === 'ea' && g.exePath.endsWith('r5apex.exe'))));
  await t('battle.net: launches through Battle.net with product code', () => {
    const ow = games.find((g) => g.source === 'battlenet');
    assert.strictEqual(ow.launch.args, '--exec="launch Pro"');
  });
  await t('battlestate: Tarkov via BsgLauncher, tracks game dir', () => {
    const g = games.find((x) => x.source === 'battlestate');
    assert.ok(g.launch.path.endsWith('BsgLauncher.exe') && g.trackDir === eft && status.battlestate.count === 1);
  });
  await t('folders: torrent games found with clean names, unpacked repack skipped, steam dupes skipped', () => {
    const f = games.filter((g) => g.source === 'folder').map((g) => g.name).sort();
    assert.deepStrictEqual(f, ['Hades', 'Lies of P']);
    assert.ok(games.find((g) => g.name === 'Lies of P').exePath.endsWith('LOP.exe'));
  });
  console.log(`\n${passed} passed`);
  fs.rmSync(tmp, { recursive: true, force: true });
})();
