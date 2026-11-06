use super::Ctx;
use crate::util::common::nz;
use crate::util::common::{exists, fix_path, join, s};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;

pub fn manifest_dir(ctx: &Ctx) -> String {
    ctx.epic_manifest_dir.clone().unwrap_or_else(|| join(&join(&join(&join(&ctx.program_data, "Epic"), "EpicGamesLauncher"), "Data"), "Manifests"))
}

pub fn scan(ctx: &Ctx) -> Vec<Value> {
    let dir = manifest_dir(ctx);
    let Ok(rd) = fs::read_dir(&dir) else { return vec![] };
    let mut games = vec![];
    let mut seen = HashSet::new();
    for e in rd.flatten() {
        if !e.file_name().to_string_lossy().to_lowercase().ends_with(".item") {
            continue;
        }
        let Ok(t) = fs::read_to_string(e.path()) else { continue };
        let Ok(m) = serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')) else { continue };
        if m["bIsIncompleteInstall"] == json!(true) {
            continue;
        }
        let cats: Vec<String> = m["AppCategories"].as_array().map(|a| a.iter().filter_map(|c| c.as_str().map(|x| x.to_lowercase())).collect()).unwrap_or_default();
        if !cats.is_empty() && !cats.iter().any(|c| c == "games") {
            continue;
        }
        let app = s(&m, "AppName");
        let main = s(&m, "MainGameAppName");
        if !main.is_empty() && !app.is_empty() && main != app {
            continue; // DLC
        }
        if app.starts_with("UE_") {
            continue;
        }
        let dir = fix_path(&s(&m, "InstallLocation"));
        if dir.is_empty() || !exists(&dir) {
            continue;
        }
        let item = s(&m, "CatalogItemId");
        let key = if item.is_empty() { app.clone() } else { item.clone() };
        if !seen.insert(key) {
            continue;
        }
        let ns = urlencoding::encode(&s(&m, "CatalogNamespace")).to_string();
        let le = s(&m, "LaunchExecutable");
        let exe = if le.is_empty() { String::new() } else { join(&dir, &le) };
        let name = nz(s(&m, "DisplayName"), app.clone());
        games.push(json!({
            "id": format!("epic:{}", app), "source": "epic", "name": name, "installDir": dir, "exePath": exe,
            "launch": { "type": "uri", "uri": format!("com.epicgames.launcher://apps/{}%3A{}%3A{}?action=launch&silent=true", ns, urlencoding::encode(&item), urlencoding::encode(&app)) },
        }));
    }
    games
}
