//! Shared helpers for store accounts: sign-in window, per-store WebView2 profile, token sealing.
use crate::app::app;
use base64::Engine;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// Each store gets its own WebView2 profile folder: cookies of the sign-in window and of the store pages live there
pub fn profile_dir(key: &str) -> PathBuf {
    app().data_dir.join("web").join(key)
}

/// Chromium switches shared by every webview of a profile (WebView2 requires the same set per folder)
pub fn browser_args() -> String {
    "--autoplay-policy=document-user-activation-required --disable-features=msSmartScreenProtection,msWebOOUI,msPdfOOUI --disk-cache-size=104857600".to_string()
}

/// Chrome-like user agent: some sign-in pages refuse unknown embedded browsers
pub const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36 Edg/138.0.0.0";

pub type Cookies = Vec<tauri::webview::Cookie<'static>>;

pub fn cookie_header(c: &Cookies) -> String {
    c.iter().map(|x| format!("{}={}", x.name(), x.value())).collect::<Vec<_>>().join("; ")
}

/// Opens a sign-in window and polls `check(current_url, cookies)` until it returns a value.
/// `cookie_url` = the site whose cookies `check` needs.
pub async fn login_window<F>(key: &str, title: &str, url: &str, cookie_url: Option<&str>, check: F) -> Result<String, String>
where
    F: Fn(&str, &Cookies) -> Option<String>,
{
    let handle = app().handle.clone();
    let label = format!("login-{}", key);
    if let Some(w) = handle.get_webview_window(&label) {
        let _ = w.close();
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    }
    let last = Arc::new(Mutex::new(url.to_string()));
    let l2 = last.clone();
    let u: url::Url = url.parse().map_err(|_| "bad url")?;
    let h2 = handle.clone();
    let lbl = label.clone();
    let title = title.to_string();
    let key2 = key.to_string();
    let win = tauri::async_runtime::spawn_blocking(move || {
        WebviewWindowBuilder::new(&h2, &lbl, WebviewUrl::External(u))
            .title(title)
            .inner_size(520.0, 760.0)
            .data_directory(profile_dir(&key2))
            .additional_browser_args(&browser_args())
            .user_agent(UA)
            .on_navigation(move |nu| {
                *l2.lock().unwrap() = nu.to_string();
                true
            })
            .build()
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    let cu: Option<url::Url> = cookie_url.and_then(|c| c.parse().ok());
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
        if handle.get_webview_window(&label).is_none() {
            return Err("Окно входа закрыто".into());
        }
        let cur = win.url().map(|u| u.to_string()).unwrap_or_else(|_| last.lock().unwrap().clone());
        let cookies = match &cu {
            Some(c) => win.cookies_for_url(c.clone()).unwrap_or_default(),
            None => vec![],
        };
        if let Some(v) = check(&cur, &cookies) {
            let _ = win.close();
            return Ok(v);
        }
    }
}

/// Fresh cookies of a store profile: loads `url` in a hidden window (the site refreshes its session), then reads cookies
pub async fn fresh_cookies(key: &str, url: &str) -> Result<Cookies, String> {
    let handle = app().handle.clone();
    let label = format!("cookies-{}", key);
    if let Some(w) = handle.get_webview_window(&label) {
        let _ = w.close();
    }
    let done = Arc::new(Mutex::new(false));
    let d2 = done.clone();
    let u: url::Url = url.parse().map_err(|_| "bad url")?;
    let h2 = handle.clone();
    let lbl = label.clone();
    let key2 = key.to_string();
    let uu = u.clone();
    let win = tauri::async_runtime::spawn_blocking(move || {
        WebviewWindowBuilder::new(&h2, &lbl, WebviewUrl::External(uu))
            .visible(false)
            .data_directory(profile_dir(&key2))
            .additional_browser_args(&browser_args())
            .user_agent(UA)
            .on_page_load(move |_w, p| {
                if matches!(p.event(), tauri::webview::PageLoadEvent::Finished) {
                    *d2.lock().unwrap() = true;
                }
            })
            .build()
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        if *done.lock().unwrap() {
            break;
        }
    }
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    let c = win.cookies_for_url(u).unwrap_or_default();
    let _ = win.close();
    Ok(c)
}

/// Tokens are encrypted with Windows DPAPI before going into library.json
pub fn seal(v: &serde_json::Value) -> String {
    let raw = v.to_string();
    match crate::util::winsys::dpapi(raw.as_bytes(), true) {
        Some(b) if cfg!(windows) => format!("dpapi:{}", base64::engine::general_purpose::STANDARD.encode(b)),
        _ => format!("raw:{}", base64::engine::general_purpose::STANDARD.encode(raw)),
    }
}

pub fn unseal(s: &str) -> Option<serde_json::Value> {
    let (kind, b) = s.split_once(':')?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b).ok()?;
    let raw = match kind {
        "dpapi" => crate::util::winsys::dpapi(&bytes, false)?,
        "raw" => bytes,
        _ => return None, // "enc:" = Electron's safeStorage, not readable any more — user signs in again
    };
    serde_json::from_slice(&raw).ok()
}

pub async fn json_req(rb: reqwest::RequestBuilder) -> Result<serde_json::Value, String> {
    let r = rb.send().await.map_err(|e| e.to_string())?;
    let st = r.status();
    let text = r.text().await.map_err(|e| e.to_string())?;
    let data: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    if !st.is_success() {
        let m = ["errorMessage", "error_description", "message", "error"].iter().find_map(|k| data[*k].as_str().map(String::from)).unwrap_or(format!("HTTP {}", st.as_u16()));
        return Err(m);
    }
    Ok(data)
}

pub fn decode_entities(s: &str) -> String {
    s.replace("&quot;", "\"").replace("&#39;", "'").replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}
