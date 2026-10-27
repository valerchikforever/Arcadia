'use strict';
// Owned-games merging, Steam games page parsing, tracker blocklist. Run: node test/accounts.js
const fs = require('fs');
const os = require('os');
const path = require('path');
const assert = require('assert');
const Module = require('module');

// accounts/* and browser.js require electron — give them a stub so pure functions can be tested
const origLoad = Module._load;
Module._load = function (req, ...rest) {
  if (req === 'electron') return { BrowserWindow: class {}, session: {}, safeStorage: { isEncryptionAvailable: () => false }, WebContentsView: class {}, shell: {}, app: {} };
  return origLoad.call(this, req, ...rest);
};

const { Store } = require('../src/main/store');
const { parseGamesPage, steamIdFromCookies } = require('../src/main/accounts/steam');
const { seal, unseal, cleanUA } = require('../src/main/accounts/common');
const { blockRe } = require('../src/main/browser');

let passed = 0;
const t = (name, fn) => { try { fn(); passed++; console.log('  ✓', name); } catch (e) { console.error('  ✗', name, '\n', e); process.exitCode = 1; } };

console.log('steam');
t('games page: data-profile-gameslist', () => {
  const json = JSON.stringify({ strProfileName: 'X', rgGames: [{ appid: 570, name: 'Dota 2', playtime_forever: 600 }, { appid: 730, name: 'Counter-Strike 2' }] });
  const html = `<html><template id="gameslist_config" data-profile-gameslist="${json.replace(/&/g, '&amp;').replace(/"/g, '&quot;')}"></template>`;
  const g = parseGamesPage(html);
  assert.deepStrictEqual(g.map((x) => x.appid), [570, 730]);
  assert.strictEqual(parseGamesPage('<html>nothing</html>'), null);
});
t('steamid from steamLoginSecure cookie', () => {
  assert.strictEqual(steamIdFromCookies([{ name: 'steamLoginSecure', value: '76561198000000001%7C%7CeyJhbGciOi' }]), '76561198000000001');
  assert.strictEqual(steamIdFromCookies([{ name: 'sessionid', value: 'x' }]), null);
});
t('token sealing round-trip + Chrome-like user agent', () => {
  assert.deepStrictEqual(unseal(seal({ token: { a: 1 } })), { token: { a: 1 } });
  assert.strictEqual(cleanUA('Mozilla/5.0 (Windows NT 10.0) Chrome/138.0 Safari/537.36 arcadia-launcher/1.3.0 Electron/38.1.0'), 'Mozilla/5.0 (Windows NT 10.0) Chrome/138.0 Safari/537.36');
});

console.log('library merge');
const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'arc-store-'));
const st = new Store(dir);
const installed = { id: 'steam:570', source: 'steam', name: 'Dota 2', installDir: 'C:/S/dota', launch: { type: 'uri', uri: 'steam://rungameid/570' } };
st.mergeScan({ games: [installed], status: { steam: { count: 1 } } });
t('owned list adds not-installed games, keeps installed ones installed', () => {
  const added = st.mergeOwned('steam', [
    { id: 'steam:570', name: 'Dota 2', installUri: 'steam://install/570', coverUrl: 'https://c/570.jpg', importedPlaytime: 99999 },
    { id: 'steam:730', name: 'Counter-Strike 2', installUri: 'steam://install/730', coverUrl: 'https://c/730.jpg', importedPlaytime: 3600 },
  ]);
  assert.strictEqual(added, 1);
  assert.strictEqual(st.games['steam:570'].installed, undefined);
  assert.strictEqual(st.games['steam:570'].owned, true);
  assert.strictEqual(st.games['steam:570'].importedPlaytime, 99999);
  assert.strictEqual(st.games['steam:730'].installed, false);
  assert.strictEqual(st.games['steam:730'].installUri, 'steam://install/730');
});
t('installing the game: scan flips it to installed, keeps owned data', () => {
  st.mergeScan({ games: [installed, { id: 'steam:730', source: 'steam', name: 'Counter-Strike 2', installDir: 'C:/S/cs2', launch: { type: 'uri', uri: 'steam://rungameid/730' } }], status: { steam: { count: 2 } } });
  const g = st.games['steam:730'];
  assert.strictEqual(g.installed, true);
  assert.strictEqual(g.owned, true);
  assert.strictEqual(g.launch.uri, 'steam://rungameid/730');
});
t('uninstalling an owned game: becomes "not installed", not "missing"', () => {
  st.mergeScan({ games: [installed], status: { steam: { count: 1 } } });
  assert.strictEqual(st.games['steam:730'].installed, false);
  assert.strictEqual(st.games['steam:730'].missing, false);
});
t('logout removes only not-installed games of that store', () => {
  st.removeOwned('steam');
  assert.ok(!st.games['steam:730']);
  assert.ok(st.games['steam:570']);
  assert.strictEqual(st.games['steam:570'].owned, false);
  assert.ok(!st.games['steam:570'].installUri);
});
t('games removed by the user stay removed on next sync', () => {
  st.mergeOwned('steam', [{ id: 'steam:440', name: 'TF2', installUri: 'steam://install/440' }]);
  st.remove('steam:440');
  st.mergeOwned('steam', [{ id: 'steam:440', name: 'TF2', installUri: 'steam://install/440' }]);
  assert.ok(!st.games['steam:440']);
});
st.flush();
fs.rmSync(dir, { recursive: true, force: true });

console.log('browser');
t('tracker blocklist', () => {
  for (const u of ['https://www.google-analytics.com/g/collect', 'https://stats.g.doubleclick.net/x', 'https://mc.yandex.ru/watch/1', 'https://cdn.cookielaw.org/consent.js']) assert.ok(blockRe.test(u), u);
  for (const u of ['https://store.steampowered.com/', 'https://shared.cloudflare.steamstatic.com/a.jpg', 'https://store.epicgames.com/ru/', 'https://notdoubleclick.net.example.com/']) assert.ok(!blockRe.test(u), u);
});
console.log('steam downloads');
const { progressFromManifest } = require('../src/main/installs');
t('download progress from appmanifest', () => {
  const p = progressFromManifest({ StateFlags: '1026', BytesToDownload: '8000', BytesDownloaded: '2000', BytesToStage: '9000', BytesStaged: '0' });
  assert.deepStrictEqual([p.phase, p.pct, p.paused], ['download', 25, false]);
  const inst = progressFromManifest({ StateFlags: String(1024 | 2 | 2097152), BytesToDownload: '8000', BytesDownloaded: '8000', BytesToStage: '9000', BytesStaged: '4500' });
  assert.deepStrictEqual([inst.phase, inst.pct], ['install', 50]);
  const paused = progressFromManifest({ StateFlags: String(2 | 512), BytesToDownload: '100', BytesDownloaded: '10' });
  assert.ok(paused.paused);
  assert.strictEqual(progressFromManifest({ StateFlags: '4', BytesToDownload: '100', BytesDownloaded: '100' }), null);
  assert.strictEqual(progressFromManifest({ StateFlags: '4' }), null);
  const upd = progressFromManifest({ StateFlags: String(4 | 1024 | 1048576), BytesToDownload: '500', BytesDownloaded: '100' });
  assert.strictEqual(upd.pct, 20);
});
console.log(`\n${passed} passed`);
