'use strict';
/* Arcadia — «ИИ»: AI services in one place. Services blocked in Russia / the CIS (or that ban accounts
   seen from there) open only with the VPN on; Arcadia closes their windows if the VPN goes down. */

// vpn: true — does not work from Russia / Belarus or bans such accounts. The mark can be changed per service.
const AI_CATALOG = [
  { id: 'chatgpt', name: 'ChatGPT', by: 'OpenAI', url: 'https://chatgpt.com', cat: 'chat', vpn: true, desc: 'Самый известный чат-бот: тексты, код, файлы, картинки, голос.' },
  { id: 'claude', name: 'Claude', by: 'Anthropic', url: 'https://claude.ai', cat: 'chat', vpn: true, desc: 'Длинные тексты и документы, программирование, аккуратные ответы.' },
  { id: 'gemini', name: 'Gemini', by: 'Google', url: 'https://gemini.google.com', cat: 'chat', vpn: true, desc: 'Ассистент Google: поиск, Gmail, Документы, YouTube.' },
  { id: 'copilot', name: 'Copilot', by: 'Microsoft', url: 'https://copilot.microsoft.com', cat: 'chat', vpn: true, desc: 'Ассистент Microsoft на моделях OpenAI, с поиском Bing.' },
  { id: 'grok', name: 'Grok', by: 'xAI', url: 'https://grok.com', cat: 'chat', vpn: true, desc: 'Чат-бот xAI с доступом к свежим постам X.' },
  { id: 'metaai', name: 'Meta AI', by: 'Meta', url: 'https://www.meta.ai', cat: 'chat', vpn: true, desc: 'Ассистент на моделях Llama и генерация картинок.' },
  { id: 'perplexity', name: 'Perplexity', by: 'Perplexity AI', url: 'https://www.perplexity.ai', cat: 'chat', vpn: false, desc: 'Поиск в интернете с ответами и ссылками на источники.' },
  { id: 'deepseek', name: 'DeepSeek', by: 'DeepSeek', url: 'https://chat.deepseek.com', cat: 'chat', vpn: false, desc: 'Сильные рассуждения и код, бесплатно.' },
  { id: 'qwen', name: 'Qwen', by: 'Alibaba', url: 'https://chat.qwen.ai', cat: 'chat', vpn: false, desc: 'Чат, код, картинки и видео от Alibaba.' },
  { id: 'mistral', name: 'Le Chat', by: 'Mistral AI', url: 'https://chat.mistral.ai', cat: 'chat', vpn: false, desc: 'Быстрый европейский чат-бот с поиском.' },
  { id: 'kimi', name: 'Kimi', by: 'Moonshot AI', url: 'https://www.kimi.com', cat: 'chat', vpn: false, desc: 'Длинный контекст и работа с документами.' },
  { id: 'zai', name: 'Z.ai (GLM)', by: 'Zhipu AI', url: 'https://chat.z.ai', cat: 'chat', vpn: false, desc: 'Модели GLM: код, агенты, презентации.' },
  { id: 'poe', name: 'Poe', by: 'Quora', url: 'https://poe.com', cat: 'chat', vpn: false, desc: 'Много моделей в одном месте.' },
  { id: 'huggingchat', name: 'HuggingChat', by: 'Hugging Face', url: 'https://huggingface.co/chat', cat: 'chat', vpn: false, desc: 'Открытые модели бесплатно.' },
  { id: 'character', name: 'Character.AI', by: 'Character.AI', url: 'https://character.ai', cat: 'chat', vpn: false, desc: 'Разговоры с персонажами.' },
  { id: 'gigachat', name: 'GigaChat', by: 'Сбер', url: 'https://giga.chat', cat: 'ru', vpn: false, desc: 'Российский ассистент Сбера.' },
  { id: 'alice', name: 'Алиса', by: 'Яндекс', url: 'https://alice.yandex.ru', cat: 'ru', vpn: false, desc: 'Ассистент Яндекса на YandexGPT.' },
  { id: 'shedevrum', name: 'Шедеврум', by: 'Яндекс', url: 'https://shedevrum.ai', cat: 'ru', vpn: false, desc: 'Картинки и видео по описанию.' },
  { id: 'notebooklm', name: 'NotebookLM', by: 'Google', url: 'https://notebooklm.google.com', cat: 'work', vpn: true, desc: 'Конспекты и подкасты по вашим документам.' },
  { id: 'aistudio', name: 'AI Studio', by: 'Google', url: 'https://aistudio.google.com', cat: 'work', vpn: true, desc: 'Свежие модели Gemini бесплатно, для разработчиков.' },
  { id: 'v0', name: 'v0', by: 'Vercel', url: 'https://v0.app', cat: 'work', vpn: false, desc: 'Сайты и интерфейсы по описанию.' },
  { id: 'sora', name: 'Sora', by: 'OpenAI', url: 'https://sora.chatgpt.com', cat: 'image', vpn: true, desc: 'Видео по описанию.' },
  { id: 'runway', name: 'Runway', by: 'Runway', url: 'https://app.runwayml.com', cat: 'image', vpn: true, desc: 'Генерация и монтаж видео.' },
  { id: 'midjourney', name: 'Midjourney', by: 'Midjourney', url: 'https://www.midjourney.com', cat: 'image', vpn: false, desc: 'Художественные изображения.' },
  { id: 'leonardo', name: 'Leonardo', by: 'Leonardo.Ai', url: 'https://app.leonardo.ai', cat: 'image', vpn: false, desc: 'Картинки, арты и текстуры для игр.' },
  { id: 'ideogram', name: 'Ideogram', by: 'Ideogram', url: 'https://ideogram.ai', cat: 'image', vpn: false, desc: 'Картинки с аккуратным текстом.' },
  { id: 'suno', name: 'Suno', by: 'Suno', url: 'https://suno.com', cat: 'audio', vpn: true, desc: 'Песни по описанию — с вокалом.' },
  { id: 'udio', name: 'Udio', by: 'Udio', url: 'https://www.udio.com', cat: 'audio', vpn: true, desc: 'Музыка по описанию.' },
  { id: 'elevenlabs', name: 'ElevenLabs', by: 'ElevenLabs', url: 'https://elevenlabs.io', cat: 'audio', vpn: true, desc: 'Озвучка, клонирование голоса, перевод речи.' },
];
const AI_CATS = [['', 'Все'], ['chat', 'Чат-боты'], ['work', 'Для работы'], ['image', 'Картинки и видео'], ['audio', 'Музыка и голос'], ['ru', 'Российские']];

const aiCfg = () => ({ mode: 'window', custom: [], vpn: {}, hidden: [], ...(S.settings.ai || {}) });
function setAi(patch) {
  S.settings.ai = { ...aiCfg(), ...patch };
  ui.lastHtml = '';
  render();
  api.setSettings({ ai: patch });
}
const aiNeedsVpn = (a, c = aiCfg()) => (a.id in c.vpn ? !!c.vpn[a.id] : !!a.vpn);
const aiAll = () => [...AI_CATALOG, ...aiCfg().custom.map((x) => ({ ...x, cat: 'custom', custom: true }))];
const aiDomain = (u) => { try { return new URL(u).hostname; } catch { return ''; } };

function aiCardHtml(a, c) {
  const need = aiNeedsVpn(a, c);
  const open = (S.aiOpen || []).includes(a.id);
  const blocked = need && !(S.net && S.net.vpn && S.net.vpn.running);
  return `<div class="ai-card${open ? ' open' : ''}" data-ai="${esc(a.id)}">
    <div class="ai-top"><div class="ai-logo" style="--h:${hue(a.name)}"><span>${esc(a.name[0])}</span><img src="https://www.google.com/s2/favicons?domain=${esc(aiDomain(a.url))}&sz=64" alt="" loading="lazy" draggable="false"></div>
      <div class="ai-t"><div class="ai-name">${esc(a.name)}${open ? '<span class="ai-open-dot" title="Открыто"></span>' : ''}</div><div class="ai-by">${esc(a.by || aiDomain(a.url))}</div></div>
      <button class="icon-btn sm ai-more" data-aia="menu" title="Ещё">${I.more}</button></div>
    <div class="ai-desc">${esc(a.desc || a.url)}</div>
    <div class="ai-bottom"><span class="ai-vpn ${need ? (blocked ? 'need' : 'ok') : 'free'}" title="${need ? 'Сервис не работает в России и СНГ: открывается только с включённым VPN, а при отключении VPN окно закрывается' : 'Работает без VPN'}">${need ? `${I.shield}${blocked ? 'Нужен VPN' : 'Через VPN'}` : 'Без VPN'}</span>
      <button class="btn sm ${blocked ? '' : 'primary'}" data-aia="open">${blocked ? I.lock + 'Открыть' : open ? 'Перейти' : 'Открыть'}</button></div>
  </div>`;
}

function renderAi() {
  const c = aiCfg();
  const cat = ui.aiCat || '';
  const q = norm(ui.search || '');
  let list = aiAll().filter((a) => !c.hidden.includes(a.id));
  if (cat) list = list.filter((a) => a.cat === cat || (cat === 'chat' && a.cat === 'custom'));
  if (q) list = list.filter((a) => norm(a.name + ' ' + (a.by || '') + ' ' + (a.desc || '')).includes(q));
  const vpn = S.net && S.net.vpn;
  const hiddenN = c.hidden.length;
  return `<div class="view-head"><h1>ИИ</h1><span class="sub">${aiAll().length} сервисов</span><span class="grow"></span>
      <div class="seg mini-seg" title="Где открывать"><button data-aia="mode" data-v="window" class="${c.mode !== 'browser' ? 'on' : ''}">В окне Arcadia</button><button data-aia="mode" data-v="browser" class="${c.mode === 'browser' ? 'on' : ''}">В браузере</button></div>
      <button class="btn" data-aia="add">${I.plus}Добавить свой</button></div>
    <div class="ai-vpnbar ${vpn && vpn.running ? 'on' : ''}">${I.shield}<span>${vpn && vpn.running ? 'VPN включён — открываются все сервисы' : 'VPN выключен — сервисы с пометкой «Нужен VPN» не откроются, пока вы его не включите'}</span><span class="grow"></span>
      <button class="btn sm${vpn && vpn.running ? ' ghost' : ' primary'}" data-aia="vpn">${vpn && vpn.running ? 'Настройки VPN' : 'Включить VPN'}</button></div>
    <div class="ai-cats chips">${AI_CATS.map(([k, l]) => `<button class="chip${cat === k ? ' on' : ''}" data-aia="cat" data-v="${k}">${l}</button>`).join('')}
      ${hiddenN ? `<span class="grow"></span><button class="link-btn" data-aia="unhide">вернуть скрытые (${hiddenN})</button>` : ''}</div>
    ${list.length ? `<div class="ai-grid">${list.map((a) => aiCardHtml(a, c)).join('')}</div>` : '<div class="empty small"><p>Ничего не найдено</p></div>'}
    <p class="hint" style="padding:0 30px 30px">Окна ИИ используют общий профиль: войдите один раз через Google или Microsoft — и вход сохранится во всех сервисах. Пометку «Нужен VPN» можно поменять в меню карточки (⋯), если сервис у вас работает иначе.</p>`;
}

async function aiOpen(a) {
  const c = aiCfg();
  const req = { id: a.id, name: a.name, url: a.url, vpn: aiNeedsVpn(a, c), mode: c.mode };
  const r = await api.ai.open(req);
  if (r && r.ok) return;
  if (r && r.needVpn) return aiVpnModal(a, req);
  toast((r && r.error) || 'Не удалось открыть', 'err');
}

function aiVpnModal(a, req) {
  const v = (S.net && S.net.vpn) || {};
  const ready = v.core && v.core.installed && (v.servers || []).length > 0;
  const { el, close } = openModal({
    title: `${I.shield}<span>${esc(a.name)} — нужен VPN</span>`, cls: 'small',
    body: `<div class="body"><p style="margin-top:0">${esc(a.name)} не работает в России и СНГ, а с российского адреса может заблокировать аккаунт. Arcadia откроет его только через VPN и закроет окно, если VPN отключится.</p>
      ${ready ? '' : '<p class="hint">VPN ещё не настроен: добавьте свой сервер или подписку в разделе «Обход блокировок».</p>'}
      <div class="row" style="justify-content:flex-end;margin-top:16px"><button class="btn ghost" data-m="cancel">Отмена</button>
      ${ready ? `<button class="btn primary" data-m="go">${I.shield}Включить VPN и открыть</button>` : `<button class="btn primary" data-m="setup">Настроить VPN</button>`}</div></div>`,
  });
  el.addEventListener('click', async (e) => {
    const b = e.target.closest('[data-m]');
    if (!b) return;
    if (b.dataset.m === 'cancel') return close();
    if (b.dataset.m === 'setup') { close(); return netModal('vpn'); }
    b.disabled = true;
    b.innerHTML = `${I.refresh}Подключаем VPN…`;
    const r = await api.vpn.start();
    if (r && r.ok === false) { toast(r.error || 'VPN не включился', 'err'); b.disabled = false; b.innerHTML = `${I.shield}Включить VPN и открыть`; return; }
    for (let i = 0; i < 40 && !(S.net.vpn && S.net.vpn.running); i++) await new Promise((res) => setTimeout(res, 250));
    close();
    const o = await api.ai.open(req);
    if (!o || !o.ok) toast((o && o.error) || 'Не удалось открыть', 'err');
  });
}

function aiMenu(a, x, y) {
  const c = aiCfg();
  const need = aiNeedsVpn(a, c);
  const m = $('#ctx');
  m.innerHTML = `<div class="sub-title">${esc(a.name)}</div>
    <button data-c="open-other">${I.ext}${c.mode === 'browser' ? 'Открыть в окне Arcadia' : 'Открыть в браузере'}</button>
    <button data-c="vpn">${I.shield}Требует VPN${need ? `<span class="check">${I.check}</span>` : ''}</button>
    <hr>${a.custom ? `<button data-c="del" class="danger">${I.trash}Удалить</button>` : `<button data-c="hide">${I.x}Скрыть</button>`}`;
  m.classList.remove('hidden');
  const r = m.getBoundingClientRect();
  m.style.left = Math.min(x, innerWidth - r.width - 8) + 'px';
  m.style.top = Math.max(8, Math.min(y, innerHeight - r.height - 8)) + 'px';
  m.onclick = async (e) => {
    const b = e.target.closest('[data-c]');
    if (!b) return;
    hideCtx();
    const k = b.dataset.c;
    if (k === 'vpn') setAi({ vpn: { ...c.vpn, [a.id]: !need } });
    else if (k === 'hide') setAi({ hidden: [...c.hidden, a.id] });
    else if (k === 'del') setAi({ custom: c.custom.filter((x) => x.id !== a.id) });
    else if (k === 'open-other') {
      const r2 = await api.ai.open({ id: a.id, name: a.name, url: a.url, vpn: need, mode: c.mode === 'browser' ? 'window' : 'browser' });
      if (r2 && r2.needVpn) aiVpnModal(a, { id: a.id, name: a.name, url: a.url, vpn: need, mode: c.mode === 'browser' ? 'window' : 'browser' });
      else if (r2 && !r2.ok) toast(r2.error, 'err');
    }
  };
}

function aiAddModal() {
  const { el, close } = openModal({
    title: `${I.plus}<span>Свой ИИ-сервис</span>`, cls: 'small',
    body: `<div class="body"><div class="field"><label>Название</label><input class="input" id="aiN" placeholder="Например, OpenRouter"></div>
      <div class="field"><label>Адрес</label><input class="input" id="aiU" placeholder="https://…"></div>
      <div class="toggle-row"><div><div class="t">Требует VPN</div><div class="d">Не открывать без VPN и закрывать при его отключении</div></div><label class="switch"><input type="checkbox" id="aiV"><span></span></label></div>
      <div class="row" style="justify-content:flex-end;margin-top:14px"><button class="btn ghost" data-m="x">Отмена</button><button class="btn primary" data-m="ok">Добавить</button></div></div>`,
  });
  el.addEventListener('click', (e) => {
    const b = e.target.closest('[data-m]');
    if (!b) return;
    if (b.dataset.m === 'x') return close();
    const name = $('#aiN', el).value.trim();
    let url = $('#aiU', el).value.trim();
    if (url && !/^https?:\/\//i.test(url)) url = 'https://' + url;
    if (!name || !aiDomain(url)) return toast('Укажите название и адрес', 'err');
    const id = 'my-' + Date.now().toString(36);
    const c = aiCfg();
    setAi({ custom: [...c.custom, { id, name, url, by: aiDomain(url), desc: url }], vpn: { ...c.vpn, [id]: $('#aiV', el).checked } });
    close();
  });
}

// clicks in the AI view
document.addEventListener('click', (e) => {
  if (ui.view !== 'ai' || ui.detail) return;
  const b = e.target.closest('[data-aia]');
  if (!b || !e.target.closest('#view')) return;
  const act = b.dataset.aia;
  const card = b.closest('[data-ai]');
  const a = card && aiAll().find((x) => x.id === card.dataset.ai);
  if (act === 'open' && a) return aiOpen(a);
  if (act === 'menu' && a) { e.stopPropagation(); const r = b.getBoundingClientRect(); return aiMenu(a, r.left - 160, r.bottom + 4); }
  if (act === 'cat') { ui.aiCat = b.dataset.v; ui.lastHtml = ''; return render(); }
  if (act === 'mode') return setAi({ mode: b.dataset.v });
  if (act === 'add') return aiAddModal();
  if (act === 'unhide') return setAi({ hidden: [] });
  if (act === 'vpn') return S.net && S.net.vpn && S.net.vpn.running ? netModal('vpn') : (S.net.vpn.core && S.net.vpn.core.installed && S.net.vpn.servers.length ? api.vpn.start() : netModal('vpn'));
});
document.addEventListener('dblclick', (e) => {
  if (ui.view !== 'ai') return;
  const card = e.target.closest('[data-ai]');
  const a = card && aiAll().find((x) => x.id === card.dataset.ai);
  if (a && !e.target.closest('button')) aiOpen(a);
});
