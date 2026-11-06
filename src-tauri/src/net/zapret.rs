//! zapret (winws.exe): finds the zapret folder, lists strategy .bat files,
//! runs the chosen one hidden with admin rights and reports whether it runs.
use crate::util::common::{exists, is_dir, join, parent};
use crate::util::winsys::{process_names, ps_quote, run_elevated};
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

static NOT_STRATEGY: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^(service|cleanup|check|update|install|uninstall|test|discord_cache|ipset|game_filter|remove|status)").unwrap());

pub fn find_winws(dir: &str) -> Option<String> {
    ["bin/winws.exe", "winws.exe", "zapret-winws/winws.exe"]
        .iter()
        .map(|p| join(dir, &p.replace('/', std::path::MAIN_SEPARATOR_STR)))
        .find(|p| exists(p))
}

fn read_text(p: &Path) -> String {
    fs::read(p).map(|b| String::from_utf8_lossy(&b).to_string()).unwrap_or_default()
}

pub fn looks_like_zapret(dir: &str) -> bool {
    if !is_dir(dir) {
        return false;
    }
    if find_winws(dir).is_some() {
        return true;
    }
    fs::read_dir(dir)
        .map(|rd| rd.flatten().any(|e| e.file_name().to_string_lossy().to_lowercase().ends_with(".bat") && read_text(&e.path()).to_lowercase().contains("winws")))
        .unwrap_or(false)
}

/// Looks for zapret folders on the Desktop, in Downloads, Documents and disk roots
pub fn detect_dirs() -> Vec<String> {
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default();
    let mut roots: Vec<String> = ["", "Desktop", "Downloads", "Documents", "OneDrive/Desktop", "OneDrive/Рабочий стол"]
        .iter()
        .map(|r| if r.is_empty() { home.clone() } else { join(&home, &r.replace('/', std::path::MAIN_SEPARATOR_STR)) })
        .collect();
    for d in "CDEFG".chars() {
        roots.push(format!("{}:\\", d));
        roots.push(format!("{}:\\Program Files", d));
    }
    static ZN: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)zapret|unlocker|discord.?youtube|winws").unwrap());
    static PN: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)zapret|unlocker|bypass|обход").unwrap());
    let mut found: Vec<String> = vec![];
    fn visit(dir: &str, depth: u32, found: &mut Vec<String>) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            let full = e.path().to_string_lossy().to_string();
            if ZN.is_match(&name) && looks_like_zapret(&full) {
                if !found.contains(&full) {
                    found.push(full);
                }
            } else if depth < 1 && PN.is_match(&name) {
                visit(&full, depth + 1, found);
            }
        }
    }
    for r in roots {
        if is_dir(&r) {
            visit(&r, 0, &mut found);
        }
    }
    found
}

pub fn list_strategies(dir: &str) -> Vec<String> {
    let Ok(rd) = fs::read_dir(dir) else { return vec![] };
    let mut out: Vec<String> = rd
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            let l = n.to_lowercase();
            ((l.ends_with(".bat") || l.ends_with(".cmd")) && !NOT_STRATEGY.is_match(&n) && read_text(&e.path()).to_lowercase().contains("winws")).then_some(n)
        })
        .collect();
    let rank = |f: &str| if f.eq_ignore_ascii_case("general.bat") { 0 } else if f.to_lowercase().starts_with("general") { 1 } else { 2 };
    out.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| natural(a, b)));
    out
}

fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase().cmp(&b.to_lowercase())
}

/// A strategy .bat -> winws.exe + arguments, so it can run hidden without a console window
pub fn parse_bat(file: &str, game_filter: bool) -> Result<(String, String, String), String> {
    let dir = parent(file);
    let name = Path::new(file).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let raw = read_text(Path::new(file)).trim_start_matches('\u{feff}').replace('\r', "");
    let mut joined = vec![];
    let mut buf = String::new();
    static CONT: Lazy<Regex> = Lazy::new(|| Regex::new(r"\^\s*$").unwrap());
    for l in raw.split('\n') {
        if CONT.is_match(l) {
            buf.push_str(&CONT.replace(l, " "));
            continue;
        }
        joined.push(format!("{}{}", buf, l));
        buf.clear();
    }
    let mut vars: HashMap<String, String> = HashMap::new();
    let gf = if game_filter { "1024-65535" } else { "12" };
    static VAR: Lazy<Regex> = Lazy::new(|| Regex::new(r"%([A-Za-z0-9_]+)%").unwrap());
    let sep = std::path::MAIN_SEPARATOR_STR;
    let expand = |s: &str, vars: &HashMap<String, String>| -> String {
        let s = s.replace("%~dp0", &format!("{}{}", dir, sep)).replace("%~DP0", &format!("{}{}", dir, sep)).replace("%~n0", &name);
        VAR.replace_all(&s, |c: &regex::Captures| {
            let k = &c[1];
            if let Some((_, v)) = vars.iter().find(|(x, _)| x.eq_ignore_ascii_case(k)) {
                return v.clone();
            }
            if k.to_lowercase().starts_with("gamefilter") {
                return gf.to_string();
            }
            std::env::var(k).unwrap_or_else(|_| c[0].to_string())
        })
        .to_string()
    };
    static SET: Lazy<Regex> = Lazy::new(|| Regex::new(r#"(?i)^set\s+"?([A-Za-z0-9_]+)=(.*?)"?\s*$"#).unwrap());
    static RUN: Lazy<Regex> = Lazy::new(|| Regex::new(r#"(?i)("?)([^"\s]*winws\.exe)"?\s+(.*)$"#).unwrap());
    static SKIP: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)tasklist|taskkill|find(str)?\s").unwrap());
    let (mut exe, mut args) = (None, String::new());
    for line in &joined {
        let t = line.trim();
        if t.is_empty() || t.to_lowercase().starts_with("rem ") || t.starts_with("::") {
            continue;
        }
        if let Some(c) = SET.captures(t) {
            let v = expand(&c[2], &vars);
            vars.insert(c[1].to_string(), v);
            continue;
        }
        if let Some(c) = RUN.captures(t) {
            if SKIP.is_match(t) {
                continue;
            }
            exe = Some(expand(&c[2], &vars));
            args = expand(&c[3], &vars).trim().to_string();
            break;
        }
    }
    let mut exe = exe.ok_or("В стратегии не найдена команда winws.exe")?;
    if !Path::new(&exe).is_absolute() {
        exe = join(&dir, &exe);
    }
    if !exists(&exe) {
        if let Some(w) = find_winws(&dir) {
            exe = w;
        }
    }
    let left: Vec<String> = VAR.find_iter(&args).map(|m| m.as_str().to_string()).collect();
    if !left.is_empty() {
        return Err(format!("Не удалось подставить переменные: {}", left.join(", ")));
    }
    let cwd = parent(&exe);
    Ok((exe, args, cwd))
}

/// Strategies reference *-user.txt lists that service.bat normally creates — make sure they exist
fn ensure_lists(args: &str) {
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r#"--(hostlist|hostlist-exclude|ipset|ipset-exclude)="([^"]+)""#).unwrap());
    for c in RE.captures_iter(args) {
        let f = &c[2];
        let base = Path::new(f).file_name().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
        if !exists(f) && base.contains("user") && is_dir(&parent(f)) {
            let _ = fs::write(f, if c[1].contains("ipset") { "203.0.113.113/32\n" } else { "domain.example.abc\n" });
        }
    }
}

#[derive(Default)]
pub struct Zapret {
    pub running: bool,
    pub busy: bool,
    pub error: String,
}

pub fn state(cfg: &Value, z: &Zapret) -> Value {
    let dir = cfg["dir"].as_str().unwrap_or("").to_string();
    let strategies = if dir.is_empty() { vec![] } else { list_strategies(&dir) };
    let cur = cfg["strategy"].as_str().unwrap_or("").to_string();
    let strategy = if strategies.contains(&cur) { cur } else { strategies.first().cloned().unwrap_or_default() };
    json!({
        "dir": dir, "valid": !dir.is_empty() && looks_like_zapret(&dir), "strategies": strategies, "strategy": strategy,
        "running": z.running, "runningStrategy": if z.running { cfg["lastStarted"].clone() } else { json!("") },
        "busy": z.busy, "error": z.error, "gameFilter": cfg["gameFilter"] == true, "autostart": cfg["autostart"] == true,
        "stopOnExit": cfg["stopOnExit"] == true,
    })
}

pub fn is_running() -> bool {
    process_names().contains("winws.exe")
}

pub fn wait_for(want: bool, ms: u64) -> bool {
    let until = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    while std::time::Instant::now() < until {
        if is_running() == want {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(600));
    }
    false
}

/// Blocking: starts a strategy with admin rights
pub fn start(dir: &str, strategy: &str, game_filter: bool) -> Result<(), String> {
    let file = join(dir, strategy);
    let script = match parse_bat(&file, game_filter) {
        Ok((exe, args, cwd)) => {
            ensure_lists(&args);
            format!(
                "Stop-Process -Name winws -Force\nStart-Sleep -Milliseconds 500\nStart-Process -FilePath {} -ArgumentList {} -WorkingDirectory {} -WindowStyle Hidden",
                ps_quote(&exe), ps_quote(&args), ps_quote(&cwd)
            )
        }
        // unusual .bat — run it as is (its console window is minimized)
        Err(_) => format!(
            "Stop-Process -Name winws -Force\nStart-Sleep -Milliseconds 500\nStart-Process -FilePath 'cmd.exe' -ArgumentList @('/c', {}) -WorkingDirectory {} -WindowStyle Minimized",
            ps_quote(&format!("\"{}\"", file)), ps_quote(dir)
        ),
    };
    run_elevated(&script, false)?;
    if !wait_for(true, 9000) {
        return Err("winws.exe не запустился. Попробуйте другую стратегию или добавьте папку zapret в исключения антивируса.".into());
    }
    Ok(())
}

pub fn stop() -> Result<(), String> {
    run_elevated("Stop-Process -Name winws -Force\nforeach($s in 'WinDivert','WinDivert14'){ sc.exe stop $s | Out-Null; sc.exe delete $s | Out-Null }", true)?;
    wait_for(false, 4000);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strategies_and_bat() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        fs::create_dir_all(d.join("bin")).unwrap();
        fs::create_dir_all(d.join("lists")).unwrap();
        fs::write(d.join("bin").join("winws.exe"), "x").unwrap();
        fs::write(
            d.join("general (ALT).bat"),
            [
                "@echo off", "chcp 65001 > nul", "cd /d \"%~dp0\"", "call service.bat status_zapret", "echo:", "set \"BIN=%~dp0bin\\\"", "set \"LISTS=%~dp0lists\\\"", "",
                "start \"zapret: %~n0\" /min \"%BIN%winws.exe\" --wf-tcp=80,443,%GameFilter% --wf-udp=443,%GameFilter% ^",
                "--filter-udp=443 --hostlist=\"%LISTS%list-general-user.txt\" --dpi-desync=fake ^", "--new --filter-tcp=443 --dpi-desync=multisplit",
            ]
            .join("\r\n"),
        )
        .unwrap();
        fs::write(d.join("general.bat"), "start \"\" /min \"%~dp0bin\\winws.exe\" --wf-tcp=443").unwrap();
        fs::write(d.join("service.bat"), "tasklist | find \"winws.exe\"").unwrap();
        fs::write(d.join("readme.bat"), "echo hi").unwrap();
        let ds = d.to_string_lossy().to_string();
        assert_eq!(list_strategies(&ds), vec!["general.bat", "general (ALT).bat"]);
        let (exe, args, _) = parse_bat(&join(&ds, "general (ALT).bat"), true).unwrap();
        assert!(exe.ends_with("winws.exe") && exists(&exe));
        assert!(args.starts_with("--wf-tcp=80,443,1024-65535 --wf-udp=443,1024-65535"), "{}", args);
        assert!(args.contains("list-general-user.txt\""));
        assert!(args.ends_with("--new --filter-tcp=443 --dpi-desync=multisplit"));
        assert!(!args.contains('%'));
        assert!(parse_bat(&join(&ds, "general (ALT).bat"), false).unwrap().1.starts_with("--wf-tcp=80,443,12 "));
    }
}
