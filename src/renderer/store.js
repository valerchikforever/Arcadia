'use strict';
/* Arcadia — «Магазины и аккаунты»: optional sign-in per launcher + built-in store browser */

ui.storeKey = ui.storeKey || 'steam';
ui.brSent = '';

const STORE_ORDER = ['steam', 'epic', 'gog', 'ea', 'ubisoft', 'battlenet', 'battlestate'];

function accountPanel(key, a) {
  const label = SOURCES[key].label;
  const acc = a.account;
  const busy = a.busy ? `<div class="acc-busy"><span class="spin-ico">${I.refresh}</span>${esc(a.busy)}</div>` : '';
  const err = a.error ? `<div class="net-error" style="margin-top:12px">${I.alert}<span>${esc(a.error)}</span></div>` : '';
  if (!a.library) {
    const signed = acc && acc.web;
    return `<div class="acc-panel">
      <div class="acc-ava" style="--c:${SOURCES[key].color}">${I.user}</div>
      <div class="acc-main">
        <div class="acc-name">${signed ? 'Сессия на сайте сохранена' : 'Вход необязателен'}</div>
        <div class="acc-sub">${esc(label)} не даёт сторонним программам список купленных игр — устанавливайте их через официальный лаунчер. Вход на сайте нужен только для страниц магазина ниже.</div>
        ${busy}
      </div>
      <div class="acc-actions">
        ${a.hasLauncher ? `<button class="btn" data-sa="launcher">${I.ext}Открыть лаунчер</button>` : ''}
        ${signed ? `<button class="btn ghost" data-sa="logout" title="Удалить cookies этого сайта">${I.logout}Выйти</button>` : `<button class="btn primary" data-sa="login">${I.user}Войти на сайте</button>`}
      </div>
    </div>${err}`;
  }
  if (!acc) {
    return `<div class="acc-panel">
      <div class="acc-ava" style="--c:${SOURCES[key].color}">${I.user}</div>
      <div class="acc-main">
        <div class="acc-name">Вход не выполнен</div>
        <div class="acc-sub">Войдите, чтобы видеть все купленные игры ${esc(label)} и устанавливать их прямо из Arcadia. Это необязательно: установленные игры находятся и без входа.</div>
        ${busy}
      </div>
      <div class="acc-actions">
        ${a.localSteam ? `<button class="btn" data-sa="local-steam" title="Без пароля: берётся аккаунт, в который вы вошли в клиенте Steam (видны публичные данные профиля)">${I.user}Аккаунт ${esc(a.localSteam.name)} из клиента</button>` : ''}
        <button class="btn primary" data-sa="login" ${a.busy ? 'disabled' : ''}>${I.user}Войти в ${esc(label)}</button>
      </div>
    </div>${err}`;
  }
  const modeNote = acc.mode === 'local' ? 'аккаунт из клиента Steam · публичные данные' : 'вход выполнен';
  return `<div class="acc-panel signed">
    ${acc.avatar ? `<img class="acc-ava img" src="${esc(acc.avatar)}">` : `<div class="acc-ava" style="--c:${SOURCES[key].color}">${esc((acc.name || label).slice(0, 1).toUpperCase())}</div>`}
    <div class="acc-main">
      <div class="acc-name">${esc(acc.name || label)} <span class="acc-ok">${I.check}</span></div>
      <div class="acc-sub">${esc(modeNote)} · купленных игр: <b>${acc.count}</b>${acc.updatedAt ? ` · обновлено ${esc(fmtAgo(acc.updatedAt))}` : ''}</div>
      ${busy}
      ${key === 'steam' ? `<div class="acc-sub" style="margin-top:4px"><a class="link" data-sa="steam-key">${a.hasApiKey ? 'Steam Web API ключ указан' : 'Профиль скрыт? Укажите Steam Web API ключ'}</a></div>` : ''}
    </div>
    <div class="acc-actions">
      <button class="btn" data-sa="show-games">${I.grid}Игры аккаунта</button>
      <button class="btn" data-sa="sync" ${a.busy ? 'disabled' : ''}>${I.refresh}Обновить</button>
      <button class="btn ghost" data-sa="logout">${I.logout}Выйти</button>
    </div>
  </div>${err}`;
}

function browserFrame(key, a) {
  const B = S.browser || {};
  const mine = B.enabled && B.key === key;
  const host = (() => { try { return new URL(B.url).host.replace(/^www\./, ''); } catch { return ''; } })();
  const toolbar = mine ? `<div class="br-bar">
      <button class="icon-btn sm" data-sa="nav" data-nav="back" ${B.canBack ? '' : 'disabled'} title="Назад">${I.back}</button>
      <button class="icon-btn sm" data-sa="nav" data-nav="forward" ${B.canFwd ? '' : 'disabled'} title="Вперёд">${I.fwd}</button>
      <button class="icon-btn sm${B.loading ? ' spin' : ''}" data-sa="nav" data-nav="${B.loading ? 'stop' : 'reload'}" title="${B.loading ? 'Остановить' : 'Обновить'}">${B.loading ? I.x : I.refresh}</button>
      <div class="br-addr" title="${esc(B.url)}"><span class="br-host">${esc(host)}</span><span class="br-title">${esc(B.title || '')}</span></div>
      ${B.loaded ? `<span class="pill" title="Память процесса страницы">${I.cpu}${B.memMB ? `${B.memMB} МБ` : '…'}</span>` : ''}
      ${B.blocked ? `<span class="pill" title="Заблокировано запросов рекламы, трекеров и видео">${I.shield}${B.blocked}</span>` : ''}
      <button class="icon-btn sm" data-sa="external" title="Открыть в обычном браузере">${I.ext}</button>
      <button class="btn sm ghost" data-sa="br-off" title="Выгрузить страницу и выключить браузерный режим">${I.power}Выключить</button>
    </div>` : '';
  let ph;
  if (!B.enabled) {
    ph = `<div class="br-ph">
      <div class="big-ico">${I.globe}</div>
      <h2>Браузерный режим выключен</h2>
      <p>Страницы магазинов не загружаются, пока вы сами их не откроете, — никакого JavaScript и нагрузки в фоне. Включённая страница полностью выгружается, когда окно скрыто в трей, свёрнуто или вы ушли из этого раздела.</p>
      <div class="row"><button class="btn primary" data-sa="enable">${I.power}Включить браузерный режим</button></div>
      <div class="hint">Экономный режим ${S.settings.browser && S.settings.browser.economy ? 'включён: без видео и WebGL' : 'выключен'} · <a class="link" data-sa="br-settings">настройки</a></div>
    </div>`;
  } else if (!mine) {
    ph = `<div class="br-ph"><div class="big-ico">${I.globe}</div><h2>${esc(SOURCES[key].label)}</h2><p>Откройте страницу из ссылок выше.</p><div class="row"><button class="btn primary" data-sa="link" data-url="${esc(a.home)}">${I.globe}Открыть магазин</button></div></div>`;
  } else {
    ph = `<div class="br-ph"><div class="big-ico">${I.cpu}</div><h2>${B.unloadedReason ? 'Страница выгружена' : 'Загрузка…'}</h2><p>${B.unloadedReason ? 'Память освобождена. Страница загрузится снова, как только раздел будет на экране.' : ''}</p></div>`;
  }
  return `<div class="br-frame">${toolbar}<div class="br-host-wrap" id="browserHost">${ph}</div></div>`;
}

function renderStore() {
  const A = S.accounts;
  if (!A) return '<div class="empty"><h2>Загрузка…</h2></div>';
  const key = A[ui.storeKey] ? ui.storeKey : 'steam';
  const a = A[key];
  const tabs = STORE_ORDER.filter((k) => A[k]).map((k) => {
    const signed = A[k].account && (A[k].account.id || A[k].account.web);
    return `<button class="store-tab${k === key ? ' on' : ''}" data-sa="tab" data-key="${k}"><span class="dot" style="--c:${SOURCES[k].color}"></span>${esc(SOURCES[k].label)}${signed ? `<span class="tab-ok">${I.check}</span>` : ''}</button>`;
  }).join('');
  const B = S.browser || {};
  const links = a.links.map(([l, u]) => `<button class="chip${B.enabled && B.key === key && B.url && B.url.startsWith(u) ? ' on' : ''}" data-sa="link" data-url="${esc(u)}">${esc(l)}</button>`).join('');
  return `<div class="store-wrap">
    <div class="view-head" style="padding-bottom:10px"><h1>Магазины и аккаунты</h1><span class="sub">вход в аккаунты необязателен</span></div>
    <div class="store-tabs">${tabs}</div>
    <div class="store-top">${accountPanel(key, a)}<div class="chips store-links">${links}</div></div>
    ${browserFrame(key, a)}
  </div>`;
}

// Position the native page view over #browserHost (or hide it while anything covers that area)
function syncBrowser() {
  const B = S.browser || {};
  const host = $('#browserHost');
  const covered = $('#modalRoot').children.length || !$('#ctx').classList.contains('hidden') || !$('#drop').classList.contains('hidden');
  let rect = null;
  if (host && !ui.detail && ui.view === 'store' && B.enabled && B.key === ui.storeKey && !covered && document.visibilityState === 'visible') {
    const r = host.getBoundingClientRect();
    if (r.width > 20 && r.height > 20) rect = { x: r.left, y: r.top, width: r.width, height: r.height };
  }
  const sig = rect ? `${Math.round(rect.x)},${Math.round(rect.y)},${Math.round(rect.width)},${Math.round(rect.height)}` : 'none';
  if (sig === ui.brSent) return;
  ui.brSent = sig;
  api.br.bounds(rect);
}

// Called by go() when the section is opened: auto-enable only if the user asked for it in settings
function storeEntered() {
  const c = S.settings.browser || {};
  if (c.autoEnable && S.browser && !S.browser.enabled && S.accounts) api.br.open(ui.storeKey, S.accounts[ui.storeKey].home);
}

function openStorePage(key, url) {
  ui.storeKey = key;
  if (ui.view !== 'store' || ui.detail) go('store');
  api.br.open(key, url);
}

async function storeAction(b) {
  const sa = b.dataset.sa;
  const key = ui.storeKey;
  const A = S.accounts || {};
  const a = A[key] || {};
  const run = async (fn, ok) => {
    b.disabled = true;
    try {
      const r = await fn();
      if (r && r.ok === false) { if (r.error && !/закрыто/.test(r.error)) toast(r.error, 'err'); }
      else if (ok) toast(typeof ok === 'function' ? ok(r) : ok, 'ok');
      return r;
    } finally { b.disabled = false; }
  };
  if (sa === 'tab') {
    ui.storeKey = b.dataset.key;
    ui.lastHtml = '';
    render();
    const B = S.browser || {};
    if (B.enabled && B.key !== ui.storeKey) api.br.open(ui.storeKey, A[ui.storeKey].home); // browser already on: follow the tab
    return;
  }
  if (sa === 'login') return run(() => api.acc.login(key), (r) => (r && r.count !== undefined ? `Вход выполнен. Купленных игр: ${r.count}` : r && r.web ? 'Сессия на сайте сохранена' : 'Окно входа закрыто'));
  if (sa === 'local-steam') return run(() => api.acc.localSteam(), (r) => `Аккаунт подключён. Купленных игр: ${r.count}`);
  if (sa === 'sync') return run(() => api.acc.sync(key), (r) => `Список обновлён: ${r.count} игр${r.added ? `, новых ${r.added}` : ''}`);
  if (sa === 'logout') {
    const ok = await confirmModal('Выйти из аккаунта?', `Arcadia удалит вход и сохранённые данные сайта ${esc(SOURCES[key].label)}. Неустановленные игры этого аккаунта пропадут из библиотеки, установленные останутся.`, 'Выйти');
    if (ok) { await api.acc.logout(key); toast('Вы вышли из аккаунта', 'ok'); }
    return;
  }
  if (sa === 'launcher') return run(() => api.acc.launcher(key));
  if (sa === 'show-games') return go('src:' + key);
  if (sa === 'steam-key') {
    const v = await promptModal('Steam Web API ключ', '', 'Ключ с steamcommunity.com/dev/apikey');
    if (v !== null) { await api.acc.steamKey(v); toast(v ? 'Ключ сохранён — нажмите «Обновить»' : 'Ключ удалён', 'ok'); }
    return;
  }
  if (sa === 'enable') return api.br.open(key, a.home);
  if (sa === 'link') return api.br.open(key, b.dataset.url);
  if (sa === 'nav') return api.br.nav(b.dataset.nav);
  if (sa === 'external') return api.br.external();
  if (sa === 'br-off') return api.br.disable();
  if (sa === 'br-settings') return settingsModal('browser');
}

(function bindStore() {
  $('#view').addEventListener('click', (e) => {
    const b = e.target.closest('[data-sa]');
    if (b && !b.disabled) { e.stopPropagation(); storeAction(b); }
  });
  // anything that may cover the page area, or resize it
  const mo = new MutationObserver(() => syncBrowser());
  mo.observe($('#modalRoot'), { childList: true });
  mo.observe($('#ctx'), { attributes: true, attributeFilter: ['class'] });
  mo.observe($('#drop'), { attributes: true, attributeFilter: ['class'] });
  new ResizeObserver(() => syncBrowser()).observe($('#view'));
  window.addEventListener('resize', syncBrowser);
  document.addEventListener('visibilitychange', syncBrowser);
  // auto-enable when the section is opened (off by default)
  let lastView = '';
  setInterval(() => {
    const cur = ui.detail ? '' : ui.view;
    if (cur === 'store' && lastView !== 'store' && S.settings.browser && S.settings.browser.autoEnable && S.browser && !S.browser.enabled && S.accounts) {
      api.br.open(ui.storeKey, S.accounts[ui.storeKey].home);
    }
    lastView = cur;
  }, 400);
})();
