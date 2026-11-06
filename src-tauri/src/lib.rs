pub mod accounts;
pub mod actions;
pub mod app;
pub mod browser;
pub mod commands;
pub mod extras;
pub mod installs;
pub mod launch;
pub mod meta;
pub mod net;
pub mod scanners;
pub mod store;
pub mod tracker;
pub mod util;

use tauri::Manager;

pub fn run() {
    let hidden = std::env::args().any(|a| a == "--hidden");
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| app::show_window()))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--hidden"])))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|_app, _sc, ev| {
                    if ev.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        app::toggle_window();
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![commands::call])
        .setup(move |a| {
            app::init(a.handle().clone());
            let start_hidden = hidden || app::app().store.lock().unwrap().settings["startMinimized"] == true;
            app::create_window(a.handle(), start_hidden)?;
            app::build_tray(a.handle())?;
            commands::setup_system();
            app::start_loops();
            let (scan, sync) = {
                let st = app::app().store.lock().unwrap();
                (st.settings["scanOnStart"] != false || st.games.is_empty(), st.settings["syncAccountsOnStart"] != false)
            };
            if scan {
                tauri::async_runtime::spawn(async {
                    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
                    app::run_scan();
                });
            } else if app::app().store.lock().unwrap().settings["autoFetchMeta"] != false {
                meta::enqueue_missing();
            }
            if sync {
                // owned libraries: at most once in 12 hours, a bit after start
                tauri::async_runtime::spawn(async {
                    tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                    let stale = app::app().store.lock().unwrap().settings["accounts"].as_object().map(|m| m.values().any(|a| a["id"].as_str().map(|x| !x.is_empty()).unwrap_or(false) && util::common::now_ms() - a["updatedAt"].as_i64().unwrap_or(0) > 12 * 3_600_000)).unwrap_or(false);
                    if stale {
                        accounts::sync_all().await;
                    }
                });
            }
            let _ = a.handle().get_window("main");
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Arcadia")
        .run(|_h, e| {
            if let tauri::RunEvent::ExitRequested { api, code, .. } = e {
                // closing the last window should not quit (tray); explicit quit passes a code
                if code.is_none() && !app::app().quitting.load(std::sync::atomic::Ordering::SeqCst) {
                    api.prevent_exit();
                }
            }
        });
}
