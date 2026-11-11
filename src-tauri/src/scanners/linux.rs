//! Linux launchers: Heroic Games Launcher (Epic via Legendary, GOG, Amazon via Nile) and Lutris.
//! Each scanner takes the folders to look in, so tests can point them at a temp directory.
use crate::util::common::{exists, file_name, nz, s};
use crate::util::linux::{config_home, data_home, home};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;

fn read_json(p: &str) -> Value {
    fs::read_to_string(p).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null)
}

/// Heroic config folders: native and Flatpak
pub fn heroic_roots() -> Vec<String> {
    vec![format!("{}/heroic", config_home()), format!("{}/.var/app/com.heroicgameslauncher.hgl/config/heroic", home())]
}

fn heroic_uri(runner: &str, app: &str) -> String {
    format!("heroic://launch/{}/{}", runner, urlencoding::encode(app))
}

/// Titles from Heroic's library caches (installed.json of GOG/Nile has no names)
fn heroic_titles(root: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for f in ["store_cache/gog_library.json", "gog_store/library.json", "store_cache/nile_library.json", "store_cache/legendary_library.json"] {
        let v = read_json(&format!("{}/{}", root, f));
        for key in ["games", "library"] {
            for g in v[key].as_array().into_iter().flatten() {
                let id = nz(s(g, "app_name"), s(g, "appName"));
                let t = s(g, "title");
                if !id.is_empty() && !t.is_empty() {
                    m.insert(id, t);
                }
            }
        }
    }
    m
}

/// Heroic play time: store/timestamp.json {appName: {totalPlayed (minutes), lastPlayed}}
fn heroic_times(root: &str) -> HashMap<String, (i64, i64)> {
    let v = read_json(&format!("{}/store/timestamp.json", root));
    let mut m = HashMap::new();
    if let Some(o) = v.as_object() {
        for (k, t) in o {
            let last = t["lastPlayed"].as_str().and_then(|x| chrono::DateTime::parse_from_rfc3339(x).ok()).map(|d| d.timestamp_millis()).unwrap_or(0);
            m.insert(k.clone(), ((t["totalPlayed"].as_f64().unwrap_or(0.0) * 60.0) as i64, last));
        }
    }
    m
}

pub fn scan_heroic_in(roots: &[String], legendary_extra: &[String]) -> Vec<Value> {
    let mut games = vec![];
    let mut seen = HashSet::new();
    for root in roots {
        if !exists(root) {
            continue;
        }
        let titles = heroic_titles(root);
        let times = heroic_times(root);
        let mut push = |id: String, source: &str, runner: &str, app: &str, name: String, dir: String, exe: String| {
            if dir.is_empty() || !exists(&dir) || !seen.insert(id.clone()) {
                return;
            }
            let (pt, last) = times.get(app).cloned().unwrap_or((0, 0));
            games.push(json!({
                "id": id, "source": source, "name": name, "installDir": dir, "exePath": exe, "launcher": "heroic",
                "launch": { "type": "uri", "uri": heroic_uri(runner, app) },
                "importedPlaytime": pt, "importedLastPlayed": last,
            }));
        };
        // Epic (Legendary)
        let mut leg = vec![format!("{}/legendaryConfig/legendary/installed.json", root)];
        leg.extend(legendary_extra.iter().cloned());
        for f in leg {
            if let Some(o) = read_json(&f).as_object() {
                for (app, g) in o {
                    if g["is_dlc"] == true {
                        continue;
                    }
                    let dir = s(g, "install_path");
                    let exe = s(g, "executable");
                    let exe = if exe.is_empty() { String::new() } else { format!("{}/{}", dir, exe) };
                    push(format!("epic:{}", app), "epic", "legendary", app, nz(s(g, "title"), app.clone()), dir, exe);
                }
            }
        }
        // GOG
        for g in read_json(&format!("{}/gog_store/installed.json", root))["installed"].as_array().into_iter().flatten() {
            if g["is_dlc"] == true {
                continue;
            }
            let app = nz(s(g, "appName"), s(g, "app_name"));
            let dir = s(g, "install_path");
            let name = titles.get(&app).cloned().unwrap_or_else(|| file_name(&dir));
            push(format!("gog:{}", app), "gog", "gog", &app, name, dir, s(g, "executable"));
        }
        // Amazon Games (Nile)
        for g in read_json(&format!("{}/nile_config/nile/installed.json", root)).as_array().into_iter().flatten() {
            let app = s(g, "id");
            let dir = s(g, "path");
            let name = titles.get(&app).cloned().unwrap_or_else(|| file_name(&dir));
            push(format!("amazon:{}", app), "amazon", "nile", &app, name, dir, String::new());
        }
    }
    games
}

pub fn scan_heroic(_: &super::Ctx) -> Vec<Value> {
    if cfg!(windows) {
        return vec![];
    }
    scan_heroic_in(&heroic_roots(), &[format!("{}/legendary/installed.json", config_home())])
}

/// Lutris keeps its library in pga.db (SQLite); the game's exe lives in games/<configpath>.yml
pub fn scan_lutris_in(db: &str, config_dir: &str) -> Vec<Value> {
    let Ok(c) = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY) else { return vec![] };
    let Ok(mut st) = c.prepare("SELECT id, name, slug, runner, directory, playtime, lastplayed, configpath FROM games WHERE installed = 1") else { return vec![] };
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            r.get::<_, Option<String>>(2)?.unwrap_or_default(),
            r.get::<_, Option<String>>(3)?.unwrap_or_default(),
            r.get::<_, Option<String>>(4)?.unwrap_or_default(),
            r.get::<_, Option<f64>>(5)?.unwrap_or(0.0),
            r.get::<_, Option<i64>>(6)?.unwrap_or(0),
            r.get::<_, Option<String>>(7)?.unwrap_or_default(),
        ))
    });
    let Ok(rows) = rows else { return vec![] };
    let exe_re = regex::Regex::new(r#"(?m)^\s*exe:\s*['"]?([^'"\n]+?)['"]?\s*$"#).unwrap();
    let mut games = vec![];
    for (id, name, slug, runner, dir, hours, last, cfgp) in rows.flatten() {
        if runner == "steam" {
            continue; // already in the library from Steam itself
        }
        let exe = fs::read_to_string(format!("{}/{}.yml", config_dir, cfgp)).ok().and_then(|t| exe_re.captures(&t).map(|c| c[1].trim().to_string())).unwrap_or_default();
        let exe = if !exe.is_empty() && !exe.starts_with('/') && !dir.is_empty() { format!("{}/{}", dir, exe) } else { exe };
        let track = if !exe.is_empty() { crate::util::common::parent(&exe) } else { dir.clone() };
        games.push(json!({
            "id": format!("lutris:{}", nz(slug, id.to_string())), "source": "lutris", "name": name, "installDir": track, "exePath": exe,
            "launch": { "type": "uri", "uri": format!("lutris:rungameid/{}", id) },
            "importedPlaytime": (hours * 3600.0) as i64, "importedLastPlayed": last * 1000,
        }));
    }
    games
}

pub fn scan_lutris(_: &super::Ctx) -> Vec<Value> {
    if cfg!(windows) {
        return vec![];
    }
    for (db, cfg) in [
        (format!("{}/lutris/pga.db", data_home()), format!("{}/lutris/games", config_home())),
        (format!("{}/.var/app/net.lutris.Lutris/data/lutris/pga.db", home()), format!("{}/.var/app/net.lutris.Lutris/config/lutris/games", home())),
    ] {
        if exists(&db) {
            return scan_lutris_in(&db, &cfg);
        }
    }
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn heroic_and_lutris() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("heroic");
        let r = root.to_string_lossy().to_string();
        let games = t.path().join("Games");
        for d in ["Fortnite", "Witcher 3", "Lutris Game"] {
            fs::create_dir_all(games.join(d)).unwrap();
        }
        let g = |d: &str| games.join(d).to_string_lossy().to_string();
        let w = |p: &str, v: Value| { let f = root.join(p); fs::create_dir_all(f.parent().unwrap()).unwrap(); fs::write(f, v.to_string()).unwrap(); };
        w("legendaryConfig/legendary/installed.json", json!({ "Fortnite": { "app_name": "Fortnite", "title": "Fortnite", "install_path": g("Fortnite"), "executable": "FortniteGame/Binaries/Win64/FortniteLauncher.exe", "is_dlc": false }, "Dlc": { "title": "DLC", "install_path": g("Fortnite"), "is_dlc": true } }));
        w("gog_store/installed.json", json!({ "installed": [{ "appName": "1207664663", "install_path": g("Witcher 3"), "platform": "windows", "is_dlc": false }] }));
        w("store_cache/gog_library.json", json!({ "games": [{ "app_name": "1207664663", "title": "The Witcher 3: Wild Hunt" }] }));
        w("store/timestamp.json", json!({ "1207664663": { "totalPlayed": 90, "lastPlayed": "2026-01-02T10:00:00.000Z" } }));
        let h = scan_heroic_in(&[r], &[]);
        assert_eq!(h.len(), 2);
        let w3 = h.iter().find(|x| x["id"] == "gog:1207664663").unwrap();
        assert_eq!(w3["name"], "The Witcher 3: Wild Hunt");
        assert_eq!(w3["importedPlaytime"], 5400);
        assert_eq!(w3["launch"]["uri"], "heroic://launch/gog/1207664663");

        let db = t.path().join("pga.db");
        let c = rusqlite::Connection::open(&db).unwrap();
        c.execute_batch("CREATE TABLE games (id INTEGER PRIMARY KEY, name TEXT, slug TEXT, runner TEXT, directory TEXT, installed INTEGER, playtime REAL, lastplayed INTEGER, configpath TEXT);").unwrap();
        c.execute("INSERT INTO games VALUES (7, 'Lutris Game', 'lutris-game', 'wine', ?1, 1, 2.5, 1700000000, 'lutris-game-123')", [g("Lutris Game")]).unwrap();
        c.execute("INSERT INTO games VALUES (8, 'Steam one', 'steam-one', 'steam', '', 1, 0, 0, 'x')", []).unwrap();
        drop(c);
        let cfg = t.path().join("lgames");
        fs::create_dir_all(&cfg).unwrap();
        fs::write(cfg.join("lutris-game-123.yml"), "game:\n  exe: bin/game.exe\n  prefix: /p\n").unwrap();
        let l = scan_lutris_in(&db.to_string_lossy(), &cfg.to_string_lossy());
        assert_eq!(l.len(), 1);
        assert_eq!(l[0]["launch"]["uri"], "lutris:rungameid/7");
        assert_eq!(l[0]["importedPlaytime"], 9000);
        assert!(l[0]["exePath"].as_str().unwrap().ends_with("Lutris Game/bin/game.exe"));
    }
}
