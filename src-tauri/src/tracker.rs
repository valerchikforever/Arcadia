//! Play time: a game is "running" while any process lives inside its folder.
//! Works for launcher-started games too (Steam/Epic/Battle.net spawn the game themselves).
use crate::util::common::{norm_path, now_ms, parent, s};
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;

static GENERIC: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)/(program files( \(x86\))?|programdata|windows|users|games|steamapps/common|steamapps|steam|epic games|gog games|ubisoft|origin games|ea games)$").unwrap()
});

pub fn track_dir_of(g: &Value) -> Option<String> {
    let mut d = s(g, "trackDir");
    if d.is_empty() {
        d = s(g, "installDir");
    }
    if d.is_empty() && g["launch"]["type"] == "exe" {
        d = parent(&s(&g["launch"], "path"));
    }
    let n = norm_path(&d);
    let drive_only = n.len() == 2 && n.ends_with(':');
    if n.is_empty() || drive_only || n.split('/').filter(|x| !x.is_empty()).count() < 2 || GENERIC.is_match(&n) {
        return None;
    }
    Some(n)
}

#[derive(Default)]
pub struct Tracker {
    pub pending: HashMap<String, i64>, // id -> deadline
    pub active: HashMap<String, i64>,  // id -> start
}

pub struct TickResult {
    pub started: Vec<String>,
    pub ended: Vec<(String, i64, i64)>,
}

impl Tracker {
    pub fn expect(&mut self, g: &Value) {
        if track_dir_of(g).is_some() {
            self.pending.insert(s(g, "id"), now_ms() + 5 * 60 * 1000);
        }
    }

    pub fn needs_scan(&self, watch_all: bool) -> bool {
        watch_all || !self.pending.is_empty() || !self.active.is_empty()
    }

    pub fn running_ids(&self) -> Vec<String> {
        self.active.keys().cloned().collect()
    }

    /// procs: normalised process paths
    pub fn tick(&mut self, games: &serde_json::Map<String, Value>, procs: &[String], watch_all: bool) -> TickResult {
        let now = now_ms();
        self.pending.retain(|_, dl| *dl > now);
        let mut res = TickResult { started: vec![], ended: vec![] };
        for g in games.values() {
            if g["missing"] == true || g["installed"] == false {
                continue;
            }
            let id = s(g, "id");
            if !(watch_all || self.pending.contains_key(&id) || self.active.contains_key(&id)) {
                continue;
            }
            let Some(dir) = track_dir_of(g) else { continue };
            let pre = format!("{}/", dir);
            let running = procs.iter().any(|p| p.starts_with(&pre));
            match (running, self.active.get(&id).cloned()) {
                (true, None) => {
                    self.active.insert(id.clone(), now);
                    self.pending.remove(&id);
                    res.started.push(id);
                }
                (false, Some(start)) => {
                    self.active.remove(&id);
                    res.ended.push((id, start, now));
                }
                _ => {}
            }
        }
        res
    }
}
