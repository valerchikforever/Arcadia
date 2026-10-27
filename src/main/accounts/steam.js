'use strict';
// Steam account: sign in on steamcommunity.com (or take the account of the local Steam client) and read owned games.
const fs = require('fs');
const path = require('path');
const { parseVDF, ci } = require('../util/vdf');
const { loginPopup, getSession, jsonFetch, decodeEntities } = require('./common');

const KEY = 'steam';

function steamIdFromCookies(cookies) {
  const c = cookies.find((x) => x.name === 'steamLoginSecure' && x.value);
  if (!c) return null;
  const m = /^(\d{17})/.exec(decodeURIComponent(c.value));
  return m ? m[1] : null;
}

async function login(parent) {
  const steamid = await loginPopup({
    key: KEY, parent, title: 'Вход в Steam',
    url: 'https://steamcommunity.com/login/home/?goto=',
    check: async (_url, _wc, ses) => steamIdFromCookies(await ses.cookies.get({ domain: 'steamcommunity.com' })),
  });
  return { id: steamid, mode: 'web' };
}

// Most recent account of the installed Steam client (no password needed, public profile data only)
function localAccount(steamPath) {
  if (!steamPath) return null;
  try {
    const d = parseVDF(fs.readFileSync(path.join(steamPath, 'config', 'loginusers.vdf'), 'utf8'));
    const users = ci(d, 'users') || {};
    let best = null;
    for (const [id, u] of Object.entries(users)) {
      const ts = parseInt(ci(u, 'Timestamp') || '0', 10);
      const recent = String(ci(u, 'MostRecent') || '0') === '1';
      const cand = { id, name: ci(u, 'PersonaName') || ci(u, 'AccountName') || id, ts: recent ? Infinity : ts };
      if (!best || cand.ts > best.ts) best = cand;
    }
    return best ? { id: best.id, name: best.name } : null;
  } catch { return null; }
}

async function profile(fetch, id) {
  try {
    const r = await fetch(`https://steamcommunity.com/profiles/${id}/?xml=1`);
    const x = await r.text();
    const tag = (t) => { const m = new RegExp(`<${t}><!\\[CDATA\\[([\\s\\S]*?)\\]\\]></${t}>|<${t}>([^<]*)</${t}>`).exec(x); return m ? (m[1] || m[2] || '').trim() : ''; };
    return { name: tag('steamID'), avatar: tag('avatarMedium') };
  } catch { return {}; }
}

function parseGamesPage(html) {
  const m = /data-profile-gameslist="([^"]+)"/.exec(html);
  if (m) {
    const d = JSON.parse(decodeEntities(m[1]));
    return d.rgGames || [];
  }
  const old = /var rgGames = (\[[\s\S]*?\]);\s*\n/.exec(html); // older layout
  if (old) return JSON.parse(old[1]);
  return null;
}

async function library({ account, fetch, apiKey }) {
  const id = account.id;
  let games = null;
  if (apiKey) {
    const d = await jsonFetch(fetch, `https://api.steampowered.com/IPlayerService/GetOwnedGames/v1/?key=${encodeURIComponent(apiKey)}&steamid=${id}&include_appinfo=1&include_played_free_games=1`);
    games = (d.response && d.response.games) || [];
  } else {
    const ses = getSession(KEY);
    const r = await ses.fetch(`https://steamcommunity.com/profiles/${id}/games/?tab=all&l=russian`, { credentials: 'include' });
    games = parseGamesPage(await r.text());
    if (!games) throw new Error('Steam не отдал список игр. Войдите через «Войти» или откройте список игр в настройках приватности профиля');
    if (!games.length && account.mode !== 'web') throw new Error('Список игр скрыт настройками приватности. Войдите в аккаунт через «Войти»');
  }
  return games.map((g) => ({
    id: `steam:${g.appid}`,
    name: g.name || `App ${g.appid}`,
    steamAppId: String(g.appid),
    installUri: `steam://install/${g.appid}`,
    storeUrl: `https://store.steampowered.com/app/${g.appid}/`,
    coverUrl: `https://shared.cloudflare.steamstatic.com/store_item_assets/steam/apps/${g.appid}/library_600x900.jpg`,
    heroUrl: `https://shared.cloudflare.steamstatic.com/store_item_assets/steam/apps/${g.appid}/library_hero.jpg`,
    importedPlaytime: (g.playtime_forever || 0) * 60,
    importedLastPlayed: (g.rtime_last_played || 0) * 1000,
  }));
}

module.exports = { KEY, login, localAccount, profile, library, parseGamesPage, steamIdFromCookies };
