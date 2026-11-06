//! VPN on the sing-box core (+ Xray for xhttp servers): servers from your own links / subscriptions,
//! "system proxy" mode (no admin rights) or "TUN" mode (all traffic, admin rights).
use crate::util::common::nz;
use super::links::parse_many;
use crate::app::app;
use crate::util::common::{exists, hash_id, now_ms, parent, s};
use crate::util::winsys::{process_names, ps_quote, quiet, refresh_inet_settings, run_elevated};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Child;

pub const CORE_NAME: &str = "arcadia-core.exe";
pub const XRAY_NAME: &str = "arcadia-xray.exe";

#[derive(Default)]
pub struct Vpn {
    pub child: Option<Child>,
    pub xray_child: Option<Child>,
    pub running: bool,
    pub running_mode: String,
    pub busy: bool,
    pub error: String,
    pub installing: String,
    pub xray_installing: String,
    pub core_version: String,
    pub xray_version: String,
    pub pings: HashMap<String, Value>,
}

pub fn dir() -> PathBuf {
    let d = app().data_dir.join("vpn");
    let _ = std::fs::create_dir_all(&d);
    d
}
pub fn core_dir() -> PathBuf {
    let d = app().data_dir.join("core");
    let _ = std::fs::create_dir_all(&d);
    d
}
pub fn core_path() -> Option<String> {
    let p = core_dir().join(CORE_NAME);
    p.exists().then(|| p.to_string_lossy().to_string())
}
pub fn xray_path() -> Option<String> {
    let p = core_dir().join(XRAY_NAME);
    p.exists().then(|| p.to_string_lossy().to_string())
}

fn cfg() -> Value {
    app().store.lock().unwrap().settings["vpn"].clone()
}
fn save_cfg(patch: Value) {
    app().store.lock().unwrap().set_settings(json!({ "vpn": patch }));
}

/// Servers used by a connection: all of them for «Автовыбор», otherwise the chosen one
pub fn pick_servers(servers: &[Value], selected: &str) -> Vec<Value> {
    if selected == "auto" {
        return servers.to_vec();
    }
    servers.iter().find(|s| s["id"] == selected).or_else(|| servers.first()).cloned().into_iter().collect()
}

/// Xray runs xhttp servers; each one is exposed to sing-box as a local SOCKS port
pub fn build_xray_config(xs: &[Value], base: u16, log: Option<&str>) -> Value {
    let mut lg = json!({ "loglevel": "warning", "access": "none" });
    if let Some(l) = log {
        lg["error"] = json!(l);
    }
    let mut outs: Vec<Value> = xs.iter().enumerate().map(|(i, s)| { let mut o = s["xray"].clone(); o["tag"] = json!(format!("out{}", i)); o }).collect();
    outs.push(json!({ "protocol": "freedom", "tag": "direct" }));
    json!({
        "log": lg,
        "inbounds": xs.iter().enumerate().map(|(i, _)| json!({ "tag": format!("in{}", i), "listen": "127.0.0.1", "port": base + i as u16, "protocol": "socks", "settings": { "auth": "noauth", "udp": true, "ip": "127.0.0.1" } })).collect::<Vec<_>>(),
        "outbounds": outs,
        "routing": { "rules": xs.iter().enumerate().map(|(i, _)| json!({ "type": "field", "inboundTag": [format!("in{}", i)], "outboundTag": format!("out{}", i) })).collect::<Vec<_>>() },
    })
}

pub struct BuildOpts<'a> {
    pub selected: &'a str,
    pub mode: &'a str,
    pub direct_ru: bool,
    pub games_direct: bool,
    pub game_dirs: Vec<String>,
    pub log: Option<String>,
    pub port: u16,
    pub xray_base: u16,
    /// only these programs go through VPN (TUN); empty = everything
    pub only_apps: Vec<String>,
    /// these programs never use VPN (TUN)
    pub bypass_apps: Vec<String>,
    pub kill_switch: bool,
}

fn re_escape(s: &str) -> String {
    regex::escape(s)
}

/// sing-box config (format of sing-box 1.12+)
pub fn build_config(servers: &[Value], o: &BuildOpts) -> Result<Value, String> {
    if servers.is_empty() {
        return Err("Добавьте хотя бы один сервер".into());
    }
    let used = pick_servers(servers, o.selected);
    let mut xi: u16 = 0;
    let mut outbounds: Vec<Value> = used
        .iter()
        .enumerate()
        .map(|(i, s)| {
            if s.get("xray").map(|x| x.is_object()).unwrap_or(false) {
                let ob = json!({ "type": "socks", "tag": format!("s{}", i), "server": "127.0.0.1", "server_port": o.xray_base + xi, "version": "5" });
                xi += 1;
                ob
            } else {
                let mut ob = s["outbound"].clone();
                ob["tag"] = json!(format!("s{}", i));
                ob
            }
        })
        .collect();
    if outbounds.len() > 1 {
        let tags: Vec<Value> = outbounds.iter().map(|x| x["tag"].clone()).collect();
        outbounds.push(json!({ "type": "urltest", "tag": "proxy", "outbounds": tags, "url": "https://www.gstatic.com/generate_204", "interval": "3m", "tolerance": 80 }));
    } else {
        outbounds[0]["tag"] = json!("proxy");
    }
    outbounds.push(json!({ "type": "direct", "tag": "direct" }));
    let ru = json!(["ru", "su", "xn--p1ai", "xn--d1acj3b"]);
    let mut rules = vec![json!({ "action": "sniff" }), json!({ "protocol": "dns", "action": "hijack-dns" }), json!({ "ip_is_private": true, "outbound": "direct" })];
    let tun = o.mode == "tun";
    if tun && xi > 0 {
        // Xray's own connections must bypass the TUN, or they loop back into it
        rules.insert(1, json!({ "process_name": [XRAY_NAME], "outbound": "direct" }));
    }
    if tun && !o.bypass_apps.is_empty() {
        rules.push(json!({ "process_name": o.bypass_apps, "outbound": "direct" }));
    }
    if o.direct_ru {
        rules.push(json!({ "domain_suffix": ru, "outbound": "direct" }));
    }
    if tun && o.games_direct && !o.game_dirs.is_empty() {
        let rx: Vec<String> = o.game_dirs.iter().map(|d| format!("(?i)^{}\\\\", re_escape(d.trim_end_matches(['\\', '/'])))).collect();
        rules.push(json!({ "process_path_regex": rx, "outbound": "direct" }));
    }
    let mut route = json!({ "rules": rules, "final": "proxy", "auto_detect_interface": true, "default_domain_resolver": "dns-local" });
    if tun && !o.only_apps.is_empty() {
        // split tunnel: listed programs through VPN, everything else directly
        route["rules"].as_array_mut().unwrap().push(json!({ "process_name": o.only_apps, "outbound": "proxy" }));
        route["final"] = json!("direct");
    }
    let inbound = if tun {
        json!({ "type": "tun", "tag": "tun-in", "interface_name": "Arcadia", "address": ["172.19.0.1/30"], "mtu": 9000, "auto_route": true, "strict_route": true, "stack": "mixed" })
    } else {
        json!({ "type": "mixed", "tag": "mixed-in", "listen": "127.0.0.1", "listen_port": o.port })
    };
    let mut log = json!({ "level": "warn", "timestamp": true });
    if let Some(l) = &o.log {
        log["output"] = json!(l);
    }
    Ok(json!({
        "log": log,
        "dns": {
            "servers": [{ "type": "https", "tag": "dns-remote", "server": "1.1.1.1", "detour": "proxy" }, { "type": "local", "tag": "dns-local" }],
            "rules": if o.direct_ru { json!([{ "domain_suffix": ru, "server": "dns-local" }]) } else { json!([]) },
            "final": "dns-remote", "strategy": "prefer_ipv4"
        },
        "inbounds": [inbound],
        "outbounds": outbounds,
        "route": route,
    }))
}

pub fn state(v: &Vpn) -> Value {
    let c = cfg();
    let servers: Vec<Value> = c["servers"].as_array().cloned().unwrap_or_default();
    json!({
        "servers": servers.iter().map(|s| json!({ "id": s["id"], "name": s["name"], "host": s["host"], "port": s["port"], "proto": s["proto"], "transport": s.get("transport").cloned().unwrap_or(json!("")), "subId": s.get("subId").cloned().unwrap_or(json!("")), "udp": s["udp"] == true, "ping": v.pings.get(s["id"].as_str().unwrap_or("")).cloned() })).collect::<Vec<_>>(),
        "subs": c["subs"], "selected": c.get("selected").cloned().unwrap_or(json!("auto")), "mode": c.get("mode").cloned().unwrap_or(json!("proxy")),
        "directRu": c["directRu"] != false, "gamesDirect": c["gamesDirect"] != false, "stopOnExit": c["stopOnExit"] != false, "autostart": c["autostart"] == true,
        "port": c["port"].as_u64().unwrap_or(2080),
        "subsAutoHours": c["subsAutoHours"].as_u64().unwrap_or(0), "killSwitch": c["killSwitch"] == true,
        "appsMode": c.get("appsMode").cloned().unwrap_or(json!("all")), "apps": c.get("apps").cloned().unwrap_or(json!([])),
        "running": v.running, "runningMode": if v.running { json!(if v.running_mode.is_empty() { s(&c, "lastMode") } else { v.running_mode.clone() }) } else { json!("") },
        "busy": v.busy, "error": v.error, "dropped": c["dropped"] == true,
        "core": { "installed": core_path().is_some(), "version": v.core_version, "installing": v.installing },
        "xray": { "needed": servers.iter().any(|s| s.get("xray").is_some()), "installed": xray_path().is_some(), "version": v.xray_version, "installing": v.xray_installing },
    })
}

pub fn detect_versions() {
    let cv = core_path().and_then(|p| quiet(&p).arg("version").output().ok()).map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    let xv = xray_path().and_then(|p| quiet(&p).arg("version").output().ok()).map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    let pick = |t: &str, re: &str| regex::Regex::new(re).unwrap().captures(t).map(|c| c[1].to_string()).unwrap_or_default();
    let mut v = app().vpn.lock().unwrap();
    v.core_version = pick(&cv, r"(?i)version\s+([\w.\-+]+)");
    v.xray_version = pick(&xv, r"(?i)Xray\s+([\w.\-+]+)");
}

// ---------------- servers ----------------
pub fn add_from_text(text: &str, sub_id: &str) -> Value {
    let (servers, errors) = parse_many(text);
    let c = cfg();
    let mut list: Vec<Value> = c["servers"].as_array().cloned().unwrap_or_default();
    let mut known: std::collections::HashSet<String> = list.iter().filter_map(|s| s["id"].as_str().map(String::from)).collect();
    let mut added = 0;
    for s in &servers {
        let key = if s.get("outbound").is_some() { s["outbound"].to_string() } else { s["xray"].to_string() };
        let id = format!("v{}", hash_id(&key));
        if !known.insert(id.clone()) {
            continue;
        }
        let mut item = json!({ "id": id, "name": s["name"], "host": s["host"], "port": s["port"], "proto": s["proto"], "udp": s["udp"] == true, "link": s.get("link").cloned().unwrap_or(json!("")), "subId": sub_id });
        if s.get("xray").is_some() {
            item["xray"] = s["xray"].clone();
            item["transport"] = s["transport"].clone();
        } else {
            item["outbound"] = s["outbound"].clone();
        }
        list.push(item);
        added += 1;
    }
    save_cfg(json!({ "servers": list }));
    json!({ "added": added, "found": servers.len(), "errors": errors })
}

async fn fetch_sub(url: &str) -> Result<String, String> {
    let r = app().http.get(url).header("User-Agent", "v2rayN/7.10").send().await.map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("Подписка ответила ошибкой {}", r.status().as_u16()));
    }
    r.text().await.map_err(|e| e.to_string())
}

pub async fn add_sub(url: &str, name: Option<String>) -> Result<Value, String> {
    let id = format!("s{}", hash_id(url));
    let body = fetch_sub(url).await?;
    let c = cfg();
    let mut subs: Vec<Value> = c["subs"].as_array().cloned().unwrap_or_default().into_iter().filter(|s| s["id"] != id.as_str()).collect();
    let host = url::Url::parse(url).ok().and_then(|u| u.host_str().map(String::from)).unwrap_or_else(|| url.to_string());
    let servers: Vec<Value> = c["servers"].as_array().cloned().unwrap_or_default().into_iter().filter(|s| s["subId"] != id.as_str()).collect();
    save_cfg(json!({ "servers": servers }));
    let mut r = add_from_text(&body, &id);
    subs.push(json!({ "id": id, "url": url, "name": name.unwrap_or(host), "updatedAt": now_ms(), "count": r["found"] }));
    save_cfg(json!({ "subs": subs }));
    if r["found"] == 0 {
        r["errors"].as_array_mut().unwrap().insert(0, json!("В подписке не найдено серверов"));
    }
    r["sub"] = json!(true);
    Ok(r)
}

pub async fn add_input(text: &str) -> Result<Value, String> {
    let t = text.trim();
    let parts: Vec<&str> = t.split_whitespace().collect();
    if parts.len() == 1 && (parts[0].starts_with("https://") || parts[0].starts_with("http://")) {
        return add_sub(parts[0], None).await;
    }
    let mut r = add_from_text(t, "");
    if r["found"] == 0 && r["errors"].as_array().map(|a| a.is_empty()).unwrap_or(true) {
        r["errors"] = json!(["Ссылки не распознаны"]);
    }
    Ok(r)
}

pub async fn refresh_subs() -> Value {
    let subs: Vec<Value> = cfg()["subs"].as_array().cloned().unwrap_or_default();
    let (mut total, mut errors) = (0, vec![]);
    for sb in subs {
        match add_sub(&s(&sb, "url"), Some(s(&sb, "name"))).await {
            Ok(r) => total += r["found"].as_i64().unwrap_or(0),
            Err(e) => errors.push(format!("{}: {}", s(&sb, "name"), e)),
        }
    }
    json!({ "found": total, "errors": errors })
}

pub fn remove_server(id: &str) {
    let c = cfg();
    let servers: Vec<Value> = c["servers"].as_array().cloned().unwrap_or_default().into_iter().filter(|s| s["id"] != id).collect();
    let sel = if c["selected"] == id { json!("auto") } else { c["selected"].clone() };
    save_cfg(json!({ "servers": servers, "selected": sel }));
}
pub fn remove_sub(id: &str) {
    let c = cfg();
    let subs: Vec<Value> = c["subs"].as_array().cloned().unwrap_or_default().into_iter().filter(|s| s["id"] != id).collect();
    let servers: Vec<Value> = c["servers"].as_array().cloned().unwrap_or_default().into_iter().filter(|s| s["subId"] != id).collect();
    save_cfg(json!({ "subs": subs, "servers": servers }));
}
pub fn rename_server(id: &str, name: &str) {
    let servers: Vec<Value> = cfg()["servers"].as_array().cloned().unwrap_or_default().into_iter().map(|mut s| { if s["id"] == id { s["name"] = json!(name); } s }).collect();
    save_cfg(json!({ "servers": servers }));
}

pub async fn ping_all() {
    let servers: Vec<Value> = cfg()["servers"].as_array().cloned().unwrap_or_default();
    {
        let mut v = app().vpn.lock().unwrap();
        for sv in &servers {
            v.pings.insert(s(sv, "id"), if sv["udp"] == true { json!("udp") } else { json!(0) });
        }
    }
    app().push();
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(8));
    let mut tasks = vec![];
    for sv in servers.into_iter().filter(|s| s["udp"] != true) {
        let sem = sem.clone();
        tasks.push(tauri::async_runtime::spawn(async move {
            let _p = sem.acquire().await;
            let addr = format!("{}:{}", s(&sv, "host"), sv["port"].as_u64().unwrap_or(443));
            let t0 = std::time::Instant::now();
            let r = tokio::time::timeout(std::time::Duration::from_secs(3), tokio::net::TcpStream::connect(addr)).await;
            let ms = match r { Ok(Ok(_)) => t0.elapsed().as_millis() as i64, _ => -1 };
            app().vpn.lock().unwrap().pings.insert(s(&sv, "id"), json!(ms));
            app().push();
        }));
    }
    for t in tasks {
        let _ = t.await;
    }
}

// ---------------- cores ----------------
async fn install_from_github(repo: &str, asset_re: &str, exe_name: &str, target: &str, label: &str, extra_re: &str, set: impl Fn(&str)) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("Доступно только в Windows".into());
    }
    set("Ищем последнюю версию…");
    let r = app().http.get(format!("https://api.github.com/repos/{}/releases/latest", repo)).header("User-Agent", "ArcadiaLauncher").header("Accept", "application/vnd.github+json").send().await.map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("GitHub недоступен ({})", r.status().as_u16()));
    }
    let rel: Value = r.json().await.map_err(|e| e.to_string())?;
    let are = regex::Regex::new(asset_re).unwrap();
    let a = rel["assets"].as_array().and_then(|a| a.iter().find(|x| are.is_match(x["name"].as_str().unwrap_or("")))).ok_or(format!("Не найден архив {} для Windows", label))?;
    set(&format!("Скачиваем {} {}…", label, s(&rel, "tag_name")));
    let z = app().http.get(s(a, "browser_download_url")).header("User-Agent", "ArcadiaLauncher").send().await.map_err(|e| e.to_string())?;
    if !z.status().is_success() {
        return Err(format!("Не удалось скачать ({})", z.status().as_u16()));
    }
    let bytes = z.bytes().await.map_err(|e| e.to_string())?;
    set("Распаковываем…");
    let dir = core_dir();
    let xre = regex::Regex::new(extra_re).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut found = false;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = f.name().rsplit('/').next().unwrap_or("").to_string();
        let dest = if name.eq_ignore_ascii_case(exe_name) { found = true; dir.join(target) } else if xre.is_match(&name) { dir.join(&name) } else { continue };
        let mut out = std::fs::File::create(&dest).map_err(|e| format!("{} (ядро сейчас запущено?)", e))?;
        std::io::copy(&mut f, &mut out).map_err(|e| e.to_string())?;
    }
    if !found {
        return Err(format!("В архиве нет {}", exe_name));
    }
    Ok(())
}

pub async fn install_core() -> Result<(), String> {
    if app().vpn.lock().unwrap().running {
        let _ = tauri::async_runtime::spawn_blocking(|| stop(true)).await;
    }
    let r = install_from_github("SagerNet/sing-box", r"(?i)windows-amd64\.zip$", "sing-box.exe", CORE_NAME, "sing-box", r"(?i)\.dll$", |t| {
        app().vpn.lock().unwrap().installing = t.to_string();
        app().push();
    })
    .await;
    app().vpn.lock().unwrap().installing.clear();
    if let Err(e) = &r {
        app().vpn.lock().unwrap().error = format!("{}. Можно скачать sing-box вручную с github.com/SagerNet/sing-box/releases и указать sing-box.exe.", e);
    }
    detect_versions();
    app().push();
    r
}

pub async fn install_xray() -> Result<(), String> {
    let r = install_from_github("XTLS/Xray-core", r"(?i)^Xray-windows-64\.zip$", "xray.exe", XRAY_NAME, "Xray", r"(?i)\.(dll|dat)$", |t| {
        app().vpn.lock().unwrap().xray_installing = t.to_string();
        app().push();
    })
    .await;
    app().vpn.lock().unwrap().xray_installing.clear();
    if let Err(e) = &r {
        app().vpn.lock().unwrap().error = format!("{}. Можно скачать Xray вручную (Xray-windows-64.zip) и указать xray.exe.", e);
    }
    detect_versions();
    app().push();
    r
}

pub fn set_core_file(file: &str, xray: bool) -> Result<(), String> {
    let dir = core_dir();
    std::fs::copy(file, dir.join(if xray { XRAY_NAME } else { CORE_NAME })).map_err(|e| e.to_string())?;
    let re = if xray { r"(?i)\.(dll|dat)$" } else { r"(?i)\.dll$" };
    let re = regex::Regex::new(re).unwrap();
    if let Ok(rd) = std::fs::read_dir(parent(file)) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if re.is_match(&n) {
                let _ = std::fs::copy(e.path(), dir.join(n));
            }
        }
    }
    detect_versions();
    Ok(())
}

// ---------------- system proxy ----------------
#[cfg(windows)]
fn inet_key() -> Option<winreg::RegKey> {
    use winreg::enums::*;
    winreg::RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings", KEY_READ | KEY_WRITE).ok()
}

#[cfg(windows)]
fn read_system_proxy() -> Value {
    let Some(k) = inet_key() else { return json!(null) };
    let en: u32 = k.get_value("ProxyEnable").unwrap_or(0);
    let sv: String = k.get_value("ProxyServer").unwrap_or_default();
    let ov: String = k.get_value("ProxyOverride").unwrap_or_default();
    json!({ "enable": en, "server": sv, "override": ov })
}
#[cfg(windows)]
fn write_system_proxy(enable: bool, server: &str, over: &str) {
    if let Some(k) = inet_key() {
        let _ = k.set_value("ProxyEnable", &(enable as u32));
        let _ = k.set_value("ProxyServer", &server.to_string());
        let _ = k.set_value("ProxyOverride", &over.to_string());
    }
    refresh_inet_settings();
}
#[cfg(not(windows))]
fn read_system_proxy() -> Value {
    json!(null)
}
#[cfg(not(windows))]
fn write_system_proxy(_e: bool, _s: &str, _o: &str) {
    refresh_inet_settings();
}

pub fn clear_proxy() {
    let c = cfg();
    if c["proxySet"] != true {
        return;
    }
    let p = &c["prevProxy"];
    write_system_proxy(p["enable"].as_u64().unwrap_or(0) == 1, &s(p, "server"), &s(p, "override"));
    save_cfg(json!({ "proxySet": false }));
}

fn core_running() -> bool {
    process_names().contains(CORE_NAME)
}

fn wait_for(want: bool, ms: u64) -> bool {
    let until = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    while std::time::Instant::now() < until {
        if core_running() == want {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    false
}

fn log_tail(f: &str) -> String {
    let t = std::fs::read_to_string(dir().join(f)).unwrap_or_default();
    let re = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    let t = re.replace_all(&t, "");
    let lines: Vec<&str> = t.trim().lines().collect();
    lines[lines.len().saturating_sub(3)..].join("\n")
}

fn kill_xray(v: &mut Vpn) {
    if let Some(mut c) = v.xray_child.take() {
        let _ = c.kill();
    }
    if cfg!(windows) {
        let _ = quiet("taskkill.exe").args(["/F", "/IM", XRAY_NAME]).output();
    }
}

/// Blocking: connects with the current settings
pub fn start() -> Result<(), String> {
    let core = core_path().ok_or("Ядро sing-box не установлено")?;
    {
        let mut v = app().vpn.lock().unwrap();
        v.busy = true;
        v.error.clear();
    }
    app().push();
    let r = (|| -> Result<(), String> {
        if app().vpn.lock().unwrap().running {
            stop(true)?;
        }
        let c = cfg();
        let mode = s(&c, "mode");
        let mode = if mode.is_empty() { "proxy".to_string() } else { mode };
        let port = c["port"].as_u64().unwrap_or(2080) as u16;
        let servers: Vec<Value> = c["servers"].as_array().cloned().unwrap_or_default();
        let selected = nz(s(&c, "selected"), "auto".to_string());
        let used = pick_servers(&servers, &selected);
        let xray_used: Vec<Value> = used.iter().filter(|s| s.get("xray").is_some()).cloned().collect();
        let xray_base = port + 20;
        let d = dir();
        if !xray_used.is_empty() {
            let xp = xray_path().ok_or("Для серверов с xhttp нужно ядро Xray — нажмите «Скачать Xray» в окне VPN")?;
            let xlog = d.join("xray.log").to_string_lossy().to_string();
            let _ = std::fs::write(&xlog, "");
            let xfile = d.join("xray.json");
            std::fs::write(&xfile, serde_json::to_string_pretty(&build_xray_config(&xray_used, xray_base, Some(&xlog))).unwrap()).map_err(|e| e.to_string())?;
            let test = quiet(&xp).args(["run", "-test", "-c"]).arg(&xfile).current_dir(core_dir()).output().map_err(|e| e.to_string())?;
            if !test.status.success() {
                let t = String::from_utf8_lossy(&test.stdout).to_string() + &String::from_utf8_lossy(&test.stderr);
                return Err(format!("Ошибка в конфигурации Xray: {}", t.trim().lines().rev().take(2).collect::<Vec<_>>().join(" ")));
            }
            let mut v = app().vpn.lock().unwrap();
            kill_xray(&mut v);
            std::thread::sleep(std::time::Duration::from_millis(300));
            v.xray_child = Some(quiet(&xp).args(["run", "-c"]).arg(&xfile).current_dir(core_dir()).spawn().map_err(|e| e.to_string())?);
            drop(v);
            std::thread::sleep(std::time::Duration::from_millis(900));
            let alive = app().vpn.lock().unwrap().xray_child.as_mut().map(|c| c.try_wait().ok().flatten().is_none()).unwrap_or(false);
            if !alive {
                return Err(format!("Xray не запустился: {}", nz(log_tail("xray.log"), "неизвестная ошибка".to_string())));
            }
        } else {
            kill_xray(&mut app().vpn.lock().unwrap());
        }
        let logf = d.join("core.log").to_string_lossy().to_string();
        let _ = std::fs::write(&logf, "");
        let game_dirs: Vec<String> = app().store.lock().unwrap().games.values().filter(|g| g["missing"] != true && g["installed"] != false && !s(g, "installDir").is_empty()).map(|g| s(g, "installDir")).collect();
        let apps: Vec<String> = c["apps"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        let apps_mode = s(&c, "appsMode");
        let config = build_config(&servers, &BuildOpts {
            selected: &selected, mode: &mode, direct_ru: c["directRu"] != false, games_direct: c["gamesDirect"] != false, game_dirs, log: Some(logf), port, xray_base,
            only_apps: if apps_mode == "only" { apps.clone() } else { vec![] },
            bypass_apps: if apps_mode == "bypass" { apps } else { vec![] },
            kill_switch: c["killSwitch"] == true,
        })?;
        let cfg_file = d.join("config.json");
        std::fs::write(&cfg_file, serde_json::to_string_pretty(&config).unwrap()).map_err(|e| e.to_string())?;
        let chk = quiet(&core).args(["check", "-c"]).arg(&cfg_file).output().map_err(|e| e.to_string())?;
        if !chk.status.success() {
            let t = String::from_utf8_lossy(&chk.stderr).to_string() + &String::from_utf8_lossy(&chk.stdout);
            let re = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
            return Err(format!("Ошибка в конфигурации: {}", re.replace_all(t.trim(), "").lines().rev().take(2).collect::<Vec<_>>().join(" ")));
        }
        if mode == "tun" {
            run_elevated(
                &format!(
                    "Stop-Process -Name arcadia-core -Force\nStart-Process -FilePath {} -ArgumentList @('run','-c',{},'-D',{}) -WorkingDirectory {} -WindowStyle Hidden",
                    ps_quote(&core), ps_quote(&cfg_file.to_string_lossy()), ps_quote(&d.to_string_lossy()), ps_quote(&core_dir().to_string_lossy())
                ),
                false,
            )?;
        } else {
            let ch = quiet(&core).args(["run", "-c"]).arg(&cfg_file).arg("-D").arg(&d).current_dir(core_dir()).spawn().map_err(|e| e.to_string())?;
            app().vpn.lock().unwrap().child = Some(ch);
        }
        let up = wait_for(true, 8000);
        std::thread::sleep(std::time::Duration::from_millis(800));
        if !up || !core_running() {
            return Err(format!("VPN не запустился: {}", nz(log_tail("core.log"), "неизвестная ошибка".to_string())));
        }
        if mode != "tun" {
            if cfg()["proxySet"] != true {
                save_cfg(json!({ "prevProxy": read_system_proxy() }));
            }
            write_system_proxy(true, &format!("127.0.0.1:{}", port), "localhost;127.*;10.*;172.16.*;172.17.*;172.18.*;172.19.*;172.2*;172.30.*;172.31.*;192.168.*;<local>");
            save_cfg(json!({ "proxySet": true }));
        }
        save_cfg(json!({ "lastMode": mode, "dropped": false, "wantOn": true }));
        let mut v = app().vpn.lock().unwrap();
        v.running_mode = mode;
        v.running = true;
        Ok(())
    })();
    {
        let mut v = app().vpn.lock().unwrap();
        v.busy = false;
        if let Err(e) = &r {
            v.error = e.clone();
            kill_xray(&mut v);
        }
    }
    app().push();
    r
}

/// Blocking: disconnects. `internal` = part of a restart (keeps "wantOn")
pub fn stop(internal: bool) -> Result<(), String> {
    if !internal {
        let mut v = app().vpn.lock().unwrap();
        v.busy = true;
        v.error.clear();
    }
    app().push();
    {
        let mut v = app().vpn.lock().unwrap();
        if let Some(mut c) = v.child.take() {
            let _ = c.kill();
        }
        kill_xray(&mut v);
    }
    clear_proxy();
    let mut res = Ok(());
    if core_running() {
        let _ = quiet("taskkill.exe").args(["/F", "/IM", CORE_NAME]).output();
        if !wait_for(false, 1500) {
            res = run_elevated("Stop-Process -Name arcadia-core -Force", true);
        }
    }
    wait_for(false, 4000);
    if !internal {
        save_cfg(json!({ "wantOn": false, "dropped": false }));
    }
    {
        let mut v = app().vpn.lock().unwrap();
        v.running = false;
        if !internal {
            v.busy = false;
        }
        if let Err(e) = &res {
            v.error = e.clone();
        }
    }
    app().push();
    res
}

/// Process watcher: notices when the core dies on its own
pub fn set_status(names: &std::collections::HashSet<String>) -> bool {
    let r = names.contains(CORE_NAME);
    let mut v = app().vpn.lock().unwrap();
    if r == v.running || v.busy {
        return false;
    }
    v.running = r;
    if !r {
        if let Some(mut c) = v.xray_child.take() {
            let _ = c.kill();
        }
        v.child = None;
        drop(v);
        let c = cfg();
        // kill switch: keep the system proxy pointing at the dead port, so nothing leaks past the VPN
        if !(c["killSwitch"] == true && c["wantOn"] == true) {
            clear_proxy();
        }
        // the VPN dropped while the user wanted it on — the kill switch / auto-reconnect logic picks it up
        if cfg()["wantOn"] == true {
            save_cfg(json!({ "dropped": true }));
        }
    }
    true
}

pub fn shutdown() {
    let mut v = app().vpn.lock().unwrap();
    if let Some(mut c) = v.child.take() {
        let _ = c.kill();
    }
    if let Some(mut c) = v.xray_child.take() {
        let _ = c.kill();
    }
    drop(v);
    clear_proxy();
}

pub fn exists_core() -> bool {
    core_path().map(|p| exists(&p)).unwrap_or(false)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::links::parse_many;
    fn opts<'a>(selected: &'a str, mode: &'a str) -> BuildOpts<'a> {
        BuildOpts { selected, mode, direct_ru: true, games_direct: true, game_dirs: vec![], log: None, port: 2080, xray_base: 2100, only_apps: vec![], bypass_apps: vec![], kill_switch: false }
    }
    #[test]
    fn configs() {
        let mut sv = parse_many("vless://u@a:443?security=tls#A\ntrojan://p@b:443#B").0;
        for (i, s) in sv.iter_mut().enumerate() {
            s["id"] = json!(format!("id{}", i));
        }
        let c = build_config(&sv, &opts("auto", "proxy")).unwrap();
        let ut = c["outbounds"].as_array().unwrap().iter().find(|o| o["tag"] == "proxy").unwrap();
        assert_eq!(ut["type"], "urltest");
        assert_eq!(c["inbounds"][0]["type"], "mixed");
        let mut o = opts("id1", "tun");
        o.game_dirs = vec![r"C:\Games\Escape From Tarkov".into()];
        let c2 = build_config(&sv, &o).unwrap();
        assert_eq!((c2["outbounds"][0]["tag"].as_str().unwrap(), c2["outbounds"][0]["type"].as_str().unwrap()), ("proxy", "trojan"));
        let rule = c2["route"]["rules"].as_array().unwrap().iter().find(|r| r.get("process_path_regex").is_some()).unwrap();
        let rx = rule["process_path_regex"][0].as_str().unwrap();
        assert!(regex::Regex::new(rx).unwrap().is_match(r"c:\games\escape from tarkov\EscapeFromTarkov.exe"), "{}", rx);
        // xhttp via xray socks
        let mut mix = parse_many("vless://u@a:443?security=tls#A\nvless://u@x:443?type=xhttp&security=tls#X1\nvless://u@y:443?type=xhttp&security=tls#X2").0;
        for (i, s) in mix.iter_mut().enumerate() {
            s["id"] = json!(format!("m{}", i));
        }
        let c3 = build_config(&mix, &opts("auto", "tun")).unwrap();
        let ports: Vec<u64> = c3["outbounds"].as_array().unwrap().iter().filter(|o| o["type"] == "socks").map(|o| o["server_port"].as_u64().unwrap()).collect();
        assert_eq!(ports, vec![2100, 2101]);
        assert_eq!(c3["route"]["rules"][1], json!({ "process_name": [XRAY_NAME], "outbound": "direct" }));
        let xs: Vec<Value> = mix.iter().filter(|s| s.get("xray").is_some()).cloned().collect();
        let x = build_xray_config(&xs, 2100, None);
        assert_eq!(x["routing"]["rules"][1], json!({ "type": "field", "inboundTag": ["in1"], "outboundTag": "out1" }));
        // split tunnel
        let mut o4 = opts("auto", "tun");
        o4.only_apps = vec!["Discord.exe".into()];
        let c4 = build_config(&sv, &o4).unwrap();
        assert_eq!(c4["route"]["final"], "direct");
    }
}
