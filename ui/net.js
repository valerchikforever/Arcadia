'use strict';
/* Arcadia — «Обход блокировок»: zapret + VPN (sing-box) */

const PROTO_COLORS = { VLESS: '#8b5cf6', VMess: '#5aa2ff', Trojan: '#ff6464', Shadowsocks: '#7fd18b', Hysteria2: '#e0b25c', TUIC: '#1fbaff' };
const stripBat = (f) => String(f || '').replace(/\.(bat|cmd)$/i, '');

function pingHtml(p) {
  if (p === 'udp') return '<span class="ping muted" title="UDP-протокол: проверка пингом недоступна">UDP</span>';
  if (p === 0) return '<span class="ping muted">…</span>';
  if (p === undefined) return '<span class="ping muted">—</span>';
  if (p < 0) return '<span class="ping bad">нет ответа</span>';
  return `<span class="ping ${p < 120 ? 'good' : p < 300 ? 'mid' : 'bad'}">${p} мс</span>`;
}

function powerHtml(kind, on, busy, title, sub, disabled) {
  return `<div class="power-card ${on ? 'on' : ''}">
    <button class="power ${on ? 'on' : ''} ${busy ? 'busy' : ''}" data-na="${kind}-toggle" ${disabled || busy ? 'disabled' : ''} title="${on ? 'Выключить' : 'Включить'}">${I.power}</button>
    <div class="pc-text"><div class="pc-title">${title}</div><div class="pc-sub">${sub}</div></div>
  </div>`;
}

function netSwitch(key, checked, label, desc, disabled) {
  return `<div class="toggle-row${disabled ? ' dim' : ''}"><div><div class="t">${label}</div>${desc ? `<div class="d">${desc}</div>` : ''}</div>
    <label class="switch"><input type="checkbox" data-nkey="${key}" ${checked ? 'checked' : ''} ${disabled ? 'disabled' : ''}><span></span></label></div>`;
}

// ---------- zapret ----------
function zapretHtml(z) {
  if (!z.valid) {
    return `${powerHtml('z', false, false, 'Zapret не настроен', 'Укажите папку, куда распакован zapret', true)}
      <div class="net-help">
        <p>Zapret обходит замедление и блокировки YouTube, Discord и других сайтов без VPN — интернет идёт напрямую, пинг в играх не растёт.</p>
        <p>Arcadia управляет уже скачанной сборкой zapret для Windows (например, <a class="link" data-na="open-url" data-url="https://github.com/Flowseal/zapret-discord-youtube/releases">zapret-discord-youtube</a>): распакуйте архив и укажите папку — в ней должны быть файлы стратегий <code>general*.bat</code> и <code>bin\\winws.exe</code>.</p>
      </div>
      <div class="row" style="margin-top:14px"><button class="btn primary" data-na="z-pick">${I.folder}Указать папку</button><button class="btn" data-na="z-detect">${I.search}Найти автоматически</button></div>
      ${z.dir ? `<div class="hint warn-text">В папке «${esc(z.dir)}» не найден winws.exe</div>` : ''}
      ${z.detected ? `<div class="detected">${z.detected.length ? z.detected.map((d) => `<button class="folder-item pick" data-na="z-use" data-dir="${esc(d)}">${I.folder}<span class="p">${esc(d)}</span><span class="chip">Выбрать</span></button>`).join('') : '<div class="hint">Папки zapret не найдены на Рабочем столе, в Загрузках и в корне дисков.</div>'}</div>` : ''}`;
  }
  const changed = z.running && z.runningStrategy && z.strategy !== z.runningStrategy;
  const sub = z.busy ? 'Ждём подтверждения прав администратора…' : z.running ? `Стратегия <b>${esc(stripBat(z.runningStrategy) || 'запущена вне Arcadia')}</b> · winws.exe работает` : 'Нажмите, чтобы включить обход блокировок';
  return `${powerHtml('z', z.running, z.busy, z.running ? 'Zapret включён' : 'Zapret выключен', sub)}
    ${z.error ? `<div class="net-error">${I.alert}<span>${esc(z.error)}</span></div>` : ''}
    <div class="field" style="margin-top:18px"><label>Стратегия</label>
      <div class="row"><select class="input" data-na-change="z-strategy">${z.strategies.map((f) => `<option value="${esc(f)}" ${f === z.strategy ? 'selected' : ''}>${esc(stripBat(f))}</option>`).join('')}</select>
      ${changed ? `<button class="btn primary" data-na="z-apply">Применить</button>` : ''}</div>
      <div class="hint">Если YouTube или Discord всё ещё не открываются — выберите другую стратегию: на разных провайдерах работают разные.</div></div>
    ${netSwitch('z.gameFilter', z.gameFilter, 'Игровой фильтр', 'Обход и для игровых портов (1024–65535). Включайте, если не работают онлайн-игры или голос в Discord во время игры. Применяется при следующем включении.')}
    ${netSwitch('z.autostart', z.autostart, 'Включать вместе с Arcadia', 'Windows будет спрашивать права администратора при запуске')}
    ${netSwitch('z.stopOnExit', z.stopOnExit, 'Выключать при выходе из Arcadia', '')}
    <div class="field" style="margin-top:14px"><label>Папка zapret</label>
      <div class="row"><span class="path">${esc(z.dir)}</span><button class="btn ghost" data-na="z-pick">Изменить</button></div></div>
    <div class="row"><button class="btn" data-na="z-open" data-sub="lists">${I.list}Списки сайтов</button><button class="btn ghost" data-na="z-open">${I.folder}Открыть папку</button></div>
    <div class="hint" style="margin-top:12px">Для включения нужны права администратора — Windows покажет запрос. Свои сайты добавляйте в <code>lists\\list-general-user.txt</code>.</div>`;
}

// ---------- VPN ----------
function vpnTopHtml(v) {
  const sel = v.selected === 'auto' ? 'Автовыбор' : (v.servers.find((s) => s.id === v.selected) || {}).name || 'Автовыбор';
  const can = v.core.installed && v.servers.length > 0;
  const sub = v.busy ? (v.mode === 'tun' ? 'Ждём подтверждения прав администратора…' : 'Подключаемся…')
    : v.running ? `<b>${esc(sel)}</b> · ${v.runningMode === 'tun' ? 'весь трафик (TUN)' : `системный прокси 127.0.0.1:${v.port}`}`
    : !v.core.installed ? 'Сначала установите ядро sing-box (ниже)' : !v.servers.length ? 'Добавьте сервер — вставьте свою ссылку ниже' : `Сервер: ${esc(sel)}`;
  const core = v.core.installing
    ? `<div class="core-row"><span class="spin-ico">${I.refresh}</span><span>${esc(v.core.installing)}</span></div>`
    : v.core.installed
      ? `<div class="core-row"><span class="ok-dot"></span><span>Ядро sing-box ${esc(v.core.version || '')}</span><span class="grow"></span><button class="btn ghost sm" data-na="v-install">Обновить</button><button class="btn ghost sm" data-na="v-log">Журнал</button></div>`
      : `<div class="core-row warn"><span>${I.download}</span><span>Ядро sing-box не установлено</span><span class="grow"></span><button class="btn primary sm" data-na="v-install">Скачать (≈15 МБ)</button><button class="btn sm" data-na="v-core">Указать файл</button></div>`;
  const x = v.xray || {};
  const xray = !x.needed && !x.installed ? ''
    : x.installing
      ? `<div class="core-row"><span class="spin-ico">${I.refresh}</span><span>${esc(x.installing)}</span></div>`
      : x.installed
        ? `<div class="core-row"><span class="ok-dot"></span><span>Ядро Xray ${esc(x.version || '')} <span class="muted-note">· для серверов xhttp</span></span><span class="grow"></span><button class="btn ghost sm" data-na="v-install-xray">Обновить</button></div>`
        : `<div class="core-row warn"><span>${I.download}</span><span>Для серверов xhttp нужно ядро Xray</span><span class="grow"></span><button class="btn primary sm" data-na="v-install-xray">Скачать Xray (≈12 МБ)</button><button class="btn sm" data-na="v-xray-file">Указать файл</button></div>`;
  return `${powerHtml('v', v.running, v.busy, v.running ? 'VPN подключён' : 'VPN отключён', sub, !can)}
    ${v.error ? `<div class="net-error">${I.alert}<span>${esc(v.error)}</span></div>` : ''}
    <div class="field" style="margin-top:16px"><label>Режим</label>
      <div class="seg wide-seg"><button data-na="v-mode" data-mode="proxy" class="${v.mode !== 'tun' ? 'on' : ''}">Системный прокси</button><button data-na="v-mode" data-mode="tun" class="${v.mode === 'tun' ? 'on' : ''}">Весь трафик (TUN)</button></div>
      <div class="hint">${v.mode === 'tun' ? 'Через VPN идёт весь трафик всех программ. Нужны права администратора.' : 'Через VPN идут браузеры и программы, которые используют системный прокси. Права администратора не нужны.'}${v.running && v.mode !== v.runningMode ? ' <b>Переподключитесь, чтобы сменить режим.</b>' : ''}</div></div>
    ${core}${xray}`;
}

function vpnListHtml(v) {
  const subName = (id) => (v.subs.find((s) => s.id === id) || {}).name || '';
  const rows = v.servers.map((s) => `<div class="srv ${v.selected === s.id ? 'sel' : ''}" data-na="v-select" data-id="${esc(s.id)}">
      <span class="radio"></span>
      <div class="srv-main"><div class="srv-name">${esc(s.name)}</div><div class="srv-sub"><span class="proto" style="--c:${PROTO_COLORS[s.proto] || '#aaa'}">${esc(s.proto)}</span>${s.transport === 'xhttp' ? '<span class="proto" style="--c:#f0a6ff" title="Работает через ядро Xray">xhttp</span>' : ''}${esc(s.host)}:${s.port}${s.subId ? ` · ${esc(subName(s.subId))}` : ''}</div></div>
      ${pingHtml(s.ping)}
      <button class="x-btn" data-na="v-del" data-id="${esc(s.id)}" title="Удалить">${I.x}</button></div>`).join('');
  const subs = v.subs.length ? `<div class="subhead" style="margin-top:18px">Подписки</div>${v.subs.map((s) => `<div class="srv sub-row"><span>${I.link}</span><div class="srv-main"><div class="srv-name">${esc(s.name)}</div><div class="srv-sub">${s.count || 0} серв. · обновлена ${esc(fmtAgo(s.updatedAt))}</div></div><button class="x-btn" data-na="v-delsub" data-id="${esc(s.id)}" title="Удалить подписку и её серверы">${I.x}</button></div>`).join('')}` : '';
  if (!v.servers.length) return `<div class="empty-mini">Серверов пока нет. Вставьте ссылку от своего VPN-сервиса или подписку — она появится здесь.</div>${subs}`;
  return `<div class="srv-head"><span>Серверы · ${v.servers.length}</span><span class="grow"></span>
      <button class="btn ghost sm" data-na="v-ping">${I.zap}Пинг</button>${v.subs.length ? `<button class="btn ghost sm" data-na="v-refresh">${I.refresh}Обновить подписки</button>` : ''}</div>
    <div class="srv-list">
      <div class="srv ${v.selected === 'auto' ? 'sel' : ''}" data-na="v-select" data-id="auto"><span class="radio"></span><div class="srv-main"><div class="srv-name">Автовыбор</div><div class="srv-sub">самый быстрый из списка, переключается сам</div></div></div>
      ${rows}
    </div>${subs}`;
}

function vpnOptsHtml(v) {
  return `${netSwitch('v.directRu', v.directRu, 'Российские сайты напрямую', 'Сайты в зонах .ru, .su и .рф открываются без VPN — банки и госуслуги не будут ругаться')}
    ${netSwitch('v.gamesDirect', v.gamesDirect, 'Игры из библиотеки напрямую', 'Игры Arcadia не идут через VPN — пинг не растёт. Работает в режиме «Весь трафик»', v.mode !== 'tun')}
    ${netSwitch('v.autostart', v.autostart, 'Подключать при запуске Arcadia', '')}
    ${netSwitch('v.stopOnExit', v.stopOnExit, 'Отключать при выходе из Arcadia', 'В режиме прокси отключение происходит всегда, чтобы не оставить интернет без соединения')}`;
}

// One toast for all broken links instead of one per link
function reportLinkErrors(errors) {
  if (!errors || !errors.length) return;
  if (errors.length === 1) return toast(errors[0], 'err');
  const reasons = [...new Set(errors.map((e) => e.replace(/^.*?…:\s*/, '')))];
  toast(`Пропущено ссылок: ${errors.length}. ${reasons.slice(0, 2).join('; ')}`, 'err');
}

// ---------- modal ----------
function netModal(tab) {
  if ($('.overlay.net-ov')) return;
  if (!S.net) return toast('Модуль ещё загружается', 'err');
  if (S.platform && S.platform !== 'win32') toast('Zapret и VPN работают только в Windows', 'err');
  const local = { tab: tab || ui.netTab || 'zapret', detected: null };
  const body = `
    <div class="tabs"><button data-ntab="zapret">${I.zap}Zapret</button><button data-ntab="vpn">${I.globe}VPN</button></div>
    <div class="body net-body">
      <div data-npane="zapret"><div data-region="z"></div><div data-region="za"></div></div>
      <div data-npane="vpn" class="hidden">
        <div data-region="vt"></div>
        <div class="field" style="margin-top:18px"><label>Добавить сервер</label>
          <textarea class="input area" id="vIn" spellcheck="false" placeholder="Вставьте одну или несколько ссылок: vless://…, vmess://…, trojan://…, ss://…, hy2://…, tuic://…&#10;или ссылку на подписку https://…"></textarea>
          <div class="row" style="margin-top:8px"><button class="btn primary" data-na="v-add">${I.plus}Добавить</button><span class="hint" style="margin:0">Поддерживаются ссылки v2rayN / Hiddify / Nekobox, подписки и JSON sing-box</span></div></div>
        <div data-region="vl"></div>
        <div class="subhead" style="margin-top:20px">Настройки</div>
        <div data-region="vo"></div>
      </div>
    </div>`;
  const { el } = openModal({ title: `${I.shield}<span>Обход блокировок</span>`, body, cls: 'net-modal', onClose: () => { ui.netRefresh = null; } });
  el.classList.add('net-ov');

  const showTab = (t) => {
    local.tab = ui.netTab = t;
    $$('[data-ntab]', el).forEach((b) => b.classList.toggle('on', b.dataset.ntab === t));
    $$('[data-npane]', el).forEach((p) => p.classList.toggle('hidden', p.dataset.npane !== t));
  };
  const regions = {};
  const setRegion = (k, html) => {
    if (regions[k] === html) return;
    regions[k] = html;
    $(`[data-region="${k}"]`, el).innerHTML = html;
  };
  const refresh = () => {
    if (!S.net) return;
    setRegion('z', zapretHtml({ ...S.net.zapret, detected: local.detected }));
    setRegion('vt', vpnTopHtml(S.net.vpn));
    setRegion('vl', vpnListHtml(S.net.vpn));
    setRegion('vo', vpnOptsHtml(S.net.vpn) + vpnExtrasHtml(S.net.vpn));
    setRegion('za', S.net.zapret.valid ? `<div class="subhead">Автоподбор</div>${autopickHtml()}` : '');
  };
  ui.netRefresh = refresh;
  refresh();
  showTab(local.tab);
  $$('[data-ntab]', el).forEach((b) => (b.onclick = () => showTab(b.dataset.ntab)));

  const run = async (btn, fn, okMsg) => {
    if (btn) btn.disabled = true;
    try {
      const r = await fn();
      if (r && r.ok === false) toast(r.error || 'Ошибка', 'err');
      else if (okMsg) toast(typeof okMsg === 'function' ? okMsg(r) : okMsg, 'ok');
      return r;
    } finally { if (btn) btn.disabled = false; }
  };

  el.addEventListener('click', async (e) => {
    const b = e.target.closest('[data-na]');
    if (!b) return;
    const a = b.dataset.na;
    const z = S.net.zapret, v = S.net.vpn;
    if (a === 'x-btn') return;
    if (a === 'open-url') return api.openUrl(b.dataset.url);
    // zapret
    if (a === 'z-toggle') return run(null, () => (z.running ? api.zapret.stop() : api.zapret.start(z.strategy)), z.running ? 'Zapret выключен' : 'Zapret включён');
    if (a === 'z-apply') return run(b, () => api.zapret.start(z.strategy), `Стратегия ${stripBat(z.strategy)} включена`);
    if (a === 'z-pick') return run(b, () => api.zapret.pickDir());
    if (a === 'z-detect') {
      b.disabled = true;
      local.detected = await api.zapret.detect();
      b.disabled = false;
      if (local.detected && local.detected.length === 1) await api.zapret.set({ dir: local.detected[0], strategy: '' });
      return refresh();
    }
    if (a === 'z-use') { local.detected = null; return api.zapret.set({ dir: b.dataset.dir, strategy: '' }); }
    if (a === 'z-open') return api.zapret.open(b.dataset.sub || '');
    if (a === 'z-autopick') return run(b, () => api.zapret.autopick());
    if (a === 'z-autopick-stop') return run(b, () => api.zapret.autopickStop(), 'Останавливаем после текущей стратегии…');
    if (a === 'v-app-add') { const n = await api.vpn.pickApp(); if (n) api.vpn.set({ apps: [...new Set([...(v.apps || []), n])] }); return; }
    if (a === 'v-app-type') {
      const n = await promptModal('Имя программы', '', 'например: chrome.exe');
      if (n) api.vpn.set({ apps: [...new Set([...(v.apps || []), /\.exe$/i.test(n) ? n : n + '.exe'])] });
      return;
    }
    if (a === 'v-app-del') return api.vpn.set({ apps: (v.apps || []).filter((x) => x !== b.dataset.app) });
    // vpn
    if (a === 'v-toggle') return run(null, () => (v.running ? api.vpn.stop() : api.vpn.start()), v.running ? 'VPN отключён' : 'VPN подключён');
    if (a === 'v-mode') return api.vpn.set({ mode: b.dataset.mode });
    if (a === 'v-install') return run(b, () => api.vpn.installCore(), 'Ядро sing-box установлено');
    if (a === 'v-core') return run(b, () => api.vpn.pickCore());
    if (a === 'v-install-xray') return run(b, () => api.vpn.installXray(), 'Ядро Xray установлено');
    if (a === 'v-xray-file') return run(b, () => api.vpn.pickXray());
    if (a === 'v-log') return api.vpn.openLog();
    if (a === 'v-add') {
      const text = $('#vIn', el).value.trim();
      if (!text) return toast('Вставьте ссылку', 'err');
      const r = await run(b, () => api.vpn.add(text));
      if (r && r.ok !== false) {
        if (r.added) { toast(`Добавлено серверов: ${r.added}`, 'ok'); $('#vIn', el).value = ''; api.vpn.ping(); }
        else if (r.found) toast('Эти серверы уже есть в списке', 'ok');
        reportLinkErrors(r.errors);
      }
      return;
    }
    if (a === 'v-ping') return run(b, () => api.vpn.ping());
    if (a === 'v-refresh') {
      const r = await run(b, () => api.vpn.refreshSubs(), (r2) => `Подписки обновлены: ${r2.found} серв.`);
      if (r) reportLinkErrors(r.errors);
      return;
    }
    if (a === 'v-del') { e.stopPropagation(); return api.vpn.removeServer(b.dataset.id); }
    if (a === 'v-delsub') {
      if (await confirmModal('Удалить подписку?', 'Подписка и все её серверы будут удалены из списка.', 'Удалить')) api.vpn.removeSub(b.dataset.id);
      return;
    }
    if (a === 'v-select') {
      if (e.target.closest('.x-btn')) return;
      await api.vpn.set({ selected: b.dataset.id });
      if (v.running) toast('Переподключите VPN, чтобы сменить сервер', 'ok');
    }
  });

  el.addEventListener('change', (e) => {
    const t = e.target;
    if (t.dataset.naChange === 'z-strategy') return api.zapret.set({ strategy: t.value });
    if (t.dataset.naChange === 'v-subs') return api.vpn.set({ subsAutoHours: Number(t.value) });
    if (t.dataset.naChange === 'v-appsmode') return api.vpn.set({ appsMode: t.value });
    const k = t.dataset.nkey;
    if (!k) return;
    const [grp, key] = k.split('.');
    (grp === 'z' ? api.zapret : api.vpn).set({ [key]: t.checked });
  });
}
