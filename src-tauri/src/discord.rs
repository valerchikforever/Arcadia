//! Discord Rich Presence over the local IPC pipe: "Играет в Arcadia — <игра>" for games Discord can't see
//! (torrents, Tarkov, anything outside Steam). Needs the Client ID of a Discord application the user created.
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::sync::Mutex;

static CONN: Mutex<Option<std::fs::File>> = Mutex::new(None);

fn frame(op: u32, payload: &Value) -> Vec<u8> {
    let body = payload.to_string().into_bytes();
    let mut v = Vec::with_capacity(8 + body.len());
    v.extend_from_slice(&op.to_le_bytes());
    v.extend_from_slice(&(body.len() as u32).to_le_bytes());
    v.extend_from_slice(&body);
    v
}

fn read_frame(f: &mut std::fs::File) -> Option<Value> {
    let mut h = [0u8; 8];
    f.read_exact(&mut h).ok()?;
    let len = u32::from_le_bytes([h[4], h[5], h[6], h[7]]) as usize;
    let mut b = vec![0u8; len.min(1 << 20)];
    f.read_exact(&mut b).ok()?;
    serde_json::from_slice(&b).ok()
}

fn connect(client_id: &str) -> Result<std::fs::File, String> {
    if !cfg!(windows) {
        return Err("Только Windows".into());
    }
    for i in 0..10 {
        let path = format!(r"\\.\pipe\discord-ipc-{}", i);
        if let Ok(mut f) = std::fs::OpenOptions::new().read(true).write(true).open(&path) {
            f.write_all(&frame(0, &json!({ "v": 1, "client_id": client_id }))).map_err(|e| e.to_string())?;
            let r = read_frame(&mut f).ok_or("Discord не ответил")?;
            if r["evt"] == "READY" {
                return Ok(f);
            }
            return Err(format!("Discord отклонил подключение: {}", r["data"]["message"].as_str().unwrap_or("проверьте Client ID")));
        }
    }
    Err("Discord не запущен".into())
}

/// Shows the activity; the connection stays open while the game runs (Discord clears the status when it closes)
pub fn set_activity(client_id: &str, game: &str, start_ms: i64, image: Option<&str>) -> Result<(), String> {
    let mut guard = CONN.lock().unwrap();
    if guard.is_none() {
        *guard = Some(connect(client_id)?);
    }
    let mut assets = json!({ "large_text": game });
    if let Some(u) = image.filter(|u| u.starts_with("https://")) {
        assets["large_image"] = json!(u);
    }
    let payload = json!({
        "cmd": "SET_ACTIVITY", "nonce": format!("{}", start_ms),
        "args": { "pid": std::process::id(), "activity": { "details": game, "timestamps": { "start": start_ms / 1000 }, "assets": assets, "instance": false } }
    });
    let f = guard.as_mut().unwrap();
    if f.write_all(&frame(1, &payload)).is_err() {
        *guard = None;
        return Err("Связь с Discord потеряна".into());
    }
    let _ = read_frame(f);
    Ok(())
}

pub fn clear() {
    let mut guard = CONN.lock().unwrap();
    if let Some(f) = guard.as_mut() {
        let _ = f.write_all(&frame(1, &json!({ "cmd": "SET_ACTIVITY", "nonce": "clear", "args": { "pid": std::process::id() } })));
    }
    *guard = None; // closing the pipe also clears it
}
