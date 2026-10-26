'use strict';
// Share links (vless/vmess/trojan/ss/hysteria2/tuic) and subscriptions -> sing-box outbounds.

function b64decode(s) {
  let t = String(s).trim().replace(/-/g, '+').replace(/_/g, '/').replace(/\s+/g, '');
  while (t.length % 4) t += '=';
  return Buffer.from(t, 'base64').toString('utf8');
}

function looksB64(s) {
  const t = String(s).trim().replace(/\s+/g, '');
  return t.length > 16 && /^[A-Za-z0-9+/=_-]+$/.test(t);
}

const dec = (s) => { try { return decodeURIComponent(s); } catch { return s; } };

// URL parsing that tolerates any scheme and IPv6 hosts
function parseUrl(link) {
  const m = /^([a-z0-9+.-]+):\/\/(?:([^@/?#]*)@)?(\[[^\]]+\]|[^:/?#]+)(?::(\d+))?\/?(?:\?([^#]*))?(?:#(.*))?$/i.exec(link.trim());
  if (!m) throw new Error('Неверный формат ссылки');
  const q = {};
  for (const part of (m[5] || '').split('&')) {
    if (!part) continue;
    const i = part.indexOf('=');
    const k = dec(i < 0 ? part : part.slice(0, i));
    q[k] = dec(i < 0 ? '' : part.slice(i + 1).replace(/\+/g, '%20'));
  }
  return {
    scheme: m[1].toLowerCase(),
    user: m[2] !== undefined ? dec(m[2]) : '',
    host: m[3].replace(/^\[|\]$/g, ''),
    port: m[4] ? parseInt(m[4], 10) : 0,
    q,
    name: m[6] ? dec(m[6]) : '',
  };
}

function tlsFrom(q, host, { defaultOn = false, alpn } = {}) {
  const sec = (q.security || '').toLowerCase();
  const on = sec === 'tls' || sec === 'reality' || sec === 'xtls' || (defaultOn && sec !== 'none');
  if (!on) return undefined;
  const tls = { enabled: true, server_name: q.sni || q.peer || q.host || host };
  if (q.allowInsecure === '1' || q.insecure === '1' || q.allowInsecure === 'true') tls.insecure = true;
  const a = q.alpn ? q.alpn.split(',').filter(Boolean) : alpn;
  if (a && a.length) tls.alpn = a;
  const fp = q.fp || (sec === 'reality' ? 'chrome' : '');
  if (fp) tls.utls = { enabled: true, fingerprint: fp === 'random' || fp === 'randomized' ? 'random' : fp };
  if (sec === 'reality') {
    if (!q.pbk) throw new Error('В ссылке reality нет публичного ключа (pbk)');
    tls.reality = { enabled: true, public_key: q.pbk, short_id: q.sid || '' };
  }
  return tls;
}

function transportFrom(type, q) {
  const t = (type || 'tcp').toLowerCase();
  if (t === 'tcp' || t === 'raw' || t === '') {
    if ((q.headerType || '') === 'http') return { type: 'http', host: q.host ? q.host.split(',') : [], path: q.path || '/' };
    return undefined;
  }
  if (t === 'ws' || t === 'websocket') {
    let p = q.path || '/';
    const tr = { type: 'ws', path: p };
    const ed = /[?&]ed=(\d+)/.exec(p);
    if (ed) {
      tr.path = p.replace(/[?&]ed=\d+/, '').replace(/\?$/, '') || '/';
      tr.max_early_data = parseInt(ed[1], 10);
      tr.early_data_header_name = 'Sec-WebSocket-Protocol';
    }
    if (q.host) tr.headers = { Host: q.host };
    return tr;
  }
  if (t === 'grpc') return { type: 'grpc', service_name: q.serviceName || q.path || '' };
  if (t === 'http' || t === 'h2') return { type: 'http', host: q.host ? q.host.split(',') : [], path: q.path || '/' };
  if (t === 'httpupgrade') return { type: 'httpupgrade', host: q.host || '', path: q.path || '/' };
  if (t === 'quic') return { type: 'quic' };
  throw new Error(`Неизвестный транспорт: ${t}`);
}

// ---------- Xray (for xhttp, which sing-box does not support) ----------
const isXhttp = (type) => /^(xhttp|splithttp)$/i.test(type || '');

function xrayStream(q, host, { defaultTls = false } = {}) {
  const sec = (q.security || (defaultTls ? 'tls' : 'none')).toLowerCase();
  const st = { network: 'xhttp', security: sec === 'reality' ? 'reality' : sec === 'tls' || sec === 'xtls' ? 'tls' : 'none' };
  const xs = { path: q.path || '/', mode: q.mode || 'auto' };
  if (q.host) xs.host = q.host;
  if (q.extra) { try { xs.extra = JSON.parse(q.extra); } catch { /* ignore broken extra */ } }
  st.xhttpSettings = xs;
  const sni = q.sni || q.peer || q.host || host;
  if (st.security === 'tls') {
    st.tlsSettings = { serverName: sni, fingerprint: q.fp || 'chrome' };
    if (q.alpn) st.tlsSettings.alpn = q.alpn.split(',').filter(Boolean);
    if (q.allowInsecure === '1' || q.insecure === '1' || q.allowInsecure === 'true') st.tlsSettings.allowInsecure = true;
  } else if (st.security === 'reality') {
    if (!q.pbk) throw new Error('В ссылке reality нет публичного ключа (pbk)');
    st.realitySettings = { serverName: sni, fingerprint: q.fp || 'chrome', publicKey: q.pbk, shortId: q.sid || '', spiderX: q.spx || '' };
  }
  return st;
}

function clean(o) {
  for (const k of Object.keys(o)) if (o[k] === undefined || o[k] === '') delete o[k];
  return o;
}

function parseVless(link) {
  const u = parseUrl(link);
  if (!u.user) throw new Error('В ссылке vless нет UUID');
  if (isXhttp(u.q.type)) {
    const settings = { address: u.host, port: u.port || 443, id: u.user, encryption: u.q.encryption || 'none' };
    if (u.q.flow && u.q.flow !== 'none') settings.flow = u.q.flow;
    return {
      name: u.name || `${u.host}:${u.port}`, host: u.host, port: u.port || 443, proto: 'VLESS', transport: 'xhttp',
      xray: { protocol: 'vless', settings, streamSettings: xrayStream(u.q, u.host) },
    };
  }
  return {
    name: u.name || `${u.host}:${u.port}`,
    host: u.host, port: u.port || 443, proto: 'VLESS',
    outbound: clean({
      type: 'vless', server: u.host, server_port: u.port || 443, uuid: u.user,
      flow: u.q.flow && u.q.flow !== 'none' ? u.q.flow.replace(/-udp443$/, '') : undefined,
      packet_encoding: 'xudp',
      tls: tlsFrom(u.q, u.host),
      transport: transportFrom(u.q.type, u.q),
    }),
  };
}

function parseVmess(link) {
  const body = link.slice('vmess://'.length).split('#')[0];
  let j;
  try { j = JSON.parse(b64decode(body)); } catch { throw new Error('Не удалось прочитать vmess-ссылку'); }
  const port = parseInt(j.port, 10) || 443;
  const q = { security: j.tls === 'tls' ? 'tls' : 'none', sni: j.sni, alpn: j.alpn, fp: j.fp, host: j.host, path: j.path, serviceName: j.path, headerType: j.type, allowInsecure: j.allowInsecure ? '1' : '', mode: j.mode };
  if (isXhttp(j.net)) {
    return {
      name: j.ps || `${j.add}:${port}`, host: j.add, port, proto: 'VMess', transport: 'xhttp',
      xray: { protocol: 'vmess', settings: { address: j.add, port, id: j.id, security: j.scy || 'auto' }, streamSettings: xrayStream(q, j.add) },
    };
  }
  return {
    name: j.ps || `${j.add}:${port}`,
    host: j.add, port, proto: 'VMess',
    outbound: clean({
      type: 'vmess', server: j.add, server_port: port, uuid: j.id,
      security: j.scy || 'auto', alter_id: parseInt(j.aid || '0', 10) || 0,
      packet_encoding: 'xudp',
      tls: tlsFrom(q, j.add),
      transport: transportFrom(j.net, q),
    }),
  };
}

function parseTrojan(link) {
  const u = parseUrl(link);
  if (isXhttp(u.q.type)) {
    return {
      name: u.name || `${u.host}:${u.port}`, host: u.host, port: u.port || 443, proto: 'Trojan', transport: 'xhttp',
      xray: { protocol: 'trojan', settings: { address: u.host, port: u.port || 443, password: u.user }, streamSettings: xrayStream(u.q, u.host, { defaultTls: true }) },
    };
  }
  return {
    name: u.name || `${u.host}:${u.port}`,
    host: u.host, port: u.port || 443, proto: 'Trojan',
    outbound: clean({
      type: 'trojan', server: u.host, server_port: u.port || 443, password: u.user,
      tls: tlsFrom(u.q, u.host, { defaultOn: true }),
      transport: transportFrom(u.q.type, u.q),
    }),
  };
}

function parseSS(link) {
  let rest = link.slice('ss://'.length);
  let name = '';
  const hash = rest.indexOf('#');
  if (hash >= 0) { name = dec(rest.slice(hash + 1)); rest = rest.slice(0, hash); }
  rest = rest.split('?')[0].replace(/\/$/, '');
  let userinfo, hostport;
  if (rest.includes('@')) {
    const at = rest.lastIndexOf('@');
    userinfo = rest.slice(0, at);
    hostport = rest.slice(at + 1);
    userinfo = dec(userinfo);
    if (!userinfo.includes(':') && looksB64(userinfo)) userinfo = b64decode(userinfo);
  } else {
    const d = b64decode(rest);
    const at = d.lastIndexOf('@');
    userinfo = d.slice(0, at);
    hostport = d.slice(at + 1);
  }
  const ci = userinfo.indexOf(':');
  const method = userinfo.slice(0, ci), password = userinfo.slice(ci + 1);
  const hm = /^(\[[^\]]+\]|[^:]+):(\d+)$/.exec(hostport);
  if (!hm || !method) throw new Error('Не удалось прочитать ss-ссылку');
  const host = hm[1].replace(/^\[|\]$/g, ''), port = parseInt(hm[2], 10);
  return { name: name || `${host}:${port}`, host, port, proto: 'Shadowsocks', outbound: { type: 'shadowsocks', server: host, server_port: port, method, password } };
}

function parseHy2(link) {
  const u = parseUrl(link.replace(/^hy2:\/\//i, 'hysteria2://'));
  const tls = { enabled: true, server_name: u.q.sni || u.host, alpn: ['h3'] };
  if (u.q.insecure === '1' || u.q.allowInsecure === '1') tls.insecure = true;
  const o = { type: 'hysteria2', server: u.host, server_port: u.port || 443, password: u.user, tls };
  if (u.q.obfs && u.q.obfs !== 'none') o.obfs = { type: u.q.obfs, password: u.q['obfs-password'] || '' };
  return { name: u.name || `${u.host}:${u.port}`, host: u.host, port: u.port || 443, proto: 'Hysteria2', udp: true, outbound: o };
}

function parseTuic(link) {
  const u = parseUrl(link);
  const [uuid, password] = u.user.split(':');
  const tls = { enabled: true, server_name: u.q.sni || u.host, alpn: (u.q.alpn || 'h3').split(',') };
  if (u.q.allow_insecure === '1' || u.q.insecure === '1' || u.q.allowInsecure === '1') tls.insecure = true;
  return {
    name: u.name || `${u.host}:${u.port}`, host: u.host, port: u.port || 443, proto: 'TUIC', udp: true,
    outbound: clean({ type: 'tuic', server: u.host, server_port: u.port || 443, uuid, password, congestion_control: u.q.congestion_control || 'bbr', udp_relay_mode: u.q.udp_relay_mode, tls }),
  };
}

function parseLink(link) {
  const l = String(link).trim();
  const scheme = (/^([a-z0-9]+):\/\//i.exec(l) || [])[1];
  switch ((scheme || '').toLowerCase()) {
    case 'vless': return parseVless(l);
    case 'vmess': return parseVmess(l);
    case 'trojan': return parseTrojan(l);
    case 'ss': return parseSS(l);
    case 'hysteria2': case 'hy2': return parseHy2(l);
    case 'tuic': return parseTuic(l);
    default: throw new Error(`Протокол не поддерживается: ${scheme || l.slice(0, 20)}`);
  }
}

const PROXY_TYPES = new Set(['vless', 'vmess', 'trojan', 'shadowsocks', 'hysteria2', 'hysteria', 'tuic', 'wireguard', 'socks', 'http', 'shadowtls', 'anytls', 'ssh']);

// Text with links, base64 subscription body, or sing-box JSON -> { servers, errors }
function parseMany(text) {
  const servers = [], errors = [];
  let body = String(text || '').trim();
  if (!body) return { servers, errors };
  if (body.startsWith('{') || body.startsWith('[')) {
    try {
      const j = JSON.parse(body);
      const list = Array.isArray(j) ? j : j.outbounds ? j.outbounds : [j];
      for (const o of list) {
        if (o && PROXY_TYPES.has(o.type) && o.server) {
          const { tag, ...ob } = o;
          servers.push({ name: tag || `${o.server}:${o.server_port}`, host: o.server, port: o.server_port, proto: o.type, outbound: ob });
        }
      }
      if (!servers.length) errors.push('В JSON не найдено серверов');
      return { servers, errors };
    } catch { /* not JSON */ }
  }
  if (!/:\/\//.test(body) && looksB64(body)) {
    try { body = b64decode(body); } catch {}
  }
  if (/^\s*(proxies|port|mixed-port):/m.test(body)) {
    errors.push('Формат Clash/YAML не поддерживается — нужна подписка с ссылками (v2rayN) или JSON sing-box');
    return { servers, errors };
  }
  for (const line of body.split(/[\r\n]+/)) {
    const l = line.trim();
    if (!l || !/^[a-z0-9]+:\/\//i.test(l)) continue;
    try { servers.push({ ...parseLink(l), link: l }); } catch (e) { errors.push(`${l.slice(0, 40)}…: ${e.message}`); }
  }
  return { servers, errors };
}

module.exports = { parseLink, parseMany, b64decode };
