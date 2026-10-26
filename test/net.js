'use strict';
// Link parser / sing-box config / zapret .bat parser tests. Run: node test/net.js
const fs = require('fs');
const os = require('os');
const path = require('path');
const assert = require('assert');
const { parseLink, parseMany } = require('../src/main/net/links');
const { buildConfig } = require('../src/main/net/vpn');
const { parseBat, listStrategies } = require('../src/main/net/zapret');

let passed = 0;
const t = (name, fn) => { try { fn(); passed++; console.log('  ✓', name); } catch (e) { console.error('  ✗', name, '\n', e); process.exitCode = 1; } };
const b64 = (s) => Buffer.from(s).toString('base64');

console.log('links');
t('vless reality + vision', () => {
  const s = parseLink('vless://11111111-2222-3333-4444-555555555555@1.2.3.4:443?encryption=none&flow=xtls-rprx-vision&security=reality&sni=www.microsoft.com&fp=chrome&pbk=PUBKEY&sid=ab12&type=tcp#%D0%9D%D0%B8%D0%B4%D0%B5%D1%80%D0%BB%D0%B0%D0%BD%D0%B4%D1%8B');
  assert.strictEqual(s.name, 'Нидерланды');
  assert.deepStrictEqual(s.outbound.tls.reality, { enabled: true, public_key: 'PUBKEY', short_id: 'ab12' });
  assert.strictEqual(s.outbound.tls.server_name, 'www.microsoft.com');
  assert.strictEqual(s.outbound.flow, 'xtls-rprx-vision');
  assert.ok(!s.outbound.transport);
});
t('vless ws tls with early data', () => {
  const s = parseLink('vless://uuid@cdn.example.com:443?type=ws&security=tls&host=my.host&path=%2Fws%3Fed%3D2048#ws');
  assert.deepStrictEqual(s.outbound.transport, { type: 'ws', path: '/ws', max_early_data: 2048, early_data_header_name: 'Sec-WebSocket-Protocol', headers: { Host: 'my.host' } });
});
t('vless grpc', () => assert.strictEqual(parseLink('vless://u@h:443?type=grpc&serviceName=svc&security=tls').outbound.transport.service_name, 'svc'));
t('vless xhttp -> Xray outbound (reality, mode, extra)', () => {
  const s = parseLink('vless://uuid@cdn6.example.top:443?encryption=none&security=reality&sni=www.ozon.ru&fp=chrome&pbk=PK&sid=ab&type=xhttp&path=%2Fapi&mode=packet-up&extra=%7B%22xmux%22%3A%7B%7D%7D#X');
  assert.strictEqual(s.transport, 'xhttp');
  assert.ok(!s.outbound);
  assert.strictEqual(s.xray.protocol, 'vless');
  assert.deepStrictEqual(s.xray.settings, { address: 'cdn6.example.top', port: 443, id: 'uuid', encryption: 'none' });
  const st = s.xray.streamSettings;
  assert.deepStrictEqual([st.network, st.security, st.xhttpSettings.path, st.xhttpSettings.mode], ['xhttp', 'reality', '/api', 'packet-up']);
  assert.deepStrictEqual(st.xhttpSettings.extra, { xmux: {} });
  assert.strictEqual(st.realitySettings.publicKey, 'PK');
});
t('vless xhttp over tls + trojan xhttp', () => {
  const a = parseLink('vless://u@h:443?type=xhttp&security=tls&sni=s.example&host=hh&alpn=h2');
  assert.deepStrictEqual(a.xray.streamSettings.tlsSettings, { serverName: 's.example', fingerprint: 'chrome', alpn: ['h2'] });
  assert.strictEqual(a.xray.streamSettings.xhttpSettings.host, 'hh');
  const b = parseLink('trojan://pw@h:443?type=xhttp#T');
  assert.strictEqual(b.xray.streamSettings.security, 'tls');
});
t('vmess base64 json', () => {
  const s = parseLink('vmess://' + b64(JSON.stringify({ v: '2', ps: 'VM', add: 'vm.example', port: '8443', id: 'uuid', aid: '0', net: 'ws', path: '/p', host: 'h', tls: 'tls', sni: 'sni.example' })));
  assert.strictEqual(s.outbound.server_port, 8443);
  assert.strictEqual(s.outbound.tls.server_name, 'sni.example');
  assert.strictEqual(s.outbound.transport.type, 'ws');
});
t('trojan defaults to TLS', () => {
  const s = parseLink('trojan://pass@t.example:443?sni=t.example#T');
  assert.strictEqual(s.outbound.tls.enabled, true);
  assert.strictEqual(s.outbound.password, 'pass');
});
t('shadowsocks SIP002 and legacy', () => {
  const a = parseLink('ss://' + b64('chacha20-ietf-poly1305:secret') + '@5.6.7.8:8388#SS');
  assert.deepStrictEqual([a.outbound.method, a.outbound.password, a.port], ['chacha20-ietf-poly1305', 'secret', 8388]);
  const b = parseLink('ss://' + b64('aes-256-gcm:p@ss:w@9.9.9.9:443') + '#old');
  assert.deepStrictEqual([b.outbound.method, b.outbound.password, b.host], ['aes-256-gcm', 'p@ss:w', '9.9.9.9']);
  const c = parseLink('ss://2022-blake3-aes-128-gcm:a2V5@1.1.1.1:443#plain');
  assert.strictEqual(c.outbound.method, '2022-blake3-aes-128-gcm');
});
t('hysteria2 with obfs', () => {
  const s = parseLink('hy2://pw@hy.example:8443?sni=hy.example&obfs=salamander&obfs-password=x&insecure=1#HY');
  assert.deepStrictEqual(s.outbound.obfs, { type: 'salamander', password: 'x' });
  assert.strictEqual(s.outbound.tls.insecure, true);
});
t('tuic', () => {
  const s = parseLink('tuic://uuid:pw@tu.example:443?congestion_control=bbr&alpn=h3&sni=tu.example#TU');
  assert.deepStrictEqual([s.outbound.uuid, s.outbound.password], ['uuid', 'pw']);
});
t('subscription body in base64 with junk lines', () => {
  const body = b64(['vless://u@a:443?security=tls#A', 'trojan://p@b:443#B', 'garbage', 'vless://u@c:443?type=xhttp#C', 'vless://u@d:443?type=quic2#D'].join('\n'));
  const r = parseMany(body);
  assert.strictEqual(r.servers.length, 3);
  assert.strictEqual(r.errors.length, 1);
});
t('sing-box JSON outbounds', () => {
  const r = parseMany(JSON.stringify({ outbounds: [{ type: 'vless', tag: 'mine', server: 'x', server_port: 1, uuid: 'u' }, { type: 'direct', tag: 'direct' }] }));
  assert.strictEqual(r.servers.length, 1);
  assert.strictEqual(r.servers[0].name, 'mine');
});

console.log('sing-box config');
const servers = parseMany('vless://u@a:443?security=tls#A\ntrojan://p@b:443#B').servers.map((s, i) => ({ ...s, id: 'id' + i }));
t('auto mode -> urltest proxy, proxy inbound', () => {
  const c = buildConfig({ servers, selected: 'auto', mode: 'proxy', directRu: true });
  const ut = c.outbounds.find((o) => o.tag === 'proxy');
  assert.strictEqual(ut.type, 'urltest');
  assert.deepStrictEqual(ut.outbounds, ['s0', 's1']);
  assert.strictEqual(c.inbounds[0].type, 'mixed');
  assert.ok(c.route.rules.some((r) => r.domain_suffix && r.outbound === 'direct'));
});
t('selected server + tun + games direct', () => {
  const c = buildConfig({ servers, selected: 'id1', mode: 'tun', directRu: false, gamesDirect: true, gameDirs: ['C:\\Games\\Escape From Tarkov'] });
  assert.deepStrictEqual([c.outbounds[0].tag, c.outbounds[0].type], ['proxy', 'trojan']);
  assert.strictEqual(c.inbounds[0].type, 'tun');
  const rule = c.route.rules.find((r) => r.process_path_regex);
  assert.strictEqual(rule.process_path_regex[0], '(?i)^C:\\\\Games\\\\Escape From Tarkov\\\\');
  assert.ok(new RegExp(rule.process_path_regex[0].replace('(?i)', ''), 'i').test('c:\\games\\escape from tarkov\\EscapeFromTarkov.exe'));
});

t('xhttp server goes through Xray socks; TUN excludes Xray process', () => {
  const { buildXrayConfig } = require('../src/main/net/vpn');
  const mix = parseMany('vless://u@a:443?security=tls#A\nvless://u@x:443?type=xhttp&security=tls#X1\nvless://u@y:443?type=xhttp&security=tls#X2').servers.map((s, i) => ({ ...s, id: 'm' + i }));
  const c = buildConfig({ servers: mix, selected: 'auto', mode: 'tun', directRu: true, xrayBase: 2100 });
  const socks = c.outbounds.filter((o) => o.type === 'socks');
  assert.deepStrictEqual(socks.map((o) => o.server_port), [2100, 2101]);
  assert.deepStrictEqual(c.outbounds.find((o) => o.tag === 'proxy').outbounds, ['s0', 's1', 's2']);
  assert.deepStrictEqual(c.route.rules[1], { process_name: ['arcadia-xray.exe'], outbound: 'direct' });
  const x = buildXrayConfig(mix.filter((s) => s.xray), 2100);
  assert.deepStrictEqual(x.inbounds.map((i) => i.port), [2100, 2101]);
  assert.strictEqual(x.outbounds[1].tag, 'out1');
  assert.deepStrictEqual(x.routing.rules[1], { type: 'field', inboundTag: ['in1'], outboundTag: 'out1' });
  const one = buildConfig({ servers: mix, selected: 'm2', mode: 'proxy', xrayBase: 2100 });
  assert.deepStrictEqual(one.outbounds.map((o) => [o.tag, o.type, o.server_port]), [['proxy', 'socks', 2100], ['direct', 'direct', undefined]]);
  assert.ok(!one.route.rules.some((r) => r.process_name));
});

console.log('zapret');
const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'zapret-'));
fs.mkdirSync(path.join(dir, 'bin')); fs.mkdirSync(path.join(dir, 'lists'));
fs.writeFileSync(path.join(dir, 'bin', 'winws.exe'), 'x');
fs.writeFileSync(path.join(dir, 'general (ALT).bat'), [
  '@echo off', 'chcp 65001 > nul', 'cd /d "%~dp0"', 'call service.bat status_zapret', 'call service.bat load_game_filter', 'echo:',
  'set "BIN=%~dp0bin\\"', 'set "LISTS=%~dp0lists\\"', 'cd /d %BIN%', '',
  'start "zapret: %~n0" /min "%BIN%winws.exe" --wf-tcp=80,443,%GameFilter% --wf-udp=443,%GameFilter% ^',
  '--filter-udp=443 --hostlist="%LISTS%list-general.txt" --hostlist="%LISTS%list-general-user.txt" --dpi-desync=fake ^',
  '--new --filter-tcp=443 --dpi-desync=multisplit',
].join('\r\n'));
fs.writeFileSync(path.join(dir, 'general.bat'), 'start "" /min "%~dp0bin\\winws.exe" --wf-tcp=443');
fs.writeFileSync(path.join(dir, 'service.bat'), 'tasklist | find "winws.exe"');
fs.writeFileSync(path.join(dir, 'readme.bat'), 'echo hi');
t('lists strategies, general first, skips service.bat', () => assert.deepStrictEqual(listStrategies(dir), ['general.bat', 'general (ALT).bat']));
t('parses winws command with variables, ^ continuations and game filter', () => {
  const p = parseBat(path.join(dir, 'general (ALT).bat'), { gameFilter: true });
  assert.strictEqual(p.exe, path.join(dir, 'bin', 'winws.exe'));
  assert.ok(p.args.startsWith('--wf-tcp=80,443,1024-65535 --wf-udp=443,1024-65535'));
  assert.ok(p.args.includes(`--hostlist="${path.join(dir, 'lists')}`) && /lists[\\/]list-general-user\.txt"/.test(p.args));
  assert.ok(p.args.endsWith('--new --filter-tcp=443 --dpi-desync=multisplit'));
  assert.ok(!/%/.test(p.args));
});
t('game filter off -> 12', () => assert.ok(parseBat(path.join(dir, 'general (ALT).bat')).args.startsWith('--wf-tcp=80,443,12 ')));
fs.rmSync(dir, { recursive: true, force: true });
console.log(`\n${passed} passed`);
