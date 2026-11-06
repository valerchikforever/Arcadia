//! Library storage: %APPDATA%\Arcadia\library.json — the same file the Electron version used,
//! so play time, collections and settings carry over.
use crate::util::common::{exists, now_ms, s};
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

pub fn default_settings() -> Value {
    json!({
        "sources": { "steam": true, "epic": true, "gog": true, "ea": true, "ubisoft": true, "battlenet": true, "battlestate": true, "folder": true },
        "scanFolders": [],
        "sgdbKey": "",
        "autoFetchMeta": true,
        "closeToTray": true,
        "autostart": false,
        "startMinimized": false,
        "minimizeOnLaunch": true,
        "trackExternal": true,
        "hotkey": "CommandOrControl+Alt+G",
        "cardSize": 190,
        "sort": "name",
        "ignoredIds": [],
        "scanOnStart": true,
        "zapret": { "dir": "", "strategy": "", "autostart": false, "gameFilter": false, "stopOnExit": false },
        "vpn": { "servers": [], "subs": [], "selected": "auto", "mode": "proxy", "directRu": true, "gamesDirect": true, "autostart": false, "stopOnExit": true, "port": 2080 },
        "accounts": {},
        "steamApiKey": "",
        "hideUninstalled": false,
        "installFilter": "all",
        "syncAccountsOnStart": true,
        "browser": { "economy": true, "blockTrackers": true, "autoEnable": false, "unloadOnMinimizeSec": 60, "unloadOnLeaveSec": 30 }
    })
}

/// Fields the scanner owns (refreshed every scan); everything else belongs to the user
const SCAN_FIELDS: &[&str] = &["source", "name", "installDir", "exePath", "launch", "trackDir", "steamAppId", "importedPlaytime", "importedLastPlayed", "localArt"];
/// Fields from a store account (owned games that may not be installed)
const OWNED_FIELDS: &[&str] = &["installUri", "storeUrl", "coverUrl", "heroUrl"];

pub struct Store {
    pub dir: PathBuf,
    pub file: PathBuf,
    pub media_dir: PathBuf,
    pub games: Map<String, Value>,
    pub collections: Vec<Value>,
    pub settings: Value,
    pub scan_status: Value,
    pub dirty: bool,
}

fn merge_defaults(def: &Value, user: &Value) -> Value {
    match (def, user) {
        (Value::Object(d), Value::Object(u)) => {
            let mut out = d.clone();
            for (k, v) in u {
                let merged = match d.get(k) {
                    // one level deeper for nested setting groups (sources, zapret, vpn, browser)
                    Some(dv @ Value::Object(_)) if v.is_object() && k != "accounts" => merge_defaults(dv, v),
                    _ => v.clone(),
                };
                out.insert(k.clone(), merged);
            }
            Value::Object(out)
        }
        (_, u) => u.clone(),
    }
}

pub fn data_dir() -> PathBuf {
    if let Ok(a) = std::env::var("ARCADIA_DATA") {
        return PathBuf::from(a);
    }
    if cfg!(windows) {
        if let Ok(a) = std::env::var("APPDATA") {
            return PathBuf::from(a).join("Arcadia");
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".config").join("Arcadia")
}

impl Store {
    pub fn new(dir: PathBuf) -> Store {
        let media_dir = dir.join("media");
        let _ = fs::create_dir_all(&media_dir);
        let mut st = Store {
            file: dir.join("library.json"),
            dir,
            media_dir,
            games: Map::new(),
            collections: vec![],
            settings: default_settings(),
            scan_status: json!({}),
            dirty: false,
        };
        st.load();
        st
    }

    fn load(&mut self) {
        for f in [self.file.clone(), self.file.with_extension("json.bak")] {
            let Ok(txt) = fs::read_to_string(&f) else { continue };
            let Ok(d) = serde_json::from_str::<Value>(&txt) else { continue };
            self.games = d.get("games").and_then(|g| g.as_object()).cloned().unwrap_or_default();
            self.collections = d.get("collections").and_then(|c| c.as_array()).cloned().unwrap_or_default();
            self.settings = merge_defaults(&default_settings(), d.get("settings").unwrap_or(&json!({})));
            return;
        }
    }

    pub fn save(&mut self) {
        self.dirty = true;
    }

    pub fn flush(&mut self) {
        self.dirty = false;
        let data = json!({ "version": 1, "games": self.games, "collections": self.collections, "settings": self.settings });
        let Ok(txt) = serde_json::to_string_pretty(&data) else { return };
        if self.file.exists() {
            let _ = fs::copy(&self.file, self.file.with_extension("json.bak"));
        }
        let tmp = self.file.with_extension("json.tmp");
        if fs::write(&tmp, txt).is_ok() {
            let _ = fs::rename(&tmp, &self.file);
        }
    }

    pub fn setting(&self, k: &str) -> &Value {
        self.settings.get(k).unwrap_or(&Value::Null)
    }

    pub fn update(&mut self, id: &str, patch: Value) -> bool {
        let Some(g) = self.games.get_mut(id).and_then(|g| g.as_object_mut()) else { return false };
        if let Value::Object(p) = patch {
            for (k, v) in p {
                g.insert(k, v);
            }
        }
        self.save();
        true
    }

    pub fn add_game(&mut self, g: Value) -> String {
        let id = s(&g, "id");
        let merged = match self.games.get(&id) {
            Some(old) => {
                let mut o = old.clone();
                for (k, v) in pick(&g, SCAN_FIELDS) {
                    o[k] = v;
                }
                o["missing"] = json!(false);
                o
            }
            None => new_game(&g),
        };
        self.games.insert(id.clone(), merged);
        if let Some(arr) = self.settings["ignoredIds"].as_array_mut() {
            arr.retain(|x| x.as_str() != Some(&id));
        }
        self.save();
        id
    }

    pub fn remove(&mut self, id: &str) {
        let Some(g) = self.games.remove(id) else { return };
        if s(&g, "source") != "manual" {
            if let Some(arr) = self.settings["ignoredIds"].as_array_mut() {
                if !arr.iter().any(|x| x.as_str() == Some(id)) {
                    arr.push(json!(id));
                }
            }
        }
        self.save();
    }

    fn ignored(&self) -> HashSet<String> {
        self.settings["ignoredIds"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
    }

    /// Merges a scan result. Returns the number of new games.
    pub fn merge_scan(&mut self, found: Vec<Value>, status: Value) -> usize {
        self.scan_status = status;
        let ignored = self.ignored();
        let mut seen = HashSet::new();
        let mut added = 0;
        for f in found {
            let id = s(&f, "id");
            if ignored.contains(&id) {
                continue;
            }
            seen.insert(id.clone());
            match self.games.get_mut(&id) {
                Some(old) => {
                    for (k, v) in pick(&f, SCAN_FIELDS) {
                        old[k] = v;
                    }
                    old["missing"] = json!(false);
                    // an owned game that just got installed keeps its account data, but gets a real launch method
                    old["installed"] = json!(true);
                }
                None => {
                    self.games.insert(id, new_game(&f));
                    added += 1;
                }
            }
        }
        let status = self.scan_status.clone();
        for g in self.games.values_mut() {
            let id = s(g, "id");
            if seen.contains(&id) {
                continue;
            }
            let src = s(g, "source");
            let st = &status[&src];
            if g["owned"].as_bool() == Some(true) {
                // bought on an account but not on disk — "not installed", not "missing"
                if st.get("error").is_none() {
                    g["installed"] = json!(false);
                    g["missing"] = json!(false);
                }
                continue;
            }
            if src == "manual" {
                let missing = g["launch"]["type"] == "exe" && !exists(&s(&g["launch"], "path"));
                g["missing"] = json!(missing);
                continue;
            }
            if st.get("enabled") == Some(&json!(false)) || st.get("error").is_some() {
                continue;
            }
            g["missing"] = json!(true);
        }
        self.save();
        added
    }

    /// Owned games from a store account. Returns how many new games appeared.
    pub fn merge_owned(&mut self, source: &str, list: &[Value]) -> usize {
        let ignored = self.ignored();
        let mut ids = HashSet::new();
        let mut added = 0;
        for o in list {
            let id = s(o, "id");
            if ignored.contains(&id) {
                continue;
            }
            ids.insert(id.clone());
            let acc = pick(o, OWNED_FIELDS);
            match self.games.get_mut(&id) {
                Some(g) => {
                    for (k, v) in acc {
                        g[k] = v;
                    }
                    g["owned"] = json!(true);
                    for k in ["importedPlaytime", "importedLastPlayed"] {
                        if o[k].as_i64().unwrap_or(0) > g[k].as_i64().unwrap_or(0) {
                            g[k] = o[k].clone();
                        }
                    }
                    for k in ["steamAppId", "developer"] {
                        if s(g, k).is_empty() && !s(o, k).is_empty() {
                            g[k] = o[k].clone();
                        }
                    }
                }
                None => {
                    let mut ng = new_game(&json!({
                        "id": id, "source": source, "name": o["name"], "steamAppId": o.get("steamAppId").cloned().unwrap_or(Value::Null),
                        "importedPlaytime": o["importedPlaytime"].as_i64().unwrap_or(0), "importedLastPlayed": o["importedLastPlayed"].as_i64().unwrap_or(0)
                    }));
                    for (k, v) in acc {
                        ng[k] = v;
                    }
                    ng["owned"] = json!(true);
                    ng["installed"] = json!(false);
                    ng["metaTried"] = json!(true);
                    ng["developer"] = json!(s(o, "developer"));
                    for k in ["genres", "tags"] {
                        if o.get(k).is_some() {
                            ng[k] = o[k].clone();
                        }
                    }
                    self.games.insert(id, ng);
                    added += 1;
                }
            }
        }
        let gone: Vec<String> = self
            .games
            .values()
            .filter(|g| s(g, "source") == source && g["owned"] == json!(true) && !ids.contains(&s(g, "id")))
            .map(|g| s(g, "id"))
            .collect();
        for id in gone {
            if self.games[&id]["installed"] == json!(false) {
                self.games.remove(&id);
            } else {
                self.games[&id]["owned"] = json!(false);
            }
        }
        self.save();
        added
    }

    pub fn remove_owned(&mut self, source: &str) {
        let ids: Vec<String> = self.games.values().filter(|g| s(g, "source") == source && g["owned"] == json!(true)).map(|g| s(g, "id")).collect();
        for id in ids {
            if self.games[&id]["installed"] == json!(false) {
                self.games.remove(&id);
            } else if let Some(g) = self.games.get_mut(&id).and_then(|g| g.as_object_mut()) {
                g.insert("owned".into(), json!(false));
                for k in OWNED_FIELDS {
                    g.remove(*k);
                }
            }
        }
        self.save();
    }

    /// Directories of manual games — so the folder scanner doesn't add them twice
    pub fn manual_dirs(&self) -> Vec<String> {
        self.games.values().filter(|g| s(g, "source") == "manual" && !s(g, "installDir").is_empty()).map(|g| s(g, "installDir")).collect()
    }

    pub fn add_session(&mut self, id: &str, start: i64, end: i64) {
        let Some(g) = self.games.get_mut(id) else { return };
        let secs = ((end - start) as f64 / 1000.0).round().max(0.0) as i64;
        if secs < 20 {
            return; // accidental start
        }
        g["playtime"] = json!(g["playtime"].as_i64().unwrap_or(0) + secs);
        g["lastPlayed"] = json!(end);
        let mut sessions = g["sessions"].as_array().cloned().unwrap_or_default();
        sessions.push(json!({ "start": start, "end": end }));
        let n = sessions.len();
        if n > 100 {
            sessions.drain(0..n - 100);
        }
        g["sessions"] = json!(sessions);
        self.save();
    }

    pub fn create_collection(&mut self, name: &str) -> Value {
        let c = json!({ "id": format!("c{:x}", now_ms()), "name": name.chars().take(60).collect::<String>() });
        self.collections.push(c.clone());
        self.save();
        c
    }
    pub fn rename_collection(&mut self, id: &str, name: &str) {
        for c in self.collections.iter_mut() {
            if s(c, "id") == id {
                c["name"] = json!(name.chars().take(60).collect::<String>());
            }
        }
        self.save();
    }
    pub fn delete_collection(&mut self, id: &str) {
        self.collections.retain(|c| s(c, "id") != id);
        for g in self.games.values_mut() {
            if let Some(a) = g["collections"].as_array_mut() {
                a.retain(|x| x.as_str() != Some(id));
            }
        }
        self.save();
    }

    /// Settings patch: nested groups are merged one level deep, like the Electron version
    pub fn set_settings(&mut self, patch: Value) {
        if let Value::Object(p) = patch {
            for (k, v) in p {
                let cur = self.settings.get(&k).cloned().unwrap_or(Value::Null);
                let nv = match (&cur, &v) {
                    (Value::Object(_), Value::Object(_)) if k != "accounts" => merge_defaults(&cur, &v),
                    _ => v,
                };
                self.settings[k.as_str()] = nv;
            }
        }
        self.save();
    }
}

fn pick(v: &Value, keys: &[&str]) -> Vec<(String, Value)> {
    keys.iter().filter_map(|k| v.get(*k).filter(|x| !x.is_null()).map(|x| (k.to_string(), x.clone()))).collect()
}

pub fn new_game(f: &Value) -> Value {
    let mut g = json!({
        "id": f["id"], "customName": "", "favorite": false, "hidden": false, "collections": [], "playtime": 0, "lastPlayed": 0,
        "lastLaunched": 0, "sessions": [], "addedAt": now_ms(), "coverPath": "", "heroPath": "", "iconPath": "", "description": "",
        "developer": "", "releaseDate": "", "genres": [], "metaTried": false, "missing": false
    });
    for (k, v) in pick(f, SCAN_FIELDS) {
        g[k] = v;
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;
    fn st() -> (tempfile::TempDir, Store) {
        let d = tempfile::tempdir().unwrap();
        let s = Store::new(d.path().to_path_buf());
        (d, s)
    }
    #[test]
    fn owned_and_scan_flow() {
        let (_d, mut st) = st();
        let inst = json!({ "id": "steam:570", "source": "steam", "name": "Dota 2", "installDir": "C:/S/dota", "launch": { "type": "uri", "uri": "steam://rungameid/570" } });
        st.merge_scan(vec![inst.clone()], json!({ "steam": { "count": 1 } }));
        let added = st.merge_owned("steam", &[
            json!({ "id": "steam:570", "name": "Dota 2", "installUri": "steam://install/570", "importedPlaytime": 99999 }),
            json!({ "id": "steam:730", "name": "Counter-Strike 2", "installUri": "steam://install/730", "importedPlaytime": 3600 }),
        ]);
        assert_eq!(added, 1);
        assert_eq!(st.games["steam:570"]["owned"], json!(true));
        assert_eq!(st.games["steam:570"]["importedPlaytime"], json!(99999));
        assert_eq!(st.games["steam:730"]["installed"], json!(false));
        // installed later
        let cs = json!({ "id": "steam:730", "source": "steam", "name": "CS2", "installDir": "C:/S/cs2", "launch": { "type": "uri", "uri": "steam://rungameid/730" } });
        st.merge_scan(vec![inst.clone(), cs], json!({ "steam": { "count": 2 } }));
        assert_eq!(st.games["steam:730"]["installed"], json!(true));
        // uninstalled again
        st.merge_scan(vec![inst.clone()], json!({ "steam": { "count": 1 } }));
        assert_eq!(st.games["steam:730"]["installed"], json!(false));
        assert_eq!(st.games["steam:730"]["missing"], json!(false));
        // logout
        st.remove_owned("steam");
        assert!(st.games.get("steam:730").is_none());
        assert_eq!(st.games["steam:570"]["owned"], json!(false));
        // removed by user stays removed
        st.merge_owned("steam", &[json!({ "id": "steam:440", "name": "TF2" })]);
        st.remove("steam:440");
        st.merge_owned("steam", &[json!({ "id": "steam:440", "name": "TF2" })]);
        assert!(st.games.get("steam:440").is_none());
    }
    #[test]
    fn settings_merge_and_persist() {
        let (d, mut st) = st();
        st.set_settings(json!({ "vpn": { "mode": "tun" }, "cardSize": 220 }));
        assert_eq!(st.settings["vpn"]["mode"], "tun");
        assert_eq!(st.settings["vpn"]["port"], 2080);
        st.flush();
        let st2 = Store::new(d.path().to_path_buf());
        assert_eq!(st2.settings["cardSize"], 220);
        assert_eq!(st2.settings["browser"]["economy"], true);
    }
}
