//! Things that happen around games and on a schedule:
//!  • per-game actions before launch / after exit (zapret, VPN, power plan, programs) — restored afterwards;
//!  • Discord status for non-Steam games;
//!  • notifications; VPN watchdog (kill switch + reconnect); subscription auto-update.
use crate::app::app;
use crate::util::common::{now_ms, s};
use crate::util::winsys::{quiet, shell_exec};
use once_cell::sync::Lazy;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;

/// What to undo after a game: id -> (zapret was running, vpn was running, previous power scheme)
static RESTORE: Lazy<Mutex<HashMap<String, (Option<bool>, Option<bool>, Option<String>)>>> = Lazy::new(|| Mutex::new(HashMap::new()));

const HIGH_PERF: &str = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";

fn active_power_scheme() -> Option<String> {
    let out = quiet("powercfg").arg("/getactivescheme").output().ok()?;
    let t = String::from_utf8_lossy(&out.stdout).to_string();
    regex::Regex::new(r"([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})").unwrap().captures(&t).map(|c| c[1].to_string())
}

fn set_power_scheme(guid: &str) {
    let _ = quiet("powercfg").args(["/setactive", guid]).output();
}

fn run_program(p: &Value) {
    let path = s(p, "path");
    if path.is_empty() {
        return;
    }
    let _ = shell_exec("open", &path, &s(p, "args"), "", true, false);
}

/// Before the game starts. Errors from zapret/VPN are reported but don't stop the game.
pub fn before_launch(g: &Value) -> Result<(), String> {
    let a = &g["actions"];
    if !a.is_object() {
        return Ok(());
    }
    let id = s(g, "id");
    let mut rec: (Option<bool>, Option<bool>, Option<String>) = (None, None, None);
    let z_on = app().zapret.lock().unwrap().running;
    match s(a, "zapret").as_str() {
        "on" if !z_on => {
            rec.0 = Some(false);
            if let Err(e) = crate::commands::zapret_start(None) {
                app().toast("err", &format!("Zapret: {}", e));
            }
        }
        "off" if z_on => {
            rec.0 = Some(true);
            let _ = crate::commands::zapret_stop();
        }
        _ => {}
    }
    let v_on = app().vpn.lock().unwrap().running;
    match s(a, "vpn").as_str() {
        "on" if !v_on => {
            rec.1 = Some(false);
            if let Err(e) = crate::net::vpn::start() {
                app().toast("err", &format!("VPN: {}", e));
            }
        }
        "off" if v_on => {
            rec.1 = Some(true);
            let _ = crate::net::vpn::stop(false);
        }
        _ => {}
    }
    if a["power"] == true {
        if let Some(cur) = active_power_scheme() {
            if !cur.eq_ignore_ascii_case(HIGH_PERF) {
                rec.2 = Some(cur);
                set_power_scheme(HIGH_PERF);
            }
        }
    }
    run_program(&a["before"]);
    if a["restore"] != false && (rec.0.is_some() || rec.1.is_some() || rec.2.is_some()) {
        RESTORE.lock().unwrap().insert(id, rec);
    }
    Ok(())
}

pub fn game_started(id: &str) {
    let g = app().store.lock().unwrap().games.get(id).cloned().unwrap_or(Value::Null);
    let st = app().store.lock().unwrap().settings["discord"].clone();
    let cid = s(&st, "clientId");
    let steam = s(&g, "source") == "steam";
    if st["enabled"] == true && !cid.is_empty() && (!steam || st["steamToo"] == true) {
        let name = { let c = s(&g, "customName"); if c.is_empty() { s(&g, "name") } else { c } };
        let img = s(&g, "coverUrl");
        std::thread::spawn(move || {
            if let Err(e) = crate::discord::set_activity(&cid, &name, now_ms(), (!img.is_empty()).then_some(img.as_str())) {
                app().log(&format!("discord: {}", e));
            }
        });
    }
}

pub fn game_ended(id: &str) {
    crate::discord::clear();
    let g = app().store.lock().unwrap().games.get(id).cloned().unwrap_or(Value::Null);
    run_program(&g["actions"]["after"]);
    let rec = RESTORE.lock().unwrap().remove(id);
    if let Some((z, v, p)) = rec {
        std::thread::spawn(move || {
            match z {
                Some(true) => { let _ = crate::commands::zapret_start(None); }
                Some(false) => { let _ = crate::commands::zapret_stop(); }
                None => {}
            }
            match v {
                Some(true) => { let _ = crate::net::vpn::start(); }
                Some(false) => { let _ = crate::net::vpn::stop(false); }
                None => {}
            }
            if let Some(p) = p {
                set_power_scheme(&p);
            }
        });
    }
}

// ---------------- notifications ----------------
pub fn notify(kind: &str, title: &str, body: &str) {
    let n = app().store.lock().unwrap().settings["notify"].clone();
    if n[kind] == false {
        return;
    }
    use tauri_plugin_notification::NotificationExt;
    let _ = app().handle.notification().builder().title(title).body(body).show();
    app().toast("ok", &format!("{} — {}", title, body));
}

pub fn download_finished(id: &str) {
    let name = app().store.lock().unwrap().games.get(id).map(|g| { let c = s(g, "customName"); if c.is_empty() { s(g, "name") } else { c } });
    if let Some(n) = name {
        notify("downloads", "Загрузка завершена", &format!("«{}» готова к игре", n));
    }
}

// ---------------- VPN watchdog ----------------
pub fn on_vpn_dropped() {
    let c = app().store.lock().unwrap().settings["vpn"].clone();
    if c["dropped"] != true || c["wantOn"] != true {
        return;
    }
    notify("vpn", "VPN отключился", if c["killSwitch"] == true { "Интернет через прокси заблокирован, пока VPN не вернётся. Переподключаемся…" } else { "Переподключаемся…" });
    std::thread::spawn(|| {
        for i in 0..5 {
            std::thread::sleep(std::time::Duration::from_secs(3 + i * 4));
            if crate::net::vpn::start().is_ok() {
                notify("vpn", "VPN снова подключён", "");
                return;
            }
        }
        // gave up: lift the block so the internet works again
        crate::net::vpn::clear_proxy();
        app().store.lock().unwrap().set_settings(json!({ "vpn": { "dropped": false, "wantOn": false } }));
        notify("vpn", "VPN не удалось переподключить", "Блокировка снята, интернет работает напрямую");
        app().push();
    });
}

// ---------------- schedules (called every few seconds) ----------------
static LAST: Lazy<Mutex<HashMap<&'static str, i64>>> = Lazy::new(|| Mutex::new(HashMap::new()));

fn due(key: &'static str, every_ms: i64) -> bool {
    let mut l = LAST.lock().unwrap();
    let now = now_ms();
    let last = *l.get(key).unwrap_or(&0);
    if now - last >= every_ms {
        l.insert(key, now);
        return last != 0 || key != "first";
    }
    false
}

pub fn tick_schedules() {
    // VPN subscriptions auto-update
    let c = app().store.lock().unwrap().settings["vpn"].clone();
    let hours = c["subsAutoHours"].as_i64().unwrap_or(0);
    if hours > 0 && c["subs"].as_array().map(|a| !a.is_empty()).unwrap_or(false) && due("subs-check", 60_000) {
        let oldest = c["subs"].as_array().unwrap().iter().map(|s| s["updatedAt"].as_i64().unwrap_or(0)).min().unwrap_or(0);
        if now_ms() - oldest > hours * 3_600_000 {
            tauri::async_runtime::spawn(async {
                let r = crate::net::vpn::refresh_subs().await;
                app().log(&format!("subs auto-update: {}", r));
                app().push();
            });
        }
    }
    // store notifications (free games, wishlist sales) — every 6 hours
    if due("store-notify", 6 * 3_600_000) {
        tauri::async_runtime::spawn(crate::extras::check_store_news());
    }
}
