'use strict';
/* Arcadia — game status, filters & list view, random pick, HowLongToBeat, launch profiles, per-game actions,
   downloads (Steam + qBittorrent), hardware monitor, integrations & notification settings, gamepad/TV mode. */

const STATUS = {
  playing: { label: 'Играю', color: '#60a5fa' },
  backlog: { label: 'Хочу пройти', color: '#c084fc' },
  done: { label: 'Пройдено', color: '#4ade80' },
  dropped: { label: 'Брошено', color: '#f87171' },
};
const statusChip = (g) => (g.status && STATUS[g.status] ? `<span class="st-chip" style="--c:${STATUS[g.status].color}">${STATUS[g.status].label}</span>` : '');

// ---------------- filters ----------------
function curFilters() {
  const f = S.settings.filters || {};
  return { status: f.status || '', genres: f.genres || [], tags: f.tags || [] };
}
const gTags = (g) => [...new Set([...(g.genres || []), ...(g.tags || [])])];
function applyFilters(list) {
  const f = curFilters();
  if (f.status) list = list.filter((g) => (f.status === 'none' ? !g.status : g.status === f.status));
  if (f.genres.length) list = list.filter((g) => f.genres.every((t) => gTags(g).includes(t)));
  return list;
}
function filtersActive() {
  const f = curFilters();
  return !!(f.status || f.genres.length);
}
function setFilters(patch) {
  const f = { ...curFilters(), ...patch };
  S.settings.filters = f;
  ui.lastHtml = '';
  render();
  api.setSettings({ filters: f });
}
function filterBarHtml(listForCounts) {
  if (!ui.filtersOpen) return '';
  const f = curFilters();
  const counts = {};
  for (const g of listForCounts) for (const t of gTags(g)) counts[t] = (counts[t] || 0) + 1;
  const tags = Object.entries(counts).sort((a, b) => b[1] - a[1]).slice(0, 40);
  const st = ['', 'playing', 'backlog', 'done', 'dropped', 'none'];
  const stLabel = (k) => (k === '' ? 'Любой' : k === 'none' ? 'Без статуса' : STATUS[k].label);
  return `<div class="filter-bar">
    <div class="fb-row"><span class="fb-k">Статус</span><div class="chips">${st.map((k) => `<button class="chip${f.status === k ? ' on' : ''}" data-act="f-status" data-v="${k}">${stLabel(k)}</button>`).join('')}</div></div>
    <div class="fb-row"><span class="fb-k">Жанры и теги</span><div class="chips">${tags.length ? tags.map(([t, n]) => `<button class="chip${f.genres.includes(t) ? ' on' : ''}" data-act="f-tag" data-v="${esc(t)}">${esc(t)} <span class="cnt">${n}</span></button>`).join('') : '<span class="hint" style="margin:0">Жанры появятся после загрузки описаний; свои теги — в «Изменить» → «Теги»</span>'}</div></div>
    ${filtersActive() ? `<div class="fb-row"><span class="fb-k"></span><button class="btn sm ghost" data-act="f-reset">${I.x}Сбросить фильтры</button></div>` : ''}
  </div>`;
}

// ---------------- list view ----------------
function rowHtml(g) {
  const src = srcOf(g);
  const run = isRunning(g.id);
  const p = dlOf(g);
  const c = coverSrc(g);
  const h = g.hltb && g.hltb.main ? `${g.hltb.main} ч` : '—';
  return `<div class="lrow${inst(g) ? '' : ' uninst'}${g.missing ? ' missing' : ''}" data-id="${esc(g.id)}">
    <div class="lr-cover">${c ? `<img src="${esc(c)}" loading="lazy" draggable="false">` : `<div class="fallback" style="--h:${hue(nameOf(g))}"></div>`}</div>
    <div class="lr-name"><div class="title">${esc(nameOf(g))}${g.favorite ? ` <span class="lr-fav">${I.heartFill}</span>` : ''}</div><div class="subline">${p ? esc(dlText(p, true)) : run ? 'Запущено' : !inst(g) ? 'Не установлена' : g.missing ? 'Не найдена' : esc(fmtAgo(lastOf(g)))}</div></div>
    <span class="badge static" style="--c:${src.color}">${esc(src.short || src.label)}</span>
    <div class="lr-st">${statusChip(g)}</div>
    <div class="lr-num" title="Наиграно">${esc(fmtTime(ptOf(g), '—'))}</div>
    <div class="lr-num" title="Сколько проходить (основной сюжет)">${h}</div>
    <div class="lr-act">${run ? '<span class="live static">Запущено</span>' : inst(g) ? `<button class="btn sm primary" data-act="play">${I.play}Играть</button>` : p ? '' : `<button class="btn sm" data-act="play">${I.download}Установить</button>`}</div>
  </div>`;
}
function itemsHtml(list) {
  // the header row lives outside the keyed list so patchKeyed only sees game rows
  return S.settings.view === 'list'
    ? `<div class="lhead"><span></span><span>Название</span><span></span><span>Статус</span><span>Наиграно</span><span>Пройти</span><span></span></div><div class="glist">${list.map(rowHtml).join('')}</div>`
    : `<div class="grid">${list.map(cardHtml).join('')}</div>`;
}
function headTools() {
  const n = filtersActive();
  return `<div class="head-tools">
    <button class="icon-btn sm${ui.filtersOpen || n ? ' on' : ''}" data-act="filters" title="Фильтры по статусу, жанрам и тегам">${I.filter}${n ? '<i class="dotm"></i>' : ''}</button>
    <div class="seg mini"><button data-act="vmode" data-v="grid" class="${S.settings.view !== 'list' ? 'on' : ''}" title="Обложки">${I.grid}</button><button data-act="vmode" data-v="list" class="${S.settings.view === 'list' ? 'on' : ''}" title="Список">${I.list}</button></div>
  </div>`;
}

// ---------------- random game ----------------
function randomModal() {
  const pools = {
    installed: () => games().filter((g) => inst(g) && !g.hidden && !g.missing),
    backlog: () => games().filter((g) => g.status === 'backlog' && !g.hidden),
    forgotten: () => games().filter((g) => inst(g) && !g.hidden && !g.missing && Date.now() - lastOf(g) > 30 * 864e5),
    view: () => viewGames().filter((g) => !g.missing),
  };
  let mode = ui.randMode || 'installed';
  let cur = null;
  const roll = () => {
    const list = pools[mode]().filter((g) => !cur || g.id !== cur.id || pools[mode]().length < 2);
    cur = list.length ? list[Math.floor(Math.random() * list.length)] : null;
    paint();
  };
  const { el, close } = openModal({ title: `${I.dice}<span>Во что поиграть?</span>`, cls: 'rand-modal', body: '<div class="body" id="rbody"></div>' });
  const paint = () => {
    const g = cur;
    $('#rbody', el).innerHTML = `<div class="seg wide-seg" style="margin-bottom:16px">${[['installed', 'Установленные'], ['backlog', 'Хочу пройти'], ['forgotten', 'Давно не запускал'], ['view', 'Из текущего раздела']].map(([k, l]) => `<button data-m="${k}" class="${mode === k ? 'on' : ''}">${l}</button>`).join('')}</div>
      ${g ? `<div class="rand-card"><div class="rc-cover">${coverHtml(g)}</div><div class="rc-info"><div class="eyebrow" style="margin:0"><span class="badge static" style="--c:${srcOf(g).color}">${esc(srcOf(g).label)}</span>${statusChip(g)}</div>
        <h2>${esc(nameOf(g))}</h2><div class="rc-meta">${ptOf(g) ? `Наиграно ${esc(fmtTime(ptOf(g)))}` : 'Ещё не запускали'}${lastOf(g) ? ` · ${esc(fmtAgo(lastOf(g)))}` : ''}${g.hltb && g.hltb.main ? ` · пройти ≈ ${g.hltb.main} ч` : ''}</div>
        ${g.description ? `<p class="desc" style="margin:10px 0 0;font-size:13.5px">${esc(g.description.slice(0, 260))}${g.description.length > 260 ? '…' : ''}</p>` : ''}
        <div class="row" style="margin-top:16px"><button class="btn primary" data-r="play">${inst(g) ? I.play + 'Играть' : I.download + 'Установить'}</button><button class="btn" data-r="again">${I.dice}Другая</button><button class="btn ghost" data-r="open">Открыть страницу</button></div></div></div>`
        : '<div class="empty-mini">В этой подборке нет игр</div>'}`;
  };
  el.addEventListener('click', (e) => {
    const m = e.target.closest('[data-m]');
    if (m) { mode = ui.randMode = m.dataset.m; cur = null; return roll(); }
    const r = e.target.closest('[data-r]');
    if (!r || !cur) return;
    if (r.dataset.r === 'again') roll();
    else if (r.dataset.r === 'play') { close(); play(cur.id); }
    else if (r.dataset.r === 'open') { close(); openDetail(cur.id); }
  });
  roll();
}

// ---------------- HowLongToBeat ----------------
function hltbHtml(g) {
  const h = g.hltb;
  const busy = ui.hltbBusy === g.id;
  if (h && (h.main || h.plus || h.all)) {
    const cell = (k, l) => `<div class="hl"><div class="k">${l}</div><div class="v">${h[k] ? `${h[k]} ч` : '—'}</div></div>`;
    const left = h.main && ptOf(g) ? Math.max(0, Math.round(h.main - ptOf(g) / 3600)) : null;
    return `<div class="subhead">Сколько проходить ${h.manual ? '<span class="muted-note">· указано вручную</span>' : `<a class="link" data-act="hltb-open" style="text-transform:none;letter-spacing:0">HowLongToBeat</a>`}</div>
      <div class="hltb">${cell('main', 'Сюжет')}${cell('plus', 'Сюжет + доп.')}${cell('all', 'На 100%')}
      <div class="hl">${left !== null ? `<div class="k">Осталось по сюжету</div><div class="v">${left ? `≈ ${left} ч` : 'пройдено?'}</div>` : ''}</div></div>
      <div class="row" style="margin-top:8px"><button class="btn sm ghost" data-act="hltb-refresh" ${busy ? 'disabled' : ''}>${I.refresh}Обновить</button><button class="btn sm ghost" data-act="hltb-manual">${I.edit}Указать вручную</button></div>`;
  }
  return `<div class="subhead">Сколько проходить</div><div class="row">${busy ? `<span class="hint" style="margin:0">${I.refresh} Ищем на HowLongToBeat…</span>` : `<button class="btn sm" data-act="hltb-refresh">${I.search}Узнать на HowLongToBeat</button>`}<button class="btn sm ghost" data-act="hltb-manual">${I.edit}Указать вручную</button>${ui.hltbErr && ui.hltbErr.id === g.id ? `<span class="hint" style="margin:0;color:var(--warn)">${esc(ui.hltbErr.msg)}</span>` : ''}</div>`;
}
async function hltbFetch(id, silent) {
  ui.hltbBusy = id;
  ui.lastHtml = '';
  render();
  const r = await api.hltb(id);
  ui.hltbBusy = null;
  if (!r || r.ok === false) { let msg = (r && r.error) || 'Не удалось'; if (/sending request|connect|timed? ?out|dns/i.test(msg)) msg = 'Нет связи с HowLongToBeat — попробуйте позже или укажите вручную'; ui.hltbErr = { id, msg }; if (!silent) toast(ui.hltbErr.msg, 'err'); }
  ui.lastHtml = '';
  render();
}
async function hltbManual(id) {
  const g = S.games[id];
  const v = await promptModal('Сколько часов на прохождение (сюжет)?', g.hltb && g.hltb.main ? String(g.hltb.main) : '', 'например: 25');
  if (v === null) return;
  const n = parseFloat(String(v).replace(',', '.'));
  if (!(n > 0)) return toast('Введите число часов', 'err');
  patchGame(id, { hltb: { main: n, manual: true } });
}

// ---------------- launch profiles ----------------
function playMenu(id, x, y) {
  const g = S.games[id];
  const prof = g.profiles || [];
  const def = g.defaultProfile ?? -1;
  const m = $('#ctx');
  m.innerHTML = `<div class="sub-title">Способ запуска</div>
    <button data-p="-1">${I.play}Обычный запуск${def === -1 ? `<span class="check">${I.check}</span>` : ''}</button>
    ${prof.map((p, i) => `<button data-p="${i}">${I.play}${esc(p.name || 'Профиль ' + (i + 1))}${def === i ? `<span class="check">${I.check}</span>` : ''}</button>`).join('')}
    <hr><button data-p="edit">${I.edit}Настроить профили…</button>`;
  m.classList.remove('hidden');
  const r = m.getBoundingClientRect();
  m.style.left = Math.min(x, innerWidth - r.width - 8) + 'px';
  m.style.top = Math.max(8, Math.min(y, innerHeight - r.height - 8)) + 'px';
  m.onclick = (e) => {
    const b = e.target.closest('[data-p]');
    if (!b) return;
    hideCtx();
    if (b.dataset.p === 'edit') return editModal(id, 'profiles');
    play(id, Number(b.dataset.p));
  };
}

function profilesPaneHtml(g) {
  const prof = g.profiles || [];
  return `<div class="hint" style="margin:0 0 12px">Несколько способов запустить одну игру: с модами и без, с разными параметрами, через другой exe. Выбор — стрелкой рядом с кнопкой «Играть».</div>
    <div id="profList">${prof.map((p, i) => profileRow(p, i)).join('')}</div>
    <button class="btn" data-pa="add">${I.plus}Добавить профиль</button>
    <div class="field" style="margin-top:16px"><label>По умолчанию для кнопки «Играть»</label><select class="input" id="profDef"><option value="-1">Обычный запуск</option>${prof.map((p, i) => `<option value="${i}" ${(g.defaultProfile ?? -1) === i ? 'selected' : ''}>${esc(p.name || 'Профиль ' + (i + 1))}</option>`).join('')}</select></div>`;
}
function profileRow(p, i) {
  return `<div class="prof" data-i="${i}">
    <div class="row"><input class="input" data-k="name" value="${esc(p.name || '')}" placeholder="Название, например «С модами»"><button class="icon-btn sm" data-pa="del" title="Удалить">${I.trash}</button></div>
    <div class="row"><input class="input" data-k="path" value="${esc(p.path || p.uri || '')}" placeholder="Путь к .exe или ссылка steam://…"><button class="btn sm" data-pa="browse">${I.folder}Обзор</button></div>
    <input class="input" data-k="args" value="${esc(p.args || '')}" placeholder="Аргументы: -dx11 -skipintro">
  </div>`;
}
function collectProfiles(el) {
  return $$('.prof', el).map((r) => {
    const v = (k) => $(`[data-k="${k}"]`, r).value.trim();
    const path = v('path');
    return { name: v('name'), path, args: v('args'), type: /^[a-z][\w+.-]*:\/\//i.test(path) && !/^[a-z]:\\/i.test(path) ? 'uri' : 'exe', uri: path };
  }).filter((p) => p.path);
}

// ---------------- per-game actions ----------------
function actionsPaneHtml(g) {
  const a = g.actions || {};
  const sel = (k, cur) => `<div class="seg">${[['', 'Не трогать'], ['on', 'Включить'], ['off', 'Выключить']].map(([v, l]) => `<button data-ak="${k}" data-v="${v}" class="${(cur || '') === v ? 'on' : ''}">${l}</button>`).join('')}</div>`;
  return `<div class="hint" style="margin:0 0 6px">Выполняется при запуске игры через Arcadia. После выхода из игры всё возвращается как было.</div>
    <div class="toggle-row"><div><div class="t">Zapret</div><div class="d">Например, включить для голоса в Discord во время игры</div></div>${sel('zapret', a.zapret)}</div>
    <div class="toggle-row"><div><div class="t">VPN</div><div class="d">Например, выключить, чтобы не рос пинг</div></div>${sel('vpn', a.vpn)}</div>
    <div class="toggle-row"><div><div class="t">Схема питания «Высокая производительность»</div><div class="d">На время игры</div></div><label class="switch"><input type="checkbox" id="aPower" ${a.power ? 'checked' : ''}><span></span></label></div>
    <div class="toggle-row"><div><div class="t">Вернуть всё после выхода</div></div><label class="switch"><input type="checkbox" id="aRestore" ${a.restore !== false ? 'checked' : ''}><span></span></label></div>
    <div class="field" style="margin-top:14px"><label>Запустить перед игрой</label><div class="row"><input class="input" id="aBefore" value="${esc((a.before && a.before.path) || '')}" placeholder="Программа: MSI Afterburner, Discord, скрипт .bat…"><button class="btn sm" data-pa="browse-before">${I.folder}Обзор</button></div></div>
    <div class="field"><label>Запустить после выхода из игры</label><div class="row"><input class="input" id="aAfter" value="${esc((a.after && a.after.path) || '')}" placeholder="Необязательно"><button class="btn sm" data-pa="browse-after">${I.folder}Обзор</button></div></div>`;
}
function collectActions(el, state) {
  return {
    zapret: state.zapret || '', vpn: state.vpn || '', power: $('#aPower', el).checked, restore: $('#aRestore', el).checked,
    before: { path: $('#aBefore', el).value.trim() }, after: { path: $('#aAfter', el).value.trim() },
  };
}

// ---------------- tags ----------------
function tagsPaneHtml(g) {
  const all = [...new Set(games().flatMap((x) => x.tags || []))].sort();
  return `<div class="field"><label>Свои теги</label><input class="input" id="eTags" value="${esc((g.tags || []).join(', '))}" placeholder="через запятую: кооп, на вечер, с модами"></div>
    ${all.length ? `<div class="chips">${all.map((t) => `<button class="chip" data-tag="${esc(t)}">${esc(t)}</button>`).join('')}</div>` : ''}
    ${(g.genres || []).length ? `<div class="hint">Жанры из магазина: ${esc(g.genres.join(', '))}</div>` : ''}`;
}

// ---------------- downloads view ----------------
function renderDownloads() {
  const steam = games().filter((g) => S.installs && S.installs[g.id]);
  const T = (S.extras && S.extras.torrents) || [];
  const terr = S.extras && S.extras.torrentError;
  const tOn = S.settings.torrent && S.settings.torrent.enabled;
  const st = (x) => ({ downloading: 'Загрузка', stalledDL: 'Нет источников', metaDL: 'Получение данных', pausedDL: 'Пауза', stoppedDL: 'Пауза', queuedDL: 'В очереди', checkingDL: 'Проверка', forcedDL: 'Загрузка', uploading: 'Раздача', stalledUP: 'Скачано', pausedUP: 'Скачано', stoppedUP: 'Скачано', queuedUP: 'Скачано', forcedUP: 'Раздача', error: 'Ошибка', missingFiles: 'Нет файлов' }[x] || x);
  return `<div class="view-head"><h1>Загрузки</h1><span class="sub">${steam.length + T.filter((t) => t.progress < 1).length} активно</span></div>
    <div class="section-label">Steam</div>
    ${steam.length ? `<div class="grid">${steam.map(cardHtml).join('')}</div>` : '<div class="dl-empty">Steam ничего не скачивает</div>'}
    <div class="section-label sect">${I.download}Торренты (qBittorrent)</div>
    ${!tOn ? `<div class="dl-empty">Подключите qBittorrent, чтобы видеть загрузки репаков здесь, а готовые игры сразу появлялись в библиотеке. <a class="link" data-act="settings-integrations">Подключить</a></div>`
      : terr ? `<div class="dl-empty warn">${esc(terr)} · <a class="link" data-act="settings-integrations">настройки</a></div>`
      : T.length ? `<div class="tlist">${T.map((t) => `<div class="trow"><div class="tname" title="${esc(t.path)}">${esc(t.name)}${t.inLibrary ? '<span class="chip on" style="margin-left:8px;padding:1px 8px">в папке библиотеки</span>' : ''}</div>
          <div class="dl-bar"><i style="width:${(t.progress * 100).toFixed(1)}%"></i></div>
          <div class="tmeta"><span>${(t.progress * 100).toFixed(1)}% · ${esc(st(t.state))}</span><span>${t.progress < 1 && t.speed ? `${fmtBytes(t.speed)}/с` : ''}${t.progress < 1 && t.eta > 0 && t.eta < 8640000 ? ` · осталось ${esc(fmtEta(t.eta))}` : ''} · ${fmtBytes(t.size)}</span></div></div>`).join('')}</div>`
      : '<div class="dl-empty">Активных торрентов нет</div>'}
    ${tOn && T.some((t) => !t.inLibrary) ? '<div class="hint" style="padding:0 30px">Чтобы скачанные игры сами добавлялись в библиотеку, сохраняйте торренты в папку, которая указана в «Настройки → Библиотека».</div>' : ''}`;
}

// ---------------- hardware monitor ----------------
ui.mon = ui.mon || { data: null, hist: {}, timer: null, err: '' };
function monPoll(on) {
  clearInterval(ui.mon.timer);
  ui.mon.timer = null;
  if (!on) return;
  const tick = async () => {
    if (ui.view !== 'monitor' || ui.detail || document.hidden) return;
    const r = await api.x('monitor-data');
    if (r && r.ok) {
      ui.mon.data = r.hardware;
      ui.mon.fps = r.fps || null;
      ui.mon.err = r.note || '';
      for (const hw of r.hardware) for (const [grp, list] of Object.entries(hw.groups)) for (const s of list) {
        const k = `${hw.name}|${grp}|${s.name}`;
        const v = parseFloat(String(s.value).replace(',', '.'));
        if (!isNaN(v)) { const h = (ui.mon.hist[k] = ui.mon.hist[k] || []); h.push(v); if (h.length > 90) h.shift(); }
      }
    } else { ui.mon.err = (r && r.error) || ''; }
    ui.lastHtml = '';
    render();
  };
  tick();
  ui.mon.timer = setInterval(tick, 1000);
}
function spark(arr, color) {
  if (!arr || arr.length < 2) return '';
  const max = Math.max(...arr, 1), min = Math.min(...arr, 0);
  const pts = arr.map((v, i) => `${(i / (arr.length - 1)) * 100},${28 - ((v - min) / (max - min || 1)) * 26}`).join(' ');
  return `<svg class="spark" viewBox="0 0 100 30" preserveAspectRatio="none"><polyline points="${pts}" fill="none" stroke="${color}" stroke-width="1.6" vector-effect="non-scaling-stroke"/></svg>`;
}
const MON_PICK = {
  cpu: [['Temperatures', /package|tctl|tdie|core \(|cpu/i, 'Температура'], ['Load', /total/i, 'Загрузка'], ['Clocks', /core #1|cores? \(average\)|core/i, 'Частота'], ['Powers', /package|cpu/i, 'Мощность']],
  gpu: [['Temperatures', /core|gpu/i, 'Температура'], ['Temperatures', /hot ?spot/i, 'Hotspot'], ['Load', /core|gpu/i, 'Загрузка'], ['Clocks', /core/i, 'Частота ядра'], ['Clocks', /memory/i, 'Частота памяти'], ['Data', /memory used|d3d dedicated memory used/i, 'Видеопамять'], ['SmallData', /memory used|d3d dedicated memory used/i, 'Видеопамять'], ['Powers', /package|board|gpu/i, 'Мощность'], ['Fans', /./, 'Вентилятор']],
  ram: [['Load', /memory/i, 'Загрузка'], ['Data', /memory used|used memory/i, 'Занято'], ['Data', /memory available|available memory/i, 'Свободно']],
  disk: [['Temperatures', /./, 'Температура'], ['Load', /used space/i, 'Занято места'], ['Throughput', /read/i, 'Чтение'], ['Throughput', /write/i, 'Запись']],
};
const MON_LABEL = { cpu: 'Процессор', gpu: 'Видеокарта', ram: 'Память', dimm: 'Модуль памяти', disk: 'Диск', board: 'Материнская плата', net: 'Сеть', other: 'Устройство' };
const MON_KINDS = [['cpu', 'Процессор'], ['gpu', 'Видеокарта'], ['ram', 'Память'], ['dimm', 'Модули памяти'], ['disk', 'Диски'], ['board', 'Мат. плата'], ['net', 'Сеть']];
// Windows lists every virtual network adapter (WFP / QoS filters, Wi-Fi Direct "Подключение по локальной сети*", VPN taps…)
const VIRTUAL_NET = /-WFP|QoS|Filter Driver|Native WiFi|Virtual|WAN Miniport|Teredo|isatap|Loopback|Pseudo|Kernel Debug|Bluetooth|\*\s*\d+|TAP-|Wintun|vEthernet|Hyper-V|VMware|VirtualBox/i;
const monCfg = () => ({ hiddenKinds: [], hidden: [], showVirtual: false, ...(S.settings.monitor || {}) });
function setMon(patch) {
  S.settings.monitor = { ...monCfg(), ...patch };
  ui.lastHtml = '';
  render();
  api.setSettings({ monitor: patch });
}
const monVisible = (h, c = monCfg()) => !c.hiddenKinds.includes(h.kind) && !c.hidden.includes(h.name) && (c.showVirtual || h.kind !== 'net' || !VIRTUAL_NET.test(h.name));
function monFilterHtml(hw) {
  const c = monCfg();
  const count = (k) => hw.filter((h) => h.kind === k).length;
  const virt = hw.filter((h) => h.kind === 'net' && VIRTUAL_NET.test(h.name)).length;
  const hiddenN = hw.filter((h) => !monVisible(h, c)).length;
  return `<div class="mon-filter"><span class="op-k" style="margin:0">Показывать</span>
    ${MON_KINDS.filter(([k]) => count(k)).map(([k, l]) => `<button class="chip${c.hiddenKinds.includes(k) ? '' : ' on'}" data-act="mon-kind" data-v="${k}">${l} <span class="cnt">${count(k)}</span></button>`).join('')}
    ${virt ? `<button class="chip${c.showVirtual ? ' on' : ''}" data-act="mon-virtual" title="Фильтры WFP/QoS, Wi-Fi Direct, адаптеры VPN и виртуальных машин">Виртуальные адаптеры <span class="cnt">${virt}</span></button>` : ''}
    <span class="grow"></span>
    ${c.hidden.length ? `<button class="link-btn" data-act="mon-unhide">вернуть скрытые вручную (${c.hidden.length})</button>` : ''}
    ${hiddenN ? `<span class="muted-note">скрыто ${hiddenN}</span>` : ''}</div>`;
}
const OSD_ITEMS = [['fps', 'FPS'], ['frametime', 'Время кадра'], ['low1', '1% low'], ['cpu', 'Загрузка CPU'], ['cputemp', 'Температура CPU'], ['cpuclk', 'Частота CPU'],
  ['gpu', 'Загрузка GPU'], ['gputemp', 'Температура GPU'], ['gpuclk', 'Частота GPU'], ['gpupower', 'Мощность GPU'], ['vram', 'Видеопамять'], ['ram', 'Память'], ['session', 'Игра и время сессии'], ['clock', 'Часы']];
const osdCfg = () => ({ enabled: true, autoShow: true, hotkey: 'Shift+F12', position: 'tl', scale: 1, opacity: 0.55, interval: 1000, items: [], mangohud: false, linuxWindow: false, ...(S.settings.osd || {}) });
function setOsd(patch) {
  S.settings.osd = { ...osdCfg(), ...patch };
  ui.lastHtml = '';
  render();
  api.setSettings({ osd: patch });
}

function osdPanelHtml() {
  const o = osdCfg();
  const vis = S.extras && S.extras.osd && S.extras.osd.visible;
  const mh = S.extras && S.extras.osd && S.extras.osd.mangohud;
  const lin = isLinux();
  const corner = (k, l) => `<button data-act="osd-set" data-k="position" data-v="${k}" class="${o.position === k ? 'on' : ''}" title="${l}"><i class="corner ${k}"></i></button>`;
  return `<div class="osd-panel">
    <div class="op-head"><div><h3>${I.tv}Оверлей в игре</h3><div class="d">${lin
      ? 'В играх, которые запускает Arcadia, показатели рисует MangoHud прямо внутри игры (Vulkan, OpenGL, DXVK под Wine/Proton). Окно-оверлей Arcadia — для игр в окне.'
      : 'Прозрачная панель поверх игры: FPS, кадр, температуры и загрузка. Ничего не внедряет в игру — безопасно с античитами. Видна в оконном и «полноэкранном без рамки» режимах; в эксклюзивном полноэкранном её закрывает игра.'}</div></div>
      ${switchHtml('osd.enabled', o.enabled !== false)}</div>
    ${o.enabled !== false ? `
    <div class="op-grid">
      <div class="op-col">
        <div class="op-k">Что показывать</div>
        <div class="chips">${OSD_ITEMS.map(([k, l]) => `<button class="chip${o.items.includes(k) ? ' on' : ''}" data-act="osd-item" data-v="${k}">${l}</button>`).join('')}</div>
      </div>
      <div class="op-col narrow">
        <div class="op-k">Угол экрана</div>
        <div class="corners">${corner('tl', 'Слева сверху')}${corner('tr', 'Справа сверху')}${corner('bl', 'Слева снизу')}${corner('br', 'Справа снизу')}</div>
        <div class="op-k">Обновлять</div>
        <div class="seg mini-seg">${[[500, '0,5 с'], [1000, '1 с'], [2000, '2 с']].map(([v, l]) => `<button data-act="osd-set-num" data-k="interval" data-v="${v}" class="${Number(o.interval || 1000) === v ? 'on' : ''}">${l}</button>`).join('')}</div>
        <div class="op-k">Размер · фон</div>
        <div class="row"><select class="input" data-osd-change="scale">${[0.8, 1, 1.25, 1.5, 2].map((v) => `<option value="${v}" ${Number(o.scale) === v ? 'selected' : ''}>${Math.round(v * 100)}%</option>`).join('')}</select>
          <input type="range" min="0" max="0.9" step="0.05" value="${o.opacity}" data-osd-change="opacity" title="Прозрачность фона"></div>
      </div>
    </div>
    ${!lin || o.linuxWindow ? `<div class="toggle-row"><div><div class="t">Показывать при запуске игры</div><div class="d">И прятать после выхода. ${lin ? '' : 'Если датчики запускаются без запроса UAC (ниже), они включатся сами.'}</div></div>${switchHtml('osd.autoShow', o.autoShow !== false)}</div>` : ''}
    <div class="toggle-row"><div><div class="t">Горячая клавиша</div><div class="d">Показать / скрыть в любой момент${lin ? ' (на Wayland глобальные клавиши могут не работать; в MangoHud та же клавиша переключает его панель)' : ''}</div></div>
      <div class="row"><input class="input hk-input" id="osdHk" value="${esc(o.hotkey || '')}" placeholder="нажмите сочетание" readonly><button class="btn sm" data-act="osd-hk-clear" title="Без клавиши">${I.x}</button></div></div>
    ${lin ? `
      <div class="toggle-row"><div><div class="t">MangoHud в играх, запущенных через Arcadia</div><div class="d">${mh ? 'MangoHud установлен' : 'Не найден — установите пакет <code>mangohud</code> (apt, dnf, pacman или Flatpak)'}. Для игр Steam добавьте в «Свойства → Параметры запуска»: <code>mangohud %command%</code>; в Heroic и Lutris — переключатель MangoHud в настройках игры.</div></div>${switchHtml('osd.mangohud', !!o.mangohud)}</div>
      <div class="toggle-row"><div><div class="t">Окно-оверлей Arcadia</div><div class="d">Для игр в окне (X11). Поверх полноэкранных игр Linux его не пускает — там работает MangoHud</div></div>${switchHtml('osd.linuxWindow', !!o.linuxWindow)}</div>` : ''}
    <div class="row" style="margin-top:12px">${!lin || o.linuxWindow ? `<button class="btn ${vis ? '' : 'primary'}" data-act="osd-toggle">${vis ? I.x + 'Скрыть оверлей' : I.tv + 'Показать сейчас'}</button>` : ''}
      ${lin ? `<button class="btn ghost" data-act="mangohud-copy">${I.list}Скопировать настройки MangoHud</button>` : ''}</div>` : ''}
  </div>`;
}

function renderMonitor() {
  const m = (S.extras && S.extras.monitor) || {};
  const lin = isLinux();
  const head = `<div class="view-head"><h1>Мониторинг</h1><span class="sub">${lin ? 'датчики системы' : 'модуль на основе LibreHardwareMonitor и PresentMon'}</span><span class="grow"></span>
    ${!lin && m.installed ? `${m.running ? `<button class="btn ghost" data-act="mon-stop">${I.power}Остановить датчики</button>` : ''}<button class="btn ghost" data-act="mon-folder">${I.folder}Папка модуля</button>` : ''}</div>`;
  const panel = osdPanelHtml();
  let body = '';
  if (!lin && !m.installed) {
    body = `<div class="empty"><div class="big-ico">${I.cpu}</div><h2>Модуль мониторинга не установлен</h2>
      <p>Температуры, загрузка, частоты и энергопотребление процессора, видеокарты, памяти и дисков, а также FPS в играх — как в MSI Afterburner. Модуль скачивается отдельно (около 8 МБ) и не занимает ресурсов, пока выключен.</p>
      ${m.installing ? `<div class="acc-busy">${I.refresh}${esc(m.installing)}</div>` : `<div class="row"><button class="btn primary" data-act="mon-install">${I.download}Установить модуль</button></div>`}
      ${m.error ? `<div class="net-error" style="max-width:520px">${I.alert}<span>${esc(m.error)}</span></div>` : ''}
      <p class="hint">Без модуля оверлей показывает загрузку процессора, память и видеокарту NVIDIA. Температуры процессора и FPS требуют модуль и прав администратора.</p></div>`;
  } else {
    const extra = !lin ? `<div class="mon-bar">
        ${m.running ? `<span class="ok-dot"></span><span>Датчики работают${m.fps ? (m.helper ? ' · счётчик FPS включён' : '') : ''}</span>` : `<span>Датчики выключены</span><button class="btn sm primary" data-act="mon-start">${I.power}Запустить</button>`}
        <span class="grow"></span>
        ${!m.fps ? (m.installing ? `<span class="acc-busy">${I.refresh}${esc(m.installing)}</span>` : `<button class="btn sm" data-act="mon-install">${I.download}Докачать счётчик FPS</button>`) : ''}
        <label class="mini-switch" title="Один запрос прав сейчас — дальше датчики запускаются без окна UAC (через планировщик заданий Windows)"><input type="checkbox" data-act-change="mon-task" ${m.task ? 'checked' : ''}><span></span>Без запроса UAC</label>
      </div>${m.error ? `<div class="net-error" style="margin:0 30px 12px">${I.alert}<span>${esc(m.error)}</span></div>` : ''}` : '';
    const f = ui.mon.fps && (ui.mon.fps.apps || []).slice().sort((a, b) => b.frames - a.frames)[0];
    const fpsCard = !lin && f ? `<div class="mon-fps"><span class="k">FPS</span><b>${Math.round(f.fps)}</b><span>${esc(f.app)} · кадр ${f.ft} мс · 1% low ${f.low1}</span></div>` : '';
    const hw = ui.mon.data;
    const grid = !m.running && !lin ? `<div class="empty small"><p>Запуск попросит права администратора: без них не видны температуры процессора и материнской платы и FPS.</p></div>`
      : !hw ? `<div class="empty small"><div class="big-ico">${I.refresh}</div><h2>${esc(ui.mon.err || 'Подключаемся к датчикам…')}</h2></div>` : monGridHtml(hw);
    body = extra + fpsCard + grid;
  }
  return head + panel + body;
}

function monGridHtml(all) {
  const color = { cpu: '#60a5fa', gpu: '#4ade80', ram: '#c084fc', dimm: '#a78bfa', disk: '#fbbf24', board: '#94a3b8', net: '#22d3ee' };
  const order = ['cpu', 'gpu', 'ram', 'disk', 'board', 'net', 'dimm'];
  const hw = all.filter((h) => monVisible(h));
  if (!hw.length) return monFilterHtml(all) + '<div class="empty small"><p>Все датчики скрыты — включите нужные категории выше.</p></div>';
  const cards = [...hw].sort((a, b) => order.indexOf(a.kind) - order.indexOf(b.kind)).map((h) => {
    const picks = [];
    for (const [grp, re, label] of MON_PICK[h.kind] || []) {
      const s = (h.groups[grp] || []).find((x) => re.test(x.name));
      if (s && !picks.some((p) => p.label === label)) picks.push({ label, s, grp });
    }
    const metrics = picks.length ? picks : Object.entries(h.groups).slice(0, 2).flatMap(([grp, l]) => l.slice(0, 2).map((s) => ({ label: s.name, s, grp })));
    const key = `${h.name}`;
    return `<div class="mon-card" style="--c:${color[h.kind] || '#aaa'}"><div class="mc-head"><span class="mc-kind">${esc(MON_LABEL[h.kind] || 'Устройство')}</span><span class="mc-name">${esc(h.name)}</span><button class="mc-hide" data-act="mon-hide" data-v="${esc(h.name)}" title="Скрыть это устройство">${I.x}</button></div>
      <div class="mc-grid">${metrics.map(({ label, s, grp }) => `<div class="mc-m"><div class="k">${esc(label)}</div><div class="v">${esc(s.value)}</div>${s.max ? `<div class="mx">макс ${esc(s.max)}</div>` : ''}${spark(ui.mon.hist[`${h.name}|${grp}|${s.name}`], color[h.kind] || '#aaa')}</div>`).join('')}</div>
      <details data-key="${esc(key)}" ${(ui.mon.open || {})[key] ? 'open' : ''}><summary>Все датчики</summary>${Object.entries(h.groups).map(([g, l]) => `<div class="mc-grp"><b>${esc(g)}</b>${l.map((s) => `<div class="mc-row"><span>${esc(s.name)}</span><span>${esc(s.value)}</span></div>`).join('')}</div>`).join('')}</details></div>`;
  }).join('');
  return monFilterHtml(all) + `<div class="mon-grid">${cards}</div>`;
}

// keyboard shortcut capture for the overlay hotkey ("Shift+F12", "Ctrl+Alt+O")
document.addEventListener('keydown', (e) => {
  if (!e.target || e.target.id !== 'osdHk') return;
  e.preventDefault();
  if (['Shift', 'Control', 'Alt', 'Meta'].includes(e.key)) return;
  const mods = [e.ctrlKey && 'Ctrl', e.altKey && 'Alt', e.shiftKey && 'Shift', e.metaKey && 'Super'].filter(Boolean);
  let k = e.code.replace(/^Key/, '').replace(/^Digit/, '');
  if (/^F\d+$/.test(e.key)) k = e.key;
  if (!mods.length && !/^F\d+$/.test(k)) return toast('Добавьте Ctrl, Alt или Shift — иначе клавиша будет мешать в игре', 'err');
  const hk = [...mods, k].join('+');
  e.target.value = hk;
  setOsd({ hotkey: hk });
  toast(`Оверлей: ${hk}`, 'ok');
}, true);
document.addEventListener('toggle', (e) => {
  const d = e.target;
  if (d.tagName === 'DETAILS' && d.dataset.key) { ui.mon.open = ui.mon.open || {}; ui.mon.open[d.dataset.key] = d.open; }
}, true);

function linuxPaneHtml() {
  const l = S.settings.linux || {};
  const r = l.runner || 'wine';
  return `<div class="hint" style="margin:0 0 12px">Как запускать игры для Windows (.exe) — например, с торрентов. Игры Steam, Heroic и Lutris запускаются своими лаунчерами с их настройками Proton/Wine.</div>
    <div class="field"><label>Чем запускать .exe</label><div class="seg wide-seg">${[['wine', 'Wine'], ['umu', 'Proton (umu-run)'], ['custom', 'Своя команда']].map(([k, t]) => `<button data-lx="runner" data-v="${k}" class="${r === k ? 'on' : ''}">${t}</button>`).join('')}</div>
      <div class="hint">${r === 'umu' ? 'umu-launcher запускает игры через Proton как Steam, но без Steam. Установите пакет umu-launcher.' : r === 'custom' ? 'Например: <code>gamescope -f -- wine</code> или путь к своему Proton. Файл игры и её аргументы добавятся в конец.' : 'Системный Wine (пакет wine). Для DirectX 11/12 установите в префикс DXVK и VKD3D (winetricks dxvk vkd3d).'}</div></div>
    ${r === 'umu' ? `<div class="field"><label>Версия Proton (PROTONPATH, необязательно)</label><input class="input" data-lx-input="protonPath" value="${esc(l.protonPath || '')}" placeholder="GE-Proton или путь к папке Proton"></div>` : ''}
    ${r === 'custom' ? `<div class="field"><label>Команда</label><input class="input" data-lx-input="customRunner" value="${esc(l.customRunner || '')}" placeholder="gamescope -f -- wine"></div>` : ''}
    <div class="field"><label>Префикс Wine (WINEPREFIX, необязательно)</label><input class="input" data-lx-input="winePrefix" value="${esc(l.winePrefix || '')}" placeholder="по умолчанию ~/.wine"></div>`;
}

// ---------------- settings: integrations & notifications ----------------
function integrationsPaneHtml() {
  const d = S.settings.discord || {}, t = S.settings.torrent || {};
  return `<div class="subhead" style="margin-top:0">Discord — «Играет в …»</div>
    <div class="toggle-row"><div><div class="t">Показывать игру в статусе Discord</div><div class="d">Для игр не из Steam: торренты, Tarkov, игры из папок. Steam показывает свои игры сам.</div></div>${switchHtml('discord.enabled', d.enabled)}</div>
    <div class="field"><label>Client ID приложения Discord</label><div class="row"><input class="input" id="dcId" value="${esc(d.clientId || '')}" placeholder="например 1234567890123456789"><button class="btn" id="dcSave">Сохранить</button><button class="btn ghost" id="dcTest">Проверить</button></div>
      <div class="hint">Discord показывает статус от имени приложения. Создайте его один раз: <a class="link" id="dcPortal">discord.com/developers</a> → New Application → назовите «Arcadia» → скопируйте Application ID сюда. Статус будет «Играет в Arcadia» с названием игры и обложкой.</div></div>
    <div class="toggle-row"><div><div class="t">И для игр Steam тоже</div></div>${switchHtml('discord.steamToo', d.steamToo)}</div>
    <div class="subhead">qBittorrent — загрузки торрентов</div>
    <div class="toggle-row"><div><div class="t">Показывать загрузки qBittorrent</div><div class="d">Прогресс в разделе «Загрузки»; готовые игры из папок библиотеки добавляются сами</div></div>${switchHtml('torrent.enabled', t.enabled)}</div>
    <div class="field"><label>Адрес веб-интерфейса</label><input class="input" id="qbUrl" value="${esc(t.url || 'http://127.0.0.1:8080')}"></div>
    <div class="row"><input class="input" id="qbUser" value="${esc(t.user || '')}" placeholder="логин"><input class="input" id="qbPass" type="password" value="${esc(t.pass || '')}" placeholder="пароль"><button class="btn" id="qbSave">Сохранить и проверить</button></div>
    <div class="hint">В qBittorrent: Инструменты → Настройки → Веб-интерфейс → включить. Можно отметить «Пропускать аутентификацию для клиентов на localhost» — тогда логин не нужен.</div>`;
}
function notifyPaneHtml() {
  const n = S.settings.notify || {};
  const row = (k, t, d) => `<div class="toggle-row"><div><div class="t">${t}</div>${d ? `<div class="d">${d}</div>` : ''}</div>${switchHtml('notify.' + k, n[k] !== false)}</div>`;
  return row('downloads', 'Загрузка игры в Steam завершена') + row('torrents', 'Торрент скачан')
    + row('freeGames', 'Новые бесплатные игры в Epic Games', 'Проверка раз в 6 часов') + row('wishlist', 'Скидки на игры из списка желаемого Steam', 'Нужен вход в Steam и открытый список желаемого')
    + row('vpn', 'VPN отключился / переподключился') + row('zapret', 'Стратегия zapret подобрана')
    + `<div class="row" style="margin-top:12px"><button class="btn" id="ntTest">${I.check}Проверить уведомление</button><button class="btn ghost" id="ntNews">${I.refresh}Проверить новости магазинов сейчас</button></div>`;
}
function bindSettingsExtras(el) {
  const on = (sel, fn) => { const x = $(sel, el); if (x) x.onclick = fn; };
  on('#dcPortal', () => api.openUrl('https://discord.com/developers/applications'));
  on('#dcSave', async () => { await api.setSettings({ discord: { clientId: $('#dcId', el).value.trim() } }); toast('Сохранено', 'ok'); });
  on('#dcTest', async () => {
    await api.setSettings({ discord: { clientId: $('#dcId', el).value.trim() } });
    const r = await api.x('discord-test');
    toast(r && r.ok ? 'Проверьте профиль в Discord — статус появится на 15 секунд' : (r && r.error) || 'Ошибка', r && r.ok ? 'ok' : 'err');
  });
  on('#qbSave', async () => {
    const t = { url: $('#qbUrl', el).value.trim(), user: $('#qbUser', el).value.trim(), pass: $('#qbPass', el).value };
    await api.setSettings({ torrent: t });
    const r = await api.x('torrent-test', t);
    toast(r && r.ok ? 'qBittorrent подключён' : (r && r.error) || 'Нет связи', r && r.ok ? 'ok' : 'err');
  });
  on('#ntTest', () => api.x('notify-test'));
  on('#ntNews', async () => { toast('Проверяем…', 'ok'); await api.x('store-news'); });
}

// ---------------- zapret auto-pick & VPN extras (net modal) ----------------
function autopickHtml() {
  const a = (S.extras && S.extras.autopick) || {};
  if (!a.running && !(a.results && a.results.length) && !a.error) {
    return `<div class="toggle-row"><div><div class="t">Подобрать стратегию автоматически</div><div class="d">Arcadia по очереди включит каждую стратегию и проверит YouTube и Discord. Один запрос прав администратора, около 10 секунд на стратегию.</div></div><button class="btn" data-na="z-autopick">${I.zap}Подобрать</button></div>`;
  }
  const rows = (a.results || []).map((r) => `<div class="ap-row${a.best === r.name ? ' best' : ''}"><span>${esc(String(r.name).replace(/\.(bat|cmd)$/i, ''))}</span><span class="ap-bar"><i style="width:${(r.ok / r.of) * 100}%"></i></span><span>${r.ok}/${r.of}</span><span class="muted-note">${r.time.toFixed(1)} с</span></div>`).join('');
  return `<div class="autopick"><div class="row" style="justify-content:space-between"><b>${a.running ? `Проверяем стратегии: ${a.done}/${a.total}…` : a.best ? `Лучшая стратегия: ${esc(a.best.replace(/\.(bat|cmd)$/i, ''))} — включена` : 'Проверка завершена'}</b>
    ${a.running ? `<button class="btn sm ghost" data-na="z-autopick-stop">Остановить</button>` : `<button class="btn sm ghost" data-na="z-autopick">${I.refresh}Ещё раз</button>`}</div>
    ${a.error ? `<div class="net-error">${I.alert}<span>${esc(a.error)}</span></div>` : ''}<div class="ap-list">${rows}</div>
    ${!a.running && !a.best && (a.results || []).length ? '<div class="hint">Ни одна стратегия не открыла сайты. Обновите сборку zapret или проверьте интернет.</div>' : ''}</div>`;
}
function vpnExtrasHtml(v) {
  const apps = v.apps || [];
  return `<div class="toggle-row"><div><div class="t">Обновлять подписки автоматически</div></div><select class="input" style="width:150px" data-na-change="v-subs">${[[0, 'Выключено'], [6, 'Каждые 6 ч'], [12, 'Каждые 12 ч'], [24, 'Раз в сутки']].map(([h, l]) => `<option value="${h}" ${v.subsAutoHours === h ? 'selected' : ''}>${l}</option>`).join('')}</select></div>
    ${netSwitch('v.killSwitch', v.killSwitch, 'Аварийный выключатель', 'Если VPN упадёт, трафик через системный прокси блокируется (а не идёт напрямую), пока Arcadia переподключается. Если не получится за минуту — блокировка снимается.')}
    <div class="toggle-row${v.mode !== 'tun' ? ' dim' : ''}"><div><div class="t">Какие программы идут через VPN</div><div class="d">Работает в режиме «Весь трафик (TUN)»</div></div>
      <select class="input" style="width:200px" data-na-change="v-appsmode" ${v.mode !== 'tun' ? 'disabled' : ''}>${[['all', 'Все программы'], ['only', 'Только выбранные'], ['bypass', 'Все, кроме выбранных']].map(([k, l]) => `<option value="${k}" ${v.appsMode === k ? 'selected' : ''}>${l}</option>`).join('')}</select></div>
    ${v.appsMode && v.appsMode !== 'all' ? `<div class="chips" style="margin:6px 0 4px">${apps.map((a) => `<span class="chip on">${esc(a)} <a class="x-inline" data-na="v-app-del" data-app="${esc(a)}">×</a></span>`).join('')}<button class="chip" data-na="v-app-add">+ Программа…</button><button class="chip" data-na="v-app-type">+ Ввести имя .exe</button></div>` : ''}`;
}

// ---------------- gamepad / TV mode ----------------
const BP = { on: false, row: 0, col: 0, rows: [], raf: 0, last: {}, repeatAt: 0 };
function bpRows() {
  const all = games().filter((g) => !g.hidden && !g.missing && inst(g));
  const recent = [...all].filter((g) => lastOf(g)).sort((a, b) => lastOf(b) - lastOf(a)).slice(0, 12);
  const fav = all.filter((g) => g.favorite);
  const playing = all.filter((g) => g.status === 'playing');
  const rest = [...all].sort((a, b) => nameOf(a).localeCompare(nameOf(b), 'ru'));
  return [['Продолжить', recent], ['Играю сейчас', playing], ['Избранное', fav], ['Все игры', rest]].filter(([, l]) => l.length);
}
function bpOpen() {
  if (BP.on) return;
  BP.on = true;
  BP.rows = bpRows();
  BP.row = 0; BP.col = 0;
  const d = document.createElement('div');
  d.id = 'bp';
  document.body.appendChild(d);
  api.win.fullscreen(true);
  bpPaint();
  BP.raf = requestAnimationFrame(bpLoop);
}
function bpClose() {
  BP.on = false;
  cancelAnimationFrame(BP.raf);
  $('#bp') && $('#bp').remove();
  api.win.fullscreen(false);
}
function bpPaint() {
  const d = $('#bp');
  if (!d) return;
  const cur = BP.rows[BP.row] && BP.rows[BP.row][1][BP.col];
  const bg = cur ? heroSrc(cur) : '';
  d.innerHTML = `<div class="bp-bg" style="${bg ? `background-image:url('${esc(bg)}')` : ''}"></div>
    <div class="bp-top"><span class="bp-logo">Arcadia</span><span class="bp-clock">${new Date().toLocaleTimeString('ru-RU', { hour: '2-digit', minute: '2-digit' })}</span></div>
    <div class="bp-info">${cur ? `<h1>${esc(nameOf(cur))}</h1><div class="bp-meta">${esc(srcOf(cur).label)} · ${ptOf(cur) ? 'наиграно ' + esc(fmtTime(ptOf(cur))) : 'ещё не запускали'}${isRunning(cur.id) ? ' · запущено' : ''}</div>` : '<h1>Нет игр</h1>'}</div>
    <div class="bp-rows">${BP.rows.map(([t, list], r) => r < BP.row - 1 ? '' : `<div class="bp-row${r === BP.row ? ' cur' : ''}"><div class="bp-rt">${esc(t)}</div><div class="bp-strip" style="--off:${r === BP.row ? Math.max(0, BP.col - 2) : 0}">${list.map((g, c) => `<div class="bp-card${r === BP.row && c === BP.col ? ' sel' : ''}">${coverHtml(g)}${g.favorite ? `<span class="bp-fav">${I.heartFill}</span>` : ''}</div>`).join('')}</div></div>`).join('')}</div>
    <div class="bp-hints"><span><b>A</b> Играть</span><span><b>X</b> Избранное</span><span><b>Y</b> Случайная</span><span><b>B</b> Выход</span></div>`;
}
function bpMove(dr, dc) {
  if (!BP.rows.length) return;
  BP.row = Math.max(0, Math.min(BP.rows.length - 1, BP.row + dr));
  const len = BP.rows[BP.row][1].length;
  BP.col = Math.max(0, Math.min(len - 1, dr ? Math.min(BP.col, len - 1) : BP.col + dc));
  bpPaint();
}
function bpAct(a) {
  const cur = BP.rows[BP.row] && BP.rows[BP.row][1][BP.col];
  if (a === 'back') return bpClose();
  if (!cur) return;
  if (a === 'play') { play(cur.id); }
  else if (a === 'fav') { toggleFav(cur.id); setTimeout(bpPaint, 50); }
  else if (a === 'rand') {
    const r = Math.floor(Math.random() * BP.rows.length);
    BP.row = r; BP.col = Math.floor(Math.random() * BP.rows[r][1].length);
    bpPaint();
  }
}
function bpLoop(t) {
  if (!BP.on) return;
  const pads = navigator.getGamepads ? [...navigator.getGamepads()].filter(Boolean) : [];
  for (const p of pads) {
    const b = (i) => p.buttons[i] && p.buttons[i].pressed;
    const ax = p.axes[0] || 0, ay = p.axes[1] || 0;
    const dir = b(12) || ay < -0.6 ? 'u' : b(13) || ay > 0.6 ? 'd' : b(14) || ax < -0.6 ? 'l' : b(15) || ax > 0.6 ? 'r' : '';
    if (dir) {
      if (BP.last.dir !== dir || t > BP.repeatAt) {
        BP.repeatAt = t + (BP.last.dir === dir ? 130 : 380);
        bpMove(dir === 'u' ? -1 : dir === 'd' ? 1 : 0, dir === 'l' ? -1 : dir === 'r' ? 1 : 0);
      }
    }
    BP.last.dir = dir;
    for (const [i, a] of [[0, 'play'], [1, 'back'], [2, 'fav'], [3, 'rand'], [9, 'back']]) {
      const k = 'b' + i;
      if (b(i) && !BP.last[k]) bpAct(a);
      BP.last[k] = b(i);
    }
  }
  BP.raf = requestAnimationFrame(bpLoop);
}
// open TV mode with Start on a gamepad (when enabled), keyboard F10
(function bindGamepadMode() {
  let prevStart = false;
  const watch = () => {
    if (!BP.on && S.settings && S.settings.gamepadStart !== false && document.hasFocus()) {
      const pads = navigator.getGamepads ? [...navigator.getGamepads()].filter(Boolean) : [];
      const start = pads.some((p) => p.buttons[9] && p.buttons[9].pressed);
      if (start && !prevStart) bpOpen();
      prevStart = start;
    }
    setTimeout(watch, 250);
  };
  window.addEventListener('gamepadconnected', () => toast('Геймпад подключён — Start открывает режим для ТВ', 'ok'));
  watch();
  document.addEventListener('keydown', (e) => {
    if (e.key === 'F10') { e.preventDefault(); BP.on ? bpClose() : bpOpen(); return; }
    if (!BP.on) return;
    const k = { ArrowUp: [-1, 0], ArrowDown: [1, 0], ArrowLeft: [0, -1], ArrowRight: [0, 1] }[e.key];
    if (k) { e.preventDefault(); e.stopImmediatePropagation(); bpMove(k[0], k[1]); }
    else if (e.key === 'Enter') { e.preventDefault(); e.stopImmediatePropagation(); bpAct('play'); }
    else if (e.key === 'Escape' || e.key === 'Backspace') { e.preventDefault(); e.stopImmediatePropagation(); bpAct('back'); }
    else if (e.key.toLowerCase() === 'f') { bpAct('fav'); }
  }, true);
  setInterval(() => { if (BP.on) { const c = $('#bp .bp-clock'); if (c) c.textContent = new Date().toLocaleTimeString('ru-RU', { hour: '2-digit', minute: '2-digit' }); } }, 15000);
})();
