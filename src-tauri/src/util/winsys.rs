//! Windows system access: registry, processes, PowerShell, elevation, ShellExecute.
//! On other systems these are harmless stubs so the rest of the code can be tested anywhere.
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Default, Serialize)]
pub struct RegKey {
    pub values: HashMap<String, String>,
    pub subkeys: Vec<(String, HashMap<String, String>)>,
}

pub type RegSnapshot = HashMap<String, RegKey>;

/// Command that never flashes a console window
pub fn quiet(cmd: &str) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(cmd);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c
}

#[cfg(windows)]
pub fn read_registry(keys: &[&str]) -> RegSnapshot {
    use winreg::enums::*;
    use winreg::RegKey as WK;
    let mut out = RegSnapshot::new();
    let vals = |k: &WK| -> HashMap<String, String> {
        let mut m = HashMap::new();
        for (name, v) in k.enum_values().flatten() {
            let s = match v.vtype {
                REG_SZ | REG_EXPAND_SZ => String::from_utf16_lossy(
                    &v.bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>(),
                )
                .trim_end_matches('\0')
                .to_string(),
                REG_DWORD if v.bytes.len() >= 4 => u32::from_le_bytes([v.bytes[0], v.bytes[1], v.bytes[2], v.bytes[3]]).to_string(),
                _ => continue,
            };
            m.insert(name, s);
        }
        m
    };
    for full in keys {
        let (hive, path) = match full.split_once('\\') {
            Some(("HKEY_LOCAL_MACHINE", p)) => (HKEY_LOCAL_MACHINE, p),
            Some(("HKEY_CURRENT_USER", p)) => (HKEY_CURRENT_USER, p),
            _ => continue,
        };
        let Ok(k) = WK::predef(hive).open_subkey_with_flags(path, KEY_READ) else { continue };
        let mut rk = RegKey { values: vals(&k), subkeys: vec![] };
        for name in k.enum_keys().flatten() {
            if let Ok(sk) = k.open_subkey_with_flags(&name, KEY_READ) {
                rk.subkeys.push((name, vals(&sk)));
            }
        }
        out.insert(full.to_string(), rk);
    }
    out
}

#[cfg(not(windows))]
pub fn read_registry(_keys: &[&str]) -> RegSnapshot {
    RegSnapshot::new()
}

/// Full paths of running process executables
pub fn process_paths() -> Vec<String> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_exe(UpdateKind::Always));
    sys.processes().values().filter_map(|p| p.exe().map(|e| e.to_string_lossy().to_string())).collect()
}

/// Lower-cased names of running processes
pub fn process_names() -> std::collections::HashSet<String> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    sys.processes().values().map(|p| p.name().to_string_lossy().to_lowercase()).collect()
}

/// Working-set memory (MB) of the given process ids
pub fn memory_mb(pids: &[u32]) -> u64 {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let mut sys = System::new();
    let list: Vec<Pid> = pids.iter().map(|p| Pid::from_u32(*p)).collect();
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&list), true, ProcessRefreshKind::nothing().with_memory());
    list.iter().filter_map(|p| sys.process(*p)).map(|p| p.memory()).sum::<u64>() / (1024 * 1024)
}

static PS_N: AtomicU64 = AtomicU64::new(0);

/// Writes a PowerShell script to a temp .ps1 (UTF-8 with BOM — Windows PowerShell 5.1 needs it for Cyrillic).
/// Scripts never go on the command line: long ones would hit the 32 767-character limit.
pub fn temp_script(script: &str, prefix: &str) -> std::io::Result<PathBuf> {
    let n = PS_N.fetch_add(1, Ordering::SeqCst);
    let f = std::env::temp_dir().join(format!("{}-{}-{}-{}.ps1", prefix, std::process::id(), super::common::now_ms(), n));
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(script.as_bytes());
    std::fs::write(&f, bytes)?;
    Ok(f)
}

pub fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Runs a PowerShell script and returns stdout
pub fn run_ps(script: &str) -> Result<String, String> {
    if !cfg!(windows) {
        return Ok(String::new());
    }
    let full = format!("[Console]::OutputEncoding=[Text.Encoding]::UTF8\n$ProgressPreference='SilentlyContinue'\n{}", script);
    let f = temp_script(&full, "arcadia").map_err(|e| e.to_string())?;
    let out = quiet("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&f)
        .output();
    let _ = std::fs::remove_file(&f);
    let out = out.map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&out.stdout).trim_start_matches('\u{feff}').to_string())
}

/// ShellExecuteEx wrapper. verb "open" handles .lnk/.url/.bat, URIs and exes that need UAC.
#[cfg(windows)]
pub fn shell_exec(verb: &str, file: &str, params: &str, dir: &str, show: bool, wait: bool) -> Result<(), String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::{CloseHandle, GetLastError};
    use windows::Win32::System::Threading::{WaitForSingleObject, INFINITE};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWNORMAL};
    let v = HSTRING::from(verb);
    let f = HSTRING::from(file);
    let p = HSTRING::from(params);
    let d = HSTRING::from(dir);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: PCWSTR(v.as_ptr()),
        lpFile: PCWSTR(f.as_ptr()),
        lpParameters: if params.is_empty() { PCWSTR::null() } else { PCWSTR(p.as_ptr()) },
        lpDirectory: if dir.is_empty() { PCWSTR::null() } else { PCWSTR(d.as_ptr()) },
        nShow: if show { SW_SHOWNORMAL.0 } else { SW_HIDE.0 },
        ..Default::default()
    };
    unsafe {
        if ShellExecuteExW(&mut info).is_err() {
            let code = GetLastError().0;
            return Err(match code {
                1223 => "Запрос прав администратора отклонён".to_string(),
                2 | 3 => format!("Файл не найден: {}", file),
                _ => format!("Не удалось запустить (код {})", code),
            });
        }
        if !info.hProcess.is_invalid() {
            if wait {
                WaitForSingleObject(info.hProcess, INFINITE);
            }
            let _ = CloseHandle(info.hProcess);
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn shell_exec(_verb: &str, file: &str, _params: &str, _dir: &str, _show: bool, _wait: bool) -> Result<(), String> {
    Command::new("xdg-open").arg(file).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Opens a URL / file / folder with its default handler
pub fn open(target: &str) -> Result<(), String> {
    shell_exec("open", target, "", "", true, false)
}

/// Runs a PowerShell script with administrator rights (UAC prompt), hidden.
/// The script lives in a temp file and deletes itself.
pub fn run_elevated(script: &str, wait: bool) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("Доступно только в Windows".into());
    }
    let body = format!(
        "$ErrorActionPreference='SilentlyContinue'\ntry {{\n{}\n}} finally {{ Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue }}",
        script
    );
    let f = temp_script(&body, "arcadia-admin").map_err(|e| e.to_string())?;
    let params = format!("-NoProfile -NonInteractive -ExecutionPolicy Bypass -WindowStyle Hidden -File \"{}\"", f.display());
    let r = shell_exec("runas", "powershell.exe", &params, "", false, wait);
    if r.is_err() {
        let _ = std::fs::remove_file(&f);
    }
    r
}

/// Tells WinINet (and so most programs) that the system proxy settings changed
#[cfg(windows)]
pub fn refresh_inet_settings() {
    use windows::Win32::Networking::WinInet::{InternetSetOptionW, INTERNET_OPTION_REFRESH, INTERNET_OPTION_SETTINGS_CHANGED};
    unsafe {
        let _ = InternetSetOptionW(None, INTERNET_OPTION_SETTINGS_CHANGED, None, 0);
        let _ = InternetSetOptionW(None, INTERNET_OPTION_REFRESH, None, 0);
    }
}
#[cfg(not(windows))]
pub fn refresh_inet_settings() {}

/// DPAPI: encrypt for the current Windows user
#[cfg(windows)]
pub fn dpapi(data: &[u8], encrypt: bool) -> Option<Vec<u8>> {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB};
    let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        let ok = if encrypt {
            CryptProtectData(&input, None, None, None, None, 0, &mut out)
        } else {
            CryptUnprotectData(&input, None, None, None, None, 0, &mut out)
        };
        if ok.is_err() {
            return None;
        }
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(out.pbData as *mut _)));
        Some(v)
    }
}
#[cfg(not(windows))]
pub fn dpapi(data: &[u8], _encrypt: bool) -> Option<Vec<u8>> {
    Some(data.to_vec())
}

/// Memory (MB) of a process and its direct children (a WebView2 page = browser process + renderers)
pub fn process_tree_mb(pid: u32) -> u64 {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_memory());
    let root = Pid::from_u32(pid);
    sys.processes().values().filter(|p| p.pid() == root || p.parent() == Some(root)).map(|p| p.memory()).sum::<u64>() / (1024 * 1024)
}
