use super::Ctx;
use crate::util::common::nz;
use crate::util::common::{exists, fix_path, join};
use crate::util::vdf::{ci, ci_path, ci_str, parse_vdf};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::sync::Mutex;

const SKIP_APPIDS: &[&str] = &["228980", "1070560", "1391110", "1628350", "1493710", "1826330", "2180100", "250820"];

static LAST_STEAM: Mutex<Option<String>> = Mutex::new(None);

fn skip_name(n: &str) -> bool {
    let l = n.to_lowercase();
    ["proton", "steam linux runtime", "steamworks common", "steam controller", "source sdk base"].iter().any(|p| l.starts_with(p)) || l == "steamvr"
}

pub fn find_steam_path(ctx: &Ctx) -> Option<String> {
    let mut c: Vec<String> = vec![];
    if let Some(p) = &ctx.steam_path {
        c.push(p.clone());
    }
    if let Some(k) = ctx.reg.get(r"HKEY_CURRENT_USER\SOFTWARE\Valve\Steam") {
        c.extend(k.values.get("SteamPath").cloned());
    }
    if let Some(k) = ctx.reg.get(r"HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Valve\Steam") {
        c.extend(k.values.get("InstallPath").cloned());
    }
    if cfg!(windows) {
        c.push(r"C:\Program Files (x86)\Steam".into());
        c.push(r"C:\Program Files\Steam".into());
    } else {
        // native package, Flatpak, Snap
        let h = crate::util::linux::home();
        for p in [".steam/steam", ".local/share/Steam", ".steam/root", ".var/app/com.valvesoftware.Steam/.local/share/Steam", "snap/steam/common/.local/share/Steam"] {
            c.push(format!("{}/{}", h, p));
        }
    }
    c.into_iter().map(|p| fix_path(&p)).find(|p| exists(&join(p, "steamapps")))
}

/// Steam folder found by the last scan (or the default location)
pub fn steam_path() -> Option<String> {
    if let Some(p) = LAST_STEAM.lock().unwrap().clone() {
        return Some(p);
    }
    find_steam_path(&Ctx::default())
}

pub fn library_folders(steam: &str) -> Vec<String> {
    let mut libs = vec![steam.to_string()];
    if let Ok(t) = fs::read_to_string(join(&join(steam, "steamapps"), "libraryfolders.vdf")) {
        let d = parse_vdf(&t);
        if let Some(root) = ci(&d, "libraryfolders").and_then(|v| v.as_object()) {
            for (k, v) in root {
                if !k.chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                let p = if let Some(s) = v.as_str() { s.to_string() } else { ci_str(v, "path") };
                if !p.is_empty() {
                    libs.push(fix_path(&p));
                }
            }
        }
    }
    // on Linux ~/.steam/steam is a symlink to ~/.local/share/Steam: compare real folders
    let real = |l: &str| if cfg!(windows) { l.to_lowercase() } else { fs::canonicalize(l).map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|_| l.to_string()) };
    let mut seen = HashSet::new();
    libs.into_iter().filter(|l| seen.insert(real(l)) && exists(&join(l, "steamapps"))).collect()
}

/// Steam's own playtime from the most recently used local profile: appid -> (seconds, lastPlayed ms)
pub fn read_playtime(steam: &str) -> std::collections::HashMap<String, (i64, i64)> {
    let mut out = std::collections::HashMap::new();
    let ud = join(steam, "userdata");
    let mut users: Vec<(String, std::time::SystemTime)> = fs::read_dir(&ud)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let f = e.path().join("config").join("localconfig.vdf");
                    fs::metadata(&f).and_then(|m| m.modified()).ok().map(|t| (f.to_string_lossy().to_string(), t))
                })
                .collect()
        })
        .unwrap_or_default();
    users.sort_by(|a, b| b.1.cmp(&a.1));
    let Some((f, _)) = users.first() else { return out };
    let Ok(t) = fs::read_to_string(f) else { return out };
    let d = parse_vdf(&t);
    if let Some(apps) = ci_path(&d, &["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"]).and_then(|a| a.as_object()) {
        for (id, v) in apps {
            let pt: i64 = ci_str(v, "Playtime").parse().unwrap_or(0);
            let lp: i64 = ci_str(v, "LastPlayed").parse().unwrap_or(0);
            if pt > 0 || lp > 0 {
                out.insert(id.clone(), (pt * 60, lp * 1000));
            }
        }
    }
    out
}

fn local_art(steam: &str, appid: &str) -> Value {
    let base = join(&join(&join(steam, "appcache"), "librarycache"), "");
    let mut art = serde_json::Map::new();
    let flat = |n: &str| join(&base, &format!("{}_{}", appid, n));
    if exists(&flat("library_600x900.jpg")) {
        art.insert("cover".into(), json!(flat("library_600x900.jpg")));
    }
    if exists(&flat("library_hero.jpg")) {
        art.insert("hero".into(), json!(flat("library_hero.jpg")));
    }
    let dir = join(&base, appid);
    fn walk(d: &std::path::Path, depth: u32, art: &mut serde_json::Map<String, Value>) {
        let Ok(rd) = fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && depth < 2 {
                walk(&p, depth + 1, art);
            } else {
                let n = e.file_name().to_string_lossy().to_lowercase();
                if !art.contains_key("cover") && (n == "library_600x900.jpg" || n == "library_capsule.jpg") {
                    art.insert("cover".into(), json!(p.to_string_lossy()));
                }
                if !art.contains_key("hero") && n == "library_hero.jpg" {
                    art.insert("hero".into(), json!(p.to_string_lossy()));
                }
            }
        }
    }
    walk(std::path::Path::new(&dir), 0, &mut art);
    Value::Object(art)
}

pub fn scan(ctx: &Ctx) -> Vec<Value> {
    let Some(steam) = find_steam_path(ctx) else { return vec![] };
    *LAST_STEAM.lock().unwrap() = Some(steam.clone());
    let playtime = read_playtime(&steam);
    let mut games = vec![];
    let mut seen = HashSet::new();
    for lib in library_folders(&steam) {
        let sa = join(&lib, "steamapps");
        let Ok(rd) = fs::read_dir(&sa) else { continue };
        for e in rd.flatten() {
            let f = e.file_name().to_string_lossy().to_string();
            if !(f.to_lowercase().starts_with("appmanifest_") && f.to_lowercase().ends_with(".acf")) {
                continue;
            }
            let Ok(t) = fs::read_to_string(e.path()) else { continue };
            let d = parse_vdf(&t);
            let Some(st) = ci(&d, "AppState") else { continue };
            let appid = ci_str(st, "appid");
            let name = ci_str(st, "name");
            let flags: i64 = ci_str(st, "StateFlags").parse().unwrap_or(4);
            if appid.is_empty() || seen.contains(&appid) || SKIP_APPIDS.contains(&appid.as_str()) || skip_name(&name) || flags & 4 == 0 {
                continue;
            }
            seen.insert(appid.clone());
            let idir = nz(ci_str(st, "installdir"), name.clone());
            let pt = playtime.get(&appid).cloned().unwrap_or((0, 0));
            games.push(json!({
                "id": format!("steam:{}", appid), "source": "steam", "name": name,
                "installDir": join(&join(&sa, "common"), &idir), "steamAppId": appid,
                "launch": { "type": "uri", "uri": format!("steam://rungameid/{}", appid) },
                "importedPlaytime": pt.0, "importedLastPlayed": pt.1, "localArt": local_art(&steam, &appid),
            }));
        }
    }
    games
}
