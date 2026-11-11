//! In-game overlay (OSD): a transparent, click-through, always-on-top window in a corner of the screen the game is on.
//! Shows FPS / frame time / 1% low (Windows, PresentMon), CPU, GPU, VRAM, RAM, session time, clock.
//! It never takes focus from the game and injects nothing into it — safe with anti-cheats. Works over windowed and
//! borderless games (most "fullscreen" modes on Windows 10/11); exclusive fullscreen hides it.
//! Linux: MangoHud draws inside the game itself (Vulkan/OpenGL/DXVK) for games Arcadia starts; the window overlay
//! is also available for windowed games on X11.
use crate::app::app;
use crate::util::common::s;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use tauri::{Emitter, Manager};

static VISIBLE: AtomicBool = AtomicBool::new(false);
static GEN: AtomicU64 = AtomicU64::new(0);
pub static HOTKEY_ID: AtomicU32 = AtomicU32::new(0);

pub const ITEMS: &[&str] = &["fps", "frametime", "low1", "cpu", "cputemp", "cpuclk", "gpu", "gputemp", "gpuclk", "gpupower", "vram", "ram", "session", "clock"];

pub fn defaults() -> Value {
    json!({
        "enabled": true, "autoShow": true, "hotkey": "Shift+F12", "position": "tl", "scale": 1.0, "opacity": 0.55,
        "items": ["fps", "frametime", "low1", "cpu", "cputemp", "gpu", "gputemp", "vram", "ram"],
        "mangohud": false, "linuxWindow": false,
    })
}

pub fn settings() -> Value {
    let mut d = defaults();
    let cur = crate::app::try_app().map(|a| a.store.lock().unwrap().settings["osd"].clone()).unwrap_or(Value::Null);
    if let (Some(o), Some(c)) = (d.as_object_mut(), cur.as_object()) {
        for (k, v) in c {
            o.insert(k.clone(), v.clone());
        }
    }
    d
}

pub fn visible() -> bool {
    VISIBLE.load(Ordering::SeqCst)
}

const W: f64 = 300.0;
const H: f64 = 360.0;
const MARGIN: f64 = 14.0;
/// Size of the drawn panel (CSS px, reported by the page): the window is cut to it, so it covers as little as possible
static CONTENT: std::sync::Mutex<(f64, f64)> = std::sync::Mutex::new((0.0, 0.0));

pub fn set_content_size(w: f64, h: f64) {
    let changed = { let mut c = CONTENT.lock().unwrap(); let ch = (c.0 - w).abs() > 1.0 || (c.1 - h).abs() > 1.0; *c = (w, h); ch };
    if changed {
        if let (true, Some(win)) = (visible(), window()) {
            place(&win, &settings());
        }
    }
}

fn window() -> Option<tauri::WebviewWindow> {
    app().handle.get_webview_window("osd")
}

fn ensure_window() -> Result<tauri::WebviewWindow, String> {
    if let Some(w) = window() {
        return Ok(w);
    }
    let w = tauri::WebviewWindowBuilder::new(&app().handle, "osd", tauri::WebviewUrl::App("osd.html".into()))
        .title("Arcadia OSD")
        .transparent(true)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        // resizable: GTK can't shrink a fixed-size window; there is no frame to drag anyway
        .resizable(true)
        .shadow(false)
        .focused(false)
        .visible(false)
        .inner_size(W, H)
        .build()
        .map_err(|e| e.to_string())?;
    // click-through. Linux: only once the window is on screen — tao panics on a window GTK hasn't realized yet
    #[cfg(windows)]
    {
        let _ = w.set_ignore_cursor_events(true);
        no_activate(&w);
    }
    Ok(w)
}

#[cfg(windows)]
fn hwnd_of(w: &tauri::WebviewWindow) -> Option<windows::Win32::Foundation::HWND> {
    w.hwnd().ok().map(|h| windows::Win32::Foundation::HWND(h.0 as _))
}

/// Never activated by clicks / showing, no taskbar or Alt+Tab entry
#[cfg(windows)]
fn no_activate(w: &tauri::WebviewWindow) {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST};
    if let Some(h) = hwnd_of(w) {
        unsafe {
            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
            SetWindowLongPtrW(h, GWL_EXSTYLE, ex | (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0) as isize);
        }
    }
}

#[cfg(windows)]
fn show_no_activate(w: &tauri::WebviewWindow) {
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, ShowWindow, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_SHOWNOACTIVATE};
    if let Some(h) = hwnd_of(w) {
        unsafe {
            let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
    }
}

/// Centre of the window in front (the game) in screen pixels
#[cfg(windows)]
fn foreground_center() -> Option<(i32, i32)> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect};
    unsafe {
        let h = GetForegroundWindow();
        let mut r = RECT::default();
        GetWindowRect(h, &mut r).ok()?;
        Some(((r.left + r.right) / 2, (r.top + r.bottom) / 2))
    }
}
#[cfg(not(windows))]
fn foreground_center() -> Option<(i32, i32)> {
    None
}

/// Moves the overlay to the chosen corner of the monitor with the game
fn place(w: &tauri::WebviewWindow, cfg: &Value) {
    let mons = w.available_monitors().unwrap_or_default();
    let fg = foreground_center();
    let m = fg
        .and_then(|(x, y)| mons.iter().find(|m| { let p = m.position(); let s = m.size(); x >= p.x && y >= p.y && x < p.x + s.width as i32 && y < p.y + s.height as i32 }).cloned())
        .or_else(|| crate::app::main_window().and_then(|mw| mw.current_monitor().ok().flatten()))
        .or_else(|| w.primary_monitor().ok().flatten());
    let Some(m) = m else { return };
    let (cw, ch) = *CONTENT.lock().unwrap();
    let (ww, hh) = if cw > 10.0 && ch > 10.0 {
        ((cw * m.scale_factor()).ceil() as i32 + 2, (ch * m.scale_factor()).ceil() as i32 + 2)
    } else {
        let k = m.scale_factor() * cfg["scale"].as_f64().unwrap_or(1.0).clamp(0.6, 2.5);
        ((W * k) as i32, (H * k) as i32)
    };
    let mg = (MARGIN * m.scale_factor()) as i32;
    let (p, sz) = (m.position(), m.size());
    let pos = s(cfg, "position");
    let x = if pos.ends_with('r') { p.x + sz.width as i32 - ww - mg } else { p.x + mg };
    let y = if pos.starts_with('b') { p.y + sz.height as i32 - hh - mg } else { p.y + mg };
    let _ = w.set_size(tauri::PhysicalSize::new(ww as u32, hh as u32));
    let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
}

pub fn show() {
    if VISIBLE.swap(true, Ordering::SeqCst) {
        return;
    }
    let gen = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    tauri::async_runtime::spawn(async move {
        let w = match ensure_window() {
            Ok(w) => w,
            Err(e) => {
                app().log(&format!("osd: {}", e));
                VISIBLE.store(false, Ordering::SeqCst);
                return;
            }
        };
        // let the page load before the first frame of data
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        place(&w, &settings());
        #[cfg(windows)]
        show_no_activate(&w);
        #[cfg(not(windows))]
        {
            let _ = w.show();
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            if w.is_visible().unwrap_or(false) {
                let _ = w.set_ignore_cursor_events(true);
            }
        }
        app().push();
        let mut n = 0u64;
        while GEN.load(Ordering::SeqCst) == gen && VISIBLE.load(Ordering::SeqCst) {
            let payload = frame().await;
            let _ = w.emit_to("osd", "osd", payload);
            n += 1;
            if n % 3 == 0 {
                // games and other topmost windows can push it back; follow the game to its monitor
                place(&w, &settings());
                #[cfg(windows)]
                show_no_activate(&w);
            }
            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        }
    });
}

pub fn hide() {
    if !VISIBLE.swap(false, Ordering::SeqCst) {
        return;
    }
    GEN.fetch_add(1, Ordering::SeqCst);
    if let Some(w) = window() {
        let _ = w.hide();
    }
    app().push();
}

pub fn toggle() {
    if visible() { hide() } else { show() }
}

/// Settings changed: re-place, or hide when switched off
pub fn refresh() {
    if settings()["enabled"] == false {
        hide();
    } else if let (true, Some(w)) = (visible(), window()) {
        place(&w, &settings());
    }
}

fn running_games() -> Vec<Value> {
    let ids = app().tracker.lock().unwrap().running_ids();
    let st = app().store.lock().unwrap();
    ids.iter().filter_map(|i| st.games.get(i).cloned()).collect()
}

async fn frame() -> Value {
    let cfg = settings();
    let games = running_games();
    let sensors = crate::sensors::read().await;
    let summary = crate::sensors::summary(sensors["hardware"].as_array().map(|v| v.as_slice()).unwrap_or(&[]));
    let (mut fps, mut fps_note) = (Value::Null, String::new());
    if cfg!(windows) {
        let dir = crate::extras::monitor_dir();
        match crate::fps::read(&dir) {
            Some(r) => {
                let dirs: Vec<String> = games.iter().filter_map(|g| crate::tracker::track_dir_of(g)).collect();
                fps = crate::fps::pick(&r, &dirs).unwrap_or(Value::Null);
                if r["pm"] != true {
                    fps_note = s(&r, "error");
                }
            }
            None => fps_note = "FPS: запустите датчики в «Мониторинге»".into(),
        }
    }
    let start = games.first().and_then(|g| app().tracker.lock().unwrap().active.get(&s(g, "id")).cloned());
    let game = games.first().map(|g| { let c = s(g, "customName"); if c.is_empty() { s(g, "name") } else { c } }).unwrap_or_default();
    json!({
        "cfg": cfg, "game": game, "session": start.map(|t| (crate::util::common::now_ms() - t) / 1000), "fps": fps, "fpsNote": fps_note,
        "s": summary, "clock": chrono::Local::now().format("%H:%M").to_string(), "noFps": !cfg!(windows),
    })
}

// ---------------- game start / end ----------------
pub fn on_game_started() {
    let c = settings();
    if c["enabled"] != true || c["autoShow"] != true {
        return;
    }
    if !cfg!(windows) && c["linuxWindow"] != true {
        return; // Linux: MangoHud inside the game instead
    }
    if cfg!(windows) {
        // sensors without a UAC prompt in the middle of a game: only when the scheduled task is set up
        crate::extras::monitor_autostart_quiet();
    }
    show();
}

pub fn on_game_ended() {
    if running_games().is_empty() && settings()["autoShow"] == true {
        hide();
    }
}

// ---------------- hotkey ----------------
pub fn register_hotkey() {
    use std::str::FromStr;
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};
    let c = settings();
    let hk = s(&c, "hotkey");
    HOTKEY_ID.store(0, Ordering::SeqCst);
    if c["enabled"] != true || hk.is_empty() {
        return;
    }
    if let Ok(sc) = Shortcut::from_str(&hk) {
        if app().handle.global_shortcut().register(sc).is_ok() {
            HOTKEY_ID.store(sc.id(), Ordering::SeqCst);
        } else {
            app().log(&format!("osd hotkey {} is taken", hk));
        }
    }
}

pub fn register_hotkey_again() {
    crate::commands::apply_hotkey();
}

// ---------------- MangoHud (Linux) ----------------
pub fn mangohud_path() -> Option<String> {
    if cfg!(windows) {
        return None;
    }
    crate::util::linux::which("mangohud")
}

/// (mangohud wrapper, MANGOHUD_CONFIG) when MangoHud is on in the settings
pub fn mangohud_wrap() -> Option<(String, String)> {
    let c = settings();
    if c["enabled"] != true || c["mangohud"] != true {
        return None;
    }
    Some((mangohud_path()?, mangohud_config(&c)))
}

pub fn mangohud_config(c: &Value) -> String {
    let items: Vec<String> = c["items"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    let on = |k: &str| if items.iter().any(|i| i == k) { 1 } else { 0 };
    let pos = match s(c, "position").as_str() { "tr" => "top-right", "bl" => "bottom-left", "br" => "bottom-right", _ => "top-left" };
    let key = s(c, "hotkey").replace("Shift", "Shift_L").replace("Ctrl", "Control_L").replace("Control+", "Control_L+").replace("Alt", "Alt_L");
    let mut v = vec![
        format!("position={}", pos),
        format!("font_size={}", (24.0 * c["scale"].as_f64().unwrap_or(1.0)).round()),
        format!("background_alpha={:.2}", c["opacity"].as_f64().unwrap_or(0.55)),
        format!("fps={}", on("fps")),
        format!("frametime={}", on("frametime")),
        format!("frame_timing={}", on("frametime")),
        format!("cpu_stats={}", on("cpu")),
        format!("cpu_temp={}", on("cputemp")),
        format!("cpu_mhz={}", on("cpuclk")),
        format!("gpu_stats={}", on("gpu")),
        format!("gpu_temp={}", on("gputemp")),
        format!("gpu_core_clock={}", on("gpuclk")),
        format!("gpu_power={}", on("gpupower")),
        format!("vram={}", on("vram")),
        format!("ram={}", on("ram")),
        format!("time={}", on("clock")),
    ];
    if on("low1") == 1 {
        v.push("fps_metrics=avg,0.01".into());
    }
    if !key.is_empty() {
        v.push(format!("toggle_hud={}", key));
    }
    v.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mangohud_cfg() {
        let mut c = defaults();
        c["position"] = json!("br");
        let t = mangohud_config(&c);
        assert!(t.starts_with("position=bottom-right,font_size=24,"));
        assert!(t.contains("fps=1") && t.contains("cpu_mhz=0") && t.contains("vram=1"));
        assert!(t.ends_with("toggle_hud=Shift_L+F12"));
    }
}
