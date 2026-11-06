//! Share links (vless/vmess/trojan/ss/hysteria2/tuic) and subscriptions -> sing-box outbounds.
//! xhttp transport (unsupported by sing-box) becomes an Xray outbound instead.
use crate::util::common::nz;
use base64::Engine;
use serde_json::{json, Map, Value};
use std::collections::HashMap;

pub fn b64decode(s: &str) -> Option<String> {
    let mut t: String = s.trim().replace('-', "+").replace('_', "/").chars().filter(|c| !c.is_whitespace()).collect();
    while t.len() % 4 != 0 {
        t.push('=');
    }
    base64::engine::general_purpose::STANDARD.decode(t).ok().map(|b| String::from_utf8_lossy(&b).to_string())
}

fn looks_b64(s: &str) -> bool {
    let t: String = s.trim().chars().filter(|c| !c.is_whitespace()).collect();
    t.len() > 16 && t.chars().all(|c| c.is_ascii_alphanumeric() || "+/=_-".contains(c))
}

fn dec(s: &str) -> String {
    urlencoding::decode(s).map(|x| x.to_string()).unwrap_or_else(|_| s.to_string())
}

pub struct Url {
    pub user: String,
    pub host: String,
    pub port: u16,
    pub q: HashMap<String, String>,
    pub name: String,
}

pub fn parse_url(link: &str) -> Result<Url, String> {
    use once_cell::sync::Lazy;
    use regex::Regex;
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)^([a-z0-9+.-]+)://(?:([^@/?#]*)@)?(\[[^\]]+\]|[^:/?#]+)(?::(\d+))?/?(?:\?([^#]*))?(?:#(.*))?$").unwrap());
    let c = RE.captures(link.trim()).ok_or("Неверный формат ссылки")?;
    let mut q = HashMap::new();
    for part in c.get(5).map(|m| m.as_str()).unwrap_or("").split('&') {
        if part.is_empty() {
            continue;
        }
        let (k, v) = part.split_once('=').unwrap_or((part, ""));
        q.insert(dec(k), dec(&v.replace('+', "%20")));
    }
    Ok(Url {
        user: c.get(2).map(|m| dec(m.as_str())).unwrap_or_default(),
        host: c[3].trim_start_matches('[').trim_end_matches(']').to_string(),
        port: c.get(4).and_then(|m| m.as_str().parse().ok()).unwrap_or(0),
        q,
        name: c.get(6).map(|m| dec(m.as_str())).unwrap_or_default(),
    })
}

fn qs(q: &HashMap<String, String>, k: &str) -> String {
    q.get(k).cloned().unwrap_or_default()
}

fn tls_from(q: &HashMap<String, String>, host: &str, default_on: bool, alpn: Option<Vec<&str>>) -> Result<Option<Value>, String> {
    let sec = qs(q, "security").to_lowercase();
    let on = sec == "tls" || sec == "reality" || sec == "xtls" || (default_on && sec != "none");
    if !on {
        return Ok(None);
    }
    let sni = [qs(q, "sni"), qs(q, "peer"), qs(q, "host"), host.to_string()].into_iter().find(|x| !x.is_empty()).unwrap();
    let mut tls = json!({ "enabled": true, "server_name": sni });
    if ["1", "true"].contains(&qs(q, "allowInsecure").as_str()) || qs(q, "insecure") == "1" {
        tls["insecure"] = json!(true);
    }
    let a: Vec<String> = if !qs(q, "alpn").is_empty() { qs(q, "alpn").split(',').filter(|x| !x.is_empty()).map(String::from).collect() } else { alpn.unwrap_or_default().into_iter().map(String::from).collect() };
    if !a.is_empty() {
        tls["alpn"] = json!(a);
    }
    let fp = { let f = qs(q, "fp"); if f.is_empty() && sec == "reality" { "chrome".to_string() } else { f } };
    if !fp.is_empty() {
        tls["utls"] = json!({ "enabled": true, "fingerprint": if fp == "random" || fp == "randomized" { "random".to_string() } else { fp } });
    }
    if sec == "reality" {
        if qs(q, "pbk").is_empty() {
            return Err("В ссылке reality нет публичного ключа (pbk)".into());
        }
        tls["reality"] = json!({ "enabled": true, "public_key": qs(q, "pbk"), "short_id": qs(q, "sid") });
    }
    Ok(Some(tls))
}

fn transport_from(t: &str, q: &HashMap<String, String>) -> Result<Option<Value>, String> {
    let t = t.to_lowercase();
    let hosts = || -> Vec<String> { qs(q, "host").split(',').filter(|x| !x.is_empty()).map(String::from).collect() };
    let path = nz(qs(q, "path"), "/".to_string());
    Ok(match t.as_str() {
        "" | "tcp" | "raw" => {
            if qs(q, "headerType") == "http" { Some(json!({ "type": "http", "host": hosts(), "path": path })) } else { None }
        }
        "ws" | "websocket" => {
            let mut tr = json!({ "type": "ws", "path": path });
            let re = regex::Regex::new(r"[?&]ed=(\d+)").unwrap();
            if let Some(c) = re.captures(&path) {
                let clean = re.replace(&path, "").trim_end_matches('?').to_string();
                tr["path"] = json!(if clean.is_empty() { "/".to_string() } else { clean });
                tr["max_early_data"] = json!(c[1].parse::<i64>().unwrap_or(0));
                tr["early_data_header_name"] = json!("Sec-WebSocket-Protocol");
            }
            if !qs(q, "host").is_empty() {
                tr["headers"] = json!({ "Host": qs(q, "host") });
            }
            Some(tr)
        }
        "grpc" => Some(json!({ "type": "grpc", "service_name": nz(qs(q, "serviceName"), qs(q, "path")) })),
        "http" | "h2" => Some(json!({ "type": "http", "host": hosts(), "path": path })),
        "httpupgrade" => Some(json!({ "type": "httpupgrade", "host": qs(q, "host"), "path": path })),
        "quic" => Some(json!({ "type": "quic" })),
        o => return Err(format!("Неизвестный транспорт: {}", o)),
    })
}

fn is_xhttp(t: &str) -> bool {
    t.eq_ignore_ascii_case("xhttp") || t.eq_ignore_ascii_case("splithttp")
}

fn xray_stream(q: &HashMap<String, String>, host: &str, default_tls: bool) -> Result<Value, String> {
    let sec = { let s = qs(q, "security").to_lowercase(); if s.is_empty() && default_tls { "tls".to_string() } else { s } };
    let security = match sec.as_str() { "reality" => "reality", "tls" | "xtls" => "tls", _ => "none" };
    let mut xs = json!({ "path": nz(qs(q, "path"), "/".to_string()), "mode": nz(qs(q, "mode"), "auto".to_string()) });
    if !qs(q, "host").is_empty() {
        xs["host"] = json!(qs(q, "host"));
    }
    if let Ok(e) = serde_json::from_str::<Value>(&qs(q, "extra")) {
        xs["extra"] = e;
    }
    let mut st = json!({ "network": "xhttp", "security": security, "xhttpSettings": xs });
    let sni = [qs(q, "sni"), qs(q, "peer"), qs(q, "host"), host.to_string()].into_iter().find(|x| !x.is_empty()).unwrap();
    let fp = nz(qs(q, "fp"), "chrome".to_string());
    if security == "tls" {
        let mut t = json!({ "serverName": sni, "fingerprint": fp });
        if !qs(q, "alpn").is_empty() {
            t["alpn"] = json!(qs(q, "alpn").split(',').filter(|x| !x.is_empty()).collect::<Vec<_>>());
        }
        if ["1", "true"].contains(&qs(q, "allowInsecure").as_str()) || qs(q, "insecure") == "1" {
            t["allowInsecure"] = json!(true);
        }
        st["tlsSettings"] = t;
    } else if security == "reality" {
        if qs(q, "pbk").is_empty() {
            return Err("В ссылке reality нет публичного ключа (pbk)".into());
        }
        st["realitySettings"] = json!({ "serverName": sni, "fingerprint": fp, "publicKey": qs(q, "pbk"), "shortId": qs(q, "sid"), "spiderX": qs(q, "spx") });
    }
    Ok(st)
}

fn clean(mut v: Value) -> Value {
    if let Some(m) = v.as_object_mut() {
        m.retain(|_, x| !(x.is_null() || x == &json!("")));
    }
    v
}

fn named(u: &Url) -> String {
    if u.name.is_empty() { format!("{}:{}", u.host, u.port) } else { u.name.clone() }
}

/// Parsed server: {name, host, port, proto, udp?, transport?, outbound | xray}
pub fn parse_link(link: &str) -> Result<Value, String> {
    let l = link.trim();
    let scheme = l.split("://").next().unwrap_or("").to_lowercase();
    match scheme.as_str() {
        "vless" => {
            let u = parse_url(l)?;
            if u.user.is_empty() {
                return Err("В ссылке vless нет UUID".into());
            }
            let port = if u.port == 0 { 443 } else { u.port };
            let flow = qs(&u.q, "flow");
            if is_xhttp(&qs(&u.q, "type")) {
                let mut settings = json!({ "address": u.host, "port": port, "id": u.user, "encryption": nz(qs(&u.q, "encryption"), "none".to_string()) });
                if !flow.is_empty() && flow != "none" {
                    settings["flow"] = json!(flow);
                }
                return Ok(json!({ "name": named(&u), "host": u.host, "port": port, "proto": "VLESS", "transport": "xhttp", "xray": { "protocol": "vless", "settings": settings, "streamSettings": xray_stream(&u.q, &u.host, false)? } }));
            }
            let ob = clean(json!({
                "type": "vless", "server": u.host, "server_port": port, "uuid": u.user,
                "flow": if !flow.is_empty() && flow != "none" { json!(flow.trim_end_matches("-udp443")) } else { Value::Null },
                "packet_encoding": "xudp", "tls": tls_from(&u.q, &u.host, false, None)?, "transport": transport_from(&qs(&u.q, "type"), &u.q)?,
            }));
            Ok(json!({ "name": named(&u), "host": u.host, "port": port, "proto": "VLESS", "outbound": ob }))
        }
        "vmess" => {
            let body = l["vmess://".len()..].split('#').next().unwrap_or("");
            let j: Value = b64decode(body).and_then(|t| serde_json::from_str(&t).ok()).ok_or("Не удалось прочитать vmess-ссылку")?;
            let gs = |k: &str| match &j[k] { Value::String(s) => s.clone(), Value::Number(n) => n.to_string(), _ => String::new() };
            let port: u16 = gs("port").parse().unwrap_or(443);
            let mut q = HashMap::new();
            for (k, v) in [("security", if gs("tls") == "tls" { "tls".to_string() } else { "none".to_string() }), ("sni", gs("sni")), ("alpn", gs("alpn")), ("fp", gs("fp")), ("host", gs("host")), ("path", gs("path")), ("serviceName", gs("path")), ("headerType", gs("type")), ("mode", gs("mode"))] {
                q.insert(k.to_string(), v);
            }
            if j["allowInsecure"] == true || gs("allowInsecure") == "1" {
                q.insert("allowInsecure".into(), "1".into());
            }
            let name = nz(gs("ps"), format!("{}:{}", gs("add"), port));
            if is_xhttp(&gs("net")) {
                return Ok(json!({ "name": name, "host": gs("add"), "port": port, "proto": "VMess", "transport": "xhttp", "xray": { "protocol": "vmess", "settings": { "address": gs("add"), "port": port, "id": gs("id"), "security": nz(gs("scy"), "auto".to_string()) }, "streamSettings": xray_stream(&q, &gs("add"), false)? } }));
            }
            let ob = clean(json!({
                "type": "vmess", "server": gs("add"), "server_port": port, "uuid": gs("id"), "security": nz(gs("scy"), "auto".to_string()),
                "alter_id": gs("aid").parse::<i64>().unwrap_or(0), "packet_encoding": "xudp", "tls": tls_from(&q, &gs("add"), false, None)?, "transport": transport_from(&gs("net"), &q)?,
            }));
            Ok(json!({ "name": name, "host": gs("add"), "port": port, "proto": "VMess", "outbound": ob }))
        }
        "trojan" => {
            let u = parse_url(l)?;
            let port = if u.port == 0 { 443 } else { u.port };
            if is_xhttp(&qs(&u.q, "type")) {
                return Ok(json!({ "name": named(&u), "host": u.host, "port": port, "proto": "Trojan", "transport": "xhttp", "xray": { "protocol": "trojan", "settings": { "address": u.host, "port": port, "password": u.user }, "streamSettings": xray_stream(&u.q, &u.host, true)? } }));
            }
            let ob = clean(json!({ "type": "trojan", "server": u.host, "server_port": port, "password": u.user, "tls": tls_from(&u.q, &u.host, true, None)?, "transport": transport_from(&qs(&u.q, "type"), &u.q)? }));
            Ok(json!({ "name": named(&u), "host": u.host, "port": port, "proto": "Trojan", "outbound": ob }))
        }
        "ss" => parse_ss(l),
        "hysteria2" | "hy2" => {
            let u = parse_url(&l.replacen("hy2://", "hysteria2://", 1))?;
            let port = if u.port == 0 { 443 } else { u.port };
            let mut tls = json!({ "enabled": true, "server_name": nz(qs(&u.q, "sni"), u.host.clone()), "alpn": ["h3"] });
            if qs(&u.q, "insecure") == "1" || qs(&u.q, "allowInsecure") == "1" {
                tls["insecure"] = json!(true);
            }
            let mut o = json!({ "type": "hysteria2", "server": u.host, "server_port": port, "password": u.user, "tls": tls });
            let ob = qs(&u.q, "obfs");
            if !ob.is_empty() && ob != "none" {
                o["obfs"] = json!({ "type": ob, "password": qs(&u.q, "obfs-password") });
            }
            Ok(json!({ "name": named(&u), "host": u.host, "port": port, "proto": "Hysteria2", "udp": true, "outbound": o }))
        }
        "tuic" => {
            let u = parse_url(l)?;
            let port = if u.port == 0 { 443 } else { u.port };
            let (uuid, pw) = u.user.split_once(':').unwrap_or((&u.user, ""));
            let mut tls = json!({ "enabled": true, "server_name": nz(qs(&u.q, "sni"), u.host.clone()), "alpn": nz(qs(&u.q, "alpn"), "h3".to_string()).split(',').collect::<Vec<_>>() });
            if ["allow_insecure", "insecure", "allowInsecure"].iter().any(|k| qs(&u.q, k) == "1") {
                tls["insecure"] = json!(true);
            }
            let ob = clean(json!({ "type": "tuic", "server": u.host, "server_port": port, "uuid": uuid, "password": pw, "congestion_control": nz(qs(&u.q, "congestion_control"), "bbr".to_string()), "udp_relay_mode": qs(&u.q, "udp_relay_mode"), "tls": tls }));
            Ok(json!({ "name": named(&u), "host": u.host, "port": port, "proto": "TUIC", "udp": true, "outbound": ob }))
        }
        o => Err(format!("Протокол не поддерживается: {}", if o.is_empty() { &l[..l.len().min(20)] } else { o })),
    }
}

fn parse_ss(link: &str) -> Result<Value, String> {
    let mut rest = link["ss://".len()..].to_string();
    let mut name = String::new();
    if let Some(h) = rest.find('#') {
        name = dec(&rest[h + 1..]);
        rest.truncate(h);
    }
    let rest = rest.split('?').next().unwrap_or("").trim_end_matches('/').to_string();
    let (userinfo, hostport) = if let Some(at) = rest.rfind('@') {
        let mut ui = dec(&rest[..at]);
        if !ui.contains(':') && looks_b64(&ui) {
            ui = b64decode(&ui).unwrap_or(ui);
        }
        (ui, rest[at + 1..].to_string())
    } else {
        let d = b64decode(&rest).ok_or("Не удалось прочитать ss-ссылку")?;
        let at = d.rfind('@').ok_or("Не удалось прочитать ss-ссылку")?;
        (d[..at].to_string(), d[at + 1..].to_string())
    };
    let (method, password) = userinfo.split_once(':').ok_or("Не удалось прочитать ss-ссылку")?;
    let (h, p) = hostport.rsplit_once(':').ok_or("Не удалось прочитать ss-ссылку")?;
    let host = h.trim_start_matches('[').trim_end_matches(']').to_string();
    let port: u16 = p.parse().map_err(|_| "Не удалось прочитать ss-ссылку")?;
    if method.is_empty() {
        return Err("Не удалось прочитать ss-ссылку".into());
    }
    Ok(json!({ "name": if name.is_empty() { format!("{}:{}", host, port) } else { name }, "host": host, "port": port, "proto": "Shadowsocks", "outbound": { "type": "shadowsocks", "server": host, "server_port": port, "method": method, "password": password } }))
}

const PROXY_TYPES: &[&str] = &["vless", "vmess", "trojan", "shadowsocks", "hysteria2", "hysteria", "tuic", "wireguard", "socks", "http", "shadowtls", "anytls", "ssh"];

/// Text with links, a base64 subscription body, or sing-box JSON -> (servers, errors)
pub fn parse_many(text: &str) -> (Vec<Value>, Vec<String>) {
    let (mut servers, mut errors) = (vec![], vec![]);
    let mut body = text.trim().to_string();
    if body.is_empty() {
        return (servers, errors);
    }
    if body.starts_with('{') || body.starts_with('[') {
        if let Ok(j) = serde_json::from_str::<Value>(&body) {
            let list = if let Some(a) = j.as_array() { a.clone() } else if let Some(a) = j["outbounds"].as_array() { a.clone() } else { vec![j.clone()] };
            for o in list {
                let t = o["type"].as_str().unwrap_or("");
                if PROXY_TYPES.contains(&t) && o.get("server").is_some() {
                    let mut ob: Map<String, Value> = o.as_object().cloned().unwrap_or_default();
                    let tag = ob.remove("tag").and_then(|x| x.as_str().map(String::from));
                    servers.push(json!({ "name": tag.unwrap_or_else(|| format!("{}:{}", o["server"].as_str().unwrap_or(""), o["server_port"])), "host": o["server"], "port": o["server_port"], "proto": t, "outbound": ob }));
                }
            }
            if servers.is_empty() {
                errors.push("В JSON не найдено серверов".into());
            }
            return (servers, errors);
        }
    }
    if !body.contains("://") && looks_b64(&body) {
        if let Some(d) = b64decode(&body) {
            body = d;
        }
    }
    let yaml = regex::Regex::new(r"(?m)^\s*(proxies|port|mixed-port):").unwrap();
    if yaml.is_match(&body) {
        errors.push("Формат Clash/YAML не поддерживается — нужна подписка с ссылками (v2rayN) или JSON sing-box".into());
        return (servers, errors);
    }
    let scheme = regex::Regex::new(r"(?i)^[a-z0-9]+://").unwrap();
    for line in body.split(['\r', '\n']) {
        let l = line.trim();
        if l.is_empty() || !scheme.is_match(l) {
            continue;
        }
        match parse_link(l) {
            Ok(mut s) => {
                s["link"] = json!(l);
                servers.push(s);
            }
            Err(e) => errors.push(format!("{}…: {}", l.chars().take(40).collect::<String>(), e)),
        }
    }
    (servers, errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn b64(s: &str) -> String {
        base64::engine::general_purpose::STANDARD.encode(s)
    }
    #[test]
    fn links() {
        let s = parse_link("vless://11111111-2222-3333-4444-555555555555@1.2.3.4:443?encryption=none&flow=xtls-rprx-vision&security=reality&sni=www.microsoft.com&fp=chrome&pbk=PUBKEY&sid=ab12&type=tcp#%D0%9D%D0%B8%D0%B4%D0%B5%D1%80%D0%BB%D0%B0%D0%BD%D0%B4%D1%8B").unwrap();
        assert_eq!(s["name"], "Нидерланды");
        assert_eq!(s["outbound"]["tls"]["reality"], json!({ "enabled": true, "public_key": "PUBKEY", "short_id": "ab12" }));
        assert_eq!(s["outbound"]["flow"], "xtls-rprx-vision");
        assert!(s["outbound"].get("transport").is_none());
        let w = parse_link("vless://uuid@cdn.example.com:443?type=ws&security=tls&host=my.host&path=%2Fws%3Fed%3D2048#ws").unwrap();
        assert_eq!(w["outbound"]["transport"], json!({ "type": "ws", "path": "/ws", "max_early_data": 2048, "early_data_header_name": "Sec-WebSocket-Protocol", "headers": { "Host": "my.host" } }));
        let x = parse_link("vless://uuid@cdn6.example.top:443?encryption=none&security=reality&sni=www.ozon.ru&fp=chrome&pbk=PK&sid=ab&type=xhttp&path=%2Fapi&mode=packet-up&extra=%7B%22xmux%22%3A%7B%7D%7D#X").unwrap();
        assert_eq!(x["transport"], "xhttp");
        assert_eq!(x["xray"]["settings"], json!({ "address": "cdn6.example.top", "port": 443, "id": "uuid", "encryption": "none" }));
        assert_eq!(x["xray"]["streamSettings"]["xhttpSettings"]["mode"], "packet-up");
        assert_eq!(x["xray"]["streamSettings"]["realitySettings"]["publicKey"], "PK");
        let vm = parse_link(&format!("vmess://{}", b64(&json!({ "v": "2", "ps": "VM", "add": "vm.example", "port": "8443", "id": "uuid", "aid": "0", "net": "ws", "path": "/p", "host": "h", "tls": "tls", "sni": "sni.example" }).to_string()))).unwrap();
        assert_eq!(vm["outbound"]["server_port"], 8443);
        assert_eq!(vm["outbound"]["tls"]["server_name"], "sni.example");
        let a = parse_link(&format!("ss://{}@5.6.7.8:8388#SS", b64("chacha20-ietf-poly1305:secret"))).unwrap();
        assert_eq!(a["outbound"]["password"], "secret");
        let b = parse_link(&format!("ss://{}#old", b64("aes-256-gcm:p@ss:w@9.9.9.9:443"))).unwrap();
        assert_eq!((b["outbound"]["password"].as_str().unwrap(), b["host"].as_str().unwrap()), ("p@ss:w", "9.9.9.9"));
        let h = parse_link("hy2://pw@hy.example:8443?sni=hy.example&obfs=salamander&obfs-password=x&insecure=1#HY").unwrap();
        assert_eq!(h["outbound"]["obfs"], json!({ "type": "salamander", "password": "x" }));
        let t = parse_link("tuic://uuid:pw@tu.example:443?congestion_control=bbr&alpn=h3&sni=tu.example#TU").unwrap();
        assert_eq!(t["outbound"]["password"], "pw");
        let tr = parse_link("trojan://pass@t.example:443?sni=t.example#T").unwrap();
        assert_eq!(tr["outbound"]["tls"]["enabled"], true);
        let (sv, er) = parse_many(&b64("vless://u@a:443?security=tls#A\ntrojan://p@b:443#B\ngarbage\nvless://u@c:443?type=xhttp#C\nvless://u@d:443?type=quic2#D"));
        assert_eq!((sv.len(), er.len()), (3, 1));
    }
}
