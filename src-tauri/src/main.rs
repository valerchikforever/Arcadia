#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    // elevated sensor helper (FPS / LibreHardwareMonitor): Arcadia.exe --monitor-helper <dir> <pid>
    if let Some(code) = arcadia_lib::run_helper_mode() {
        std::process::exit(code);
    }
    arcadia_lib::run();
}
