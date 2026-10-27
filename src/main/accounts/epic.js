'use strict';
// Epic Games account: the same OAuth flow open-source launchers (Legendary / Heroic) use.
const { loginPopup, jsonFetch, pool } = require('./common');

const KEY = 'epic';
const CLIENT_ID = '34a02cf8f4414e29b15921876da36f9a';
const CLIENT_SECRET = 'daafbccc737745039dffe53d94fc76cf';
const BASIC = Buffer.from(`${CLIENT_ID}:${CLIENT_SECRET}`).toString('base64');
const REDIRECT = `https://www.epicgames.com/id/api/redirect?clientId=${CLIENT_ID}&responseType=code`;
const OAUTH = 'https://account-public-service-prod03.ol.epicgames.com/account/api/oauth/token';
const LIBRARY = 'https://library-service.live.use1a.on.epicgames.com/library/api/public/items';
const CATALOG = 'https://catalog-public-service-prod06.ol.epicgames.com/catalog/api/shared/namespace';

async function tokenRequest(fetch, params) {
  return jsonFetch(fetch, OAUTH, {
    method: 'POST',
    headers: { Authorization: `Basic ${BASIC}`, 'Content-Type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({ ...params, token_type: 'eg1' }).toString(),
  });
}

function tokenToAccount(t) {
  return {
    id: t.account_id,
    name: t.displayName || '',
    mode: 'oauth',
    token: { access: t.access_token, refresh: t.refresh_token, expires: Date.parse(t.expires_at) || Date.now() + 3600e3, refreshExpires: Date.parse(t.refresh_expires_at) || 0 },
  };
}

async function login(parent, fetch) {
  const code = await loginPopup({
    key: KEY, parent, title: 'Вход в Epic Games',
    url: `https://www.epicgames.com/id/login?redirectUrl=${encodeURIComponent(REDIRECT)}`,
    check: async (url, wc) => {
      if (!url.startsWith('https://www.epicgames.com/id/api/redirect')) return null;
      const txt = await wc.executeJavaScript('document.body && document.body.innerText');
      const d = JSON.parse(txt || '{}');
      if (d.authorizationCode) return d.authorizationCode;
      // Signed in but the page came back without a code (e.g. still finishing login) — ask again
      if (d.redirectUrl || d.sid !== undefined) wc.loadURL(REDIRECT);
      return null;
    },
  });
  return tokenToAccount(await tokenRequest(fetch, { grant_type: 'authorization_code', code }));
}

// Returns a fresh access token, refreshing it if needed; may return an updated account to save
async function ensureToken(account, fetch) {
  const t = account.token || {};
  if (t.access && t.expires > Date.now() + 120e3) return { access: t.access, account: null };
  if (!t.refresh) throw new Error('Сессия Epic истекла — войдите снова');
  try {
    const upd = tokenToAccount(await tokenRequest(fetch, { grant_type: 'refresh_token', refresh_token: t.refresh }));
    return { access: upd.token.access, account: { ...account, ...upd, name: upd.name || account.name } };
  } catch (e) {
    throw new Error('Сессия Epic истекла — войдите снова');
  }
}

async function library({ account, fetch, onAccount }) {
  const { access, account: upd } = await ensureToken(account, fetch);
  if (upd && onAccount) onAccount(upd);
  const H = { Authorization: `Bearer ${access}` };
  const records = [];
  let cursor = '';
  for (let page = 0; page < 50; page++) {
    const d = await jsonFetch(fetch, `${LIBRARY}?includeMetadata=true${cursor ? `&cursor=${encodeURIComponent(cursor)}` : ''}`, { headers: H });
    records.push(...(d.records || []));
    cursor = d.responseMetadata && d.responseMetadata.nextCursor;
    if (!cursor) break;
  }
  const seen = new Set();
  const items = records.filter((r) => {
    if (!r.appName || !r.catalogItemId || r.namespace === 'ue' || seen.has(r.appName)) return false;
    if (r.sandboxType && r.sandboxType !== 'PUBLIC') return false;
    seen.add(r.appName);
    return true;
  });
  const details = await pool(items, 6, async (r) => {
    const d = await jsonFetch(fetch, `${CATALOG}/${encodeURIComponent(r.namespace)}/bulk/items?id=${encodeURIComponent(r.catalogItemId)}&includeDLCDetails=true&includeMainGameDetails=true&country=RU&locale=ru`, { headers: H });
    return d && d[r.catalogItemId];
  });
  const out = [];
  items.forEach((r, i) => {
    const c = details[i];
    if (!c) return;
    if (c.mainGameItem) return; // DLC
    const cats = (c.categories || []).map((x) => x.path);
    if (cats.length && !cats.includes('games') && !cats.includes('applications')) return;
    if (cats.includes('addons') || cats.includes('digitalextras')) return;
    const img = (type) => ((c.keyImages || []).find((k) => k.type === type) || {}).url;
    const ns = encodeURIComponent(r.namespace), item = encodeURIComponent(r.catalogItemId), app = encodeURIComponent(r.appName);
    out.push({
      id: `epic:${r.appName}`,
      name: c.title || r.appName,
      installUri: `com.epicgames.launcher://apps/${ns}%3A${item}%3A${app}?action=install`,
      coverUrl: img('DieselGameBoxTall') || img('OfferImageTall') || img('Thumbnail') || '',
      heroUrl: img('DieselGameBox') || img('OfferImageWide') || '',
      developer: c.developer || '',
    });
  });
  return out;
}

module.exports = { KEY, login, library, ensureToken };
