//! Games without a launcher (torrents, repacks, portable): every sub-folder of a watched folder is a game.
use crate::util::common::{clean_name, file_name, is_dir, is_inside, norm_path, parent, stem};
use crate::util::exefinder::{find_game_exe, list_exes};
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{json, Value};
use std::fs;

static GENERIC: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^(bin|bin32|bin64|binaries|win32|win64|x64|x86|game|games|release|retail|client|app|exe|build)$").unwrap());

/// Readable name + game folder for an exe ("…/Game/Binaries/Win64/x.exe" -> "Game")
pub fn name_for_exe(exe: &str) -> (String, String) {
    let mut dir = parent(exe);
    for _ in 0..4 {
        if GENERIC.is_match(&file_name(&dir)) {
            dir = parent(&dir);
        } else {
            break;
        }
    }
    let n = clean_name(&file_name(&dir));
    (if n.is_empty() { clean_name(&stem(exe)) } else { n }, dir)
}

pub fn make_folder_game(dir: &str, exe: &str) -> Value {
    json!({
        "id": format!("folder:{}", norm_path(dir)), "source": "folder", "name": clean_name(&file_name(dir)),
        "installDir": dir, "exePath": exe, "launch": { "type": "exe", "path": exe, "args": "", "cwd": parent(exe) },
    })
}

pub fn scan(folders: &[String], claimed: &[String]) -> Vec<Value> {
    let claimed: Vec<String> = claimed.iter().map(|c| norm_path(c)).filter(|c| !c.is_empty()).collect();
    let is_claimed = |d: &str| claimed.iter().any(|c| is_inside(d, c) || is_inside(c, d));
    let mut games = vec![];
    for root in folders {
        if !is_dir(root) {
            continue;
        }
        // the folder itself is one game (exe right inside it)
        if !list_exes(root, 0).is_empty() && !is_claimed(root) {
            if let Some(exe) = find_game_exe(root, &file_name(root), 0) {
                games.push(make_folder_game(root, &exe));
                continue;
            }
        }
        let Ok(rd) = fs::read_dir(root) else { continue };
        for e in rd.flatten() {
            if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let dir = e.path().to_string_lossy().to_string();
            if is_claimed(&dir) {
                continue;
            }
            // an unpacked repack with only setup.exe has no game exe yet -> skipped
            if let Some(exe) = find_game_exe(&dir, &clean_name(&e.file_name().to_string_lossy()), 3) {
                games.push(make_folder_game(&dir, &exe));
            }
        }
    }
    games
}
