'use strict';
// Store accounts manager: optional sign-in per launcher, owned-games sync, store pages.
const { getSession, seal, unseal } = require('./common');
const steam = require('./steam');
const epic = require('./epic');
const gog = require('./gog');
const { getSteamPath } = require('../scanners/steam');

// Every launcher: quick links for the built-in browser; "library" = owned games can be imported
const SERVICES = {
  steam: {
    library: true, home: 'https://store.steampowered.com/', launcher: 'steam://open/main',
    links: [
      ['Магазин', 'https://store.steampowered.com/'], ['Скидки', 'https://store.steampowered.com/specials'],
      ['Список желаемого', 'https://store.steampowered.com/wishlist/'], ['Профиль', 'https://steamcommunity.com/my/'],
      ['Друзья', 'https://steamcommunity.com/my/friends/'], ['Сообщество', 'https://steamcommunity.com/'],
      ['Торговая площадка', 'https://steamcommunity.com/market/'],
    ],
  },
  epic: {
    library: true, home: 'https://store.epicgames.com/ru/', launcher: 'com.epicgames.launcher://store',
    links: [
      ['Магазин', 'https://store.epicgames.com/ru/'], ['Бесплатные игры', 'https://store.epicgames.com/ru/free-games'],
      ['Список желаемого', 'https://store.epicgames.com/ru/wishlist'], ['Аккаунт', 'https://www.epicgames.com/account/personal'],
    ],
  },
  gog: {
    library: true, home: 'https://www.gog.com/ru/games', launcher: 'goggalaxy://openStoreView',
    links: [
      ['Магазин', 'https://www.gog.com/ru/games'], ['Скидки', 'https://www.gog.com/ru/games?discounted=true'],
      ['Мои игры', 'https://www.gog.com/ru/account'], ['Список желаемого', 'https://www.gog.com/ru/account/wishlist'],
    ],
  },
  ea: {
    library: false, home: 'https://www.ea.com/ru/games', launcher: 'origin2://library/open',
    login: 'https://myaccount.ea.com/cp-ui/aboutme/index',
    links: [['Игры', 'https://www.ea.com/ru/games'], ['Аккаунт', 'https://myaccount.ea.com/cp-ui/aboutme/index']],
  },
  ubisoft: {
    library: false, home: 'https://store.ubisoft.com/ru/home', launcher: 'uplay://open',
    login: 'https://account.ubisoft.com/',
    links: [['Магазин', 'https://store.ubisoft.com/ru/home'], ['Аккаунт', 'https://account.ubisoft.com/']],
  },
  battlenet: {
    library: false, home: 'https://shop.battle.net/ru-ru', launcher: 'battlenet://',
    login: 'https://account.battle.net/',
    links: [['Магазин', 'https://shop.battle.net/ru-ru'], ['Аккаунт', 'https://account.battle.net/']],
  },
  battlestate: {
    library: false, home: 'https://www.escapefromtarkov.com/', launcher: '',
    login: 'https://www.escapefromtarkov.com/profile',
    links: [['Escape from Tarkov', 'https://www.escapefromtarkov.com/'], ['Профиль', 'https://www.escapefromtarkov.com/profile'], ['Arena', 'https://arena.tarkov.com/']],
  },
};
const PROVIDERS = { steam, epic, gog };

class Accounts {
  constructor(store, { fetch, log, onChange, getWindow }) {
    this.store = store;
    this.fetch = fetch;
    this.log = log || (() => {});
    this.onChange = onChange || (() => {});
    this.getWindow = getWindow;
    this.busy = {}; // key -> text
    this.errors = {};
  }

  get data() {
    if (!this.store.settings.accounts) this.store.settings.accounts = {};
    return this.store.settings.accounts;
  }

  // Account with decrypted secrets (only inside the main process)
  account(key) {
    const a = this.data[key];
    if (!a) return null;
    return { ...a, ...(unseal(a.secret) || {}) };
  }

  saveAccount(key, acc) {
    const { token, ...pub } = acc;
    const prev = this.data[key] || {};
    this.data[key] = { ...prev, ...pub, secret: token ? seal({ token }) : prev.secret, updatedAt: prev.updatedAt };
    this.store.save();
    this.onChange();
  }

  localSteam() {
    // cached: state() is called on every UI refresh
    if (!this._local || Date.now() - this._local.t > 60e3) this._local = { t: Date.now(), v: steam.localAccount(getSteamPath()) };
    return this._local.v;
  }

  state() {
    const steamLocal = this.localSteam();
    const out = {};
    for (const [key, s] of Object.entries(SERVICES)) {
      const a = this.data[key] || null;
      out[key] = {
        library: s.library,
        links: s.links,
        home: s.home,
        hasLauncher: !!s.launcher,
        account: a ? { name: a.name || '', id: a.id || '', avatar: a.avatar || '', mode: a.mode, count: a.count || 0, updatedAt: a.updatedAt || 0, web: !!a.web } : null,
        busy: this.busy[key] || '',
        error: this.errors[key] || '',
        localSteam: key === 'steam' && steamLocal && (!a || a.id !== steamLocal.id) ? steamLocal : null,
        hasApiKey: key === 'steam' && !!this.store.settings.steamApiKey,
      };
    }
    return out;
  }

  setBusy(key, text) {
    if (text) this.busy[key] = text; else delete this.busy[key];
    this.onChange();
  }

  async login(key) {
    const s = SERVICES[key];
    if (!s) throw new Error('Неизвестный сервис');
    this.errors[key] = '';
    this.setBusy(key, 'Ожидаем вход в окне…');
    try {
      const parent = this.getWindow();
      if (!s.library) {
        // Web-only services: keep a signed-in session for their pages in the built-in browser
        const { loginPopup } = require('./common');
        await loginPopup({ key, parent, title: 'Вход — закройте окно, когда войдёте', url: s.login, check: async () => null, pollMs: 60000 }).catch(() => {});
        // mark as signed in only if the site actually left session cookies
        const cookies = await getSession(key).cookies.get({ url: s.login }).catch(() => []);
        const signed = cookies.filter((c) => !c.session || c.expirationDate).length >= 3;
        if (signed) {
          this.data[key] = { ...(this.data[key] || {}), web: true, mode: 'web', name: this.data[key]?.name || '' };
          this.store.save();
        }
        return { ok: true, web: signed };
      }
      const acc = await PROVIDERS[key].login(parent, this.fetch);
      if (key === 'steam') Object.assign(acc, await steam.profile(this.fetch, acc.id));
      this.saveAccount(key, acc);
    } catch (e) {
      if (!/закрыто/.test(e.message)) this.errors[key] = e.message;
      throw e;
    } finally {
      this.setBusy(key, '');
    }
    return this.sync(key);
  }

  async useLocalSteam() {
    const loc = steam.localAccount(getSteamPath());
    if (!loc) throw new Error('Клиент Steam не найден');
    const prof = await steam.profile(this.fetch, loc.id);
    this.saveAccount('steam', { id: loc.id, name: prof.name || loc.name, avatar: prof.avatar || '', mode: 'local' });
    return this.sync('steam');
  }

  async sync(key) {
    const p = PROVIDERS[key];
    const acc = this.account(key);
    if (!p || !acc) return { ok: false, error: 'Сначала войдите в аккаунт' };
    this.errors[key] = '';
    this.setBusy(key, 'Загружаем купленные игры…');
    try {
      const list = await p.library({
        account: acc, fetch: this.fetch, apiKey: key === 'steam' ? this.store.settings.steamApiKey : '',
        onAccount: (upd) => this.saveAccount(key, upd),
      });
      const added = this.store.mergeOwned(key, list);
      this.data[key] = { ...this.data[key], count: list.length, updatedAt: Date.now() };
      this.store.save();
      return { ok: true, count: list.length, added };
    } catch (e) {
      this.errors[key] = e.message;
      this.log('account sync', key, e.message);
      throw e;
    } finally {
      this.setBusy(key, '');
    }
  }

  async syncAll() {
    for (const key of Object.keys(PROVIDERS)) {
      if (this.data[key] && this.data[key].id) await this.sync(key).catch(() => {});
    }
  }

  async logout(key) {
    delete this.data[key];
    this.errors[key] = '';
    this.store.removeOwned(key);
    this.store.save();
    try { await getSession(key).clearStorageData(); } catch {}
    this.onChange();
    return { ok: true };
  }

  launcherUri(key) { return (SERVICES[key] || {}).launcher || ''; }
}

module.exports = { Accounts, SERVICES };
