//! Games added by hand: .exe, .lnk, .url (e.g. steam:// links), .bat or a whole folder
use super::folders::name_for_exe;
use crate::util::common::{clean_name, file_name, is_dir, norm_path, parent, stem};
use crate::util::exefinder::find_game_exe;
use crate::util::winsys::{ps_quote, run_ps};
use serde_json::{json, Value};

fn read_url_file(p: &str) -> Option<String> {
    let t = std::fs::read_to_string(p).ok()?;
    t.lines().find_map(|l| l.trim().strip_prefix("URL=").map(|u| u.trim().to_string()))
}

/// Target, arguments and working folder of a Windows shortcut
fn read_lnk(p: &str) -> Option<(String, String, String)> {
    let out = run_ps(&format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut({}); Write-Output $s.TargetPath; Write-Output $s.Arguments; Write-Output $s.WorkingDirectory",
        ps_quote(p)
    ))
    .ok()?;
    let mut l = out.lines().map(|x| x.trim().to_string());
    let t = l.next().unwrap_or_default();
    if t.is_empty() {
        return None;
    }
    Some((t, l.next().unwrap_or_default(), l.next().unwrap_or_default()))
}

pub fn make_manual_game(file: &str) -> Result<Value, String> {
    let ext = std::path::Path::new(file).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if is_dir(file) {
        let exe = find_game_exe(file, &clean_name(&file_name(file)), 3).ok_or("В папке не найден исполняемый файл игры")?;
        return Ok(json!({
            "id": format!("manual:{}", norm_path(&exe)), "source": "manual", "name": clean_name(&file_name(file)),
            "installDir": file, "exePath": exe, "launch": { "type": "exe", "path": exe, "args": "", "cwd": parent(&exe) },
        }));
    }
    if ext == "url" {
        let uri = read_url_file(file).ok_or("Не удалось прочитать ярлык")?;
        let steam = uri.strip_prefix("steam://rungameid/").map(|x| x.chars().take_while(|c| c.is_ascii_digit()).collect::<String>());
        return Ok(json!({
            "id": format!("manual:{}", norm_path(file)), "source": "manual", "name": clean_name(&stem(file)), "installDir": "",
            "steamAppId": steam, "launch": { "type": "uri", "uri": uri },
        }));
    }
    let (mut target, mut args, mut cwd) = (file.to_string(), String::new(), String::new());
    if ext == "lnk" {
        if let Some((t, a, c)) = read_lnk(file) {
            target = t;
            args = a;
            cwd = c;
        }
    }
    let te = target.to_lowercase();
    if !(te.ends_with(".exe") || te.ends_with(".bat") || te.ends_with(".cmd") || te.ends_with(".lnk")) {
        return Err("Поддерживаются .exe, .lnk, .url, .bat и папки".into());
    }
    let (name, dir) = name_for_exe(&target);
    Ok(json!({
        "id": format!("manual:{}", norm_path(&target)), "source": "manual",
        "name": if ext == "lnk" { clean_name(&stem(file)) } else { name },
        "installDir": dir, "exePath": target,
        "launch": { "type": "exe", "path": target, "args": args, "cwd": if cwd.is_empty() { parent(&target) } else { cwd } },
    }))
}
