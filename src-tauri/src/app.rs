//! Application core: shared state, UI updates, window, tray, background loops.
use crate::installs::Installs;
use crate::net::{vpn::Vpn, zapret::Zapret};
use crate::store::Store;
use crate::tracker::Tracker;
use crate::util::common::{now_ms, s};
use once_cell::sync::OnceCell;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, EventTarget, Manager};

pub struct App {
    pub handle: AppHandle,
    pub data_dir: PathBuf,
    pub store: Mutex<Store>,
    pub http: reqwest::Client,
    pub scanning: AtomicBool,
    pub quitting: AtomicBool,
    pub last_error: Mutex<String>,
    pub tracker: Mutex<Tracker>,
    pub installs: Mutex<Installs>,
    pub zapret: Mutex<Zapret>,
    pub vpn: Mutex<Vpn>,
    pub accounts: Mutex<crate::accounts::Accounts>,
    pub browser: Mutex<crate::browser::Browser>,
    push_pending: AtomicBool,
}

static APP: OnceCell<Arc<App>> = OnceCell::new();

pub fn app() -> &'static Arc<App> {
    APP.get().expect("app not initialised")
}

/// The app, if it is running (unit tests run code without it)
pub fn try_app() -> Option<&'static Arc<App>> {
    APP.get()
}

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

impl App {
    pub fn log(&self, msg: &str) {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(self.data_dir.join("arcadia.log")) {
            let _ = writeln!(f, "[{}] {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), msg);
        }
    }

    /// Debounced state update to the UI (+ tray)
    pub fn push(&self) {
        if self.push_pending.swap(true, Ordering::SeqCst) {
            return;
        }
        tauri::async_runtime::spawn(async {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            let a = app();
            a.push_pending.store(false, Ordering::SeqCst);
            let snap = tauri::async_runtime::spawn_blocking(snapshot).await.unwrap_or(Value::Null);
            let _ = a.handle.emit_to(EventTarget::webview("ui"), "state", snap);
            update_tray();
        });
    }

    pub fn toast(&self, kind: &str, msg: &str) {
        let _ = self.handle.emit_to(EventTarget::webview("ui"), "toast", json!({ "type": kind, "msg": msg }));
    }
}

pub fn snapshot() -> Value {
    let a = app();
    let (games, collections, settings, scan_status) = {
        let st = a.store.lock().unwrap();
        (Value::Object(st.games.clone()), json!(st.collections), st.settings.clone(), st.scan_status.clone())
    };
    let zcfg = settings["zapret"].clone();
    let zap = { let z = a.zapret.lock().unwrap(); crate::net::zapret::state(&zcfg, &z) };
    let vpn = { let v = a.vpn.lock().unwrap(); crate::net::vpn::state(&v) };
    json!({
        "games": games, "collections": collections, "settings": settings, "scanStatus": scan_status,
        "running": a.tracker.lock().unwrap().running_ids(), "scanning": a.scanning.load(Ordering::SeqCst),
        "metaPending": crate::meta::pending(), "lastError": a.last_error.lock().unwrap().clone(),
        "net": { "zapret": zap, "vpn": vpn }, "accounts": crate::accounts::state(), "browser": crate::browser::state(),
        "installs": Value::Object(a.installs.lock().unwrap().active.clone()),
        "extras": crate::extras::state(), "aiOpen": crate::ai::open_ids(),
        "platform": if cfg!(windows) { "win32" } else { "linux" }, "version": VERSION,
    })
}

// ---------------- window ----------------
pub fn main_window() -> Option<tauri::Window> {
    app().handle.get_window("main")
}

pub fn show_window() {
    if let Some(w) = main_window() {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        crate::browser::on_restored();
    }
}

pub fn hide_window() {
    if let Some(w) = main_window() {
        let _ = w.hide();
        crate::browser::on_hidden();
    }
}

pub fn toggle_window() {
    if let Some(w) = main_window() {
        if w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false) && !w.is_minimized().unwrap_or(false) {
            hide_window();
        } else {
            show_window();
        }
    }
}

pub fn create_window(handle: &AppHandle, hidden: bool) -> tauri::Result<()> {
    use tauri::webview::WebviewBuilder;
    use tauri::window::WindowBuilder;
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/icon.png"))?;
    let win = WindowBuilder::new(handle, "main")
        .title("Arcadia")
        .inner_size(1400.0, 880.0)
        .min_inner_size(1000.0, 640.0)
        .decorations(false)
        .visible(false)
        .icon(icon)?
        .background_color(tauri::window::Color(14, 10, 18, 255))
        .build()?;
    let size = win.inner_size()?.to_logical::<f64>(win.scale_factor()?);
    let wb = WebviewBuilder::new("ui", tauri::WebviewUrl::App("index.html".into())).auto_resize();
    win.add_child(wb, tauri::LogicalPosition::new(0.0, 0.0), size)?;
    let w2 = win.clone();
    win.on_window_event(move |e| match e {
        tauri::WindowEvent::CloseRequested { api, .. } => {
            if !app().quitting.load(Ordering::SeqCst) && app().store.lock().unwrap().settings["closeToTray"] != false {
                api.prevent_close();
                hide_window();
            } else {
                quit();
            }
        }
        tauri::WindowEvent::Resized(_) => {
            if w2.is_minimized().unwrap_or(false) {
                crate::browser::on_minimized();
            } else {
                crate::browser::on_restored();
                // the store page follows the layout; the UI re-sends its rectangle
                let _ = w2.emit_to(EventTarget::webview("ui"), "layout", ());
            }
        }
        tauri::WindowEvent::DragDrop(dd) => {
            use tauri::DragDropEvent::*;
            let payload = match dd {
                Enter { paths, .. } => json!({ "type": "enter", "paths": paths }),
                Over { .. } => return,
                Drop { paths, .. } => json!({ "type": "drop", "paths": paths }),
                Leave => json!({ "type": "leave" }),
                _ => return,
            };
            let _ = w2.emit_to(EventTarget::webview("ui"), "dragdrop", payload);
        }
        _ => {}
    });
    if !hidden {
        let _ = win.show();
    }
    Ok(())
}

// ---------------- tray ----------------
fn display_name(g: &Value) -> String {
    let c = s(g, "customName");
    if c.is_empty() { s(g, "name") } else { c }
}

pub fn build_tray(handle: &AppHandle) -> tauri::Result<()> {
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;
    TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip("Arcadia")
        .show_menu_on_left_click(false)
        .on_menu_event(|_app, ev| tray_action(ev.id().as_ref()))
        .on_tray_icon_event(|_t, ev| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                toggle_window();
            }
        })
        .build(handle)?;
    update_tray();
    Ok(())
}

fn tray_action(id: &str) {
    match id {
        "open" => show_window(),
        "quit" => quit(),
        "rescan" => run_scan(),
        "net" => {
            show_window();
            let _ = app().handle.emit_to(EventTarget::webview("ui"), "open-net", ());
        }
        "zapret" => {
            tauri::async_runtime::spawn_blocking(|| {
                let r = if app().zapret.lock().unwrap().running { crate::commands::zapret_stop() } else { crate::commands::zapret_start(None) };
                if let Err(e) = r {
                    app().toast("err", &e);
                }
            });
        }
        "vpn" => {
            tauri::async_runtime::spawn_blocking(|| {
                let r = if app().vpn.lock().unwrap().running { crate::net::vpn::stop(false) } else { crate::net::vpn::start() };
                if let Err(e) = r {
                    app().toast("err", &e);
                }
            });
        }
        x if x.starts_with("game:") => {
            let gid = x[5..].to_string();
            tauri::async_runtime::spawn(async move {
                let r = crate::commands::do_launch(&gid).await;
                if r["ok"] == false {
                    app().toast("err", &s(&r, "error"));
                }
            });
        }
        _ => {}
    }
}

pub fn update_tray() {
    let a = app();
    let h = a.handle.clone();
    let Some(tray) = h.tray_by_id("main") else { return };
    let (recent, favs, running, zs, vs) = {
        let st = a.store.lock().unwrap();
        let mut games: Vec<&Value> = st.games.values().filter(|g| g["hidden"] != true && g["missing"] != true && g["installed"] != false).collect();
        let last = |g: &Value| ["lastPlayed", "lastLaunched", "importedLastPlayed"].iter().map(|k| g[*k].as_i64().unwrap_or(0)).max().unwrap_or(0);
        games.sort_by_key(|g| -last(g));
        let recent: Vec<(String, String)> = games.iter().filter(|g| last(g) > 0).take(6).map(|g| (s(g, "id"), display_name(g))).collect();
        let favs: Vec<(String, String)> = games.iter().filter(|g| g["favorite"] == true && !recent.iter().any(|r| r.0 == s(g, "id"))).take(6).map(|g| (s(g, "id"), display_name(g))).collect();
        let running: Vec<String> = a.tracker.lock().unwrap().running_ids().iter().filter_map(|id| st.games.get(id)).map(display_name).collect();
        drop(st);
        (recent, favs, running, a.zapret.lock().unwrap().running, a.vpn.lock().unwrap().running)
    };
    let build = || -> tauri::Result<Menu<tauri::Wry>> {
        let m = Menu::new(&h)?;
        m.append(&MenuItem::with_id(&h, "open", "Открыть Arcadia", true, None::<&str>)?)?;
        m.append(&PredefinedMenuItem::separator(&h)?)?;
        if !running.is_empty() {
            m.append(&MenuItem::new(&h, "Сейчас запущено", false, None::<&str>)?)?;
            for n in &running {
                m.append(&MenuItem::new(&h, format!("▶ {}", n), false, None::<&str>)?)?;
            }
            m.append(&PredefinedMenuItem::separator(&h)?)?;
        }
        for (title, list) in [("Недавние", &recent), ("Избранное", &favs)] {
            if !list.is_empty() {
                m.append(&MenuItem::new(&h, title, false, None::<&str>)?)?;
                for (id, n) in list.iter() {
                    m.append(&MenuItem::with_id(&h, format!("game:{}", id), n.chars().take(48).collect::<String>(), true, None::<&str>)?)?;
                }
                m.append(&PredefinedMenuItem::separator(&h)?)?;
            }
        }
        m.append(&CheckMenuItem::with_id(&h, "zapret", "Zapret", true, zs, None::<&str>)?)?;
        m.append(&CheckMenuItem::with_id(&h, "vpn", "VPN", true, vs, None::<&str>)?)?;
        m.append(&MenuItem::with_id(&h, "net", "Обход блокировок…", true, None::<&str>)?)?;
        m.append(&PredefinedMenuItem::separator(&h)?)?;
        m.append(&MenuItem::with_id(&h, "rescan", "Обновить библиотеку", true, None::<&str>)?)?;
        m.append(&MenuItem::with_id(&h, "quit", "Выход", true, None::<&str>)?)?;
        Ok(m)
    };
    if let Ok(m) = build() {
        let _ = tray.set_menu(Some(m));
    }
    let dl: Vec<String> = a.installs.lock().unwrap().active.iter().map(|(id, p)| {
        let n = a.store.lock().unwrap().games.get(id).map(display_name).unwrap_or_else(|| s(p, "name"));
        format!("{} {}%", n, p["pct"].as_f64().unwrap_or(0.0).floor())
    }).collect();
    let mut tip = if running.is_empty() { "Arcadia".to_string() } else { format!("Arcadia — {}", running[0]) };
    if !dl.is_empty() {
        tip.push_str(&format!("\nЗагрузка: {}", dl.join(", ")));
    }
    let _ = tray.set_tooltip(Some(tip));
}

// ---------------- actions ----------------
pub fn run_scan() {
    let a = app();
    if a.scanning.swap(true, Ordering::SeqCst) {
        return;
    }
    a.last_error.lock().unwrap().clear();
    a.push();
    tauri::async_runtime::spawn_blocking(|| {
        let a = app();
        let (settings, manual) = {
            let st = a.store.lock().unwrap();
            (st.settings.clone(), st.manual_dirs())
        };
        let (games, status) = crate::scanners::scan_all(&settings, &manual, None);
        let n = games.len();
        let added = a.store.lock().unwrap().merge_scan(games, status.clone());
        a.log(&format!("scan done: {} found, {} new, {}", n, added, status));
        a.scanning.store(false, Ordering::SeqCst);
        if settings["autoFetchMeta"] != false {
            crate::meta::enqueue_missing();
        }
        a.push();
    });
}

pub fn quit() {
    let a = app();
    if a.quitting.swap(true, Ordering::SeqCst) {
        return;
    }
    // finish play sessions, restore system proxy, stop VPN / zapret as configured (max 8 s)
    let now = now_ms();
    let active: Vec<(String, i64)> = a.tracker.lock().unwrap().active.drain().collect();
    {
        let mut st = a.store.lock().unwrap();
        for (id, start) in active {
            st.add_session(&id, start, now);
        }
        st.flush();
    }
    std::thread::spawn(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let a = app();
            let (vcfg, zcfg) = { let st = a.store.lock().unwrap(); (st.settings["vpn"].clone(), st.settings["zapret"].clone()) };
            crate::net::vpn::shutdown();
            if a.vpn.lock().unwrap().running && vcfg["stopOnExit"] != false {
                let _ = crate::net::vpn::stop(false);
            }
            if a.zapret.lock().unwrap().running && zcfg["stopOnExit"] == true {
                let _ = crate::net::zapret::stop();
            }
            let _ = tx.send(());
        });
        let _ = rx.recv_timeout(std::time::Duration::from_secs(8));
        app().store.lock().unwrap().flush();
        app().handle.exit(0);
    });
}

// ---------------- background loops ----------------
pub fn start_loops() {
    // save library
    tauri::async_runtime::spawn(async {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
            let mut st = app().store.lock().unwrap();
            if st.dirty {
                st.flush();
            }
        }
    });
    // play time tracker
    tauri::async_runtime::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        loop {
            let busy = tauri::async_runtime::spawn_blocking(|| {
                let a = app();
                let watch_all = a.store.lock().unwrap().settings["trackExternal"] != false;
                if !a.tracker.lock().unwrap().needs_scan(watch_all) {
                    return false;
                }
                let procs: Vec<String> = crate::util::winsys::process_paths().iter().map(|p| crate::util::common::norm_path(p)).collect();
                let games = a.store.lock().unwrap().games.clone();
                let r = a.tracker.lock().unwrap().tick(&games, &procs, watch_all);
                if !r.started.is_empty() || !r.ended.is_empty() {
                    let mut st = a.store.lock().unwrap();
                    for id in &r.started {
                        st.update(id, json!({ "lastPlayed": now_ms() }));
                    }
                    for (id, s0, e) in &r.ended {
                        st.add_session(id, *s0, *e);
                    }
                    drop(st);
                    for id in &r.started {
                        crate::commands::on_game_started(id);
                    }
                    for (id, _, _) in &r.ended {
                        crate::commands::on_game_ended(id);
                    }
                    a.push();
                }
                let t = a.tracker.lock().unwrap();
                !t.pending.is_empty() || !t.active.is_empty()
            })
            .await
            .unwrap_or(false);
            tokio::time::sleep(std::time::Duration::from_secs(if busy { 10 } else { 30 })).await;
        }
    });
    // Steam downloads
    tauri::async_runtime::spawn(async {
        loop {
            let (busy, visible) = tauri::async_runtime::spawn_blocking(|| {
                let a = app();
                let (changed, finished) = a.installs.lock().unwrap().tick();
                for id in &finished {
                    let uninstalled = a.store.lock().unwrap().games.get(id).map(|g| g["installed"] == false).unwrap_or(false);
                    if uninstalled {
                        std::thread::sleep(std::time::Duration::from_millis(1500));
                        run_scan();
                    }
                    crate::commands::on_download_finished(id);
                }
                if changed {
                    a.push();
                }
                let vis = main_window().map(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false)).unwrap_or(false);
                (!a.installs.lock().unwrap().active.is_empty(), vis)
            })
            .await
            .unwrap_or((false, false));
            let ms = match (busy, visible) { (true, true) => 2000, (true, false) => 6000, (false, true) => 8000, _ => 30000 };
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
        }
    });
    // zapret / VPN process watcher
    tauri::async_runtime::spawn(async {
        let mut first = true;
        loop {
            tauri::async_runtime::spawn_blocking(move || crate::commands::poll_net(first)).await.ok();
            first = false;
            let vis = main_window().map(|w| w.is_visible().unwrap_or(false)).unwrap_or(false);
            tokio::time::sleep(std::time::Duration::from_secs(if vis { 4 } else { 12 })).await;
        }
    });
}

pub fn init(handle: AppHandle) {
    let dir = crate::store::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let http = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).gzip(true).build().unwrap();
    let a = App {
        handle,
        store: Mutex::new(Store::new(dir.clone())),
        data_dir: dir,
        http,
        scanning: AtomicBool::new(false),
        quitting: AtomicBool::new(false),
        last_error: Mutex::new(String::new()),
        tracker: Mutex::new(Tracker::default()),
        installs: Mutex::new(Installs::default()),
        zapret: Mutex::new(Zapret::default()),
        vpn: Mutex::new(Vpn::default()),
        accounts: Mutex::new(Default::default()),
        browser: Mutex::new(Default::default()),
        push_pending: AtomicBool::new(false),
    };
    let _ = APP.set(Arc::new(a));
}
