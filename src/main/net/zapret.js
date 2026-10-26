'use strict';
// zapret (winws.exe) control: finds the zapret folder, lists strategy .bat files,
// runs the chosen strategy hidden with admin rights and reports whether it is running.
const fs = require('fs');
const path = require('path');
const os = require('os');
const { exists, isDir } = require('../util/common');
const { runElevated, processNames, psq } = require('./elevate');

const NOT_STRATEGY = /^(service|cleanup|check|update|install|uninstall|test|discord_cache|ipset|game_filter|remove|status)/i;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function findWinws(dir) {
  for (const p of [path.join(dir, 'bin', 'winws.exe'), path.join(dir, 'winws.exe'), path.join(dir, 'zapret-winws', 'winws.exe')]) if (exists(p)) return p;
  return null;
}

function looksLikeZapret(dir) {
  if (!isDir(dir)) return false;
  if (findWinws(dir)) return true;
  try { return fs.readdirSync(dir).some((f) => /\.bat$/i.test(f) && /winws/i.test(fs.readFileSync(path.join(dir, f), 'utf8'))); } catch { return false; }
}

// Look for zapret folders in the usual places (Desktop, Downloads, disk roots…)
function detectDirs() {
  const home = os.homedir();
  const roots = [home, path.join(home, 'Desktop'), path.join(home, 'Downloads'), path.join(home, 'Documents'), path.join(home, 'OneDrive', 'Desktop'), path.join(home, 'OneDrive', 'Рабочий стол')];
  for (const d of 'CDEFG') roots.push(`${d}:\\`, `${d}:\\Program Files`);
  const found = new Set();
  const visit = (dir, depth) => {
    let ents = [];
    try { ents = fs.readdirSync(dir, { withFileTypes: true }); } catch { return; }
    for (const e of ents) {
      if (!e.isDirectory()) continue;
      const full = path.join(dir, e.name);
      if (/zapret|unlocker|discord.?youtube|winws/i.test(e.name) && looksLikeZapret(full)) found.add(full);
      else if (depth < 1 && /zapret|unlocker|bypass|обход/i.test(e.name)) visit(full, depth + 1);
    }
  };
  for (const r of roots) if (isDir(r)) visit(r, 0);
  return [...found];
}

function listStrategies(dir) {
  if (!dir || !isDir(dir)) return [];
  let files = [];
  try { files = fs.readdirSync(dir).filter((f) => /\.(bat|cmd)$/i.test(f) && !NOT_STRATEGY.test(f)); } catch { return []; }
  const out = [];
  for (const f of files) {
    try { if (/winws/i.test(fs.readFileSync(path.join(dir, f), 'utf8'))) out.push(f); } catch {}
  }
  const rank = (f) => (/^general\.bat$/i.test(f) ? 0 : /^general/i.test(f) ? 1 : 2);
  return out.sort((a, b) => rank(a) - rank(b) || a.localeCompare(b, 'ru', { numeric: true }));
}

// Turn a strategy .bat into "winws.exe + arguments" so it can run hidden without the console window.
function parseBat(file, { gameFilter = false } = {}) {
  const dir = path.dirname(file);
  const name = path.basename(file, path.extname(file));
  const raw = fs.readFileSync(file, 'utf8').replace(/^﻿/, '').replace(/\r/g, '');
  const joined = [];
  let buf = '';
  for (const l of raw.split('\n')) {
    if (/\^\s*$/.test(l)) { buf += l.replace(/\^\s*$/, ' '); continue; }
    joined.push(buf + l);
    buf = '';
  }
  const vars = {};
  const gf = gameFilter ? '1024-65535' : '12';
  const expand = (s) => s
    .replace(/%~dp0/gi, dir + path.sep)
    .replace(/%~n0/gi, name)
    .replace(/%([A-Za-z0-9_]+)%/g, (m, k) => {
      const key = Object.keys(vars).find((x) => x.toLowerCase() === k.toLowerCase());
      if (key) return vars[key];
      if (/^gamefilter/i.test(k)) return gf;
      if (process.env[k] !== undefined) return process.env[k];
      return m;
    });
  let exe = null, args = null;
  for (const line of joined) {
    const t = line.trim();
    if (!t || /^(rem\b|::)/i.test(t)) continue;
    const set = /^set\s+"?([A-Za-z0-9_]+)=(.*?)"?\s*$/i.exec(t);
    if (set) { vars[set[1]] = expand(set[2]); continue; }
    const m = /("?)([^"\s]*winws\.exe)\1\s+(.*)$/i.exec(t);
    if (m && !/tasklist|taskkill|find(str)?\s/i.test(t)) { exe = expand(m[2]); args = expand(m[3]).trim(); break; }
  }
  if (!exe) throw new Error('В стратегии не найдена команда winws.exe');
  if (!path.isAbsolute(exe)) exe = path.join(dir, exe);
  if (!exists(exe)) exe = findWinws(dir) || exe;
  const left = (args.match(/%[A-Za-z0-9_]+%/g) || []).filter((v, i, a) => a.indexOf(v) === i);
  if (left.length) throw new Error(`Не удалось подставить переменные: ${left.join(', ')}`);
  return { exe, args, cwd: path.dirname(exe) };
}

// Strategies reference *-user.txt lists that service.bat normally creates — make sure they exist.
function ensureLists(args) {
  const re = /--(?:hostlist|hostlist-exclude|ipset|ipset-exclude)="([^"]+)"/g;
  let m;
  while ((m = re.exec(args))) {
    const f = m[1];
    if (!exists(f) && /user/i.test(path.basename(f)) && isDir(path.dirname(f))) {
      try { fs.writeFileSync(f, /ipset/i.test(m[0]) ? '203.0.113.113/32\n' : 'domain.example.abc\n'); } catch {}
    }
  }
}

class Zapret {
  constructor(store, { onChange, log }) {
    this.store = store;
    this.onChange = onChange || (() => {});
    this.log = log || (() => {});
    this.running = false;
    this.busy = false;
    this.error = '';
  }

  get cfg() { return this.store.settings.zapret; }

  state() {
    const dir = this.cfg.dir;
    const strategies = listStrategies(dir);
    return {
      dir,
      valid: !!dir && looksLikeZapret(dir),
      strategies,
      strategy: this.cfg.strategy && strategies.includes(this.cfg.strategy) ? this.cfg.strategy : strategies[0] || '',
      running: this.running,
      runningStrategy: this.running ? this.cfg.lastStarted || '' : '',
      busy: this.busy,
      error: this.error,
      gameFilter: !!this.cfg.gameFilter,
      autostart: !!this.cfg.autostart,
      stopOnExit: !!this.cfg.stopOnExit,
    };
  }

  setStatus(names) {
    const r = names.has('winws.exe');
    if (r !== this.running) { this.running = r; this.onChange(); }
  }

  async waitFor(want, ms = 9000) {
    const until = Date.now() + ms;
    while (Date.now() < until) {
      const r = (await processNames()).has('winws.exe');
      if (r === want) return true;
      await sleep(600);
    }
    return false;
  }

  async start(strategy) {
    const st = this.state();
    if (!st.valid) throw new Error('Укажите папку zapret');
    strategy = strategy || st.strategy;
    if (!strategy) throw new Error('В папке нет стратегий (.bat файлов с winws.exe)');
    const file = path.join(st.dir, strategy);
    this.busy = true; this.error = ''; this.onChange();
    try {
      let script;
      try {
        const p = parseBat(file, { gameFilter: this.cfg.gameFilter });
        ensureLists(p.args);
        script = `Stop-Process -Name winws -Force
Start-Sleep -Milliseconds 500
Start-Process -FilePath ${psq(p.exe)} -ArgumentList ${psq(p.args)} -WorkingDirectory ${psq(p.cwd)} -WindowStyle Hidden`;
      } catch (e) {
        // Unusual .bat — run it as is (its console window will be minimized)
        this.log('zapret parse', strategy, e.message);
        script = `Stop-Process -Name winws -Force
Start-Sleep -Milliseconds 500
Start-Process -FilePath 'cmd.exe' -ArgumentList @('/c', ${psq('"' + file + '"')}) -WorkingDirectory ${psq(st.dir)} -WindowStyle Minimized`;
      }
      await runElevated(script);
      const ok = await this.waitFor(true);
      if (!ok) throw new Error('winws.exe не запустился. Попробуйте другую стратегию или добавьте папку zapret в исключения антивируса.');
      this.store.settings.zapret = { ...this.cfg, strategy, lastStarted: strategy };
      this.store.save();
      this.running = true;
    } catch (e) {
      this.error = e.message;
      throw e;
    } finally {
      this.busy = false;
      this.onChange();
    }
  }

  async stop() {
    this.busy = true; this.error = ''; this.onChange();
    try {
      await runElevated(`Stop-Process -Name winws -Force
foreach($s in 'WinDivert','WinDivert14'){ sc.exe stop $s | Out-Null; sc.exe delete $s | Out-Null }`, { wait: true });
      await this.waitFor(false, 4000);
      this.running = false;
    } catch (e) {
      this.error = e.message;
      throw e;
    } finally {
      this.busy = false;
      this.onChange();
    }
  }
}

module.exports = { Zapret, parseBat, listStrategies, detectDirs, looksLikeZapret };
