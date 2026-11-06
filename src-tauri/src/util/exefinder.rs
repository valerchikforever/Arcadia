//! Picks the main game executable inside a folder.
use super::common::match_key;
use once_cell::sync::Lazy;
use regex::Regex;
use std::fs;
use std::path::Path;

static BAD_EXE: Lazy<Regex> = Lazy::new(|| {
    let list = [
        r"unins\d*", "uninstall.*", "setup.*", "install(er)?", "installer.*", "vc_?redist.*", "vcredist.*", "dxsetup", "dxwebsetup",
        "directx.*", "dotnet.*", r"ndp\d+.*", "oalinst", "physx.*", "crash.*", ".*crashhandler.*", ".*crashreport.*",
        "unitycrashhandler(32|64)?", "ue4prereqsetup.*", "ueprereqsetup.*", "prereq.*", "redist.*", "updater", "update",
        "autoupdate.*", "patcher", "easyanticheat.*", "eac.*", "beservice.*", "be_?setup.*", "battleye.*", "quicksfv",
        "notification_helper", "cefprocess", "cefsharp.*", "zfgamebrowser", "dowser", "touchup", "cleanup", "activation(ui)?",
        "register", "registration", "report", "bugreport.*", "errorreporter", "helper", "service", "quickbms", "cmd", "python(w)?",
        "java(w)?", "node", "7z.*", "unrar", "winrar", "unarc", "isdone", "dosbox.*", "gamelauncherhelper", "link2ea", "eadesktop",
        "origin", "gogbgsvc", "galaxyclient.*", "steam", "steamservice", "steamerrorreporter.*", "epicgameslauncher", "upc",
        "uplaywebcore", r"battle\.net", "agent", "dxdiag", "svc.*",
    ];
    Regex::new(&format!("(?i)^({})$", list.join("|"))).unwrap()
});

const SKIP_DIRS: &[&str] = &[
    "_commonredist", "commonredist", "redist", "_redist", "redistributables", "directx", "__installer", "support",
    "prerequisites", "_prerequisites", "dotnet", "vcredist", "msvc", "easyanticheat", "battleye", "eac", "installers", "tools",
    "sdk", "docs", "manual", "soundtrack", "ost", "artbook", "extras", "bonus", "crashreportclient", "thirdparty", "third_party",
    "_temp", "temp", "logs", "screenshots", "saves", "mods", "mono", "monobleedingedge", "webhelper", "cef", "node_modules",
    ".git", "shadercache", "cache", "locales", "swiftshader",
];

#[derive(Debug, Clone)]
pub struct Exe {
    pub path: String,
    pub base: String,
    pub depth: u32,
    pub size: u64,
}

pub fn is_bad_exe(base: &str) -> bool {
    BAD_EXE.is_match(base)
}

pub fn list_exes(dir: &str, max_depth: u32) -> Vec<Exe> {
    let mut out = Vec::new();
    let mut budget: i32 = 4000;
    fn walk(d: &Path, depth: u32, max: u32, budget: &mut i32, out: &mut Vec<Exe>) {
        if *budget <= 0 {
            return;
        }
        let Ok(rd) = fs::read_dir(d) else { return };
        for e in rd.flatten() {
            *budget -= 1;
            if *budget <= 0 {
                return;
            }
            let Ok(ft) = e.file_type() else { continue };
            let name = e.file_name().to_string_lossy().to_string();
            let full = e.path();
            if ft.is_dir() {
                if depth < max && !SKIP_DIRS.contains(&name.to_lowercase().as_str()) {
                    walk(&full, depth + 1, max, budget, out);
                }
            } else if ft.is_file() && name.to_lowercase().ends_with(".exe") {
                let base = name[..name.len() - 4].to_string();
                if BAD_EXE.is_match(&base) {
                    continue;
                }
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                out.push(Exe { path: full.to_string_lossy().to_string(), base, depth, size });
            }
        }
    }
    walk(Path::new(dir), 0, max_depth, &mut budget, &mut out);
    out
}

fn score(exe: &Exe, hint: &str) -> f64 {
    static ARCH: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)[-_](win)?(64|32|x64|x86|shipping|dx\d+|vulkan)\b").unwrap());
    static CFG: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)(config|settings|setup|editor|server|benchmark|tool|debug|test)").unwrap());
    let b = match_key(&ARCH.replace_all(&exe.base, " "));
    let h = match_key(hint);
    let mut s = 0.0;
    if !h.is_empty() {
        let cb = b.replace(' ', "");
        let ch = h.replace(' ', "");
        if cb == ch {
            s += 60.0;
        } else if ch.starts_with(&cb) || cb.starts_with(&ch) {
            s += 40.0;
        } else {
            let th: std::collections::HashSet<&str> = h.split(' ').collect();
            let common = b.split(' ').filter(|t| t.chars().count() > 1 && th.contains(t)).count();
            s += common as f64 * 15.0;
            let acr: String = h.split(' ').filter_map(|w| w.chars().next()).collect();
            if acr.chars().count() >= 2 && cb.starts_with(&acr) {
                s += 30.0;
            }
        }
    }
    s -= exe.depth as f64 * 12.0;
    s += ((exe.size as f64 / 65536.0).max(1.0).log2() * 3.0).min(25.0);
    let lb = exe.base.to_lowercase();
    if lb.contains("launcher") {
        s -= 6.0;
    }
    if CFG.is_match(&exe.base) {
        s -= 25.0;
    }
    if lb.contains("shipping") {
        s += 4.0;
    }
    if lb.contains("64") || lb.contains("x64") {
        s += 2.0;
    }
    s
}

/// Best guess at the main game executable inside dir
pub fn find_game_exe(dir: &str, hint: &str, max_depth: u32) -> Option<String> {
    let exes = list_exes(dir, max_depth);
    exes.iter()
        .map(|e| (score(e, hint), e))
        .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(_, e)| e.path.clone())
}
