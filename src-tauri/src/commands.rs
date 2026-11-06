//! All UI requests go through one command: call(channel, args) — the same channel names as the Electron version,
//! so the interface code stays the same.
use crate::util::common::nz;
use crate::app::{app, hide_window, quit, run_scan, show_window};
use crate::util::common::{exists, now_ms, parent, s};
use crate::util::winsys::open;
use serde_json::{json, Value};
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

fn ok() -> Value {
    json!({ "ok": true })
}
fn err(e: impl ToString) -> Value {
    json!({ "ok": false, "error": e.to_string() })
}
fn arg(a: &[Value], i: usize) -> Value {
    a.get(i).cloned().unwrap_or(Value::Null)
}
fn sarg(a: &[Value], i: usize) -> String {
    a.get(i).and_then(|v| v.as_str()).unwrap_or("").to_string()
}
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    tauri::async_runtime::spawn_blocking(f).await.expect("task")
}

#[tauri::command]
pub async fn call(ch: String, args: Vec<Value>) -> Value {
    let r = dispatch(&ch, &args).await;
    if r["ok"] == false {
        app().log(&format!("{}: {}", ch, s(&r, "error")));
    }
    r
}

pub async fn do_launch(id: &str) -> Value {
    do_launch_profile(id, None).await
}

/// profile: index into the game's launch profiles (None = the usual way)
/// profile: None = the game's default, Some(-1) = plain launch, Some(i) = profile i
pub async fn do_launch_profile(id: &str, profile: Option<i64>) -> Value {
    let Some(mut g) = app().store.lock().unwrap().games.get(id).cloned() else { return err("Игра не найдена") };
    let idx = profile.or_else(|| g["defaultProfile"].as_i64()).filter(|i| *i >= 0).map(|i| i as usize);
    if let Some(p) = idx.and_then(|i| g["profiles"].get(i)).cloned() {
        let mut o = p.clone();
        if s(&o, "type").is_empty() {
            let path = s(&o, "path");
            o["type"] = json!(if path.contains("://") { "uri" } else { "exe" });
            if path.contains("://") { o["uri"] = json!(path); }
        }
        g["launchOverride"] = o;
    }
    if g["installed"] == false {
        let uri = s(&g, "installUri");
        if uri.is_empty() {
            return err("Неизвестно, как установить эту игру");
        }
        return match open(&uri) {
            Ok(_) => json!({ "ok": true, "install": true }),
            Err(e) => err(format!("Не удалось открыть лаунчер: {}", e)),
        };
    }
    let g2 = g.clone();
    if let Err(e) = blocking(move || crate::actions::before_launch(&g2)).await {
        return err(e);
    }
    let g3 = g.clone();
    if let Err(e) = blocking(move || crate::launch::launch_game(&g3)).await {
        return err(e);
    }
    app().store.lock().unwrap().update(id, json!({ "lastLaunched": now_ms() }));
    app().tracker.lock().unwrap().expect(&g);
    if app().store.lock().unwrap().settings["minimizeOnLaunch"] != false {
        if let Some(w) = crate::app::main_window() {
            let _ = w.minimize();
        }
    }
    app().push();
    ok()
}

fn add_files(paths: Vec<String>) -> Value {
    let (mut added, mut errors) = (vec![], vec![]);
    for p in paths {
        match crate::scanners::manual::make_manual_game(&p) {
            Ok(g) => {
                let id = app().store.lock().unwrap().add_game(g);
                crate::meta::enqueue(&id, None, false);
                added.push(id);
            }
            Err(e) => errors.push(format!("{}: {}", crate::util::common::file_name(&p), e)),
        }
    }
    app().push();
    json!({ "added": added, "errors": errors })
}

fn pick_files(title: &str, filters: &[(&str, &[&str])], many: bool) -> Vec<String> {
    let mut d = app().handle.dialog().file().set_title(title);
    if let Some(w) = crate::app::main_window() {
        d = d.set_parent(&w);
    }
    for (n, e) in filters {
        d = d.add_filter(*n, e);
    }
    let to_s = |f: tauri_plugin_dialog::FilePath| f.into_path().ok().map(|p| p.to_string_lossy().to_string());
    if many {
        d.blocking_pick_files().unwrap_or_default().into_iter().filter_map(to_s).collect()
    } else {
        d.blocking_pick_file().and_then(to_s).into_iter().collect()
    }
}

fn pick_folder(title: &str) -> Option<String> {
    let mut d = app().handle.dialog().file().set_title(title);
    if let Some(w) = crate::app::main_window() {
        d = d.set_parent(&w);
    }
    d.blocking_pick_folder().and_then(|f| f.into_path().ok()).map(|p| p.to_string_lossy().to_string())
}

pub fn zapret_start(strategy: Option<String>) -> Result<(), String> {
    let cfg = app().store.lock().unwrap().settings["zapret"].clone();
    let st = { let z = app().zapret.lock().unwrap(); crate::net::zapret::state(&cfg, &z) };
    if st["valid"] != true {
        return Err("Укажите папку zapret".into());
    }
    let strategy = strategy.filter(|x| !x.is_empty()).unwrap_or_else(|| s(&st, "strategy"));
    if strategy.is_empty() {
        return Err("В папке нет стратегий (.bat файлов с winws.exe)".into());
    }
    {
        let mut z = app().zapret.lock().unwrap();
        z.busy = true;
        z.error.clear();
    }
    app().push();
    let r = crate::net::zapret::start(&s(&st, "dir"), &strategy, cfg["gameFilter"] == true);
    {
        let mut z = app().zapret.lock().unwrap();
        z.busy = false;
        match &r {
            Ok(_) => z.running = true,
            Err(e) => z.error = e.clone(),
        }
    }
    if r.is_ok() {
        app().store.lock().unwrap().set_settings(json!({ "zapret": { "strategy": strategy, "lastStarted": strategy } }));
    }
    app().push();
    r
}

pub fn zapret_stop() -> Result<(), String> {
    {
        let mut z = app().zapret.lock().unwrap();
        z.busy = true;
        z.error.clear();
    }
    app().push();
    let r = crate::net::zapret::stop();
    {
        let mut z = app().zapret.lock().unwrap();
        z.busy = false;
        match &r {
            Ok(_) => z.running = false,
            Err(e) => z.error = e.clone(),
        }
    }
    app().push();
    r
}

/// Watches winws / VPN core processes; autostarts them once
pub fn poll_net(first: bool) {
    let names = crate::util::winsys::process_names();
    let a = app();
    {
        let mut z = a.zapret.lock().unwrap();
        let r = names.contains("winws.exe");
        if r != z.running && !z.busy {
            z.running = r;
            drop(z);
            a.push();
        }
    }
    if crate::net::vpn::set_status(&names) {
        a.push();
        crate::actions::on_vpn_dropped();
    }
    if first {
        crate::net::vpn::detect_versions();
        let (z, v) = { let st = a.store.lock().unwrap(); (st.settings["zapret"].clone(), st.settings["vpn"].clone()) };
        if v["proxySet"] == true && !a.vpn.lock().unwrap().running {
            crate::net::vpn::clear_proxy(); // left over after a crash
        }
        if z["autostart"] == true && !a.zapret.lock().unwrap().running {
            let _ = zapret_start(None);
        }
        if v["autostart"] == true && !a.vpn.lock().unwrap().running && crate::net::vpn::core_path().is_some() {
            let _ = crate::net::vpn::start();
        }
        a.push();
    }
    crate::actions::tick_schedules();
}

pub fn on_game_started(id: &str) {
    crate::actions::game_started(id);
}
pub fn on_game_ended(id: &str) {
    crate::actions::game_ended(id);
}
pub fn on_download_finished(id: &str) {
    crate::actions::download_finished(id);
}

fn apply_hotkey() -> bool {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let gs = app().handle.global_shortcut();
    let _ = gs.unregister_all();
    let hk = s(&app().store.lock().unwrap().settings, "hotkey");
    hk.is_empty() || gs.register(hk.as_str()).is_ok()
}

fn apply_autostart() {
    use tauri_plugin_autostart::ManagerExt;
    let on = app().store.lock().unwrap().settings["autostart"] == true;
    let al = app().handle.autolaunch();
    let _ = if on { al.enable() } else { al.disable() };
}

pub fn setup_system() {
    apply_hotkey();
    apply_autostart();
}

async fn dispatch(ch: &str, a: &[Value]) -> Value {
    let app = app();
    match ch {
        "state:get" => blocking(crate::app::snapshot).await,
        "scan" => { run_scan(); ok() }
        "launch" => do_launch_profile(&sarg(a, 0), arg(a, 1).as_i64()).await,
        "game:update" => {
            let mut clean = serde_json::Map::new();
            for k in ["customName", "favorite", "hidden", "collections", "launchOverride", "notes", "status", "profiles", "defaultProfile", "actions", "tags", "hltb"] {
                if let Some(v) = arg(a, 1).get(k) {
                    clean.insert(k.into(), v.clone());
                }
            }
            app.store.lock().unwrap().update(&sarg(a, 0), Value::Object(clean));
            app.push();
            ok()
        }
        "game:remove" => { app.store.lock().unwrap().remove(&sarg(a, 0)); app.push(); ok() }
        "game:add-files" => {
            let paths: Vec<String> = arg(a, 0).as_array().map(|x| x.iter().filter_map(|p| p.as_str().map(String::from)).collect()).unwrap_or_default();
            blocking(move || add_files(paths)).await
        }
        "game:open-folder" => {
            let g = app.store.lock().unwrap().games.get(&sarg(a, 0)).cloned().unwrap_or(Value::Null);
            let p = nz(s(&g, "installDir"), parent(&s(&g["launch"], "path")));
            if !p.is_empty() && exists(&p) { let _ = open(&p); }
            json!({ "ok": !p.is_empty() })
        }
        "game:set-image" => {
            let src = arg(a, 2);
            match crate::meta::set_image(&sarg(a, 0), &sarg(a, 1), src["file"].as_str().map(String::from), src["url"].as_str().map(String::from)).await {
                Ok(_) => { app.push(); ok() }
                Err(e) => err(e),
            }
        }
        "game:refetch" => { let t = sarg(a, 1); crate::meta::enqueue(&sarg(a, 0), (!t.is_empty()).then_some(t), true); ok() }
        "game:reset-playtime" => { app.store.lock().unwrap().update(&sarg(a, 0), json!({ "playtime": 0, "sessions": [] })); app.push(); ok() }
        "game:hltb" => match crate::extras::hltb(&sarg(a, 0)).await { Ok(v) => v, Err(e) => err(e) },
        "dialog:pick-games" => {
            let files = blocking(|| pick_files("Добавить игру", &[("Игры и ярлыки", &["exe", "lnk", "url", "bat"])], true)).await;
            blocking(move || add_files(files)).await
        }
        "dialog:pick-folder" => json!(blocking(|| pick_folder("Папка с играми")).await),
        "dialog:pick-file" => {
            let kind = sarg(a, 0);
            let f = blocking(move || if kind == "image" { pick_files("Изображение", &[("Изображения", &["jpg", "jpeg", "png", "webp", "gif"])], false) } else { pick_files("Программа", &[("Программы", &["exe", "bat", "cmd", "lnk"])], false) }).await;
            json!(f.into_iter().next())
        }
        "col:create" => { let c = app.store.lock().unwrap().create_collection(&sarg(a, 0)); app.push(); c }
        "col:rename" => { app.store.lock().unwrap().rename_collection(&sarg(a, 0), &sarg(a, 1)); app.push(); ok() }
        "col:delete" => { app.store.lock().unwrap().delete_collection(&sarg(a, 0)); app.push(); ok() }
        "settings:set" => {
            let patch = arg(a, 0);
            let before = app.store.lock().unwrap().settings["sources"].clone();
            app.store.lock().unwrap().set_settings(patch.clone());
            let mut hotkey_ok = true;
            if patch.get("hotkey").is_some() { hotkey_ok = apply_hotkey(); }
            if patch.get("autostart").is_some() { apply_autostart(); }
            let after = app.store.lock().unwrap().settings["sources"].clone();
            if patch.get("scanFolders").is_some() || (patch.get("sources").is_some() && before != after) { run_scan(); }
            app.push();
            json!({ "ok": true, "hotkeyOk": hotkey_ok })
        }
        "settings:unignore" => { app.store.lock().unwrap().set_settings(json!({ "ignoredIds": [] })); run_scan(); ok() }
        "meta:fetch-missing" => { crate::meta::enqueue_missing(); ok() }
        "meta:refetch-all" => {
            let ids: Vec<String> = app.store.lock().unwrap().games.values().filter(|g| g["missing"] != true && g["installed"] != false).map(|g| s(g, "id")).collect();
            for id in ids { crate::meta::enqueue(&id, None, false); }
            ok()
        }
        "app:open-data" => { let _ = open(&app.data_dir.to_string_lossy()); ok() }
        "app:open-url" => {
            let u = sarg(a, 0);
            if u.starts_with("https://") || u == "steam://open/downloads" { let _ = open(&u); }
            ok()
        }
        "app:quit" => { quit(); ok() }
        "win:minimize" => { if let Some(w) = crate::app::main_window() { let _ = w.minimize(); } ok() }
        "win:maximize" => {
            if let Some(w) = crate::app::main_window() {
                if w.is_maximized().unwrap_or(false) { let _ = w.unmaximize(); } else { let _ = w.maximize(); }
            }
            ok()
        }
        "win:close" => {
            if app.store.lock().unwrap().settings["closeToTray"] != false { hide_window(); } else { quit(); }
            ok()
        }
        "win:drag" => { if let Some(w) = crate::app::main_window() { let _ = w.start_dragging(); } ok() }
        "win:show" => { show_window(); ok() }
        "win:fullscreen" => {
            if let Some(w) = crate::app::main_window() { let on = arg(a, 0) == true; let _ = w.set_fullscreen(on); }
            ok()
        }

        // zapret
        "zapret:set" => { app.store.lock().unwrap().set_settings(json!({ "zapret": arg(a, 0) })); app.push(); ok() }
        "zapret:detect" => json!(blocking(crate::net::zapret::detect_dirs).await),
        "zapret:pick-dir" => {
            let d = blocking(|| pick_folder("Папка zapret")).await;
            if let Some(d) = &d { app.store.lock().unwrap().set_settings(json!({ "zapret": { "dir": d, "strategy": "" } })); app.push(); }
            json!(d)
        }
        "zapret:start" => { let st = sarg(a, 0); match blocking(move || zapret_start(Some(st))).await { Ok(_) => ok(), Err(e) => err(e) } }
        "zapret:stop" => match blocking(zapret_stop).await { Ok(_) => ok(), Err(e) => err(e) },
        "zapret:open" => {
            let d = s(&app.store.lock().unwrap().settings["zapret"], "dir");
            if d.is_empty() { return err("нет папки"); }
            let sub = sarg(a, 0);
            let p = if sub.is_empty() { d.clone() } else { crate::util::common::join(&d, &sub) };
            let _ = open(if exists(&p) { &p } else { &d });
            ok()
        }
        "zapret:autopick" => { tauri::async_runtime::spawn(crate::extras::zapret_autopick()); ok() }
        "zapret:autopick-stop" => { crate::extras::AUTOPICK_STOP.store(true, std::sync::atomic::Ordering::SeqCst); ok() }

        // vpn
        "vpn:set" => {
            let mut clean = serde_json::Map::new();
            for k in ["selected", "mode", "directRu", "gamesDirect", "autostart", "stopOnExit", "port", "subsAutoHours", "killSwitch", "appsMode", "apps"] {
                if let Some(v) = arg(a, 0).get(k) { clean.insert(k.into(), v.clone()); }
            }
            app.store.lock().unwrap().set_settings(json!({ "vpn": clean }));
            app.push();
            ok()
        }
        "vpn:add" => match crate::net::vpn::add_input(&sarg(a, 0)).await { Ok(mut r) => { r["ok"] = json!(true); app.push(); r } Err(e) => err(e) },
        "vpn:refresh-subs" => { let mut r = crate::net::vpn::refresh_subs().await; r["ok"] = json!(true); app.push(); r }
        "vpn:remove-server" => { crate::net::vpn::remove_server(&sarg(a, 0)); app.push(); ok() }
        "vpn:remove-sub" => { crate::net::vpn::remove_sub(&sarg(a, 0)); app.push(); ok() }
        "vpn:rename-server" => { crate::net::vpn::rename_server(&sarg(a, 0), &sarg(a, 1).chars().take(80).collect::<String>()); app.push(); ok() }
        "vpn:ping" => { crate::net::vpn::ping_all().await; app.push(); ok() }
        "vpn:install-core" => match crate::net::vpn::install_core().await { Ok(_) => ok(), Err(e) => err(e) },
        "vpn:install-xray" => match crate::net::vpn::install_xray().await { Ok(_) => ok(), Err(e) => err(e) },
        "vpn:pick-core" | "vpn:pick-xray" => {
            let xray = ch == "vpn:pick-xray";
            let f = blocking(move || pick_files(if xray { "Укажите xray.exe" } else { "Укажите sing-box.exe" }, &[("exe", &["exe"])], false)).await;
            match f.first() {
                Some(p) => match blocking({ let p = p.clone(); move || crate::net::vpn::set_core_file(&p, xray) }).await { Ok(_) => { app.push(); ok() } Err(e) => err(e) },
                None => json!({ "ok": false }),
            }
        }
        "vpn:pick-app" => {
            let f = blocking(|| pick_files("Программа", &[("Программы", &["exe"])], false)).await;
            json!(f.first().map(|p| crate::util::common::file_name(p)))
        }
        "vpn:start" => match blocking(crate::net::vpn::start).await { Ok(_) => ok(), Err(e) => err(e) },
        "vpn:stop" => match blocking(|| crate::net::vpn::stop(false)).await { Ok(_) => ok(), Err(e) => err(e) },
        "vpn:open-log" => { let _ = open(&crate::net::vpn::dir().to_string_lossy()); ok() }

        // accounts
        "acc:login" => match crate::accounts::login(&sarg(a, 0)).await { Ok(v) => { app.push(); v } Err(e) => { app.push(); err(e) } },
        "acc:local-steam" => match crate::accounts::use_local_steam().await { Ok(v) => v, Err(e) => err(e) },
        "acc:sync" => match crate::accounts::sync(&sarg(a, 0)).await { Ok(v) => v, Err(e) => err(e) },
        "acc:logout" => { let k = sarg(a, 0); blocking(move || crate::accounts::logout(&k)).await; ok() }
        "acc:launcher" => {
            let uri = crate::accounts::launcher_uri(&sarg(a, 0));
            if uri.is_empty() { return err("Для этого сервиса нет ссылки на лаунчер"); }
            match open(&uri) { Ok(_) => ok(), Err(e) => err(e) }
        }
        "acc:steam-key" => { app.store.lock().unwrap().set_settings(json!({ "steamApiKey": sarg(a, 0).trim() })); app.push(); ok() }

        // built-in browser
        "br:open" => { let (k, u) = (sarg(a, 0), sarg(a, 1)); match blocking(move || crate::browser::open_page(&k, &u)).await { Ok(_) => ok(), Err(e) => err(e) } }
        "br:bounds" => {
            let r = arg(a, 0);
            let rect = r.is_object().then(|| (r["x"].as_f64().unwrap_or(0.0), r["y"].as_f64().unwrap_or(0.0), r["width"].as_f64().unwrap_or(0.0), r["height"].as_f64().unwrap_or(0.0)));
            blocking(move || crate::browser::set_bounds(rect)).await;
            ok()
        }
        "br:nav" => { crate::browser::nav(&sarg(a, 0)); ok() }
        "br:unload" => { blocking(|| crate::browser::unload("manual")).await; ok() }
        "br:disable" => { blocking(crate::browser::disable).await; ok() }
        "br:external" => { crate::browser::external(); ok() }
        "br:full" => { let on = arg(a, 0) == true; blocking(move || crate::browser::set_full(on)).await; ok() }
        "br:set" => {
            let patch = arg(a, 0);
            app.store.lock().unwrap().set_settings(json!({ "browser": patch.clone() }));
            if patch.get("economy").is_some() {
                // economy options apply to a newly created page
                let (k, u, loaded) = { let b = app.browser.lock().unwrap(); (b.key.clone(), b.url.clone(), b.loaded) };
                if loaded { blocking(move || { crate::browser::unload("settings"); let _ = crate::browser::open_page(&k, &u); }).await; }
            }
            app.push();
            ok()
        }

        // features
        x if x.starts_with("x:") => crate::extras::dispatch(&x[2..], a).await,
        _ => err(format!("Неизвестная команда {}", ch)),
    }
}

#[allow(dead_code)]
fn _keep() {
    let _ = app().handle.get_window("main");
}
