//! Linux helpers: home/XDG folders, finding programs, Wine path mapping, pkexec.
//! Compiled everywhere (pure path logic is unit-tested on any system); the process helpers only make sense on Linux.
use std::path::Path;
use std::process::Command;

pub fn home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/root".into())
}

pub fn config_home() -> String {
    std::env::var("XDG_CONFIG_HOME").ok().filter(|x| !x.is_empty()).unwrap_or_else(|| format!("{}/.config", home()))
}

pub fn data_home() -> String {
    std::env::var("XDG_DATA_HOME").ok().filter(|x| !x.is_empty()).unwrap_or_else(|| format!("{}/.local/share", home()))
}

/// Full path of a program on PATH
pub fn which(prog: &str) -> Option<String> {
    if prog.contains('/') {
        return Path::new(prog).exists().then(|| prog.to_string());
    }
    let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".into());
    path.split(':').map(|d| format!("{}/{}", d, prog)).find(|p| Path::new(p).is_file())
}

/// Runs a command, returns trimmed stdout when it succeeded
pub fn output(prog: &str, args: &[&str]) -> Option<String> {
    let o = Command::new(prog).args(args).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Runs a shell command as root through polkit (a password dialog). Blocks until it finishes.
pub fn pkexec(shell: &str) -> Result<(), String> {
    if which("pkexec").is_none() {
        return Err("Не найден pkexec (polkit) — выполните команду вручную через sudo".into());
    }
    let o = Command::new("pkexec").args(["sh", "-c", shell]).output().map_err(|e| e.to_string())?;
    match o.status.code() {
        Some(0) => Ok(()),
        Some(126) | Some(127) => Err("Запрос прав администратора отклонён".into()),
        _ => Err(String::from_utf8_lossy(&o.stderr).trim().chars().take(300).collect::<String>().replace('\n', " ")),
    }
}

pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// A Windows path seen inside a Wine process ("Z:\home\u\Games\x.exe", "C:\Games\x.exe") -> Linux path.
/// prefix: the process's WINEPREFIX (or Proton's …/pfx).
pub fn wine_to_unix(p: &str, prefix: Option<&str>) -> Option<String> {
    let b = p.as_bytes();
    if b.len() < 3 || !b[0].is_ascii_alphabetic() || b[1] != b':' || !(b[2] == b'\\' || b[2] == b'/') {
        return None;
    }
    let rest = p[3..].replace('\\', "/");
    let drive = (b[0] as char).to_ascii_lowercase();
    if drive == 'z' {
        return Some(format!("/{}", rest));
    }
    let pre = prefix.map(String::from).unwrap_or_else(|| format!("{}/.wine", home()));
    // dosdevices/<x>: is a symlink to the drive's real folder; C: is normally drive_c
    let link = format!("{}/dosdevices/{}:", pre.trim_end_matches('/'), drive);
    if let Ok(t) = std::fs::read_link(&link) {
        let base = if t.is_absolute() { t } else { Path::new(&format!("{}/dosdevices", pre)).join(t) };
        return Some(format!("{}/{}", base.to_string_lossy().trim_end_matches('/'), rest));
    }
    (drive == 'c').then(|| format!("{}/drive_c/{}", pre.trim_end_matches('/'), rest))
}

/// Desktop session for system proxy settings
pub fn desktop() -> String {
    let d = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase();
    if d.contains("kde") { "kde".into() } else if d.is_empty() { String::new() } else { "gnome".into() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wine_paths() {
        assert_eq!(wine_to_unix(r"Z:\home\u\Games\Hades\Hades.exe", None).unwrap(), "/home/u/Games/Hades/Hades.exe");
        assert_eq!(wine_to_unix(r"C:\Games\X\x.exe", Some("/p/pfx")).unwrap(), "/p/pfx/drive_c/Games/X/x.exe");
        assert!(wine_to_unix("/usr/bin/wine", None).is_none());
        assert!(wine_to_unix("wine64-preloader", None).is_none());
    }
}
