//! Built-in store browser (a WebView2 child view over the «Магазины» section). Costs nothing while hidden:
//!  • nothing is created until «Включить браузерный режим»;
//!  • one page at a time; "new tab" links open in place;
//!  • the page is destroyed when the window hides to tray, after N s minimized, after N s away from the section;
//!  • ads/trackers blocked; economy mode blocks trailers/video and autoplay.
use crate::accounts::common::{browser_args, profile_dir, UA};
use crate::app::app;
use crate::util::winsys::open;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::webview::{NewWindowResponse, PageLoadEvent, WebviewBuilder};
use tauri::{LogicalPosition, LogicalSize, Manager, Webview, WebviewUrl};

const BLOCK_HOSTS: &[&str] = &[
    "doubleclick.net", "googlesyndication.com", "google-analytics.com", "googletagmanager.com", "googleadservices.com", "analytics.google.com",
    "facebook.net", "connect.facebook.net", "hotjar.com", "hotjar.io", "scorecardresearch.com", "adnxs.com", "criteo.com", "criteo.net",
    "taboola.com", "outbrain.com", "quantserve.com", "mc.yandex.ru", "top-fwz1.mail.ru", "bat.bing.com", "clarity.ms", "snap.licdn.com",
    "ads.linkedin.com", "adsrvr.org", "amazon-adsystem.com", "onetrust.com", "cookielaw.org", "branch.io", "sentry.io", "newrelic.com",
    "nr-data.net", "datadoghq.com", "segment.io", "segment.com", "mixpanel.com", "amplitude.com", "optimizely.com", "demdex.net",
    "omtrdc.net", "everesttech.net", "appsflyer.com",
];
pub static BLOCK_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(&format!(r"(?i)^https?://([^/]*\.)?({})(/|:|$)", BLOCK_HOSTS.iter().map(|h| regex::escape(h)).collect::<Vec<_>>().join("|"))).unwrap()
});
static EXTERNAL: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)^(steam|com\.epicgames\.launcher|goggalaxy|uplay|battlenet|origin2?|eadm|link2ea|mailto):").unwrap());
static VIDEO_EXT: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\.(webm|mp4|m4s|m3u8|mpd)(\?|$)").unwrap());

pub static BLOCKED: AtomicU64 = AtomicU64::new(0);
static GEN: AtomicU64 = AtomicU64::new(0);
const LABEL: &str = "store";

#[derive(Default)]
pub struct Browser {
    pub enabled: bool,
    pub loaded: bool,
    pub key: String,
    pub url: String,
    pub title: String,
    pub loading: bool,
    pub want_visible: bool,
    pub bounds: Option<(f64, f64, f64, f64)>,
    pub unloaded_reason: String,
    pub full: bool,
    pub mem_mb: u64,
}

fn cfg() -> Value {
    app().store.lock().unwrap().settings["browser"].clone()
}

fn view() -> Option<Webview> {
    app().handle.get_webview(LABEL)
}

pub fn state() -> Value {
    let b = app().browser.lock().unwrap();
    json!({
        "enabled": b.enabled, "loaded": b.loaded, "key": b.key, "url": b.url, "title": b.title, "loading": b.loading,
        "canBack": b.loaded, "canFwd": b.loaded, "full": b.full, "memMB": b.mem_mb, "blocked": BLOCKED.load(Ordering::Relaxed),
        "unloadedReason": b.unloaded_reason,
    })
}

/// Should this request be blocked? (trackers always, video/trailers in economy mode)
pub fn should_block(url: &str, is_media: bool, trackers: bool, economy: bool) -> bool {
    (trackers && BLOCK_RE.is_match(url)) || (economy && (is_media || VIDEO_EXT.is_match(url)))
}

#[cfg(windows)]
fn install_blocker(wv: &Webview) {
    let _ = wv.with_webview(|pw| unsafe {
        use webview2_com::Microsoft::Web::WebView2::Win32::*;
        use webview2_com::WebResourceRequestedEventHandler;
        use windows::core::{Interface, HSTRING, PWSTR};
        let Ok(core) = pw.controller().CoreWebView2() else { return };
        let env = pw.environment();
        let _ = core.AddWebResourceRequestedFilter(&HSTRING::from("*"), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL);
        let handler = WebResourceRequestedEventHandler::create(Box::new(move |_s, args: Option<ICoreWebView2WebResourceRequestedEventArgs>| {
            let Some(args) = args else { return Ok(()) };
            let mut ctx = COREWEBVIEW2_WEB_RESOURCE_CONTEXT::default();
            let _ = args.ResourceContext(&mut ctx);
            let mut uri = PWSTR::null();
            if let Ok(req) = args.Request() {
                let _ = req.Uri(&mut uri);
            }
            let url = webview2_com::take_pwstr(uri);
            let c = cfg();
            if should_block(&url, ctx == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA, c["blockTrackers"] != false, c["economy"] != false) {
                if let Ok(resp) = env.CreateWebResourceResponse(None, 403, &HSTRING::from("Blocked"), &HSTRING::from("")) {
                    let _ = args.SetResponse(&resp);
                    BLOCKED.fetch_add(1, Ordering::Relaxed);
                }
            }
            Ok(())
        }));
        let mut token = Default::default();
        let _ = core.add_WebResourceRequested(&handler, &mut token);
        let _ = core.cast::<ICoreWebView2>();
    });
}
#[cfg(not(windows))]
fn install_blocker(_wv: &Webview) {}

/// Browser process id of the page, for the memory readout
#[cfg(windows)]
fn page_pid(wv: &Webview) -> Option<u32> {
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = wv.with_webview(move |pw| unsafe {
        let mut pid = 0u32;
        if let Ok(core) = pw.controller().CoreWebView2() {
            let _ = core.BrowserProcessId(&mut pid);
        }
        let _ = tx.send(pid);
    });
    rx.recv_timeout(std::time::Duration::from_secs(2)).ok().filter(|p| *p != 0)
}
#[cfg(not(windows))]
fn page_pid(_wv: &Webview) -> Option<u32> {
    None
}

fn create_view(key: &str, url: &str) -> Result<(), String> {
    let win = app().handle.get_window("main").ok_or("нет окна")?;
    let u: url::Url = url.parse().map_err(|_| "Неверный адрес")?;
    let builder = WebviewBuilder::new(LABEL, WebviewUrl::External(u))
        .data_directory(profile_dir(key))
        .additional_browser_args(&browser_args())
        .user_agent(UA)
        .on_navigation(|u| {
            let s = u.as_str();
            if s.starts_with("http://") || s.starts_with("https://") || s.starts_with("about:") || s.starts_with("data:") || s.starts_with("blob:") {
                return true;
            }
            if EXTERNAL.is_match(s) {
                let _ = open(s); // «Установить» / «Играть» on store pages -> the launcher
            }
            false
        })
        .on_new_window(|u, _f| {
            let s = u.to_string();
            if s.starts_with("http") {
                if let Some(v) = view() {
                    let _ = v.navigate(u); // new tabs open in place: one page = one process
                }
            } else if EXTERNAL.is_match(&s) {
                let _ = open(&s);
            }
            NewWindowResponse::Deny
        })
        .on_page_load(|_w, p| {
            {
                let mut b = app().browser.lock().unwrap();
                b.loading = matches!(p.event(), PageLoadEvent::Started);
                b.url = p.url().to_string();
            }
            app().push();
        })
        .on_document_title_changed(|_w, t| {
            app().browser.lock().unwrap().title = t;
            app().push();
        });
    let (x, y, w, h) = app().browser.lock().unwrap().bounds.unwrap_or((0.0, 0.0, 10.0, 10.0));
    let wv = win.add_child(builder, LogicalPosition::new(x, y), LogicalSize::new(w.max(1.0), h.max(1.0))).map_err(|e| e.to_string())?;
    install_blocker(&wv);
    let _ = wv.hide();
    Ok(())
}

fn apply() {
    let Some(wv) = view() else { return };
    let b = app().browser.lock().unwrap();
    match (b.want_visible, b.bounds) {
        (true, Some((x, y, w, h))) => {
            let _ = wv.set_position(LogicalPosition::new(x, y));
            let _ = wv.set_size(LogicalSize::new(w.max(1.0), h.max(1.0)));
            let _ = wv.show();
        }
        _ => {
            let _ = wv.hide();
        }
    }
}

fn main_window_on_screen() -> bool {
    app().handle.get_window("main").map(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false)).unwrap_or(false)
}

/// Load a store page (called by the UI). Runs off the main thread.
pub fn open_page(key: &str, url: &str) -> Result<(), String> {
    let same = {
        let mut b = app().browser.lock().unwrap();
        b.enabled = true;
        b.unloaded_reason.clear();
        let same = b.loaded && b.key == key;
        b.key = key.into();
        b.url = url.into();
        same
    };
    cancel();
    if same {
        if let (Some(v), Ok(u)) = (view(), url.parse()) {
            let _ = v.navigate(u);
        }
    } else {
        destroy(); // a page can't change its profile (cookies of another store)
        create_view(key, url)?;
        app().browser.lock().unwrap().loaded = true;
    }
    apply();
    start_mem();
    app().push();
    Ok(())
}

/// Recreate the page after it was unloaded, if the browser area is on screen again
pub fn ensure() {
    let (k, u) = {
        let b = app().browser.lock().unwrap();
        if !b.enabled || b.loaded || !b.want_visible || b.url.is_empty() {
            return;
        }
        (b.key.clone(), b.url.clone())
    };
    if main_window_on_screen() {
        let _ = open_page(&k, &u);
    }
}

pub fn set_bounds(rect: Option<(f64, f64, f64, f64)>) {
    let loaded = {
        let mut b = app().browser.lock().unwrap();
        b.want_visible = rect.is_some();
        if rect.is_some() {
            b.bounds = rect;
        }
        b.loaded
    };
    if rect.is_some() {
        cancel();
        ensure();
    } else if loaded {
        schedule(cfg()["unloadOnLeaveSec"].as_u64().unwrap_or(30), "left");
    }
    apply();
}

pub fn schedule(sec: u64, reason: &'static str) {
    let g = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    if !app().browser.lock().unwrap().loaded {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(sec)).await;
        if GEN.load(Ordering::SeqCst) == g {
            unload(reason);
        }
    });
}

pub fn cancel() {
    GEN.fetch_add(1, Ordering::SeqCst);
}

fn destroy() {
    if let Some(v) = view() {
        if let Ok(u) = v.url() {
            app().browser.lock().unwrap().url = u.to_string();
        }
        let _ = v.close();
    }
    let mut b = app().browser.lock().unwrap();
    b.loaded = false;
    b.loading = false;
}

/// Kill the page; it is reloaded from its URL when shown again
pub fn unload(reason: &str) {
    cancel();
    if !app().browser.lock().unwrap().loaded {
        return;
    }
    destroy();
    {
        let mut b = app().browser.lock().unwrap();
        b.unloaded_reason = reason.into();
        b.mem_mb = 0;
    }
    app().log(&format!("browser unloaded: {}", reason));
    app().push();
}

pub fn disable() {
    if app().browser.lock().unwrap().full {
        set_full(false);
    }
    app().browser.lock().unwrap().enabled = false;
    unload("off");
}

pub fn close_if_key(key: &str) {
    if app().browser.lock().unwrap().key == key {
        disable();
    }
}

pub fn nav(action: &str) {
    let Some(v) = view() else { return };
    let _ = match action {
        "back" => v.eval("history.back()"),
        "forward" => v.eval("history.forward()"),
        "reload" => v.reload(),
        "stop" => v.eval("window.stop()"),
        _ => Ok(()),
    };
}

pub fn external() {
    let u = app().browser.lock().unwrap().url.clone();
    if !u.is_empty() {
        let _ = open(&u);
    }
}

pub fn set_full(on: bool) {
    app().browser.lock().unwrap().full = on;
    if let Some(w) = app().handle.get_window("main") {
        let _ = w.set_fullscreen(on);
    }
    app().push();
}

fn start_mem() {
    static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(async {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            let Some(v) = view() else { break };
            let pid = tauri::async_runtime::spawn_blocking(move || page_pid(&v)).await.ok().flatten();
            if let Some(pid) = pid {
                let mb = tauri::async_runtime::spawn_blocking(move || crate::util::winsys::process_tree_mb(pid)).await.unwrap_or(0);
                let changed = {
                    let mut b = app().browser.lock().unwrap();
                    let c = b.mem_mb != mb;
                    b.mem_mb = mb;
                    c
                };
                if changed {
                    app().push();
                }
            }
        }
        RUNNING.store(false, Ordering::SeqCst);
    });
}

/// Main window events
pub fn on_hidden() {
    unload("hidden");
}
pub fn on_minimized() {
    schedule(cfg()["unloadOnMinimizeSec"].as_u64().unwrap_or(60), "minimized");
}
pub fn on_restored() {
    cancel();
    ensure();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blocklist() {
        for u in ["https://www.google-analytics.com/g/collect", "https://stats.g.doubleclick.net/x", "https://mc.yandex.ru/watch/1", "https://cdn.cookielaw.org/consent.js"] {
            assert!(should_block(u, false, true, false), "{}", u);
        }
        for u in ["https://store.steampowered.com/", "https://shared.cloudflare.steamstatic.com/a.jpg", "https://notdoubleclick.net.example.com/"] {
            assert!(!should_block(u, false, true, true), "{}", u);
        }
        assert!(should_block("https://video.akamai.steamstatic.com/store_trailers/1/movie480.webm?t=1", false, false, true));
        assert!(!should_block("https://video.akamai.steamstatic.com/store_trailers/1/movie480.webm", false, false, false));
    }
}
