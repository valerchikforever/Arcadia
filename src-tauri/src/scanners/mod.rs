pub mod epic;
pub mod folders;
pub mod gog_time;
pub mod linux;
pub mod manual;
pub mod registry;
pub mod steam;

use crate::util::winsys::{read_registry, RegSnapshot};
use serde_json::{json, Map, Value};
use std::collections::HashSet;

pub const REG_KEYS: &[&str] = &[
    r"HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
    r"HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    r"HKEY_CURRENT_USER\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
    r"HKEY_CURRENT_USER\SOFTWARE\Valve\Steam",
    r"HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Valve\Steam",
    r"HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\GOG.com\Games",
    r"HKEY_LOCAL_MACHINE\SOFTWARE\GOG.com\Games",
    r"HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs",
    r"HKEY_LOCAL_MACHINE\SOFTWARE\Ubisoft\Launcher\Installs",
];

/// One "Uninstall" registry entry
pub struct Uninst {
    pub key: String,
    pub values: std::collections::HashMap<String, String>,
}
impl Uninst {
    pub fn get(&self, k: &str) -> String {
        self.values.get(k).cloned().unwrap_or_default()
    }
}

#[derive(Default)]
pub struct Ctx {
    pub reg: RegSnapshot,
    pub uninstall: Vec<Uninst>,
    pub program_data: String,
    pub steam_path: Option<String>,
    pub epic_manifest_dir: Option<String>,
}

impl Ctx {
    pub fn from_registry(reg: RegSnapshot) -> Ctx {
        let mut uninstall = vec![];
        for k in &REG_KEYS[..3] {
            if let Some(e) = reg.get(*k) {
                for (name, vals) in &e.subkeys {
                    uninstall.push(Uninst { key: format!("{}\\{}", k, name), values: vals.clone() });
                }
            }
        }
        Ctx { reg, uninstall, program_data: std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into()), ..Default::default() }
    }
}

/// Scans every enabled launcher plus watched folders.
pub fn scan_all(settings: &Value, extra_claimed: &[String], ctx: Option<Ctx>) -> (Vec<Value>, Value) {
    let ctx = ctx.unwrap_or_else(|| Ctx::from_registry(read_registry(REG_KEYS)));
    let sources = &settings["sources"];
    let mut games: Vec<Value> = vec![];
    let mut status = Map::new();
    let mut ids = HashSet::new();
    type ScanFn = fn(&Ctx) -> Vec<Value>;
    let list: [(&str, ScanFn); 9] = [
        ("steam", steam::scan),
        ("epic", epic::scan),
        ("gog", registry::scan_gog),
        ("ea", registry::scan_ea),
        ("ubisoft", registry::scan_ubisoft),
        ("battlenet", registry::scan_battlenet),
        ("battlestate", registry::scan_battlestate),
        ("heroic", linux::scan_heroic),
        ("lutris", linux::scan_lutris),
    ];
    for (key, f) in list {
        if sources[key] == json!(false) {
            status.insert(key.into(), json!({ "enabled": false }));
            continue;
        }
        let found: Vec<Value> = f(&ctx).into_iter().filter(|g| ids.insert(g["id"].as_str().unwrap_or("").to_string())).collect();
        status.insert(key.into(), json!({ "count": found.len() }));
        games.extend(found);
    }
    if sources["folder"] != json!(false) {
        let mut claimed: Vec<String> = games.iter().filter_map(|g| g["installDir"].as_str().map(String::from)).filter(|s| !s.is_empty()).collect();
        claimed.extend(extra_claimed.iter().cloned());
        let folders: Vec<String> = settings["scanFolders"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        let found = folders::scan(&folders, &claimed);
        status.insert("folder".into(), json!({ "count": found.len() }));
        games.extend(found);
    }
    (games, Value::Object(status))
}

#[cfg(test)]
mod tests;
