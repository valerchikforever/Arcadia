'use strict';
// Built-in store browser. Designed to cost nothing while it is not on screen:
//  • nothing is created until the user presses «Включить браузерный режим»;
//  • only ONE page is alive at a time (switching store = same view, other pages are not kept);
//  • the page process is destroyed when the window is hidden to tray, after N s minimized
//    and after N s away from the «Магазины» section — and recreated only when shown again;
//  • ads/trackers are blocked, and «экономный режим» blocks auto-playing trailers and WebGL.
const { WebContentsView, shell, app } = require('electron');
const { getSession } = require('./accounts/common');

const BLOCK_HOSTS = [
  'doubleclick.net', 'googlesyndication.com', 'google-analytics.com', 'googletagmanager.com', 'googleadservices.com',
  'analytics.google.com', 'facebook.net', 'connect.facebook.net', 'hotjar.com', 'hotjar.io', 'scorecardresearch.com',
  'adnxs.com', 'criteo.com', 'criteo.net', 'taboola.com', 'outbrain.com', 'quantserve.com', 'mc.yandex.ru', 'top-fwz1.mail.ru',
  'bat.bing.com', 'clarity.ms', 'tiktok.com/i18n/pixel', 'snap.licdn.com', 'ads.linkedin.com', 'adsrvr.org', 'amazon-adsystem.com',
  'onetrust.com', 'cookielaw.org', 'branch.io', 'sentry.io', 'newrelic.com', 'nr-data.net', 'datadoghq.com', 'segment.io', 'segment.com',
  'mixpanel.com', 'amplitude.com', 'optimizely.com', 'demdex.net', 'omtrdc.net', 'everesttech.net', 'appsflyer.com',
];
const EXTERNAL_SCHEMES = /^(steam|com\.epicgames\.launcher|goggalaxy|uplay|battlenet|origin2?|eadm|link2ea|mailto):/i;
const blockRe = new RegExp(`^https?://([^/]*\\.)?(${BLOCK_HOSTS.map((h) => h.replace(/[.]/g, '\\.').replace(/\//g, '\\/')).join('|')})(/|:|$)`, 'i');

class StoreBrowser {
  constructor(store, { getWindow, onChange, log }) {
    this.store = store;
    this.getWindow = getWindow;
    this.onChange = onChange || (() => {});
    this.log = log || (() => {});
    this.enabled = false; // user pressed «Включить» in this session
    this.view = null;
    this.key = '';
    this.url = '';
    this.title = '';
    this.loading = false;
    this.wantVisible = false;
    this.bounds = null;
    this.timer = null;
    this.unloadedReason = '';
    this.blocked = 0;
    this.sessions = new Set();
    this.memMB = 0;
    this.memTimer = null;
  }

  get cfg() { return this.store.settings.browser; }

  attachWindow(win) {
    win.on('hide', () => this.unload('hidden'));
    win.on('minimize', () => this.schedule(this.cfg.unloadOnMinimizeSec, 'minimized'));
    win.on('restore', () => { this.cancel(); this.ensure(); });
    win.on('show', () => { this.cancel(); this.ensure(); });
  }

  prepareSession(key) {
    const ses = getSession(key);
    if (this.sessions.has(key)) return ses;
    this.sessions.add(key);
    ses.webRequest.onBeforeRequest({ urls: ['http://*/*', 'https://*/*'] }, (d, cb) => {
      const c = this.cfg;
      if (c.blockTrackers && blockRe.test(d.url)) { this.blocked++; return cb({ cancel: true }); }
      // Economy: trailers and background videos are the heaviest thing on store pages
      if (c.economy && d.resourceType === 'media') { this.blocked++; return cb({ cancel: true }); }
      cb({});
    });
    return ses;
  }

  createView() {
    const win = this.getWindow();
    if (!win || win.isDestroyed()) return null;
    const ses = this.prepareSession(this.key);
    const economy = this.cfg.economy;
    const view = new WebContentsView({
      webPreferences: {
        session: ses, sandbox: true, contextIsolation: true, nodeIntegration: false,
        backgroundThrottling: true, spellcheck: false, enableWebSQL: false,
        autoplayPolicy: economy ? 'document-user-activation-required' : 'no-user-gesture-required',
        webgl: !economy, v8CacheOptions: 'code',
      },
    });
    view.setBackgroundColor('#0e0a12');
    const wc = view.webContents;
    const upd = () => {
      if (wc.isDestroyed()) return;
      this.url = wc.getURL() || this.url;
      this.title = wc.getTitle();
      this.onChange();
    };
    wc.on('did-start-loading', () => { this.loading = true; this.onChange(); });
    wc.on('did-stop-loading', () => { this.loading = false; upd(); });
    wc.on('did-navigate', upd);
    wc.on('did-navigate-in-page', upd);
    wc.on('page-title-updated', upd);
    wc.on('render-process-gone', () => { this.unload('crashed'); });
    wc.setWindowOpenHandler(({ url }) => {
      if (/^https?:/i.test(url)) wc.loadURL(url); // new tabs open in place: one page = one process
      else if (EXTERNAL_SCHEMES.test(url)) shell.openExternal(url);
      return { action: 'deny' };
    });
    wc.on('will-navigate', (e, url) => {
      if (!/^https?:/i.test(url)) {
        e.preventDefault();
        if (EXTERNAL_SCHEMES.test(url)) shell.openExternal(url); // «Установить», «Играть» on store pages → the launcher
      }
    });
    wc.on('will-frame-navigate', (e) => {
      if (e.isMainFrame) return;
      if (!/^https?:|^about:|^data:|^blob:/i.test(e.url)) e.preventDefault();
    });
    win.contentView.addChildView(view);
    view.setVisible(false);
    return view;
  }

  // Load a page of a store; called by the UI
  open(key, url) {
    this.enabled = true;
    this.unloadedReason = '';
    if (this.view && this.key !== key) this.destroyView(); // a page can't change its session (cookies of another store)
    this.key = key;
    this.url = url;
    if (!this.view) this.view = this.createView();
    if (!this.view) return;
    this.view.webContents.loadURL(url).catch(() => {});
    this.applyBounds();
    this.startMem();
    this.onChange();
  }

  // Recreate the page after it was unloaded, if the browser is on screen
  ensure() {
    const win = this.getWindow();
    if (!this.enabled || this.view || !this.wantVisible || !this.url || !win || !win.isVisible() || win.isMinimized()) return;
    this.open(this.key, this.url);
  }

  setBounds(rect) {
    this.wantVisible = !!rect;
    this.bounds = rect;
    if (rect) { this.cancel(); this.ensure(); }
    else if (this.view) this.schedule(this.cfg.unloadOnLeaveSec, 'left');
    this.applyBounds();
  }

  applyBounds() {
    if (!this.view) return;
    if (this.wantVisible && this.bounds) {
      const b = this.bounds;
      this.view.setBounds({ x: Math.round(b.x), y: Math.round(b.y), width: Math.max(1, Math.round(b.width)), height: Math.max(1, Math.round(b.height)) });
      this.view.setVisible(true);
    } else {
      this.view.setVisible(false);
    }
  }

  schedule(sec, reason) {
    this.cancel();
    if (!this.view) return;
    const ms = Math.max(0, Number(sec) || 0) * 1000;
    this.timer = setTimeout(() => this.unload(reason), ms);
  }

  cancel() { clearTimeout(this.timer); this.timer = null; }

  destroyView() {
    const v = this.view;
    this.view = null;
    if (!v) return;
    try { const win = this.getWindow(); if (win && !win.isDestroyed()) win.contentView.removeChildView(v); } catch {}
    try { if (!v.webContents.isDestroyed()) v.webContents.close(); } catch {}
    this.loading = false;
  }

  // Kill the page's process; it is reloaded from this.url when shown again
  unload(reason = 'manual') {
    this.cancel();
    if (!this.view) return;
    try { if (!this.view.webContents.isDestroyed()) this.url = this.view.webContents.getURL() || this.url; } catch {}
    this.destroyView();
    this.unloadedReason = reason;
    this.stopMem();
    this.log('browser unloaded', reason);
    this.onChange();
  }

  disable() {
    this.enabled = false;
    this.unload('off');
  }

  nav(action) {
    if (!this.view) return;
    const wc = this.view.webContents;
    const h = wc.navigationHistory;
    if (action === 'back' && h.canGoBack()) h.goBack();
    else if (action === 'forward' && h.canGoForward()) h.goForward();
    else if (action === 'reload') wc.reload();
    else if (action === 'stop') wc.stop();
  }

  openExternal() {
    if (this.url) shell.openExternal(this.url);
  }

  // Memory of the page's renderer process, so the user sees what the browser costs
  startMem() {
    if (this.memTimer) return;
    const tick = () => {
      if (!this.view) return;
      try {
        const pid = this.view.webContents.getOSProcessId();
        const m = app.getAppMetrics().find((x) => x.pid === pid);
        const mb = m ? Math.round((m.memory.workingSetSize || 0) / 1024) : 0;
        if (mb !== this.memMB) { this.memMB = mb; this.onChange(); }
      } catch {}
    };
    this.memTimer = setInterval(tick, 3000);
    setTimeout(tick, 1500);
  }

  stopMem() { clearInterval(this.memTimer); this.memTimer = null; this.memMB = 0; }

  state() {
    let canBack = false, canFwd = false;
    if (this.view && !this.view.webContents.isDestroyed()) {
      const h = this.view.webContents.navigationHistory;
      canBack = h.canGoBack(); canFwd = h.canGoForward();
    }
    return {
      enabled: this.enabled, loaded: !!this.view, key: this.key, url: this.url, title: this.title, loading: this.loading,
      canBack, canFwd, memMB: this.memMB, blocked: this.blocked, unloadedReason: this.unloadedReason,
    };
  }
}

module.exports = { StoreBrowser, blockRe };
