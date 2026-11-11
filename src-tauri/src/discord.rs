//! Discord Rich Presence over the local IPC pipe: "Играет в Arcadia — <игра>" for games Discord can't see
//! (torrents, Tarkov, anything outside Steam). Needs the Client ID of a Discord application the user created.
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::sync::Mutex;

/// Windows: named pipe \\.\pipe\discord-ipc-N; Linux: unix socket $XDG_RUNTIME_DIR/discord-ipc-N
#[cfg(windows)]
type Conn = std::fs::File;
#[cfg(unix)]
type Conn = std::os::unix::net::UnixStream;

static CONN: Mutex<Option<Conn>> = Mutex::new(None);

fn frame(op: u32, payload: &Value) -> Vec<u8> {
    let body = payload.to_string().into_bytes();
    let mut v = Vec::with_capacity(8 + body.len());
    v.extend_from_slice(&op.to_le_bytes());
    v.extend_from_slice(&(body.len() as u32).to_le_bytes());
    v.extend_from_slice(&body);
    v
}

fn read_frame(f: &mut Conn) -> Option<Value> {
    let mut h = [0u8; 8];
    f.read_exact(&mut h).ok()?;
    let len = u32::from_le_bytes([h[4], h[5], h[6], h[7]]) as usize;
    let mut b = vec![0u8; len.min(1 << 20)];
    f.read_exact(&mut b).ok()?;
    serde_json::from_slice(&b).ok()
}

fn handshake(mut f: Conn, client_id: &str) -> Result<Conn, String> {
    f.write_all(&frame(0, &json!({ "v": 1, "client_id": client_id }))).map_err(|e| e.to_string())?;
    let r = read_frame(&mut f).ok_or("Discord не ответил")?;
    if r["evt"] == "READY" {
        return Ok(f);
    }
    Err(format!("Discord отклонил подключение: {}", r["data"]["message"].as_str().unwrap_or("проверьте Client ID")))
}

#[cfg(windows)]
fn connect(client_id: &str) -> Result<Conn, String> {
    for i in 0..10 {
        let path = format!(r"\\.\pipe\discord-ipc-{}", i);
        if let Ok(f) = std::fs::OpenOptions::new().read(true).write(true).open(&path) {
            return handshake(f, client_id);
        }
    }
    Err("Discord не запущен".into())
}

#[cfg(unix)]
fn connect(client_id: &str) -> Result<Conn, String> {
    let run = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    // the regular app, Flatpak and Snap builds put the socket in different places
    for dir in [run.clone(), format!("{}/app/com.discordapp.Discord", run), format!("{}/snap.discord", run), "/tmp".into()] {
        for i in 0..10 {
            if let Ok(f) = std::os::unix::net::UnixStream::connect(format!("{}/discord-ipc-{}", dir, i)) {
                let _ = f.set_read_timeout(Some(std::time::Duration::from_secs(5)));
                return handshake(f, client_id);
            }
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
