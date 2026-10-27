'use strict';
// Shared helpers for store accounts: login popup, encrypted token storage, HTTP helpers.
const { BrowserWindow, session, safeStorage } = require('electron');

const partitionOf = (key) => `persist:acc-${key}`;

// Chrome-like user agent: some login pages refuse "Electron" browsers
function cleanUA(ua) {
  return String(ua || '').replace(/\s(Electron|arcadia[\w-]*|Arcadia)\/\S+/gi, '');
}

const prepared = new Set();
function getSession(key) {
  const ses = session.fromPartition(partitionOf(key));
  if (!prepared.has(key)) {
    prepared.add(key);
    ses.setUserAgent(cleanUA(ses.getUserAgent()));
    // Pages never get camera/mic/location/notifications
    ses.setPermissionRequestHandler((_wc, perm, cb) => cb(['clipboard-sanitized-write', 'fullscreen'].includes(perm)));
  }
  return ses;
}

// Opens a small sign-in window and resolves when `check(url, webContents)` returns a value.
function loginPopup({ key, url, title, parent, check, pollMs = 1500 }) {
  return new Promise((resolve, reject) => {
    const ses = getSession(key);
    const w = new BrowserWindow({
      width: 520, height: 760, parent, title, autoHideMenuBar: true, backgroundColor: '#ffffff',
      webPreferences: { session: ses, sandbox: true, contextIsolation: true, spellcheck: false },
    });
    let done = false;
    let timer = null;
    const finish = (err, val) => {
      if (done) return;
      done = true;
      clearInterval(timer);
      if (!w.isDestroyed()) w.close();
      err ? reject(err) : resolve(val);
    };
    const probe = async () => {
      if (done || w.isDestroyed()) return;
      try {
        const v = await check(w.webContents.getURL(), w.webContents, ses);
        if (v) finish(null, v);
      } catch (e) { finish(e); }
    };
    w.webContents.setWindowOpenHandler(({ url: u }) => { w.webContents.loadURL(u); return { action: 'deny' }; });
    w.webContents.on('did-navigate', probe);
    w.webContents.on('did-navigate-in-page', probe);
    w.webContents.on('did-finish-load', probe);
    w.on('closed', () => finish(new Error('Окно входа закрыто')));
    timer = setInterval(probe, pollMs);
    w.loadURL(url);
  });
}

// Tokens are encrypted with Windows DPAPI (safeStorage) before being written to library.json
function seal(obj) {
  const json = JSON.stringify(obj);
  if (safeStorage.isEncryptionAvailable()) return 'enc:' + safeStorage.encryptString(json).toString('base64');
  return 'raw:' + Buffer.from(json).toString('base64');
}
function unseal(str) {
  if (!str) return null;
  try {
    if (str.startsWith('enc:')) return JSON.parse(safeStorage.decryptString(Buffer.from(str.slice(4), 'base64')));
    if (str.startsWith('raw:')) return JSON.parse(Buffer.from(str.slice(4), 'base64').toString('utf8'));
  } catch {}
  return null;
}

async function jsonFetch(fetch, url, opts = {}) {
  const r = await fetch(url, opts);
  const text = await r.text();
  let data = null;
  try { data = JSON.parse(text); } catch {}
  if (!r.ok) {
    const msg = (data && (data.errorMessage || data.error_description || data.message || data.error)) || `HTTP ${r.status}`;
    const e = new Error(msg);
    e.status = r.status;
    throw e;
  }
  return data;
}

// Runs async fn over items with limited parallelism
async function pool(items, n, fn) {
  const out = new Array(items.length);
  let i = 0;
  await Promise.all(Array.from({ length: Math.min(n, items.length) }, async () => {
    while (i < items.length) { const k = i++; try { out[k] = await fn(items[k], k); } catch (e) { out[k] = null; } }
  }));
  return out;
}

const decodeEntities = (s) => String(s || '')
  .replace(/&quot;/g, '"').replace(/&#39;|&apos;/g, "'").replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&');

module.exports = { partitionOf, getSession, cleanUA, loginPopup, seal, unseal, jsonFetch, pool, decodeEntities };
