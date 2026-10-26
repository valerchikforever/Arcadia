'use strict';
// VPN on the sing-box core: servers from your own links / subscriptions,
// "system proxy" mode (no admin rights) or "TUN" mode (all traffic, admin rights).
const fs = require('fs');
const path = require('path');
const net = require('net');
const { spawn, execFile } = require('child_process');
const { exists, hashId } = require('../util/common');
const { runPS, isWin } = require('../util/winsys');
const { runElevated, processNames, psq } = require('./elevate');
const { parseMany } = require('./links');

const CORE_NAME = 'arcadia-core.exe';
const XRAY_NAME = 'arcadia-xray.exe';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const reEsc = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

// Servers actually used by a connection: all of them for «Автовыбор», otherwise just the chosen one
function pickServers(servers, selected) {
  if (selected === 'auto') return servers;
  const one = servers.find((s) => s.id === selected);
  return [one || servers[0]].filter(Boolean);
}

// Xray runs xhttp servers; each one is exposed to sing-box as a local SOCKS port.
function buildXrayConfig(xrayServers, basePort, logFile) {
  return {
    log: { loglevel: 'warning', access: 'none', ...(logFile ? { error: logFile } : {}) },
    inbounds: xrayServers.map((s, i) => ({ tag: `in${i}`, listen: '127.0.0.1', port: basePort + i, protocol: 'socks', settings: { auth: 'noauth', udp: true, ip: '127.0.0.1' } })),
    outbounds: [...xrayServers.map((s, i) => ({ ...s.xray, tag: `out${i}` })), { protocol: 'freedom', tag: 'direct' }],
    routing: { rules: xrayServers.map((s, i) => ({ type: 'field', inboundTag: [`in${i}`], outboundTag: `out${i}` })) },
  };
}

// ---------- sing-box config (format of sing-box 1.12+) ----------
function buildConfig({ servers, selected, mode, directRu, gamesDirect, gameDirs = [], logFile, port = 2080, xrayBase = 0 }) {
  if (!servers.length) throw new Error('Добавьте хотя бы один сервер');
  const used = pickServers(servers, selected);
  let xi = 0;
  const outbounds = used.map((s, i) => (s.xray
    ? { type: 'socks', tag: `s${i}`, server: '127.0.0.1', server_port: xrayBase + xi++, version: '5' }
    : { ...s.outbound, tag: `s${i}` }));
  if (outbounds.length > 1) {
    outbounds.push({ type: 'urltest', tag: 'proxy', outbounds: outbounds.map((o) => o.tag), url: 'https://www.gstatic.com/generate_204', interval: '3m', tolerance: 80 });
  } else {
    outbounds[0].tag = 'proxy';
  }
  outbounds.push({ type: 'direct', tag: 'direct' });

  const ruSuffix = ['ru', 'su', 'xn--p1ai', 'xn--d1acj3b'];
  const rules = [
    { action: 'sniff' },
    { protocol: 'dns', action: 'hijack-dns' },
    { ip_is_private: true, outbound: 'direct' },
  ];
  // Xray's own connections to the server must bypass the TUN, otherwise they loop back into it
  if (mode === 'tun' && xi > 0) rules.splice(1, 0, { process_name: [XRAY_NAME], outbound: 'direct' });
  if (directRu) rules.push({ domain_suffix: ruSuffix, outbound: 'direct' });
  if (mode === 'tun' && gamesDirect && gameDirs.length) {
    rules.push({ process_path_regex: gameDirs.map((d) => '(?i)^' + reEsc(d.replace(/[\\/]+$/, '')) + '\\\\'), outbound: 'direct' });
  }

  const inbound = mode === 'tun'
    ? { type: 'tun', tag: 'tun-in', interface_name: 'Arcadia', address: ['172.19.0.1/30'], mtu: 9000, auto_route: true, strict_route: true, stack: 'mixed' }
    : { type: 'mixed', tag: 'mixed-in', listen: '127.0.0.1', listen_port: port };

  return {
    log: { level: 'warn', timestamp: true, ...(logFile ? { output: logFile } : {}) },
    dns: {
      servers: [
        { type: 'https', tag: 'dns-remote', server: '1.1.1.1', detour: 'proxy' },
        { type: 'local', tag: 'dns-local' },
      ],
      rules: directRu ? [{ domain_suffix: ruSuffix, server: 'dns-local' }] : [],
      final: 'dns-remote',
      strategy: 'prefer_ipv4',
    },
    inbounds: [inbound],
    outbounds,
    route: { rules, final: 'proxy', auto_detect_interface: true, default_domain_resolver: 'dns-local' },
  };
}

// ---------- Windows system proxy ----------
const WININET_REFRESH = `
$sig='[DllImport("wininet.dll", SetLastError=true)] public static extern bool InternetSetOption(IntPtr h, int o, IntPtr b, int l);'
$t=Add-Type -MemberDefinition $sig -Name WinInet -Namespace ArcadiaNet -PassThru
$t::InternetSetOption([IntPtr]::Zero,39,[IntPtr]::Zero,0) | Out-Null
$t::InternetSetOption([IntPtr]::Zero,37,[IntPtr]::Zero,0) | Out-Null`;
const IS_KEY = "'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings'";

async function readSystemProxy() {
  const out = await runPS(`$p=Get-ItemProperty ${IS_KEY}; ConvertTo-Json -Compress @{enable=[int]$p.ProxyEnable; server=[string]$p.ProxyServer; override=[string]$p.ProxyOverride}`);
  try { return JSON.parse(out.trim()); } catch { return null; }
}

async function setSystemProxy(port) {
  await runPS(`Set-ItemProperty ${IS_KEY} -Name ProxyEnable -Value 1
Set-ItemProperty ${IS_KEY} -Name ProxyServer -Value '127.0.0.1:${port}'
Set-ItemProperty ${IS_KEY} -Name ProxyOverride -Value 'localhost;127.*;10.*;172.16.*;172.17.*;172.18.*;172.19.*;172.2*;172.30.*;172.31.*;192.168.*;<local>'
${WININET_REFRESH}`);
}

async function restoreSystemProxy(prev) {
  const p = prev || { enable: 0, server: '', override: '' };
  await runPS(`Set-ItemProperty ${IS_KEY} -Name ProxyEnable -Value ${p.enable ? 1 : 0}
Set-ItemProperty ${IS_KEY} -Name ProxyServer -Value ${psq(p.server || '')}
Set-ItemProperty ${IS_KEY} -Name ProxyOverride -Value ${psq(p.override || '')}
${WININET_REFRESH}`);
}

function tcpPing(host, port, timeout = 3000) {
  return new Promise((resolve) => {
    const t0 = Date.now();
    const s = net.connect({ host, port, timeout });
    const done = (v) => { s.destroy(); resolve(v); };
    s.once('connect', () => done(Date.now() - t0));
    s.once('timeout', () => done(-1));
    s.once('error', () => done(-1));
  });
}

class Vpn {
  constructor(store, { onChange, log, fetch, dataDir, getGameDirs }) {
    this.store = store;
    this.onChange = onChange || (() => {});
    this.log = log || (() => {});
    this.fetch = fetch;
    this.dir = path.join(dataDir, 'vpn');
    this.coreDir = path.join(dataDir, 'core');
    fs.mkdirSync(this.dir, { recursive: true });
    fs.mkdirSync(this.coreDir, { recursive: true });
    this.getGameDirs = getGameDirs || (() => []);
    this.child = null;
    this.running = false;
    this.busy = false;
    this.error = '';
    this.installing = '';
    this.coreVersion = '';
    this.pings = {};
    this.xrayChild = null;
    this.xrayVersion = '';
    this.xrayInstalling = '';
    this.runningMode = '';
    this.detectVersion();
    this.detectXrayVersion();
  }

  get cfg() { return this.store.settings.vpn; }
  saveCfg(patch) { this.store.settings.vpn = { ...this.cfg, ...patch }; this.store.save(); }

  corePath() {
    const own = path.join(this.coreDir, CORE_NAME);
    if (exists(own)) return own;
    if (this.cfg.corePath && exists(this.cfg.corePath)) return this.cfg.corePath;
    return null;
  }

  detectVersion() {
    const p = this.corePath();
    if (!p) { this.coreVersion = ''; return; }
    execFile(p, ['version'], { windowsHide: true, timeout: 10000 }, (err, out) => {
      const m = /version\s+([\w.\-+]+)/i.exec(String(out || ''));
      this.coreVersion = m ? m[1] : err ? '' : '?';
      this.onChange();
    });
  }

  state() {
    const c = this.cfg;
    return {
      servers: (c.servers || []).map((s) => ({ id: s.id, name: s.name, host: s.host, port: s.port, proto: s.proto, transport: s.transport || '', subId: s.subId || '', udp: !!s.udp, ping: this.pings[s.id] })),
      subs: c.subs || [],
      selected: c.selected || 'auto',
      mode: c.mode || 'proxy',
      directRu: c.directRu !== false,
      gamesDirect: c.gamesDirect !== false,
      stopOnExit: c.stopOnExit !== false,
      port: c.port || 2080,
      running: this.running,
      runningMode: this.running ? this.runningMode || c.lastMode || c.mode : '',
      busy: this.busy,
      error: this.error,
      core: { installed: !!this.corePath(), version: this.coreVersion, installing: this.installing },
      xray: { needed: (c.servers || []).some((s) => s.xray), installed: !!this.xrayPath(), version: this.xrayVersion, installing: this.xrayInstalling },
    };
  }

  setStatus(names) {
    const r = names.has(CORE_NAME);
    if (!r && this.xrayChild && !this.busy) this.killXray();
    if (r !== this.running) {
      this.running = r;
      if (!r && this.cfg.proxySet) this.clearProxy().catch(() => {});
      this.onChange();
    }
  }

  // ---------- servers ----------
  addFromText(text, subId) {
    const { servers, errors } = parseMany(text);
    const list = [...(this.cfg.servers || [])];
    const known = new Set(list.map((s) => s.id));
    let added = 0;
    for (const s of servers) {
      const id = 'v' + hashId(JSON.stringify(s.outbound || s.xray));
      if (known.has(id)) continue;
      known.add(id);
      const item = { id, name: s.name, host: s.host, port: s.port, proto: s.proto, udp: !!s.udp, link: s.link || '', subId: subId || '' };
      if (s.xray) { item.xray = s.xray; item.transport = s.transport; } else item.outbound = s.outbound;
      list.push(item);
      added++;
    }
    this.saveCfg({ servers: list });
    this.onChange();
    return { added, found: servers.length, errors };
  }

  async fetchSub(url) {
    const r = await this.fetch(url, { headers: { 'User-Agent': 'v2rayN/7.10' } });
    if (!r.ok) throw new Error(`Подписка ответила ошибкой ${r.status}`);
    return r.text();
  }

  async addInput(text) {
    const t = String(text || '').trim();
    const lines = t.split(/\s+/).filter(Boolean);
    // A bare https link is a subscription
    if (lines.length === 1 && /^https?:\/\//i.test(lines[0])) return this.addSub(lines[0]);
    const r = this.addFromText(t);
    if (!r.found && !r.errors.length) r.errors.push('Ссылки не распознаны');
    return r;
  }

  async addSub(url, name) {
    const id = 's' + hashId(url);
    const body = await this.fetchSub(url);
    const subs = (this.cfg.subs || []).filter((s) => s.id !== id);
    const host = (() => { try { return new URL(url).hostname; } catch { return url; } })();
    subs.push({ id, url, name: name || host, updatedAt: Date.now() });
    this.saveCfg({ subs, servers: (this.cfg.servers || []).filter((s) => s.subId !== id) });
    const r = this.addFromText(body, id);
    const sub = subs.find((s) => s.id === id);
    sub.count = r.found;
    this.saveCfg({ subs });
    if (!r.found) r.errors.unshift('В подписке не найдено серверов');
    return { ...r, sub: true };
  }

  async refreshSubs() {
    const errors = [];
    let total = 0;
    for (const s of [...(this.cfg.subs || [])]) {
      try { const r = await this.addSub(s.url, s.name); total += r.found; } catch (e) { errors.push(`${s.name}: ${e.message}`); }
    }
    return { found: total, errors };
  }

  removeServer(id) {
    this.saveCfg({ servers: (this.cfg.servers || []).filter((s) => s.id !== id), selected: this.cfg.selected === id ? 'auto' : this.cfg.selected });
    this.onChange();
  }

  removeSub(id) {
    this.saveCfg({ subs: (this.cfg.subs || []).filter((s) => s.id !== id), servers: (this.cfg.servers || []).filter((s) => s.subId !== id) });
    this.onChange();
  }

  renameServer(id, name) {
    this.saveCfg({ servers: (this.cfg.servers || []).map((s) => (s.id === id ? { ...s, name } : s)) });
    this.onChange();
  }

  async pingAll() {
    const list = [...(this.cfg.servers || [])];
    for (const s of list) this.pings[s.id] = s.udp ? 'udp' : 0;
    this.onChange();
    let i = 0;
    const worker = async () => {
      while (i < list.length) {
        const s = list[i++];
        if (s.udp) continue;
        this.pings[s.id] = await tcpPing(s.host, s.port);
        this.onChange();
      }
    };
    await Promise.all(Array.from({ length: 8 }, worker));
  }

  // ---------- core ----------
  // Download the latest Windows build from GitHub releases and put <exeName> into coreDir as <target>
  async installFromGithub({ repo, asset, exeName, target, label, setStatus, extra = /\.(dll)$/i }) {
    if (!isWin) throw new Error('Доступно только в Windows');
    setStatus('Ищем последнюю версию…');
    const r = await this.fetch(`https://api.github.com/repos/${repo}/releases/latest`, { headers: { 'User-Agent': 'ArcadiaLauncher', Accept: 'application/vnd.github+json' } });
    if (!r.ok) throw new Error(`GitHub недоступен (${r.status})`);
    const rel = await r.json();
    const a = (rel.assets || []).find((x) => asset.test(x.name));
    if (!a) throw new Error(`Не найден архив ${label} для Windows`);
    setStatus(`Скачиваем ${label} ${rel.tag_name || ''}…`);
    const z = await this.fetch(a.browser_download_url, { headers: { 'User-Agent': 'ArcadiaLauncher' } });
    if (!z.ok) throw new Error(`Не удалось скачать (${z.status})`);
    const zip = path.join(this.coreDir, `${target}.zip`);
    fs.writeFileSync(zip, Buffer.from(await z.arrayBuffer()));
    setStatus('Распаковываем…');
    const unpack = path.join(this.coreDir, `unpack-${target}`);
    fs.rmSync(unpack, { recursive: true, force: true });
    await runPS(`Expand-Archive -LiteralPath ${psq(zip)} -DestinationPath ${psq(unpack)} -Force`, 120000);
    const find = (d) => {
      for (const e of fs.readdirSync(d, { withFileTypes: true })) {
        const f = path.join(d, e.name);
        if (e.isDirectory()) { const r2 = find(f); if (r2) return r2; } else if (e.name.toLowerCase() === exeName) return f;
      }
      return null;
    };
    const exe = find(unpack);
    if (!exe) throw new Error(`В архиве нет ${exeName}`);
    if (this.running) await this.stop(true);
    fs.copyFileSync(exe, path.join(this.coreDir, target));
    for (const e of fs.readdirSync(path.dirname(exe))) if (extra.test(e)) fs.copyFileSync(path.join(path.dirname(exe), e), path.join(this.coreDir, e));
    fs.rmSync(unpack, { recursive: true, force: true });
    fs.rmSync(zip, { force: true });
  }

  async installCore() {
    this.error = '';
    try {
      await this.installFromGithub({
        repo: 'SagerNet/sing-box', asset: /windows-amd64\.zip$/i, exeName: 'sing-box.exe', target: CORE_NAME, label: 'sing-box',
        setStatus: (t) => { this.installing = t; this.onChange(); },
      });
      this.detectVersion();
    } catch (e) {
      this.error = `${e.message}. Можно скачать sing-box вручную с github.com/SagerNet/sing-box/releases и указать sing-box.exe.`;
      throw e;
    } finally {
      this.installing = '';
      this.onChange();
    }
  }

  // ---------- Xray core (only for xhttp servers) ----------
  xrayPath() {
    const own = path.join(this.coreDir, XRAY_NAME);
    return exists(own) ? own : null;
  }

  detectXrayVersion() {
    const p = this.xrayPath();
    if (!p) { this.xrayVersion = ''; return; }
    execFile(p, ['version'], { windowsHide: true, timeout: 10000 }, (err, out) => {
      const m = /Xray\s+([\w.\-+]+)/i.exec(String(out || ''));
      this.xrayVersion = m ? m[1] : err ? '' : '?';
      this.onChange();
    });
  }

  async installXray() {
    this.error = '';
    try {
      await this.installFromGithub({
        repo: 'XTLS/Xray-core', asset: /^Xray-windows-64\.zip$/i, exeName: 'xray.exe', target: XRAY_NAME, label: 'Xray',
        extra: /\.(dll|dat)$/i,
        setStatus: (t) => { this.xrayInstalling = t; this.onChange(); },
      });
      this.detectXrayVersion();
    } catch (e) {
      this.error = `${e.message}. Можно скачать Xray вручную с github.com/XTLS/Xray-core/releases (Xray-windows-64.zip) и указать xray.exe.`;
      throw e;
    } finally {
      this.xrayInstalling = '';
      this.onChange();
    }
  }

  setXrayFile(file) {
    fs.copyFileSync(file, path.join(this.coreDir, XRAY_NAME));
    for (const e of fs.readdirSync(path.dirname(file))) if (/\.(dll|dat)$/i.test(e)) { try { fs.copyFileSync(path.join(path.dirname(file), e), path.join(this.coreDir, e)); } catch {} }
    this.detectXrayVersion();
    this.onChange();
  }

  checkXray(cfgFile) {
    return new Promise((resolve, reject) => {
      execFile(this.xrayPath(), ['run', '-test', '-c', cfgFile], { windowsHide: true, timeout: 20000, cwd: this.coreDir }, (err, out, errOut) => {
        if (err) reject(new Error('Ошибка в конфигурации Xray: ' + String(out || errOut || err.message).trim().split('\n').slice(-2).join(' ')));
        else resolve();
      });
    });
  }

  killXray() {
    if (this.xrayChild) { try { this.xrayChild.kill(); } catch {} this.xrayChild = null; }
    if (isWin) execFile('taskkill.exe', ['/F', '/IM', XRAY_NAME], { windowsHide: true }, () => {});
  }

  setCoreFile(file) {
    fs.copyFileSync(file, path.join(this.coreDir, CORE_NAME));
    for (const e of fs.readdirSync(path.dirname(file))) if (/\.dll$/i.test(e)) { try { fs.copyFileSync(path.join(path.dirname(file), e), path.join(this.coreDir, e)); } catch {} }
    this.detectVersion();
    this.onChange();
  }

  check(core, cfgFile) {
    return new Promise((resolve, reject) => {
      execFile(core, ['check', '-c', cfgFile], { windowsHide: true, timeout: 20000 }, (err, out, errOut) => {
        if (err) reject(new Error('Ошибка в конфигурации: ' + String(errOut || out || err.message).replace(/\x1b\[[0-9;]*m/g, '').trim().split('\n').slice(-2).join(' ')));
        else resolve();
      });
    });
  }

  logTail() {
    try { return fs.readFileSync(path.join(this.dir, 'core.log'), 'utf8').replace(/\x1b\[[0-9;]*m/g, '').trim().split('\n').slice(-3).join('\n'); } catch { return ''; }
  }

  async waitFor(want, ms = 8000) {
    const until = Date.now() + ms;
    while (Date.now() < until) {
      if ((await processNames()).has(CORE_NAME) === want) return true;
      await sleep(500);
    }
    return false;
  }

  async clearProxy() {
    if (!this.cfg.proxySet) return;
    await restoreSystemProxy(this.cfg.prevProxy);
    this.saveCfg({ proxySet: false });
  }

  async start() {
    const core = this.corePath();
    if (!core) throw new Error('Ядро sing-box не установлено');
    const c = this.state();
    this.busy = true; this.error = ''; this.onChange();
    try {
      if (this.running) await this.stop(true);
      const logFile = path.join(this.dir, 'core.log');
      try { fs.writeFileSync(logFile, ''); } catch {}
      const used = pickServers(this.cfg.servers || [], c.selected);
      const xrayUsed = used.filter((s) => s.xray);
      const xrayBase = c.port + 20;
      if (xrayUsed.length) {
        if (!this.xrayPath()) throw new Error('Для серверов с xhttp нужно ядро Xray — нажмите «Скачать Xray» в окне VPN');
        const xLog = path.join(this.dir, 'xray.log');
        try { fs.writeFileSync(xLog, ''); } catch {}
        const xFile = path.join(this.dir, 'xray.json');
        fs.writeFileSync(xFile, JSON.stringify(buildXrayConfig(xrayUsed, xrayBase, xLog), null, 2));
        await this.checkXray(xFile);
        this.killXray();
        await sleep(300);
        this.xrayChild = spawn(this.xrayPath(), ['run', '-c', xFile], { cwd: this.coreDir, windowsHide: true, stdio: 'ignore' });
        this.xrayChild.on('exit', () => { this.xrayChild = null; });
        await sleep(900);
        if (!this.xrayChild) {
          let tail = '';
          try { tail = fs.readFileSync(xLog, 'utf8').trim().split('\n').slice(-2).join(' '); } catch {}
          throw new Error('Xray не запустился: ' + (tail || 'неизвестная ошибка'));
        }
      } else {
        this.killXray();
      }
      const config = buildConfig({
        servers: this.cfg.servers || [], selected: c.selected, mode: c.mode, directRu: c.directRu, gamesDirect: c.gamesDirect,
        gameDirs: this.getGameDirs(), logFile, port: c.port, xrayBase,
      });
      const cfgFile = path.join(this.dir, 'config.json');
      fs.writeFileSync(cfgFile, JSON.stringify(config, null, 2));
      await this.check(core, cfgFile);
      if (c.mode === 'tun') {
        await runElevated(`Stop-Process -Name arcadia-core -Force
Start-Process -FilePath ${psq(core)} -ArgumentList @('run','-c',${psq(cfgFile)},'-D',${psq(this.dir)}) -WorkingDirectory ${psq(this.coreDir)} -WindowStyle Hidden`);
      } else {
        this.child = spawn(core, ['run', '-c', cfgFile, '-D', this.dir], { cwd: this.coreDir, windowsHide: true, stdio: 'ignore' });
        this.child.on('exit', () => { this.child = null; this.clearProxy().catch(() => {}); });
      }
      const up = await this.waitFor(true);
      await sleep(800);
      const still = (await processNames()).has(CORE_NAME);
      if (!up || !still) throw new Error('VPN не запустился: ' + (this.logTail() || 'неизвестная ошибка'));
      if (c.mode !== 'tun') {
        if (!this.cfg.proxySet) this.saveCfg({ prevProxy: await readSystemProxy() });
        await setSystemProxy(c.port);
        this.saveCfg({ proxySet: true });
      }
      this.saveCfg({ lastMode: c.mode });
      this.runningMode = c.mode;
      this.running = true;
    } catch (e) {
      this.killXray();
      this.error = e.message;
      throw e;
    } finally {
      this.busy = false;
      this.onChange();
    }
  }

  async stop(keepBusy) {
    if (!keepBusy) { this.busy = true; this.error = ''; this.onChange(); }
    try {
      if (this.child) { try { this.child.kill(); } catch {} this.child = null; }
      this.killXray();
      await this.clearProxy().catch(() => {});
      if ((await processNames()).has(CORE_NAME)) {
        // started earlier or with admin rights: try normally, then elevated
        await new Promise((r) => execFile('taskkill.exe', ['/F', '/IM', CORE_NAME], { windowsHide: true }, () => r()));
        if (!(await this.waitFor(false, 1500))) await runElevated('Stop-Process -Name arcadia-core -Force', { wait: true });
      }
      await this.waitFor(false, 4000);
      this.running = false;
    } catch (e) {
      this.error = e.message;
      throw e;
    } finally {
      if (!keepBusy) { this.busy = false; this.onChange(); }
    }
  }

  // Called when Arcadia quits
  shutdownSync() {
    if (this.child) { try { this.child.kill(); } catch {} }
    if (this.xrayChild) { try { this.xrayChild.kill(); } catch {} }
  }
}

module.exports = { Vpn, buildConfig, buildXrayConfig, pickServers, tcpPing, CORE_NAME, XRAY_NAME };
