//! Extra features. (Filled in by the feature step.)
use serde_json::{json, Value};
use std::sync::atomic::AtomicBool;

pub static AUTOPICK_STOP: AtomicBool = AtomicBool::new(false);

pub async fn hltb(_id: &str) -> Result<Value, String> {
    Err("ещё не готово".into())
}
pub async fn zapret_autopick() {}
pub async fn dispatch(_ch: &str, _a: &[Value]) -> Value {
    json!({ "ok": false, "error": "ещё не готово" })
}
