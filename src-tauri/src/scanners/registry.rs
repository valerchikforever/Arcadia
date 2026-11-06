//! GOG, Ubisoft Connect, EA app, Battle.net and BattleState (Tarkov) — all discovered via the Windows registry.
use crate::util::common::nz;
use super::{Ctx, Uninst};
use crate::util::common::{exists, file_name, fix_path, join, parent};
use crate::util::exefinder::find_game_exe;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;

const GOG: &[&str] = &[r"HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\GOG.com\Games", r"HKEY_LOCAL_MACHINE\SOFTWARE\GOG.com\Games"];
const UBI: &[&str] = &[r"HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs", r"HKEY_LOCAL_MACHINE\SOFTWARE\Ubisoft\Launcher\Installs"];

fn subkeys<'a>(ctx: &'a Ctx, keys: &[&str]) -> Vec<&'a (String, HashMap<String, String>)> {
    keys.iter().filter_map(|k| ctx.reg.get(*k)).flat_map(|k| k.subkeys.iter()).collect()
}
fn v(m: &HashMap<String, String>, k: &str) -> String {
    m.get(k).cloned().or_else(|| m.iter().find(|(x, _)| x.eq_ignore_ascii_case(k)).map(|(_, y)| y.clone())).unwrap_or_default()
}

pub fn scan_gog(ctx: &Ctx) -> Vec<Value> {
    let times: HashMap<String, i64> = super::gog_time::read().into_iter().collect();
    let mut games = vec![];
    let mut seen = HashSet::new();
    for (name, vals) in subkeys(ctx, GOG) {
        let id = nz(v(vals, "gameID"), name.clone());
        if seen.contains(&id) || !v(vals, "dependsOn").trim().is_empty() {
            continue;
        }
        let dir = fix_path(&v(vals, "path"));
        if dir.is_empty() || !exists(&dir) {
            continue;
        }
        let gname = v(vals, "gameName");
        let mut exe = fix_path(&v(vals, "exe"));
        if !exists(&exe) {
            exe = find_game_exe(&dir, &gname, 3).unwrap_or_default();
        }
        seen.insert(id.clone());
        let wd = fix_path(&v(vals, "workingDir"));
        games.push(json!({
            "id": format!("gog:{}", id), "source": "gog", "name": if gname.is_empty() { file_name(&dir) } else { gname },
            "installDir": dir, "exePath": exe, "importedPlaytime": times.get(&id).cloned().unwrap_or(0),
            "launch": { "type": "exe", "path": exe, "args": v(vals, "launchParam"), "cwd": if wd.is_empty() { dir.clone() } else { wd } },
        }));
    }
    games
}

pub fn scan_ubisoft(ctx: &Ctx) -> Vec<Value> {
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)Uplay Install (\d+)$").unwrap());
    let mut names = HashMap::new();
    for u in &ctx.uninstall {
        if let Some(c) = RE.captures(&u.key) {
            let dn = u.get("DisplayName");
            if !dn.is_empty() {
                names.insert(c[1].to_string(), dn);
            }
        }
    }
    let mut games = vec![];
    for (id, vals) in subkeys(ctx, UBI) {
        let dir = fix_path(&v(vals, "InstallDir"));
        if dir.is_empty() || !exists(&dir) {
            continue;
        }
        let name = names.get(id).cloned().unwrap_or_else(|| file_name(&dir));
        games.push(json!({
            "id": format!("ubisoft:{}", id), "source": "ubisoft", "name": name, "installDir": dir,
            "exePath": find_game_exe(&dir, &name, 3).unwrap_or_default(),
            "launch": { "type": "uri", "uri": format!("uplay://launch/{}/0", id) },
        }));
    }
    games
}

fn ea_exe_from_installer_data(dir: &str) -> Option<String> {
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)<filePath>([^<]+)</filePath>").unwrap());
    static PFX: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\[[^\]]*\]").unwrap());
    let xml = fs::read_to_string(join(&join(dir, "__Installer"), "installerdata.xml")).ok()?;
    for c in RE.captures_iter(&xml) {
        let rel = PFX.replace(&c[1], "").trim_start_matches(['\\', '/']).to_string();
        if !rel.to_lowercase().ends_with(".exe") {
            continue;
        }
        let full = join(dir, &rel.replace('\\', std::path::MAIN_SEPARATOR_STR));
        if exists(&full) {
            return Some(full);
        }
    }
    None
}

pub fn scan_ea(ctx: &Ctx) -> Vec<Value> {
    static PUB: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)(electronic arts|^ea\b|ea swiss|ea digital illusions|respawn|criterion|bioware|maxis|codemasters|motive)").unwrap());
    static UN: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)(EAInstaller|EA Desktop|EADesktop|Origin)").unwrap());
    static SELF: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)^(ea app|ea|origin|ea desktop)$").unwrap());
    let mut games = vec![];
    let mut seen = HashSet::new();
    for u in &ctx.uninstall {
        let name = u.get("DisplayName");
        if name.is_empty() || SELF.is_match(name.trim()) {
            continue;
        }
        let dir = fix_path(&u.get("InstallLocation"));
        if dir.is_empty() || !exists(&dir) {
            continue;
        }
        let has_data = exists(&join(&join(&dir, "__Installer"), "installerdata.xml"));
        if !(has_data || (UN.is_match(&u.get("UninstallString")) && PUB.is_match(&u.get("Publisher")))) {
            continue;
        }
        if !seen.insert(dir.to_lowercase()) {
            continue;
        }
        let Some(exe) = ea_exe_from_installer_data(&dir).or_else(|| find_game_exe(&dir, &name, 3)) else { continue };
        let key = u.key.rsplit('\\').next().unwrap_or(&name).to_lowercase();
        games.push(json!({
            "id": format!("ea:{}", key), "source": "ea", "name": name, "installDir": dir, "exePath": exe,
            "launch": { "type": "exe", "path": exe, "args": "", "cwd": parent(&exe) },
        }));
    }
    games
}

/// uid (from uninstall string) -> Battle.net product code used by --exec="launch CODE"
pub fn bnet_code(uid: &str) -> Option<&'static str> {
    Some(match uid {
        "wow" => "WoW", "wow_classic" | "wow_classic_era" => "WoWC", "d3" | "diablo3" => "D3", "hs_beta" | "hearthstone" => "WTCG",
        "heroes" => "Hero", "prometheus" | "overwatch" => "Pro", "s2" => "S2", "s1" => "S1", "w3" => "W3", "osi" => "OSI",
        "fenris" => "Fen", "anbs" => "ANBS", "viper" => "VIPR", "odin" => "ODIN", "zeus" => "ZEUS", "fore" => "FORE",
        "lazarus" => "LAZR", "rtro" => "RTRO", "wlby" => "WLBY",
        _ => return None,
    })
}

fn find_bnet(ctx: &Ctx) -> Option<String> {
    for u in &ctx.uninstall {
        if u.get("DisplayName").trim().eq_ignore_ascii_case("battle.net") && !u.get("InstallLocation").is_empty() {
            for n in ["Battle.net.exe", "Battle.net Launcher.exe"] {
                let p = join(&fix_path(&u.get("InstallLocation")), n);
                if exists(&p) {
                    return Some(p);
                }
            }
        }
    }
    [r"C:\Program Files (x86)\Battle.net\Battle.net.exe", r"C:\Program Files\Battle.net\Battle.net.exe"].iter().map(|s| s.to_string()).find(|p| exists(p))
}

pub fn scan_battlenet(ctx: &Ctx) -> Vec<Value> {
    static UID: Lazy<Regex> = Lazy::new(|| Regex::new(r#"(?i)--uid=([^\s"]+)"#).unwrap());
    static LOC: Lazy<Regex> = Lazy::new(|| Regex::new(r"_[a-z]{2}[a-z]{2}$").unwrap());
    let bnet = find_bnet(ctx);
    let mut games = vec![];
    for u in &ctx.uninstall {
        let us = u.get("UninstallString");
        if !us.to_lowercase().contains("battle.net") || !us.to_lowercase().contains("--uid=") {
            continue;
        }
        let name = u.get("DisplayName");
        if name.is_empty() || name.trim().eq_ignore_ascii_case("battle.net") {
            continue;
        }
        let dir = fix_path(&u.get("InstallLocation"));
        if dir.is_empty() || !exists(&dir) {
            continue;
        }
        let uid = UID.captures(&us).map(|c| c[1].to_lowercase()).unwrap_or_default();
        let uid = LOC.replace(&uid, "").to_string();
        let exe = find_game_exe(&dir, &name, 3).unwrap_or_default();
        let launch = match (&bnet, bnet_code(&uid)) {
            (Some(b), Some(code)) => json!({ "type": "exe", "path": b, "args": format!("--exec=\"launch {}\"", code), "cwd": parent(b) }),
            _ if !exe.is_empty() => json!({ "type": "exe", "path": exe, "args": "", "cwd": parent(&exe) }),
            _ => continue,
        };
        let id = if uid.is_empty() { name.to_lowercase() } else { uid };
        games.push(json!({ "id": format!("battlenet:{}", id), "source": "battlenet", "name": name, "installDir": dir, "exePath": exe, "launch": launch }));
    }
    games
}

pub fn scan_battlestate(ctx: &Ctx) -> Vec<Value> {
    let bsg: Vec<&Uninst> = ctx
        .uninstall
        .iter()
        .filter(|u| u.get("Publisher").to_lowercase().contains("battlestate") || u.get("DisplayName").to_lowercase().contains("bsglauncher"))
        .collect();
    let mut launcher = None;
    for u in &bsg {
        if u.get("DisplayName").to_lowercase().contains("launcher") && !u.get("InstallLocation").is_empty() {
            let p = join(&fix_path(&u.get("InstallLocation")), "BsgLauncher.exe");
            if exists(&p) {
                launcher = Some(p);
            }
        }
    }
    if launcher.is_none() {
        launcher = [r"C:\Battlestate Games\BsgLauncher\BsgLauncher.exe", r"C:\Program Files\Battlestate Games\BsgLauncher\BsgLauncher.exe"]
            .iter()
            .map(|s| s.to_string())
            .find(|p| exists(p));
    }
    let mut games = vec![];
    for u in &bsg {
        let name = u.get("DisplayName");
        if name.is_empty() || name.to_lowercase().contains("launcher") {
            continue;
        }
        let dir = fix_path(&u.get("InstallLocation"));
        if dir.is_empty() || !exists(&dir) {
            continue;
        }
        let exe = find_game_exe(&dir, &name, 3).unwrap_or_default();
        // Tarkov cannot start without its launcher (auth): open BsgLauncher, track the game folder
        let launch = match &launcher {
            Some(l) => json!({ "type": "exe", "path": l, "args": "", "cwd": parent(l) }),
            None if !exe.is_empty() => json!({ "type": "exe", "path": exe, "args": "", "cwd": dir }),
            None => continue,
        };
        let id = name.to_lowercase().split_whitespace().collect::<Vec<_>>().join("-");
        games.push(json!({ "id": format!("battlestate:{}", id), "source": "battlestate", "name": name, "installDir": dir, "exePath": exe, "trackDir": dir, "launch": launch }));
    }
    games
}
