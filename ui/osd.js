'use strict';
// In-game overlay: draws what the Rust side sends once a second ("osd" event).
(function () {
  const T = window.__TAURI__;
  const box = document.getElementById('box');
  const r0 = (v) => (v == null ? null : Math.round(v));
  const r1 = (v) => (v == null ? null : Math.round(v * 10) / 10);
  const esc = (t) => String(t).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
  const val = (v, unit) => (v == null ? '<b>—</b>' : `<b>${v}</b><i>${unit}</i>`);
  const temp = (v) => (v == null ? '' : `<span class="sep">·</span><b${v >= 85 ? ' class="hot"' : ''}>${r0(v)}</b><i>°C</i>`);
  const dur = (s) => { const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60); return h ? `${h}:${String(m).padStart(2, '0')}` : `${m} мин`; };
  const row = (k, html, c, cls) => `<div class="row${cls ? ' ' + cls : ''}"><span class="k" style="--c:${c}">${k}</span><span class="v">${html}</span></div>`;

  function paint(p) {
    const cfg = p.cfg || {};
    const items = new Set(cfg.items || []);
    const s = p.s || {};
    document.body.className = cfg.position || 'tl';
    box.style.setProperty('--a', cfg.opacity ?? 0.55);
    let h = '';
    if (items.has('session') && p.game) h += `<div class="game">${esc(p.game)}${p.session != null ? ' · ' + dur(p.session) : ''}</div>`;
    const f = p.fps;
    if ((items.has('fps') || items.has('frametime') || items.has('low1')) && (f || !p.noFps)) {
      if (f) {
        const cls = f.fps < 30 ? 'low' : f.fps < 55 ? 'mid' : '';
        let t = items.has('fps') ? `<b class="${cls}">${r0(f.fps)}</b>` : '';
        if (items.has('frametime')) t += `<span class="sep">·</span><b>${r1(f.ft)}</b><i>мс</i>`;
        if (items.has('low1') && f.low1) t += `<span class="sep">·</span><i>1%</i> <b>${r0(f.low1)}</b>`;
        h += row('FPS', t, '#6ee7b7', 'fps');
      } else if (p.fpsNote) h += row('FPS', `<span class="note">${esc(p.fpsNote)}</span>`, '#6ee7b7');
      else h += row('FPS', '<b>—</b>', '#6ee7b7', 'fps');
    }
    if (items.has('cpu') || items.has('cputemp') || items.has('cpuclk')) {
      let t = items.has('cpu') ? val(r0(s.cpu), '%') : '';
      if (items.has('cputemp')) t += temp(s.cputemp);
      if (items.has('cpuclk') && s.cpuclk) t += `<span class="sep">·</span><b>${r0(s.cpuclk)}</b><i>МГц</i>`;
      h += row('CPU', t, '#60a5fa');
    }
    if (s.gpu != null || s.gputemp != null || s.vram != null) {
      if (items.has('gpu') || items.has('gputemp') || items.has('gpuclk') || items.has('gpupower')) {
        let t = items.has('gpu') ? val(r0(s.gpu), '%') : '';
        if (items.has('gputemp')) t += temp(s.gputemp);
        if (items.has('gpuclk') && s.gpuclk) t += `<span class="sep">·</span><b>${r0(s.gpuclk)}</b><i>МГц</i>`;
        if (items.has('gpupower') && s.gpupower) t += `<span class="sep">·</span><b>${r0(s.gpupower)}</b><i>Вт</i>`;
        h += row('GPU', t, '#4ade80');
      }
      if (items.has('vram') && s.vram != null) h += row('VRAM', `<b>${r1(s.vram / 1024)}</b>${s.vramTotal ? `<i>/ ${r0(s.vramTotal / 1024)} ГБ</i>` : '<i>ГБ</i>'}`, '#4ade80');
    }
    if (items.has('ram') && s.ramUsed != null) h += row('RAM', `<b>${r1(s.ramUsed)}</b>${s.ramTotal ? `<i>/ ${r0(s.ramTotal)} ГБ</i>` : '<i>ГБ</i>'}`, '#c084fc');
    if (items.has('clock')) h += row('', `<b>${esc(p.clock || '')}</b>`, '#aab');
    box.innerHTML = h || '<div class="note">Оверлей Arcadia</div>';
    box.style.zoom = cfg.scale || 1;
    // tell Arcadia how big the panel is: the window is cut to it
    const r = box.getBoundingClientRect();
    const w = Math.ceil(r.width), hh = Math.ceil(r.height);
    if (T && (Math.abs(w - last.w) > 1 || Math.abs(hh - last.h) > 1)) { last.w = w; last.h = hh; T.core.invoke('call', { ch: 'osd:size', args: [w, hh] }); }
  }
  const last = { w: 0, h: 0 };

  if (T && T.event) T.event.listen('osd', (e) => paint(e.payload));
  // preview in a plain browser
  else paint({ cfg: { items: ['fps', 'frametime', 'low1', 'cpu', 'cputemp', 'gpu', 'gputemp', 'vram', 'ram', 'session'], position: 'tl' }, game: 'Cyberpunk 2077', session: 3720, fps: { fps: 142, ft: 7.04, low1: 98 }, s: { cpu: 34, cputemp: 67, gpu: 97, gputemp: 71, vram: 6200, vramTotal: 8192, ramUsed: 14.2, ramTotal: 32 } });
})();
