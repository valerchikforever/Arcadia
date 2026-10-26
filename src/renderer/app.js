'use strict';
/* Arcadia — renderer */
// `api` is exposed globally by preload.js (contextBridge)
const I = window.ICONS;

const SOURCES = {
  steam: { label: 'Steam', color: '#66c0f4' },
  epic: { label: 'Epic Games', color: '#dcdcdc' },
  gog: { label: 'GOG', color: '#c27cf6' },
  ea: { label: 'EA app', color: '#ff6464' },
  ubisoft: { label: 'Ubisoft Connect', color: '#5aa2ff' },
  battlenet: { label: 'Battle.net', color: '#1fbaff' },
  battlestate: { label: 'BattleState', color: '#e0b25c' },
  folder: { label: 'Папки и торренты', short: 'Папка', color: '#7fd18b' },
  manual: { label: 'Добавлены вручную', short: 'Вручную', color: '#f0a6ff' },
};

let S = { games: {}, collections: [], settings: {}, running: [], scanStatus: {}, scanning: false, metaPending: 0 };
const ui = { view: 'all', search: '', detail: null, scroll: {}, lastHtml: '', lastNav: '' };

// ---------------- helpers ----------------
const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];
const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const nameOf = (g) => g.customName || g.name || 'Без названия';
const ptOf = (g) => (g.source === 'steam' ? Math.max(g.playtime || 0, g.importedPlaytime || 0) : g.playtime || 0);
const lastOf = (g) => Math.max(g.lastPlayed || 0, g.lastLaunched || 0, g.importedLastPlayed || 0);
const srcOf = (g) => SOURCES[g.source] || { label: g.source, color: '#aaa' };
const img = (p, g) => (p ? api.fileUrl(p, g && g.mediaVersion) : '');
const games = () => Object.values(S.games);
const isRunning = (id) => S.running.includes(id);
const norm = (s) => String(s || '').toLowerCase().replace(/ё/g, 'е');

function fmtTime(sec, empty = '') {
  if (!sec) return empty;
  const h = Math.floor(sec / 3600), m = Math.round((sec % 3600) / 60);
  if (h >= 100) return `${h.toLocaleString('ru-RU')} ч`;
  if (h) return m ? `${h} ч ${m} мин` : `${h} ч`;
  return `${Math.max(1, m)} мин`;
}
function fmtAgo(ts) {
  if (!ts) return 'никогда';
  const d0 = new Date(); d0.setHours(0, 0, 0, 0);
  const diff = Math.floor((d0 - new Date(ts).setHours(0, 0, 0, 0)) / 864e5);
  if (diff <= 0) return 'сегодня';
  if (diff === 1) return 'вчера';
  if (diff < 7) return `${diff} дн. назад`;
  return new Date(ts).toLocaleDateString('ru-RU', { day: 'numeric', month: 'short', year: diff > 300 ? 'numeric' : undefined });
}
function hue(s) { let h = 0; for (const c of String(s)) h = (h * 31 + c.charCodeAt(0)) % 360; return h; }
function plural(n, one, few, many) {
  const a = Math.abs(n) % 100, b = a % 10;
  if (a > 10 && a < 20) return many;
  if (b > 1 && b < 5) return few;
  if (b === 1) return one;
  return many;
}

// ---------------- data views ----------------
function viewGames(view = ui.view) {
  let list = games();
  if (view === 'hidden') list = list.filter((g) => g.hidden);
  else {
    list = list.filter((g) => !g.hidden);
    if (view === 'favorites') list = list.filter((g) => g.favorite);
    else if (view === 'recent') list = list.filter((g) => lastOf(g));
    else if (view === 'missing') list = list.filter((g) => g.missing);
    else if (view.startsWith('src:')) list = list.filter((g) => g.source === view.slice(4));
    else if (view.startsWith('col:')) list = list.filter((g) => (g.collections || []).includes(view.slice(4)));
    if (view !== 'missing' && !view.startsWith('src:')) list = list.filter((g) => !g.missing || (g.collections || []).length || g.favorite);
  }
  if (ui.search) {
    const q = norm(ui.search);
    list = list.filter((g) => norm(nameOf(g)).includes(q) || norm(g.developer).includes(q));
  }
  const sort = view === 'recent' ? 'recent' : S.settings.sort || 'name';
  const byName = (a, b) => nameOf(a).localeCompare(nameOf(b), 'ru', { sensitivity: 'base', numeric: true });
  const cmp = {
    name: byName,
    recent: (a, b) => lastOf(b) - lastOf(a) || byName(a, b),
    playtime: (a, b) => ptOf(b) - ptOf(a) || byName(a, b),
    added: (a, b) => (b.addedAt || 0) - (a.addedAt || 0) || byName(a, b),
  }[sort] || byName;
  return list.sort(cmp);
}

function viewTitle(view) {
  if (view === 'all') return 'Библиотека';
  if (view === 'favorites') return 'Избранное';
  if (view === 'recent') return 'Недавние';
  if (view === 'hidden') return 'Скрытые';
  if (view === 'missing') return 'Не найдены';
  if (view === 'stats') return 'Статистика';
  if (view.startsWith('src:')) return (SOURCES[view.slice(4)] || {}).label || view.slice(4);
  if (view.startsWith('col:')) { const c = S.collections.find((x) => x.id === view.slice(4)); return c ? c.name : 'Коллекция'; }
  return '';
}

// ---------------- sidebar ----------------
function navItem(view, icon, label, count, extra = '') {
  const active = !ui.detail && ui.view === view ? ' active' : '';
  return `<button class="nav-item${active}" data-view="${esc(view)}" ${extra}>${icon}<span class="lbl">${esc(label)}</span>${count !== undefined ? `<span class="cnt">${count}</span>` : ''}</button>`;
}

function renderNav() {
  const all = games();
  const vis = all.filter((g) => !g.hidden && !g.missing);
  const cnt = (f) => vis.filter(f).length;
  let html = '<div class="nav-group">';
  html += navItem('all', I.grid, 'Все игры', vis.length);
  html += navItem('favorites', I.heart, 'Избранное', cnt((g) => g.favorite));
  html += navItem('recent', I.recent, 'Недавние', cnt((g) => lastOf(g)));
  html += navItem('stats', I.chart, 'Статистика');
  const hid = all.filter((g) => g.hidden).length;
  if (hid) html += navItem('hidden', I.eyeOff, 'Скрытые', hid);
  const miss = all.filter((g) => g.missing && !g.hidden).length;
  if (miss) html += navItem('missing', I.alert, 'Не найдены', miss);
  const n = S.net;
  if (n) {
    const dots = `<span class="net-dots"><i class="${n.zapret.running ? 'on' : ''}" title="zapret"></i><i class="${n.vpn.running ? 'on' : ''}" title="VPN"></i></span>`;
    html += `<button class="nav-item" data-act="net">${I.shield}<span class="lbl">Обход блокировок</span>${dots}</button>`;
  }
  html += '</div>';

  const srcCounts = Object.keys(SOURCES).map((k) => [k, all.filter((g) => g.source === k && !g.hidden).length]).filter(([, n]) => n);
  if (srcCounts.length) {
    html += '<div class="nav-group"><div class="nav-title">Источники</div>';
    for (const [k, n] of srcCounts) html += navItem('src:' + k, `<span class="dot" style="--c:${SOURCES[k].color}"></span>`, SOURCES[k].label, n);
    html += '</div>';
  }
  html += `<div class="nav-group"><div class="nav-title">Коллекции<button data-act="new-col" title="Новая коллекция">${I.plus}</button></div>`;
  if (!S.collections.length) html += '<div style="padding:4px 10px;color:var(--text-3);font-size:12.5px">Создайте коллекцию и перетащите в неё игры</div>';
  for (const c of S.collections) {
    html += navItem('col:' + c.id, I.stack, c.name, all.filter((g) => (g.collections || []).includes(c.id) && !g.hidden).length, `data-col="${esc(c.id)}"`);
  }
  html += '</div>';
  if (html !== ui.lastNav) { $('#nav').innerHTML = html; ui.lastNav = html; }

  const total = all.reduce((s, g) => s + ptOf(g), 0);
  $('#totalTime').innerHTML = `Всего наиграно<b>${fmtTime(total, '0 ч')}</b>`;
}

// ---------------- library ----------------
function coverHtml(g, cls = 'art') {
  if (g.coverPath) return `<img class="${cls}" src="${esc(img(g.coverPath, g))}" loading="lazy" draggable="false">`;
  return `<div class="fallback" style="--h:${hue(nameOf(g))}">${g.iconPath ? `<img src="${esc(img(g.iconPath, g))}" draggable="false">` : ''}<span>${esc(nameOf(g))}</span></div>`;
}

function cardHtml(g) {
  const src = srcOf(g);
  const run = isRunning(g.id);
  const sub = run ? 'Запущено' : g.missing ? 'Не найдена' : ptOf(g) ? fmtTime(ptOf(g)) : 'Не запускалась';
  return `<div class="card${run ? ' running' : ''}${g.missing ? ' missing' : ''}" data-id="${esc(g.id)}" tabindex="0" draggable="true">
    <div class="cover">
      ${g.coverPath ? `<div class="fallback" style="--h:${hue(nameOf(g))}"><span>${esc(nameOf(g))}</span></div>` : ''}
      ${coverHtml(g)}
      <div class="shade"></div>
      <span class="badge" style="--c:${src.color}">${esc(src.short || src.label)}</span>
      <button class="fav-btn${g.favorite ? ' on' : ''}" data-act="fav" title="Избранное">${g.favorite ? I.heartFill : I.heart}</button>
      ${run ? '<span class="live">Запущено</span>' : `<button class="play-btn" data-act="play">${I.play}Играть</button>`}
    </div>
    <div class="meta"><div class="title" title="${esc(nameOf(g))}">${esc(nameOf(g))}</div><div class="subline">${esc(sub)}</div></div>
  </div>`;
}

function continueHtml() {
  if (ui.view !== 'all' || ui.search) return '';
  const recent = games().filter((g) => !g.hidden && !g.missing && lastOf(g) > Date.now() - 45 * 864e5)
    .sort((a, b) => lastOf(b) - lastOf(a)).slice(0, 4);
  if (recent.length < 2) return '';
  return `<div class="section-label" style="padding-top:4px">Продолжить играть</div><div class="continue">${recent.map((g) => {
    const bg = g.heroPath || g.coverPath;
    return `<div class="cont-card" data-id="${esc(g.id)}" style="${bg ? `background-image:url('${esc(img(bg, g))}')` : `background:linear-gradient(135deg,hsl(${hue(nameOf(g))} 45% 25%),#1a1320)`}">
      <div class="in"><div class="t">${esc(nameOf(g))}</div><div class="m">${isRunning(g.id) ? 'Запущено сейчас' : `${esc(fmtAgo(lastOf(g)))} · ${esc(fmtTime(ptOf(g), 'меньше минуты'))}`}</div></div>
      <button class="play-mini" data-act="play" title="Играть">${I.play}</button></div>`;
  }).join('')}</div><div class="section-label">Все игры</div>`;
}

function emptyHtml() {
  if (!games().length) {
    return `<div class="empty"><div class="big-ico">${I.sparkle}</div><h2>${S.scanning ? 'Ищем ваши игры…' : 'Библиотека пока пуста'}</h2>
      <p>Arcadia находит игры из Steam, Epic Games, GOG, EA app, Ubisoft Connect, Battle.net и BattleState. Игры с торрентов и без лаунчера — добавьте папку, где они лежат, или перетащите .exe прямо в окно.</p>
      <div class="row"><button class="btn primary" data-act="scan">${I.refresh}Найти игры</button><button class="btn" data-act="settings-folders">${I.folder}Указать папки с играми</button><button class="btn" data-act="add">${I.plus}Добавить вручную</button></div></div>`;
  }
  if (ui.search) return `<div class="empty"><div class="big-ico">${I.search}</div><h2>Ничего не найдено</h2><p>По запросу «${esc(ui.search)}» игр нет.</p></div>`;
  if (ui.view.startsWith('col:')) return `<div class="empty"><div class="big-ico">${I.stack}</div><h2>Коллекция пуста</h2><p>Перетащите игру на название коллекции слева или добавьте через правый клик по обложке.</p></div>`;
  if (ui.view === 'favorites') return `<div class="empty"><div class="big-ico">${I.heart}</div><h2>Нет избранных игр</h2><p>Нажмите на сердечко на обложке, чтобы закрепить игру здесь.</p></div>`;
  return `<div class="empty"><div class="big-ico">${I.grid}</div><h2>Здесь пока пусто</h2></div>`;
}

function renderLibrary() {
  const list = viewGames();
  const isCol = ui.view.startsWith('col:');
  const head = `<div class="view-head"><h1>${esc(viewTitle(ui.view))}</h1><span class="sub">${list.length} ${plural(list.length, 'игра', 'игры', 'игр')}</span><span class="grow"></span>
    ${isCol ? `<button class="btn ghost" data-act="rename-col">${I.edit}Переименовать</button><button class="btn ghost danger" data-act="delete-col">${I.trash}Удалить коллекцию</button>` : ''}
    ${ui.view === 'hidden' && list.length ? '<span class="sub">Правый клик → «Показать», чтобы вернуть игру</span>' : ''}</div>`;
  if (!list.length) return head + emptyHtml();
  return head + continueHtml() + `<div class="grid">${list.map(cardHtml).join('')}</div>`;
}

// ---------------- detail ----------------
function renderDetail() {
  const g = S.games[ui.detail];
  if (!g) { ui.detail = null; return renderLibrary(); }
  const src = srcOf(g);
  const run = isRunning(g.id);
  const heroBg = g.heroPath || g.coverPath;
  const year = (String(g.releaseDate || '').match(/\d{4}/) || [])[0];
  const sessions = [...(g.sessions || [])].reverse().slice(0, 8);
  const L = g.launchOverride && (g.launchOverride.path || g.launchOverride.uri) ? g.launchOverride : g.launch || {};
  const how = L.type === 'uri' ? L.uri : `${L.path || ''}${L.args ? ' ' + L.args : ''}`;
  const steamNote = g.source === 'steam' && g.importedPlaytime ? '<div class="sub" style="font-size:12px;color:var(--text-3);margin-top:-14px;margin-bottom:18px">Время в игре учитывает статистику Steam</div>' : '';
  return `<div class="detail">
    <div class="hero${g.heroPath ? '' : ' blur'}" style="${heroBg ? `background-image:url('${esc(img(heroBg, g))}')` : `background:linear-gradient(135deg,hsl(${hue(nameOf(g))} 45% 22%),var(--bg))`}">
      <button class="btn back-btn" data-act="back">${I.back}Назад</button>
    </div>
    <div class="detail-body">
      <div><div class="detail-cover">${g.coverPath ? '' : ''}${coverHtml(g)}</div></div>
      <div>
        <div class="eyebrow"><span class="badge" style="--c:${src.color}">${esc(src.label)}</span>${g.developer ? `<span>${esc(g.developer)}</span>` : ''}${year ? `<span>· ${year}</span>` : ''}</div>
        <h1>${esc(nameOf(g))}</h1>
        ${g.missing ? `<div class="warn-box">${I.alert}<span>Игра не найдена при последнем сканировании — возможно, её удалили или перенесли. Укажите новый путь в «Изменить» или удалите из библиотеки.</span></div>` : ''}
        <div class="actions">
          ${run ? `<span class="running-note"><span class="live" style="position:static">Запущено</span></span>` : `<button class="btn primary big" data-act="play">${I.play}Играть</button>`}
          <button class="icon-btn${g.favorite ? ' on' : ''}" data-act="fav" title="Избранное">${g.favorite ? I.heartFill : I.heart}</button>
          <button class="icon-btn" data-act="edit" title="Изменить">${I.edit}</button>
          <button class="icon-btn" data-act="folder" title="Открыть папку">${I.folder}</button>
          <button class="icon-btn" data-act="more" title="Ещё">${I.more}</button>
        </div>
        <div class="stats">
          <div class="stat"><div class="k">Наиграно</div><div class="v">${esc(fmtTime(ptOf(g), '—'))}</div></div>
          <div class="stat"><div class="k">Последний запуск</div><div class="v">${esc(lastOf(g) ? fmtAgo(lastOf(g)) : '—')}</div></div>
          <div class="stat"><div class="k">Сессий</div><div class="v">${(g.sessions || []).length}</div></div>
          <div class="stat"><div class="k">В библиотеке с</div><div class="v">${g.addedAt ? new Date(g.addedAt).toLocaleDateString('ru-RU') : '—'}</div></div>
        </div>
        ${steamNote}
        ${g.description ? `<p class="desc">${esc(g.description)}</p>` : ''}
        ${(g.genres || []).length ? `<div class="chips">${g.genres.map((x) => `<span class="chip">${esc(x)}</span>`).join('')}</div>` : ''}
        <div class="subhead">Коллекции</div>
        <div class="chips">${S.collections.map((c) => `<button class="chip${(g.collections || []).includes(c.id) ? ' on' : ''}" data-act="toggle-col" data-col="${esc(c.id)}">${esc(c.name)}</button>`).join('')}<button class="chip" data-act="new-col-for">+ Новая коллекция</button></div>
        ${sessions.length ? `<div class="subhead">Последние сессии</div><div class="sessions">${sessions.map((s) => `<div class="session"><span>${new Date(s.start).toLocaleString('ru-RU', { day: 'numeric', month: 'long', hour: '2-digit', minute: '2-digit' })}</span><b>${esc(fmtTime((s.end - s.start) / 1000))}</b></div>`).join('')}</div>` : ''}
        <div class="subhead">Сведения</div>
        <div class="info">
          <div class="k">Источник</div><div class="v">${esc(src.label)}</div>
          ${g.installDir ? `<div class="k">Папка</div><div class="v">${esc(g.installDir)}</div>` : ''}
          <div class="k">Запуск</div><div class="v">${esc(how || '—')}</div>
        </div>
      </div>
    </div>
  </div>`;
}

// ---------------- stats ----------------
function renderStats() {
  const list = games().filter((g) => !g.hidden);
  const total = list.reduce((s, g) => s + ptOf(g), 0);
  const played = list.filter((g) => ptOf(g) > 0).sort((a, b) => ptOf(b) - ptOf(a));
  const top = played.slice(0, 12);
  const max = top.length ? ptOf(top[0]) : 1;
  const days = [];
  const d0 = new Date(); d0.setHours(0, 0, 0, 0);
  for (let i = 13; i >= 0; i--) days.push({ t: d0.getTime() - i * 864e5, s: 0 });
  let week = 0;
  for (const g of list) for (const s of g.sessions || []) {
    const idx = days.findIndex((d) => s.start >= d.t && s.start < d.t + 864e5);
    const dur = (s.end - s.start) / 1000;
    if (idx >= 0) days[idx].s += dur;
    if (s.start > Date.now() - 7 * 864e5) week += dur;
  }
  const dmax = Math.max(...days.map((d) => d.s), 1);
  const bySrc = Object.keys(SOURCES).map((k) => [k, list.filter((g) => g.source === k).reduce((s, g) => s + ptOf(g), 0), list.filter((g) => g.source === k).length]).filter(([, , n]) => n).sort((a, b) => b[1] - a[1]);
  const smax = Math.max(...bySrc.map((x) => x[1]), 1);
  return `<div class="view-head"><h1>Статистика</h1><span class="sub">по всем лаунчерам</span></div>
    <div class="stat-tiles">
      <div class="tile"><div class="k">Игр в библиотеке</div><div class="v">${list.length}</div><div class="s">${played.length} ${plural(played.length, 'запускалась', 'запускались', 'запускались')}</div></div>
      <div class="tile"><div class="k">Всего наиграно</div><div class="v">${esc(fmtTime(total, '0 ч'))}</div><div class="s">≈ ${Math.round(total / 86400)} ${plural(Math.round(total / 86400), 'сутки', 'суток', 'суток')}</div></div>
      <div class="tile"><div class="k">За последние 7 дней</div><div class="v">${esc(fmtTime(week, '0 ч'))}</div><div class="s">учтено Arcadia</div></div>
      <div class="tile"><div class="k">Любимая игра</div><div class="v" title="${esc(top[0] ? nameOf(top[0]) : '')}">${esc(top[0] ? nameOf(top[0]) : '—')}</div><div class="s">${esc(top[0] ? fmtTime(ptOf(top[0])) : '')}</div></div>
    </div>
    <div class="panels">
      <div class="panel"><h3>Больше всего времени</h3>${top.length ? top.map((g) => `<div class="bar-row" data-id="${esc(g.id)}">${g.coverPath ? `<img src="${esc(img(g.coverPath, g))}">` : '<div class="ph"></div>'}<div><div class="nm">${esc(nameOf(g))}</div><div class="bar"><i style="width:${Math.max(2, (ptOf(g) / max) * 100)}%"></i></div></div><div class="tv">${esc(fmtTime(ptOf(g)))}</div></div>`).join('') : '<p style="color:var(--text-3)">Пока нет данных — запустите игру через Arcadia.</p>'}</div>
      <div style="display:flex;flex-direction:column;gap:14px;min-width:0">
        <div class="panel"><h3>Активность за 2 недели</h3><div class="days">${days.map((d) => `<div class="day" title="${new Date(d.t).toLocaleDateString('ru-RU')}: ${esc(fmtTime(d.s, '0 мин'))}"><i class="${d.s ? '' : 'zero'}" style="height:${d.s ? Math.max(4, (d.s / dmax) * 100) : 2}%"></i><span>${new Date(d.t).getDate()}</span></div>`).join('')}</div></div>
        <div class="panel"><h3>По лаунчерам</h3>${bySrc.map(([k, s, n]) => `<div class="src-row"><span>${esc(SOURCES[k].label)}</span><div class="bar" style="--c:${SOURCES[k].color}"><i style="width:${s ? Math.max(2, (s / smax) * 100) : 0}%"></i></div><span class="tv">${s ? esc(fmtTime(s)) : n + ' ' + plural(n, 'игра', 'игры', 'игр')}</span></div>`).join('')}</div>
      </div>
    </div>`;
}

// ---------------- render loop ----------------
function render() {
  document.documentElement.style.setProperty('--card', (S.settings.cardSize || 190) + 'px');
  renderNav();
  const view = $('#view');
  let html;
  if (ui.detail) html = renderDetail();
  else if (ui.view === 'stats') html = renderStats();
  else html = renderLibrary();
  if (html !== ui.lastHtml) {
    const top = view.scrollTop;
    view.innerHTML = html;
    view.scrollTop = top;
    ui.lastHtml = html;
  }
  const scan = $('#btnScan');
  scan.innerHTML = `${I.refresh}${S.scanning ? 'Сканирование…' : 'Обновить'}`;
  scan.classList.toggle('spin', S.scanning);
  scan.disabled = S.scanning;
  if (S.net) {
    const z = S.net.zapret.running, v = S.net.vpn.running;
    $('#pillNet').innerHTML = `${I.shield}<span class="nd ${z ? 'on' : ''}"></span>Zapret<span class="nd ${v ? 'on' : ''}"></span>VPN`;
    $('#pillNet').classList.toggle('active', z || v);
  }
  if (ui.netRefresh) ui.netRefresh();
  const ms = $('#metaStatus');
  ms.classList.toggle('hidden', !S.metaPending);
  ms.textContent = `Загрузка обложек: ${S.metaPending}`;
  if ($('#sort').value !== (S.settings.sort || 'name')) $('#sort').value = S.settings.sort || 'name';
  if (document.activeElement !== $('#size')) $('#size').value = S.settings.cardSize || 190;
}

function go(view) {
  ui.scroll[ui.view] = $('#view').scrollTop;
  ui.view = view;
  ui.detail = null;
  ui.lastHtml = '';
  render();
  $('#view').scrollTop = ui.scroll[view] || 0;
}

function openDetail(id) {
  ui.scroll[ui.view] = $('#view').scrollTop;
  ui.detail = id;
  ui.lastHtml = '';
  render();
  $('#view').scrollTop = 0;
}

function closeDetail() {
  ui.detail = null;
  ui.lastHtml = '';
  render();
  $('#view').scrollTop = ui.scroll[ui.view] || 0;
}

// ---------------- actions ----------------
async function play(id) {
  const g = S.games[id];
  if (!g) return;
  toast(`Запускаем «${nameOf(g)}»…`, 'ok');
  const r = await api.launch(id);
  if (!r || !r.ok) toast((r && r.error) || 'Не удалось запустить игру', 'err');
}

async function patchGame(id, patch) {
  Object.assign(S.games[id], patch); // optimistic
  render();
  await api.updateGame(id, patch);
}

function toggleFav(id) { patchGame(id, { favorite: !S.games[id].favorite }); }

function toggleCollection(id, colId) {
  const cur = new Set(S.games[id].collections || []);
  cur.has(colId) ? cur.delete(colId) : cur.add(colId);
  patchGame(id, { collections: [...cur] });
}

async function newCollection(forGameId) {
  const name = await promptModal('Новая коллекция', '', 'Например: «С друзьями», «Пройти», «Хорроры»');
  if (!name) return;
  const c = await api.createCollection(name);
  if (forGameId && c && c.id) {
    S.collections.push(c);
    toggleCollection(forGameId, c.id);
  }
}

async function addGames() {
  const r = await api.pickGames();
  reportAdd(r);
}

function reportAdd(r) {
  if (!r) return;
  if (r.added && r.added.length) {
    toast(r.added.length === 1 ? 'Игра добавлена' : `Добавлено игр: ${r.added.length}`, 'ok');
    if (r.added.length === 1) setTimeout(() => openDetail(r.added[0]), 300);
  }
  for (const e of r.errors || []) toast(e, 'err');
}

async function removeGame(id) {
  const g = S.games[id];
  const ok = await confirmModal('Удалить из библиотеки?', `«${esc(nameOf(g))}» пропадёт из Arcadia. Сама игра на диске останется${g.source !== 'manual' ? ', и при следующих сканированиях она не будет добавляться снова (это можно сбросить в настройках)' : ''}.`, 'Удалить');
  if (!ok) return;
  if (ui.detail === id) closeDetail();
  await api.removeGame(id);
}

// ---------------- context menu ----------------
function showCtx(id, x, y) {
  const g = S.games[id];
  if (!g) return;
  const cols = S.collections.map((c) => `<button data-c="col" data-col="${esc(c.id)}">${I.stack}${esc(c.name)}${(g.collections || []).includes(c.id) ? `<span class="check">${I.check}</span>` : ''}</button>`).join('');
  const m = $('#ctx');
  m.innerHTML = `
    <button data-c="play">${I.play}Играть</button>
    <button data-c="open">${I.grid}Открыть страницу игры</button>
    <button data-c="fav">${g.favorite ? I.heartFill : I.heart}${g.favorite ? 'Убрать из избранного' : 'В избранное'}</button>
    <hr><div class="sub-title">Коллекции</div>${cols}<button data-c="newcol">${I.plus}Новая коллекция…</button>
    <hr>
    <button data-c="edit">${I.edit}Изменить…</button>
    <button data-c="folder">${I.folder}Открыть папку</button>
    <button data-c="refetch">${I.image}Обновить обложку и описание</button>
    <button data-c="hide">${g.hidden ? I.eye : I.eyeOff}${g.hidden ? 'Показать в библиотеке' : 'Скрыть'}</button>
    <button data-c="remove" class="danger">${I.trash}Удалить из библиотеки</button>`;
  m.classList.remove('hidden');
  const r = m.getBoundingClientRect();
  m.style.left = Math.min(x, innerWidth - r.width - 8) + 'px';
  m.style.top = Math.max(8, Math.min(y, innerHeight - r.height - 8)) + 'px';
  m.onclick = (e) => {
    const b = e.target.closest('button');
    if (!b) return;
    hideCtx();
    const c = b.dataset.c;
    if (c === 'play') play(id);
    else if (c === 'open') openDetail(id);
    else if (c === 'fav') toggleFav(id);
    else if (c === 'col') toggleCollection(id, b.dataset.col);
    else if (c === 'newcol') newCollection(id);
    else if (c === 'edit') editModal(id);
    else if (c === 'folder') api.openFolder(id);
    else if (c === 'refetch') { api.refetch(id); toast('Ищем обложку и описание…', 'ok'); }
    else if (c === 'hide') patchGame(id, { hidden: !g.hidden });
    else if (c === 'remove') removeGame(id);
  };
}
function hideCtx() { $('#ctx').classList.add('hidden'); }

// ---------------- modals ----------------
function openModal({ title, body, footer = '', cls = '', onClose }) {
  const ov = document.createElement('div');
  ov.className = 'overlay';
  ov.innerHTML = `<div class="modal ${cls}"><header><h2>${title}</h2><button class="icon-btn" data-x>${I.x}</button></header>${body}${footer ? `<footer>${footer}</footer>` : ''}</div>`;
  $('#modalRoot').appendChild(ov);
  const close = () => { ov.remove(); onClose && onClose(); };
  ov.addEventListener('mousedown', (e) => { if (e.target === ov) close(); });
  $('[data-x]', ov).onclick = close;
  ov._close = close;
  return { el: ov, close };
}

function promptModal(title, value = '', placeholder = '') {
  return new Promise((resolve) => {
    let done = false;
    const { el, close } = openModal({
      title, cls: 'small',
      body: `<div class="body"><input class="input" id="pm" value="${esc(value)}" placeholder="${esc(placeholder)}" maxlength="60"></div>`,
      footer: '<button class="btn ghost" data-cancel>Отмена</button><button class="btn primary" data-ok>Готово</button>',
      onClose: () => { if (!done) resolve(null); },
    });
    const inp = $('#pm', el);
    setTimeout(() => { inp.focus(); inp.select(); }, 30);
    const ok = () => { const v = inp.value.trim(); if (!v) return; done = true; close(); resolve(v); };
    $('[data-ok]', el).onclick = ok;
    $('[data-cancel]', el).onclick = close;
    inp.onkeydown = (e) => { if (e.key === 'Enter') ok(); };
  });
}

function confirmModal(title, text, okLabel = 'OK') {
  return new Promise((resolve) => {
    let done = false;
    const { el, close } = openModal({
      title, cls: 'small',
      body: `<div class="body" style="color:var(--text-2)">${text}</div>`,
      footer: `<button class="btn ghost" data-cancel>Отмена</button><button class="btn primary" data-ok>${esc(okLabel)}</button>`,
      onClose: () => { if (!done) resolve(false); },
    });
    $('[data-ok]', el).onclick = () => { done = true; close(); resolve(true); };
    $('[data-cancel]', el).onclick = close;
  });
}

function switchHtml(key, checked) {
  return `<label class="switch"><input type="checkbox" data-key="${esc(key)}" ${checked ? 'checked' : ''}><span></span></label>`;
}

function editModal(id) {
  const g = S.games[id];
  if (!g) return;
  const ov = g.launchOverride && (g.launchOverride.path || g.launchOverride.uri) ? g.launchOverride : null;
  const base = g.launch || {};
  const baseHow = base.type === 'uri' ? `через ${srcOf(g).label}` : base.path || '—';
  let mode = ov ? 'custom' : 'default';
  const body = `
    <div class="tabs"><button class="on" data-tab="main">Основное</button><button data-tab="art">Обложка и баннер</button></div>
    <div class="body">
      <div data-pane="main">
        <div class="field"><label>Название</label><input class="input" id="eName" value="${esc(nameOf(g))}" placeholder="${esc(g.name)}"></div>
        <div class="field"><label>Способ запуска</label>
          <div class="seg" id="eMode"><button data-m="default">${g.source === 'manual' || g.source === 'folder' ? 'Найденный файл' : 'Через лаунчер'}</button><button data-m="custom">Свой файл</button></div>
          <div class="hint" id="eDefault">${esc(baseHow)}</div>
          <div id="eCustom" style="margin-top:10px">
            <div class="row"><input class="input" id="eExe" value="${esc(ov ? ov.path : base.path || g.exePath || '')}" placeholder="Путь к .exe"><button class="btn" id="eBrowse">${I.folder}Обзор</button></div>
            <div class="field" style="margin-top:10px;margin-bottom:0"><label>Аргументы запуска</label><input class="input" id="eArgs" value="${esc(ov ? ov.args || '' : '')}" placeholder="например: -windowed -skipintro"></div>
          </div>
        </div>
        <div class="field"><label>Время в игре</label><div class="row"><span style="color:var(--text-2)">${esc(fmtTime(g.playtime, '0 мин'))} учтено Arcadia</span><button class="btn ghost danger" id="eReset" style="margin-left:auto">Сбросить</button></div></div>
      </div>
      <div data-pane="art" class="hidden">
        <div class="field"><label>Обложка (вертикальная, 600×900)</label>
          <div class="img-edit"><div class="prev" id="pCover" style="${g.coverPath ? `background-image:url('${esc(img(g.coverPath, g))}')` : ''}"></div>
            <div><div class="row" style="margin-bottom:10px"><button class="btn" data-pick="cover">${I.image}Выбрать файл</button></div>
              <div class="row"><input class="input" id="uCover" placeholder="или ссылка на картинку https://…"><button class="btn" data-url="cover">Загрузить</button></div></div></div></div>
        <div class="field"><label>Баннер на странице игры (широкий)</label>
          <div class="img-edit wide"><div class="prev wide" id="pHero" style="${g.heroPath ? `background-image:url('${esc(img(g.heroPath, g))}')` : ''}"></div>
            <div class="row"><button class="btn" data-pick="hero">${I.image}Выбрать файл</button><input class="input" id="uHero" placeholder="или ссылка https://…"><button class="btn" data-url="hero">Загрузить</button></div></div></div>
        <div class="field"><label>Найти автоматически по названию</label>
          <div class="row"><input class="input" id="eTerm" value="${esc(nameOf(g))}"><button class="btn" id="eRefetch">${I.search}Найти</button></div>
          <div class="hint">Ищет в Steam${S.settings.sgdbKey ? ' и SteamGridDB' : ' (добавьте ключ SteamGridDB в настройках — тогда найдутся и игры, которых нет в Steam)'}.</div></div>
      </div>
    </div>`;
  const { el, close } = openModal({ title: 'Изменить игру', body, footer: '<button class="btn ghost" data-cancel>Отмена</button><button class="btn primary" data-save>Сохранить</button>' });
  const setMode = (m) => {
    mode = m;
    $$('#eMode button', el).forEach((b) => b.classList.toggle('on', b.dataset.m === m));
    $('#eCustom', el).classList.toggle('hidden', m !== 'custom');
    $('#eDefault', el).classList.toggle('hidden', m === 'custom');
  };
  setMode(mode);
  $('#eMode', el).onclick = (e) => { const b = e.target.closest('button'); if (b) setMode(b.dataset.m); };
  $$('.tabs button', el).forEach((b) => (b.onclick = () => {
    $$('.tabs button', el).forEach((x) => x.classList.toggle('on', x === b));
    $$('[data-pane]', el).forEach((p) => p.classList.toggle('hidden', p.dataset.pane !== b.dataset.tab));
  }));
  $('#eBrowse', el).onclick = async () => { const p = await api.pickFile('exe'); if (p) $('#eExe', el).value = p; };
  $('#eReset', el).onclick = async () => {
    if (await confirmModal('Сбросить время?', 'Время и история сессий этой игры в Arcadia будут обнулены.', 'Сбросить')) { await api.resetPlaytime(id); toast('Время сброшено', 'ok'); }
  };
  const refreshPrev = () => setTimeout(() => {
    const ng = S.games[id];
    $('#pCover', el).style.backgroundImage = ng.coverPath ? `url('${img(ng.coverPath, ng)}')` : '';
    $('#pHero', el).style.backgroundImage = ng.heroPath ? `url('${img(ng.heroPath, ng)}')` : '';
  }, 400);
  $$('[data-pick]', el).forEach((b) => (b.onclick = async () => {
    const p = await api.pickFile('image');
    if (!p) return;
    const r = await api.setImage(id, b.dataset.pick, { file: p });
    if (r && r.ok === false) toast(r.error, 'err'); else refreshPrev();
  }));
  $$('[data-url]', el).forEach((b) => (b.onclick = async () => {
    const inp = b.dataset.url === 'hero' ? $('#uHero', el) : $('#uCover', el);
    const url = inp.value.trim();
    if (!/^https?:\/\//.test(url)) return toast('Вставьте ссылку на изображение', 'err');
    b.disabled = true;
    const r = await api.setImage(id, b.dataset.url, { url });
    b.disabled = false;
    if (r && r.ok === false) toast(r.error, 'err'); else { inp.value = ''; refreshPrev(); }
  }));
  $('#eRefetch', el).onclick = async () => {
    await api.refetch(id, $('#eTerm', el).value.trim());
    toast('Ищем…', 'ok');
    setTimeout(refreshPrev, 2500);
    setTimeout(refreshPrev, 6000);
  };
  $('[data-cancel]', el).onclick = close;
  $('[data-save]', el).onclick = async () => {
    const name = $('#eName', el).value.trim();
    const patch = { customName: name && name !== g.name ? name : '' };
    if (mode === 'custom') {
      const p = $('#eExe', el).value.trim();
      if (!p) return toast('Укажите файл для запуска', 'err');
      patch.launchOverride = { type: /^[a-z][\w+.-]*:\/\//i.test(p) && !/^[a-z]:\\/i.test(p) ? 'uri' : 'exe', path: p, uri: p, args: $('#eArgs', el).value.trim() };
      if (patch.launchOverride.type === 'exe') delete patch.launchOverride.uri; else delete patch.launchOverride.path;
    } else patch.launchOverride = null;
    await patchGame(id, patch);
    close();
  };
}

function settingsModal(tab = 'library') {
  const st = S.settings;
  const srcRows = Object.entries(SOURCES).filter(([k]) => k !== 'manual').map(([k, v]) => {
    const s = (S.scanStatus || {})[k] || {};
    const status = s.error ? `<span class="src-status err">ошибка: ${esc(s.error)}</span>` : s.count !== undefined ? `<span class="src-status ${s.count ? 'ok' : ''}">${s.count ? `найдено: ${s.count}` : 'не найдено'}</span>` : '';
    return `<div class="toggle-row"><div><div class="t"><span class="dot" style="display:inline-block;width:8px;height:8px;border-radius:50%;background:${v.color};margin-right:8px"></span>${esc(v.label)}</div><div class="d">${status}</div></div>${switchHtml('sources.' + k, st.sources[k] !== false)}</div>`;
  }).join('');
  const folders = () => (S.settings.scanFolders || []).map((f, i) => `<div class="folder-item">${I.folder}<span class="p">${esc(f)}</span><button data-rm="${i}" title="Убрать">${I.x}</button></div>`).join('') || '<div class="folder-item" style="color:var(--text-3)">Папки не добавлены</div>';
  const beh = [
    ['closeToTray', 'Сворачивать в трей при закрытии', 'Arcadia продолжит считать время и будет доступна из трея'],
    ['autostart', 'Запускать вместе с Windows', 'Стартует свёрнутой в трей'],
    ['startMinimized', 'Всегда запускать свёрнутой', ''],
    ['minimizeOnLaunch', 'Сворачивать окно при запуске игры', ''],
    ['trackExternal', 'Учитывать игры, запущенные не через Arcadia', 'Время засчитается, даже если игра запущена из Steam или ярлыка'],
    ['scanOnStart', 'Обновлять библиотеку при запуске', ''],
  ].map(([k, t, d]) => `<div class="toggle-row"><div><div class="t">${t}</div>${d ? `<div class="d">${d}</div>` : ''}</div>${switchHtml(k, !!st[k])}</div>`).join('');
  const body = `
    <div class="tabs"><button data-tab="library">Библиотека</button><button data-tab="art">Обложки</button><button data-tab="behavior">Поведение</button><button data-tab="about">Данные</button></div>
    <div class="body">
      <div data-pane="library">
        <div class="field"><label>Папки с играми без лаунчера (торренты, репаки, портативные)</label>
          <div class="folder-list" id="fList">${folders()}</div>
          <button class="btn" id="fAdd">${I.plus}Добавить папку</button>
          <div class="hint">Каждая подпапка считается отдельной игрой, Arcadia сама найдёт нужный .exe и очистит название от тегов вроде [FitGirl Repack] или v1.2.3. Если exe определился неверно — поправьте его в «Изменить».</div></div>
        <div class="field"><label>Лаунчеры</label>${srcRows}<div class="toggle-row"><div><div class="t"><span class="dot" style="display:inline-block;width:8px;height:8px;border-radius:50%;background:${SOURCES.folder.color};margin-right:8px"></span>Сканировать папки из списка выше</div></div>${switchHtml('sources.folder', st.sources.folder !== false)}</div></div>
      </div>
      <div data-pane="art" class="hidden">
        <div class="toggle-row"><div><div class="t">Загружать обложки и описания автоматически</div><div class="d">Из кэша Steam и магазина Steam — ключ не нужен</div></div>${switchHtml('autoFetchMeta', !!st.autoFetchMeta)}</div>
        <div class="field" style="margin-top:16px"><label>Ключ SteamGridDB API (необязательно)</label>
          <div class="row"><input class="input" id="sgdb" value="${esc(st.sgdbKey || '')}" placeholder="вставьте ключ" spellcheck="false"><button class="btn" id="sgdbSave">Сохранить</button></div>
          <div class="hint">Нужен для игр, которых нет в Steam (например, Escape from Tarkov). Бесплатно: <a class="link" id="sgdbLink">steamgriddb.com → Preferences → API</a></div></div>
        <div class="row"><button class="btn" id="fetchMissing">${I.image}Найти недостающие обложки</button><button class="btn ghost" id="refetchAll">${I.refresh}Обновить все</button></div>
      </div>
      <div data-pane="behavior" class="hidden">
        ${beh}
        <div class="toggle-row"><div><div class="t">Горячая клавиша для показа Arcadia</div><div class="d">Нажмите сочетание в поле. Backspace — отключить.</div></div><input class="input" id="hk" style="width:220px;text-align:center" readonly value="${esc(prettyHotkey(st.hotkey))}"></div>
      </div>
      <div data-pane="about" class="hidden">
        <div class="toggle-row"><div><div class="t">Папка с данными</div><div class="d">Библиотека, обложки и журнал работы</div></div><button class="btn" id="openData">${I.folder}Открыть</button></div>
        <div class="toggle-row"><div><div class="t">Вернуть удалённые игры</div><div class="d">Удалённых из библиотеки: ${(st.ignoredIds || []).length}. Они снова появятся после сканирования.</div></div><button class="btn" id="unignore" ${(st.ignoredIds || []).length ? '' : 'disabled'}>Вернуть</button></div>
        <div class="toggle-row"><div><div class="t">Arcadia ${esc(S.version || '')}</div><div class="d">Steam · Epic Games · GOG · EA app · Ubisoft Connect · Battle.net · BattleState · папки</div></div><button class="btn ghost danger" id="quit">Выйти из Arcadia</button></div>
      </div>
    </div>`;
  const { el } = openModal({ title: 'Настройки', body });
  const showTab = (t) => {
    $$('.tabs button', el).forEach((x) => x.classList.toggle('on', x.dataset.tab === t));
    $$('[data-pane]', el).forEach((p) => p.classList.toggle('hidden', p.dataset.pane !== t));
  };
  showTab(tab);
  $$('.tabs button', el).forEach((b) => (b.onclick = () => showTab(b.dataset.tab)));
  el.addEventListener('change', async (e) => {
    const k = e.target.dataset && e.target.dataset.key;
    if (!k) return;
    const v = e.target.checked;
    const patch = k.startsWith('sources.') ? { sources: { [k.slice(8)]: v } } : { [k]: v };
    await api.setSettings(patch);
  });
  const refreshFolders = () => { $('#fList', el).innerHTML = folders(); };
  $('#fAdd', el).onclick = async () => {
    const p = await api.pickFolder();
    if (!p) return;
    const list = [...new Set([...(S.settings.scanFolders || []), p])];
    S.settings.scanFolders = list;
    refreshFolders();
    await api.setSettings({ scanFolders: list });
    toast('Папка добавлена — ищем игры', 'ok');
  };
  $('#fList', el).onclick = async (e) => {
    const b = e.target.closest('[data-rm]');
    if (!b) return;
    const list = (S.settings.scanFolders || []).filter((_, i) => i !== +b.dataset.rm);
    S.settings.scanFolders = list;
    refreshFolders();
    await api.setSettings({ scanFolders: list });
  };
  $('#sgdbSave', el).onclick = async () => { await api.setSettings({ sgdbKey: $('#sgdb', el).value.trim() }); toast('Ключ сохранён', 'ok'); };
  $('#sgdbLink', el).onclick = () => api.openUrl('https://www.steamgriddb.com/profile/preferences/api');
  $('#fetchMissing', el).onclick = () => { api.fetchMissing(); toast('Ищем обложки…', 'ok'); };
  $('#refetchAll', el).onclick = () => { api.refetchAll(); toast('Обновляем обложки для всей библиотеки…', 'ok'); };
  $('#openData', el).onclick = () => api.openData();
  $('#unignore', el).onclick = () => { api.unignore(); toast('Удалённые игры вернутся после сканирования', 'ok'); };
  $('#quit', el).onclick = () => api.quit();
  const hk = $('#hk', el);
  hk.onkeydown = async (e) => {
    e.preventDefault();
    if (e.key === 'Backspace' || e.key === 'Delete') { hk.value = 'Отключено'; await api.setSettings({ hotkey: '' }); return; }
    if (['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) return;
    const parts = [];
    if (e.ctrlKey) parts.push('CommandOrControl');
    if (e.altKey) parts.push('Alt');
    if (e.shiftKey) parts.push('Shift');
    if (!parts.length && !/^F\d+$/.test(e.key)) return toast('Используйте Ctrl, Alt или Shift вместе с клавишей', 'err');
    let key = e.code.startsWith('Key') ? e.code.slice(3) : e.code.startsWith('Digit') ? e.code.slice(5) : e.key.length === 1 ? e.key.toUpperCase() : e.key;
    if (key === ' ') key = 'Space';
    parts.push(key);
    const acc = parts.join('+');
    const r = await api.setSettings({ hotkey: acc });
    hk.value = prettyHotkey(acc);
    if (r && r.hotkeyOk === false) toast('Это сочетание занято другой программой', 'err');
    else toast('Горячая клавиша сохранена', 'ok');
  };
}

function prettyHotkey(h) { return h ? h.replace('CommandOrControl', 'Ctrl').replace(/\+/g, ' + ') : 'Отключено'; }

// ---------------- toasts ----------------
function toast(msg, type = 'ok') {
  const t = document.createElement('div');
  t.className = 'toast ' + type;
  t.innerHTML = `${type === 'err' ? I.alert : I.check}<span>${esc(msg)}</span>`;
  $('#toasts').appendChild(t);
  setTimeout(() => { t.style.transition = 'opacity .3s'; t.style.opacity = '0'; setTimeout(() => t.remove(), 300); }, type === 'err' ? 6000 : 2600);
}

// ---------------- events ----------------
function bind() {
  $('#btnSettings').innerHTML = I.gear;
  $('.search-ico').innerHTML = I.search;
  $('#btnAdd').innerHTML = `${I.plus}Добавить игру`;
  $('#btnAdd').onclick = addGames;
  $('#btnScan').onclick = () => api.scan();
  $('#btnSettings').onclick = () => settingsModal();
  $('#pillNet').onclick = () => netModal();

  let searchT;
  $('#search').addEventListener('input', (e) => {
    clearTimeout(searchT);
    searchT = setTimeout(() => {
      ui.search = e.target.value.trim();
      if (ui.detail) ui.detail = null;
      if (ui.view === 'stats') ui.view = 'all';
      render();
    }, 80);
  });
  $('#sort').onchange = (e) => { S.settings.sort = e.target.value; render(); api.setSettings({ sort: e.target.value }); };
  let sizeT;
  $('#size').addEventListener('input', (e) => {
    S.settings.cardSize = +e.target.value;
    document.documentElement.style.setProperty('--card', e.target.value + 'px');
    clearTimeout(sizeT);
    sizeT = setTimeout(() => api.setSettings({ cardSize: +e.target.value }), 400);
  });

  $('#nav').addEventListener('click', (e) => {
    if (e.target.closest('[data-act="new-col"]')) return newCollection();
    if (e.target.closest('[data-act="net"]')) return netModal();
    const b = e.target.closest('[data-view]');
    if (b) go(b.dataset.view);
  });
  $('#nav').addEventListener('contextmenu', async (e) => {
    const b = e.target.closest('[data-col]');
    if (!b) return;
    e.preventDefault();
    go('col:' + b.dataset.col);
  });
  // drag a game card onto a collection
  $('#nav').addEventListener('dragover', (e) => {
    const b = e.target.closest('[data-col]');
    if (b && e.dataTransfer.types.includes('text/arcadia-id')) { e.preventDefault(); b.classList.add('drop-target'); }
  });
  $('#nav').addEventListener('dragleave', (e) => { const b = e.target.closest('[data-col]'); if (b) b.classList.remove('drop-target'); });
  $('#nav').addEventListener('drop', (e) => {
    const b = e.target.closest('[data-col]');
    const id = e.dataTransfer.getData('text/arcadia-id');
    if (!b || !id) return;
    e.preventDefault();
    b.classList.remove('drop-target');
    if (!(S.games[id].collections || []).includes(b.dataset.col)) toggleCollection(id, b.dataset.col);
    const c = S.collections.find((x) => x.id === b.dataset.col);
    toast(`«${nameOf(S.games[id])}» → ${c ? c.name : ''}`, 'ok');
  });

  const view = $('#view');
  view.addEventListener('click', async (e) => {
    const actEl = e.target.closest('[data-act]');
    const card = e.target.closest('[data-id]');
    const id = card ? card.dataset.id : ui.detail;
    if (actEl) {
      e.stopPropagation();
      const a = actEl.dataset.act;
      if (a === 'play') return play(id);
      if (a === 'fav') return toggleFav(id);
      if (a === 'back') return closeDetail();
      if (a === 'edit') return editModal(id);
      if (a === 'folder') return api.openFolder(id);
      if (a === 'more') { const r = actEl.getBoundingClientRect(); return showCtx(id, r.left, r.bottom + 6); }
      if (a === 'toggle-col') return toggleCollection(id, actEl.dataset.col);
      if (a === 'new-col-for') return newCollection(id);
      if (a === 'scan') return api.scan();
      if (a === 'add') return addGames();
      if (a === 'settings-folders') return settingsModal('library');
      if (a === 'rename-col') {
        const c = S.collections.find((x) => 'col:' + x.id === ui.view);
        const n = c && (await promptModal('Переименовать коллекцию', c.name));
        if (n) api.renameCollection(c.id, n);
        return;
      }
      if (a === 'delete-col') {
        const c = S.collections.find((x) => 'col:' + x.id === ui.view);
        if (c && (await confirmModal('Удалить коллекцию?', `Коллекция «${esc(c.name)}» будет удалена. Игры останутся в библиотеке.`, 'Удалить'))) { await api.deleteCollection(c.id); go('all'); }
        return;
      }
    }
    if (card && !ui.detail) openDetail(id);
  });
  view.addEventListener('dblclick', (e) => {
    const card = e.target.closest('.card');
    if (card && !e.target.closest('[data-act]')) { e.preventDefault(); play(card.dataset.id); }
  });
  view.addEventListener('contextmenu', (e) => {
    const card = e.target.closest('[data-id]');
    const id = card ? card.dataset.id : ui.detail;
    if (!id) return;
    e.preventDefault();
    showCtx(id, e.clientX, e.clientY);
  });
  view.addEventListener('keydown', (e) => {
    const card = e.target.closest('.card');
    if (card && e.key === 'Enter') play(card.dataset.id);
  });
  view.addEventListener('dragstart', (e) => {
    const card = e.target.closest('.card');
    if (!card) return;
    e.dataTransfer.setData('text/arcadia-id', card.dataset.id);
    e.dataTransfer.effectAllowed = 'copy';
    card.classList.add('dragging');
  });
  view.addEventListener('dragend', (e) => { const c = e.target.closest('.card'); if (c) c.classList.remove('dragging'); });

  document.addEventListener('mousedown', (e) => { if (!e.target.closest('#ctx')) hideCtx(); });
  window.addEventListener('blur', hideCtx);
  view.addEventListener('scroll', hideCtx);
  document.addEventListener('keydown', (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'f') { e.preventDefault(); $('#search').focus(); $('#search').select(); return; }
    if (e.key === 'Escape') {
      if (!$('#ctx').classList.contains('hidden')) return hideCtx();
      const ov = $$('.overlay').pop();
      if (ov) return ov._close();
      if (ui.detail) return closeDetail();
      if (document.activeElement === $('#search') && ui.search) { $('#search').value = ''; ui.search = ''; render(); }
    }
    if (e.key === 'F5') { e.preventDefault(); api.scan(); }
  });

  // drag & drop files from Explorer
  let dragDepth = 0;
  const hasFiles = (e) => e.dataTransfer && [...e.dataTransfer.types].includes('Files');
  window.addEventListener('dragenter', (e) => { if (hasFiles(e)) { dragDepth++; $('#drop').classList.remove('hidden'); } });
  window.addEventListener('dragleave', (e) => { if (hasFiles(e) && --dragDepth <= 0) { dragDepth = 0; $('#drop').classList.add('hidden'); } });
  window.addEventListener('dragover', (e) => { if (hasFiles(e)) e.preventDefault(); });
  window.addEventListener('drop', async (e) => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    dragDepth = 0;
    $('#drop').classList.add('hidden');
    const paths = [...e.dataTransfer.files].map((f) => api.pathForFile(f)).filter(Boolean);
    if (paths.length) reportAdd(await api.addFiles(paths));
  });
}

// ---------------- boot ----------------
(async function boot() {
  bind();
  // broken image files: drop the <img> so the generated fallback shows instead
  document.addEventListener('error', (e) => { if (e.target && e.target.tagName === 'IMG') e.target.style.visibility = 'hidden'; }, true);
  api.onState((s) => { S = s; render(); });
  api.onToast((t) => toast(t.msg, t.type));
  api.onOpenNet(() => netModal());
  S = await api.getState();
  render();
})();
