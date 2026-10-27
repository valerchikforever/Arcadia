'use strict';
// GOG account: GOG Galaxy OAuth (as used by Heroic / Playnite), owned games from embed.gog.com.
const { loginPopup, jsonFetch } = require('./common');

const KEY = 'gog';
const CLIENT_ID = '46899977096215655';
const CLIENT_SECRET = '9d85c43b1482497dbbce61f6e4aa173a433796eeae2ca8c5f6129f2dc4de46d9';
const REDIRECT = 'https://embed.gog.com/on_login_success?origin=client';
const AUTH = `https://auth.gog.com/auth?client_id=${CLIENT_ID}&redirect_uri=${encodeURIComponent(REDIRECT)}&response_type=code&layout=client2`;

function tokenUrl(params) {
  return `https://auth.gog.com/token?${new URLSearchParams({ client_id: CLIENT_ID, client_secret: CLIENT_SECRET, ...params })}`;
}

function toToken(t) {
  return { access: t.access_token, refresh: t.refresh_token, expires: Date.now() + (t.expires_in || 3600) * 1000 };
}

async function login(parent, fetch) {
  const code = await loginPopup({
    key: KEY, parent, title: 'Вход в GOG',
    url: AUTH,
    check: async (url) => (url.startsWith('https://embed.gog.com/on_login_success') ? new URL(url).searchParams.get('code') : null),
  });
  const t = await jsonFetch(fetch, tokenUrl({ grant_type: 'authorization_code', code, redirect_uri: REDIRECT }));
  const token = toToken(t);
  let name = '';
  try { name = (await jsonFetch(fetch, 'https://embed.gog.com/userData.json', { headers: { Authorization: `Bearer ${token.access}` } })).username || ''; } catch {}
  return { id: String(t.user_id || ''), name, mode: 'oauth', token };
}

async function ensureToken(account, fetch) {
  const t = account.token || {};
  if (t.access && t.expires > Date.now() + 120e3) return { access: t.access, account: null };
  if (!t.refresh) throw new Error('Сессия GOG истекла — войдите снова');
  try {
    const token = toToken(await jsonFetch(fetch, tokenUrl({ grant_type: 'refresh_token', refresh_token: t.refresh })));
    return { access: token.access, account: { ...account, token } };
  } catch {
    throw new Error('Сессия GOG истекла — войдите снова');
  }
}

async function library({ account, fetch, onAccount }) {
  const { access, account: upd } = await ensureToken(account, fetch);
  if (upd && onAccount) onAccount(upd);
  const H = { Authorization: `Bearer ${access}` };
  const out = [];
  let total = 1;
  for (let page = 1; page <= total && page <= 100; page++) {
    const d = await jsonFetch(fetch, `https://embed.gog.com/account/getFilteredProducts?mediaType=1&sortBy=title&page=${page}`, { headers: H });
    total = d.totalPages || 1;
    for (const p of d.products || []) {
      if (p.isGame === false) continue;
      const img = p.image ? `https:${p.image}` : '';
      out.push({
        id: `gog:${p.id}`,
        name: p.title,
        installUri: `goggalaxy://openGameView/${p.id}`,
        storeUrl: p.url ? `https://www.gog.com${p.url}` : '',
        coverUrl: img ? `${img}_392.jpg` : '',
        heroUrl: img ? `${img}_product_card_v2_mobile_slider_639.jpg` : '',
      });
    }
  }
  return out;
}

module.exports = { KEY, login, library, ensureToken };
