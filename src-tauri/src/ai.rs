//! «ИИ»: AI services in their own windows (one shared sign-in profile) or in the system browser.
//! Services that refuse Russia / the CIS (or ban accounts seen from there) open only while the VPN runs,
//! and their windows close as soon as the VPN goes down — so the real IP never reaches them.
use crate::app::app;
use crate::util::common::s;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Mutex;
use tauri::Manager;

/// Windows opened for services that need the VPN (labels)
static GUARDED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn label(id: &str) -> String {
    let clean: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' }).collect();
    format!("ai-{}", clean)
}

pub fn vpn_on() -> bool {
    app().vpn.lock().unwrap().running
}

/// Open a service. args: { id, name, url, vpn (needs VPN), mode: "window" | "browser" }
pub fn open(a: &Value) -> Value {
    let (id, name, url) = (s(a, "id"), s(a, "name"), s(a, "url"));
    let needs_vpn = a["vpn"] == true;
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return json!({ "ok": false, "error": "Неверный адрес" });
    }
    if needs_vpn && !vpn_on() {
        return json!({ "ok": false, "needVpn": true, "error": format!("{} не работает в России и СНГ — сначала включите VPN", name) });
    }
    if s(a, "mode") == "browser" {
        return match crate::util::winsys::open(&url) {
            Ok(()) => json!({ "ok": true }),
            Err(e) => json!({ "ok": false, "error": e }),
        };
    }
    let lb = label(&id);
    if let Some(w) = app().handle.get_webview_window(&lb) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        return json!({ "ok": true });
    }
    let Ok(u) = url.parse::<url::Url>() else { return json!({ "ok": false, "error": "Неверный адрес" }) };
    // one profile for all services: a Google / Microsoft sign-in works everywhere
    let profile = app().data_dir.join("web").join("ai");
    let built = tauri::WebviewWindowBuilder::new(&app().handle, &lb, tauri::WebviewUrl::External(u))
        .title(format!("{} — Arcadia", name))
        .inner_size(1180.0, 860.0)
        .min_inner_size(480.0, 400.0)
        .data_directory(profile)
        .on_new_window(|u, _| {
            // sign-in pop-ups and links open in the system browser only if they leave http(s)
            if u.scheme() == "http" || u.scheme() == "https" {
                tauri::webview::NewWindowResponse::Allow
            } else {
                let _ = crate::util::winsys::open(u.as_str());
                tauri::webview::NewWindowResponse::Deny
            }
        })
        .build();
    match built {
        Ok(w) => {
            if let Some(icon) = app().handle.default_window_icon() {
                let _ = w.set_icon(icon.clone());
            }
            if needs_vpn {
                GUARDED.lock().unwrap().get_or_insert_with(HashSet::new).insert(lb);
            }
            json!({ "ok": true })
        }
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    }
}

/// Called by the network poll: VPN down -> close the windows of services that need it
pub fn guard() {
    if vpn_on() {
        return;
    }
    let labels: Vec<String> = GUARDED.lock().unwrap().as_mut().map(|g| g.drain().collect()).unwrap_or_default();
    let mut closed = vec![];
    for lb in labels {
        if let Some(w) = app().handle.get_webview_window(&lb) {
            closed.push(w.title().unwrap_or_default().replace(" — Arcadia", ""));
            let _ = w.destroy();
        }
    }
    if !closed.is_empty() {
        crate::actions::notify("vpn", "VPN выключен — окна ИИ закрыты", &format!("{}: без VPN эти сервисы увидели бы ваш настоящий адрес. Включите VPN и откройте их снова.", closed.join(", ")));
    }
}

/// Which service windows are open (for the «открыто» marks)
pub fn open_ids() -> Vec<String> {
    app().handle.webview_windows().keys().filter_map(|k| k.strip_prefix("ai-").map(String::from)).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn labels() {
        assert_eq!(super::label("chatgpt"), "ai-chatgpt");
        assert_eq!(super::label("my ai/1"), "ai-my-ai-1");
    }
}
