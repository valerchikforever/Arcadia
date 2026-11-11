//! Starting games.
//! Windows: ShellExecute handles exes that need administrator rights, shortcuts and URIs.
//! Linux: URIs go to xdg-open; native builds start directly; Windows .exe files go through Wine / Proton (umu-run);
//! .desktop files run their Exec line. MangoHud (in-game overlay) wraps what Arcadia starts itself.
use crate::util::common::{exists, parent, s};
use crate::util::winsys::{open, shell_exec};
use serde_json::Value;

/// The launch method in effect: the user's override, or what the scanner found
pub fn launch_spec(g: &Value) -> Value {
    let o = &g["launchOverride"];
    if o.is_object() && (!s(o, "path").is_empty() || !s(o, "uri").is_empty()) {
        o.clone()
    } else {
        g["launch"].clone()
    }
}

pub fn run_spec(l: &Value) -> Result<(), String> {
    match s(l, "type").as_str() {
        "uri" => open(&s(l, "uri")),
        _ => {
            let path = s(l, "path");
            if path.is_empty() || !exists(&path) {
                return Err(format!("Файл не найден: {}", if path.is_empty() { "—" } else { &path }));
            }
            let cwd = { let c = s(l, "cwd"); if !c.is_empty() && exists(&c) { c } else { parent(&path) } };
            if cfg!(windows) {
                shell_exec("open", &path, &s(l, "args"), &cwd, true, false)
            } else {
                spawn_unix(&path, &s(l, "args"), &cwd)
            }
        }
    }
}

pub fn launch_game(g: &Value) -> Result<(), String> {
    let l = launch_spec(g);
    if l.is_null() {
        return Err("Не указан способ запуска".into());
    }
    run_spec(&l)
}

fn linux_settings() -> Value {
    crate::app::try_app().map(|a| a.store.lock().unwrap().settings["linux"].clone()).unwrap_or(Value::Null)
}

/// Command line for a game on Linux (without MangoHud): (program, args, extra env)
pub fn unix_command(path: &str, args: &str, cfg: &Value) -> Result<(String, Vec<String>, Vec<(String, String)>), String> {
    use crate::util::common::parse_args;
    use crate::util::linux::which;
    let mut a = parse_args(args);
    let l = path.to_lowercase();
    let mut env = vec![];
    if l.ends_with(".desktop") {
        let t = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let exec = t.lines().map(str::trim).find_map(|x| x.strip_prefix("Exec=")).ok_or("В .desktop-файле нет строки Exec")?;
        let clean: String = exec.split_whitespace().filter(|w| !(w.len() == 2 && w.starts_with('%'))).collect::<Vec<_>>().join(" ");
        return Ok(("sh".into(), vec!["-c".into(), clean], env));
    }
    if l.ends_with(".exe") || l.ends_with(".bat") || l.ends_with(".msi") {
        let prefix = s(cfg, "winePrefix");
        if !prefix.is_empty() {
            env.push(("WINEPREFIX".into(), prefix.clone()));
        }
        let runner = s(cfg, "runner");
        let (prog, mut pre): (String, Vec<String>) = match runner.as_str() {
            "umu" | "proton" => {
                let umu = which("umu-run").ok_or("Не найден umu-run. Установите umu-launcher или выберите Wine в «Настройки → Linux»")?;
                env.push(("GAMEID".into(), "0".into()));
                if !s(cfg, "protonPath").is_empty() {
                    env.push(("PROTONPATH".into(), s(cfg, "protonPath")));
                }
                (umu, vec![])
            }
            "custom" => {
                let mut parts = parse_args(&s(cfg, "customRunner"));
                if parts.is_empty() {
                    return Err("Не задана своя команда запуска в «Настройки → Linux»".into());
                }
                let p = parts.remove(0);
                (p, parts)
            }
            _ => (which("wine").ok_or("Не найден Wine. Установите wine или выберите Proton (umu-run) в «Настройки → Linux»")?, vec![]),
        };
        pre.push(path.to_string());
        pre.append(&mut a);
        return Ok((prog, pre, env));
    }
    if l.ends_with(".sh") {
        let mut v = vec![path.to_string()];
        v.append(&mut a);
        return Ok(("sh".into(), v, env));
    }
    // native binary / AppImage: make sure it is executable (unpacked archives often lose the bit)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(m) = std::fs::metadata(path) {
            let mut p = m.permissions();
            if p.mode() & 0o100 == 0 {
                p.set_mode(p.mode() | 0o755);
                let _ = std::fs::set_permissions(path, p);
            }
        }
    }
    Ok((path.to_string(), a, env))
}

fn spawn_unix(path: &str, args: &str, cwd: &str) -> Result<(), String> {
    let cfg = linux_settings();
    let (mut prog, mut argv, env) = unix_command(path, args, &cfg)?;
    let mut cmd_env = env;
    // in-game overlay: MangoHud wraps the whole command (works for Vulkan, OpenGL, and DXVK/VKD3D under Wine)
    if let Some(m) = crate::osd::mangohud_wrap() {
        cmd_env.push(("MANGOHUD_CONFIG".into(), m.1));
        argv.insert(0, prog);
        prog = m.0;
    }
    let mut c = std::process::Command::new(&prog);
    c.args(&argv).current_dir(cwd).envs(cmd_env);
    // the game must outlive Arcadia and must not inherit its stdin/stdout
    c.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0);
    }
    c.spawn().map(|_| ()).map_err(|e| format!("Не удалось запустить {}: {}", prog, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn unix_commands() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("x.desktop");
        std::fs::write(&d, "[Desktop Entry]\nName=X\nExec=/opt/x/run --fast %U\n").unwrap();
        let (p, a, _) = unix_command(&d.to_string_lossy(), "", &json!({})).unwrap();
        assert_eq!(p, "sh");
        assert_eq!(a[1], "/opt/x/run --fast");
        let (p, a, e) = unix_command("/g/Hades.exe", "-dx11", &json!({ "runner": "custom", "customRunner": "gamescope -f -- wine", "winePrefix": "/pfx" })).unwrap();
        assert_eq!(p, "gamescope");
        assert_eq!(a, vec!["-f", "--", "wine", "/g/Hades.exe", "-dx11"]);
        assert_eq!(e[0], ("WINEPREFIX".to_string(), "/pfx".to_string()));
        let (p, a, _) = unix_command("/g/start.sh", "", &json!({})).unwrap();
        assert_eq!((p.as_str(), a[0].as_str()), ("sh", "/g/start.sh"));
    }
}
