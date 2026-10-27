'use strict';
// Covers, banners and descriptions: local Steam cache -> Steam store (no key) -> SteamGridDB (optional key).
const fs = require('fs');
const path = require('path');
const { hashId, exists, similarity, matchKey } = require('./util/common');

const STEAM_CDN = 'https://shared.cloudflare.steamstatic.com/store_item_assets/';
const OLD_CDN = (id, f) => `https://cdn.cloudflare.steamstatic.com/steam/apps/${id}/${f}`;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function decodeEntities(s) {
  return String(s || '')
    .replace(/<br\s*\/?>/gi, '\n')
    .replace(/<[^>]+>/g, '')
    .replace(/&quot;/g, '"').replace(/&#39;|&apos;/g, "'").replace(/&amp;/g, '&')
    .replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&nbsp;/g, ' ')
    .trim();
}

class Meta {
  constructor(store, { fetch, getIcon, onUpdate, log }) {
    this.store = store;
    this.fetch = fetch;
    this.getIcon = getIcon;
    this.onUpdate = onUpdate || (() => {});
    this.log = log || (() => {});
    this.queue = [];
    this.active = 0;
    this.inQueue = new Set();
    this.lastSteamCall = 0;
  }

  pending() { return this.queue.length + this.active; }

  enqueueMissing() {
    for (const g of Object.values(this.store.games)) {
      if (g.missing || g.installed === false) continue; // not installed: covers come straight from the store CDN
      if (!g.metaTried || (!g.coverPath && !g.iconPath)) this.enqueue(g.id);
    }
  }

  enqueue(id, opts = {}) {
    if (this.inQueue.has(id) && !opts.force) return;
    this.inQueue.add(id);
    if (opts.force) this.queue.unshift({ id, opts }); else this.queue.push({ id, opts });
    this.pump();
  }

  pump() {
    while (this.active < 3 && this.queue.length) {
      const { id, opts } = this.queue.shift();
      this.active++;
      this.process(id, opts)
        .catch((e) => this.log('meta', id, e.message))
        .finally(() => { this.active--; this.inQueue.delete(id); this.onUpdate(); this.pump(); });
    }
    this.onUpdate();
  }

  async json(url, headers) {
    const r = await this.fetch(url, { headers: { 'User-Agent': 'ArcadiaLauncher/1.0', ...(headers || {}) } });
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    return r.json();
  }

  async steamThrottle() {
    const wait = this.lastSteamCall + 350 - Date.now();
    this.lastSteamCall = Math.max(Date.now(), this.lastSteamCall + 350);
    if (wait > 0) await sleep(wait);
  }

  async download(url, dest) {
    try {
      const r = await this.fetch(url, { headers: { 'User-Agent': 'ArcadiaLauncher/1.0' } });
      if (!r.ok) return null;
      const type = r.headers.get('content-type') || '';
      if (!type.startsWith('image/')) return null;
      const buf = Buffer.from(await r.arrayBuffer());
      if (buf.length < 1024) return null;
      const ext = type.includes('png') ? '.png' : type.includes('webp') ? '.webp' : '.jpg';
      const file = dest + ext;
      fs.writeFileSync(file, buf);
      return file;
    } catch { return null; }
  }

  copyLocal(src, dest) {
    try {
      const file = dest + path.extname(src);
      fs.copyFileSync(src, file);
      return file;
    } catch { return null; }
  }

  async steamSearch(term) {
    await this.steamThrottle();
    const d = await this.json(`https://store.steampowered.com/api/storesearch/?term=${encodeURIComponent(term)}&l=english&cc=US`);
    const items = (d && d.items) || [];
    const key = matchKey(term);
    const exact = items.find((i) => matchKey(i.name) === key);
    if (exact) return String(exact.id);
    let best = null, bestSim = 0;
    for (const i of items.slice(0, 5)) {
      const s = similarity(term, i.name);
      if (s > bestSim) { bestSim = s; best = i; }
    }
    return best && bestSim >= 0.6 ? String(best.id) : null;
  }

  async steamAssets(appId) {
    await this.steamThrottle();
    const input = { ids: [{ appid: Number(appId) }], context: { language: 'russian', country_code: 'RU' }, data_request: { include_assets: true } };
    try {
      const d = await this.json(`https://api.steampowered.com/IStoreBrowseService/GetItems/v1/?input_json=${encodeURIComponent(JSON.stringify(input))}`);
      const a = d && d.response && d.response.store_items && d.response.store_items[0] && d.response.store_items[0].assets;
      if (a && a.asset_url_format) {
        const u = (f) => (f ? STEAM_CDN + a.asset_url_format.replace('${FILENAME}', f) : null);
        return { cover: u(a.library_capsule_2x || a.library_capsule), hero: u(a.library_hero_2x || a.library_hero), header: u(a.header) };
      }
    } catch {}
    return {};
  }

  async steamDetails(appId) {
    await this.steamThrottle();
    try {
      const d = await this.json(`https://store.steampowered.com/api/appdetails?appids=${appId}&l=russian`);
      return d && d[appId] && d[appId].success ? d[appId].data : null;
    } catch { return null; }
  }

  async sgdb(name, steamAppId) {
    const key = this.store.settings.sgdbKey;
    if (!key) return {};
    const H = { Authorization: `Bearer ${key}` };
    const base = 'https://www.steamgriddb.com/api/v2';
    let gid = null;
    try {
      if (steamAppId) {
        const g = await this.json(`${base}/games/steam/${steamAppId}`, H);
        gid = g && g.data && g.data.id;
      }
      if (!gid) {
        const s = await this.json(`${base}/search/autocomplete/${encodeURIComponent(name)}`, H);
        gid = s && s.data && s.data[0] && s.data[0].id;
      }
      if (!gid) return {};
      const [grids, heroes] = await Promise.all([
        this.json(`${base}/grids/game/${gid}?dimensions=600x900,342x482,660x930&types=static`, H).catch(() => null),
        this.json(`${base}/heroes/game/${gid}?types=static`, H).catch(() => null),
      ]);
      return {
        cover: grids && grids.data && grids.data[0] && grids.data[0].url,
        hero: heroes && heroes.data && heroes.data[0] && heroes.data[0].url,
      };
    } catch (e) { this.log('sgdb', e.message); return {}; }
  }

  async process(id, { term, force } = {}) {
    const g = this.store.get(id);
    if (!g) return;
    const name = term || g.customName || g.name;
    const base = path.join(this.store.mediaDir, hashId(id));
    const patch = { metaTried: true };
    const keepCover = g.userCover && !force;
    const keepHero = g.userHero && !force;
    let cover = null, hero = null;

    // 1) Steam's own library cache (offline, instant)
    if (!term && g.localArt) {
      if (!keepCover && g.localArt.cover && exists(g.localArt.cover)) cover = this.copyLocal(g.localArt.cover, base + '-cover');
      if (!keepHero && g.localArt.hero && exists(g.localArt.hero)) hero = this.copyLocal(g.localArt.hero, base + '-hero');
    }

    // 2) Steam store (works for most PC games, even if you own them elsewhere)
    let appId = term ? null : g.steamAppId || g.metaAppId;
    try { if (!appId) appId = await this.steamSearch(name); } catch (e) { this.log('steam search', e.message); }
    if (appId) {
      patch.metaAppId = appId;
      if ((!cover && !keepCover) || (!hero && !keepHero)) {
        const a = await this.steamAssets(appId);
        if (!cover && !keepCover) cover = (a.cover && await this.download(a.cover, base + '-cover')) || await this.download(OLD_CDN(appId, 'library_600x900_2x.jpg'), base + '-cover') || await this.download(OLD_CDN(appId, 'library_600x900.jpg'), base + '-cover');
        if (!hero && !keepHero) hero = (a.hero && await this.download(a.hero, base + '-hero')) || await this.download(OLD_CDN(appId, 'library_hero.jpg'), base + '-hero');
      }
      const det = await this.steamDetails(appId);
      if (det) {
        patch.description = decodeEntities(det.short_description);
        patch.developer = (det.developers || []).join(', ');
        patch.releaseDate = (det.release_date && det.release_date.date) || '';
        patch.genres = (det.genres || []).map((x) => x.description).slice(0, 5);
        if (!cover && !keepCover && det.header_image) cover = await this.download(det.header_image, base + '-cover');
      }
    }

    // 3) SteamGridDB (needs a free API key; great for non-Steam games like Tarkov)
    if ((!cover && !keepCover) || (!hero && !keepHero) || (term && this.store.settings.sgdbKey)) {
      const s = await this.sgdb(name, appId);
      if (s.cover && (!cover || term) && !keepCover) cover = (await this.download(s.cover, base + '-cover')) || cover;
      if (s.hero && (!hero || term) && !keepHero) hero = (await this.download(s.hero, base + '-hero')) || hero;
    }

    if (cover) { patch.coverPath = cover; patch.userCover = false; }
    if (hero) { patch.heroPath = hero; patch.userHero = false; }

    // 4) Icon from the exe as a last-resort visual
    if (!g.iconPath && g.exePath && this.getIcon) {
      const icon = await this.getIcon(g.exePath, base + '-icon.png');
      if (icon) patch.iconPath = icon;
    }
    patch.mediaVersion = Date.now();
    this.store.update(id, patch);
  }

  // User picked a file or pasted a link
  async setImage(id, kind, { file, url }) {
    const g = this.store.get(id);
    if (!g) throw new Error('Игра не найдена');
    const dest = path.join(this.store.mediaDir, `${hashId(id)}-${kind}-user-${Date.now().toString(36)}`);
    let out = null;
    if (file) out = this.copyLocal(file, dest);
    else if (url) out = await this.download(url, dest);
    if (!out) throw new Error('Не удалось загрузить изображение');
    const patch = kind === 'hero' ? { heroPath: out, userHero: true } : { coverPath: out, userCover: true };
    patch.mediaVersion = Date.now();
    this.store.update(id, patch);
  }
}

module.exports = { Meta, decodeEntities };
