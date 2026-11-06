//! Per-game actions around launch, VPN watchdog, schedules. (Filled in by the feature step.)
use serde_json::Value;

pub fn before_launch(_g: &Value) -> Result<(), String> {
    Ok(())
}
pub fn game_started(_id: &str) {}
pub fn game_ended(_id: &str) {}
pub fn download_finished(_id: &str) {}
pub fn on_vpn_dropped() {}
pub fn tick_schedules() {}
