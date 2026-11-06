//! Steam download / install progress, read from steamapps/appmanifest_*.acf.
//! Only files whose modification time changed are re-read.
use crate::scanners::steam::{library_folders, steam_path};
use crate::util::common::{join, now_ms};
use crate::util::vdf::{ci, ci_str, parse_vdf};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::fs;
use std::time::SystemTime;

// EAppState bits
const FULLY_INSTALLED: i64 = 4;
const UPDATE_RUNNING: i64 = 256;
const UPDATE_PAUSED: i64 = 512;
const UPDATE_STARTED: i64 = 1024;
const UNINSTALLING: i64 = 2048;
const VALIDATING: i64 = 131072;
const ADDING_FILES: i64 = 262144;
const PREALLOCATING: i64 = 524288;
const DOWNLOADING: i64 = 1048576;
const STAGING: i64 = 2097152;
const COMMITTING: i64 = 4194304;

/// manifest fields -> progress, or None when nothing is going on
pub fn progress_from_manifest(st: &Value) -> Option<Value> {
    let flags: i64 = ci_str(st, "StateFlags").parse().unwrap_or(0);
    let num = |k: &str| -> f64 { ci_str(st, k).parse().unwrap_or(0.0) };
    let (to_dl, dl, to_stage, staged) = (num("BytesToDownload"), num("BytesDownloaded"), num("BytesToStage"), num("BytesStaged"));
    let idle = flags & FULLY_INSTALLED != 0 && flags & (UPDATE_RUNNING | UPDATE_STARTED | UPDATE_PAUSED | VALIDATING) == 0;
    if idle || flags & UNINSTALLING != 0 {
        return None;
    }
    let busy = UPDATE_RUNNING | UPDATE_STARTED | UPDATE_PAUSED | DOWNLOADING | STAGING | COMMITTING | PREALLOCATING | ADDING_FILES | VALIDATING;
    if flags & busy == 0 && !(to_dl > 0.0 && dl < to_dl) {
        return None;
    }
    let phase = if flags & VALIDATING != 0 {
        "validate"
    } else if flags & PREALLOCATING != 0 {
        "allocate"
    } else if flags & (STAGING | COMMITTING) != 0 || (to_dl > 0.0 && dl >= to_dl && to_stage > 0.0) {
        "install"
    } else {
        "download"
    };
    let (done, total) = if phase == "install" && to_stage > 0.0 { (staged, to_stage) } else { (dl, to_dl) };
    // Steam keeps 1026 (UpdateRequired|UpdateStarted) during a normal download; only 512 means paused
    let paused = flags & UPDATE_PAUSED != 0;
    let pct = if total > 0.0 { ((done / total) * 1000.0).floor().min(1000.0) / 10.0 } else { 0.0 };
    Some(json!({ "phase": phase, "paused": paused, "done": done, "total": total, "pct": pct }))
}

#[derive(Default)]
pub struct Installs {
    cache: HashMap<String, (SystemTime, String, String, Option<Value>)>, // file -> (mtime, appid, name, progress)
    pub active: Map<String, Value>,
    samples: HashMap<String, Vec<(i64, f64, String)>>,
}

impl Installs {
    fn read_all(&mut self) -> Map<String, Value> {
        let mut out = Map::new();
        let Some(steam) = steam_path() else { return out };
        for lib in library_folders(&steam) {
            let dir = join(&lib, "steamapps");
            let Ok(rd) = fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let f = e.file_name().to_string_lossy().to_lowercase();
                if !(f.starts_with("appmanifest_") && f.ends_with(".acf")) {
                    continue;
                }
                let full = e.path().to_string_lossy().to_string();
                let Ok(mtime) = e.metadata().and_then(|m| m.modified()) else { continue };
                let fresh = self.cache.get(&full).map(|c| c.0 == mtime).unwrap_or(false);
                if !fresh {
                    let mut appid: String = f.chars().filter(|c| c.is_ascii_digit()).collect();
                    let mut name = String::new();
                    let mut prog = None;
                    if let Ok(t) = fs::read_to_string(&full) {
                        let d = parse_vdf(&t);
                        if let Some(st) = ci(&d, "AppState") {
                            let a = ci_str(st, "appid");
                            if !a.is_empty() {
                                appid = a;
                            }
                            name = ci_str(st, "name");
                            prog = progress_from_manifest(st);
                        }
                    }
                    self.cache.insert(full.clone(), (mtime, appid, name, prog));
                }
                let c = &self.cache[&full];
                if let Some(p) = &c.3 {
                    let mut p = p.clone();
                    p["name"] = json!(c.2);
                    out.insert(format!("steam:{}", c.1), p);
                }
            }
        }
        out
    }

    /// Returns (changed, finished ids)
    pub fn tick(&mut self) -> (bool, Vec<String>) {
        let now = now_ms();
        let cur = self.read_all();
        let mut changed = false;
        for (id, p) in &cur {
            let phase = p["phase"].as_str().unwrap_or("").to_string();
            let done = p["done"].as_f64().unwrap_or(0.0);
            let sm = self.samples.entry(id.clone()).or_default();
            sm.retain(|x| now - x.0 < 20000 && x.2 == phase);
            if sm.last().map(|x| x.1 != done).unwrap_or(true) {
                sm.push((now, done, phase.clone()));
            }
            let mut speed = 0.0;
            if sm.len() > 1 && p["paused"] != true {
                let (a, b) = (&sm[0], &sm[sm.len() - 1]);
                if b.0 > a.0 {
                    speed = ((b.1 - a.1) / ((b.0 - a.0) as f64 / 1000.0)).max(0.0);
                }
            }
            let total = p["total"].as_f64().unwrap_or(0.0);
            let mut next = p.clone();
            next["speed"] = json!(speed.round());
            next["eta"] = json!(if speed > 0.0 && total > 0.0 { ((total - done) / speed).round() } else { 0.0 });
            let prev = self.active.get(id);
            if prev.map(|pv| pv["done"] != next["done"] || pv["phase"] != next["phase"] || pv["paused"] != next["paused"] || pv["speed"] != next["speed"]).unwrap_or(true) {
                changed = true;
            }
            self.active.insert(id.clone(), next);
        }
        let finished: Vec<String> = self.active.keys().filter(|k| !cur.contains_key(*k)).cloned().collect();
        for id in &finished {
            self.active.remove(id);
            self.samples.remove(id);
            changed = true;
        }
        (changed, finished)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn progress() {
        let p = progress_from_manifest(&json!({ "StateFlags": "1026", "BytesToDownload": "8000", "BytesDownloaded": "2000", "BytesToStage": "9000", "BytesStaged": "0" })).unwrap();
        assert_eq!((p["phase"].as_str().unwrap(), p["pct"].as_f64().unwrap(), p["paused"].as_bool().unwrap()), ("download", 25.0, false));
        let i = progress_from_manifest(&json!({ "StateFlags": (1024 | 2 | 2097152).to_string(), "BytesToDownload": "8000", "BytesDownloaded": "8000", "BytesToStage": "9000", "BytesStaged": "4500" })).unwrap();
        assert_eq!((i["phase"].as_str().unwrap(), i["pct"].as_f64().unwrap()), ("install", 50.0));
        assert!(progress_from_manifest(&json!({ "StateFlags": (2 | 512).to_string(), "BytesToDownload": "100", "BytesDownloaded": "10" })).unwrap()["paused"] == true);
        assert!(progress_from_manifest(&json!({ "StateFlags": "4", "BytesToDownload": "100", "BytesDownloaded": "100" })).is_none());
        let u = progress_from_manifest(&json!({ "StateFlags": (4 | 1024 | 1048576).to_string(), "BytesToDownload": "500", "BytesDownloaded": "100" })).unwrap();
        assert_eq!(u["pct"].as_f64().unwrap(), 20.0);
    }
}
