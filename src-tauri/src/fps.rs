//! Frame rate on Windows: PresentMon (Intel, open source) reads present events from ETW — no injection into games,
//! so it is safe with anti-cheats (BattlEye, EAC, Vanguard). ETW needs administrator rights, so Arcadia starts
//! itself once more as an elevated "sensor helper" (`Arcadia.exe --monitor-helper <dir> <pid>`):
//! it runs LibreHardwareMonitor and PresentMon, sums the frames up per process every second
//! and writes <dir>/fps.json. The normal Arcadia only reads that file. The helper quits with Arcadia
//! or when <dir>/stop.flag appears (stopping needs no second UAC prompt).
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const PM_EXE: &str = "PresentMon.exe";
pub const LHM_EXE: &str = "LibreHardwareMonitor.exe";

/// Columns of a PresentMon CSV header -> (app, pid, frame time ms)
pub fn columns(header: &str) -> Option<(usize, usize, usize)> {
    let cols: Vec<String> = header.split(',').map(|c| c.trim().to_lowercase()).collect();
    let idx = |names: &[&str]| names.iter().find_map(|n| cols.iter().position(|c| c == n));
    Some((idx(&["application"])?, idx(&["processid"])?, idx(&["msbetweenpresents", "frametime", "msbetweendisplaychange"])?))
}

#[derive(Default)]
pub struct Agg {
    apps: HashMap<u32, (String, VecDeque<(Instant, f64)>)>,
}

impl Agg {
    pub fn push(&mut self, pid: u32, app: &str, ms: f64, at: Instant) {
        if !(ms > 0.0 && ms < 5000.0) {
            return;
        }
        let e = self.apps.entry(pid).or_insert_with(|| (app.to_string(), VecDeque::new()));
        e.1.push_back((at, ms));
        // keep 10 s of frames for the 1% low
        while e.1.front().map(|f| at.duration_since(f.0) > Duration::from_secs(10)).unwrap_or(false) {
            e.1.pop_front();
        }
    }

    /// fps (last second), average frame time, 1% low fps (last 10 s)
    pub fn report(&mut self, now: Instant) -> Vec<Value> {
        self.apps.retain(|_, (_, q)| q.back().map(|f| now.duration_since(f.0) < Duration::from_secs(3)).unwrap_or(false));
        let mut out = vec![];
        for (pid, (app, q)) in &self.apps {
            let last: Vec<f64> = q.iter().filter(|f| now.duration_since(f.0) <= Duration::from_secs(1)).map(|f| f.1).collect();
            if last.is_empty() {
                continue;
            }
            let avg = last.iter().sum::<f64>() / last.len() as f64;
            let mut all: Vec<f64> = q.iter().map(|f| f.1).collect();
            all.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
            // 1% low = average frame rate of the slowest 1% of frames
            let worst = &all[..(all.len() / 100).max(1)];
            let p99 = worst.iter().sum::<f64>() / worst.len() as f64;
            out.push(json!({ "pid": pid, "app": app, "fps": (1000.0 / avg * 10.0).round() / 10.0, "ft": (avg * 100.0).round() / 100.0, "low1": (1000.0 / p99).round(), "frames": last.len() }));
        }
        out
    }
}

fn log(dir: &Path, m: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("helper.log")) {
        let _ = writeln!(f, "{} {}", chrono::Local::now().format("%H:%M:%S"), m);
    }
}

fn write_json(dir: &Path, v: &Value) {
    let tmp = dir.join("fps.json.tmp");
    if std::fs::write(&tmp, v.to_string()).is_ok() {
        let _ = std::fs::rename(&tmp, dir.join("fps.json"));
    }
}

fn arcadia_alive(parent: u32) -> bool {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    let me = std::process::id();
    if parent != 0 {
        return sys.process(sysinfo::Pid::from_u32(parent)).is_some();
    }
    // started from the scheduled task: any other Arcadia process counts
    sys.processes().values().any(|p| p.pid().as_u32() != me && p.name().to_string_lossy().eq_ignore_ascii_case("arcadia.exe"))
}

fn spawn_pm(exe: &Path, v1: bool) -> std::io::Result<std::process::Child> {
    let mut c = crate::util::winsys::quiet(&exe.to_string_lossy());
    c.args(["--output_stdout", "--session_name", "ArcadiaFPS", "--stop_existing_session"]);
    if v1 {
        c.arg("--v1_metrics"); // MsBetweenPresents column (PresentMon 2.x)
    }
    c.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn()
}

/// Entry of the elevated helper process. Returns the exit code.
pub fn helper_main(args: &[String]) -> i32 {
    let Some(dir) = args.first().map(PathBuf::from) else { return 2 };
    let parent: u32 = args.get(1).and_then(|x| x.parse().ok()).unwrap_or(0);
    let _ = std::fs::remove_file(dir.join("stop.flag"));
    log(&dir, &format!("helper start, parent {}", parent));
    let names = crate::util::winsys::process_names();
    // sensors
    let lhm = dir.join(LHM_EXE);
    let mut lhm_child = None;
    if lhm.exists() && !names.contains(&LHM_EXE.to_lowercase()) {
        match crate::util::winsys::quiet(&lhm.to_string_lossy()).current_dir(&dir).spawn() {
            Ok(c) => lhm_child = Some(c),
            Err(e) => log(&dir, &format!("LHM: {}", e)),
        }
    }
    // frame rate
    let agg = std::sync::Arc::new(std::sync::Mutex::new(Agg::default()));
    let pm_exe = dir.join(PM_EXE);
    let mut pm: Option<std::process::Child> = None;
    let mut pm_error = String::new();
    if pm_exe.exists() {
        for v1 in [true, false] {
            match spawn_pm(&pm_exe, v1) {
                Ok(mut c) => {
                    let out = c.stdout.take().unwrap();
                    let a = agg.clone();
                    std::thread::spawn(move || {
                        let mut cols = None;
                        for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                            if line.starts_with("Application,") {
                                cols = columns(&line);
                                continue;
                            }
                            let Some((ia, ip, ims)) = cols else { continue };
                            let f: Vec<&str> = line.split(',').collect();
                            if f.len() <= ims.max(ip).max(ia) {
                                continue;
                            }
                            let app = f[ia];
                            if app.eq_ignore_ascii_case("dwm.exe") {
                                continue;
                            }
                            if let (Ok(pid), Ok(ms)) = (f[ip].trim().parse::<u32>(), f[ims].trim().parse::<f64>()) {
                                a.lock().unwrap().push(pid, app, ms, Instant::now());
                            }
                        }
                    });
                    std::thread::sleep(Duration::from_millis(2500));
                    if c.try_wait().ok().flatten().is_none() {
                        pm = Some(c);
                        pm_error.clear();
                        break;
                    }
                    pm_error = "PresentMon завершился сразу после запуска".into();
                    log(&dir, &format!("PresentMon exited (v1 metrics: {})", v1));
                }
                Err(e) => {
                    pm_error = format!("PresentMon: {}", e);
                    break;
                }
            }
        }
    } else {
        pm_error = "Счётчик FPS (PresentMon) не установлен".into();
    }
    let mut ticks = 0u64;
    loop {
        // report twice a second (fps over the last second) so the overlay lags at most ~0.5 s
        std::thread::sleep(Duration::from_millis(500));
        ticks += 1;
        let apps = agg.lock().unwrap().report(Instant::now());
        let pm_alive = pm.as_mut().map(|c| c.try_wait().ok().flatten().is_none()).unwrap_or(false);
        write_json(&dir, &json!({ "t": crate::util::common::now_ms(), "pm": pm_alive, "error": if pm_alive { "" } else { pm_error.as_str() }, "apps": apps }));
        let stop = dir.join("stop.flag").exists();
        if stop || (ticks % 6 == 0 && !arcadia_alive(parent)) {
            log(&dir, if stop { "stop requested" } else { "Arcadia exited" });
            let _ = std::fs::remove_file(dir.join("stop.flag"));
            if let Some(mut c) = pm.take() {
                let _ = c.kill();
            }
            // stop sensors too (ours, or one started by an earlier helper)
            if let Some(mut c) = lhm_child.take() {
                let _ = c.kill();
            } else {
                let _ = crate::util::winsys::quiet("taskkill.exe").args(["/F", "/IM", LHM_EXE]).output();
            }
            write_json(&dir, &json!({ "t": 0, "pm": false, "apps": [] }));
            return 0;
        }
    }
}

/// The helper's last report if it is alive
pub fn read(dir: &Path) -> Option<Value> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("fps.json")).ok()?).ok()?;
    let t = v["t"].as_i64().unwrap_or(0);
    (crate::util::common::now_ms() - t < 5000).then_some(v)
}

/// Process id of the window in front (the game, when playing)
#[cfg(windows)]
pub fn foreground_pid() -> Option<u32> {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    unsafe {
        let h = GetForegroundWindow();
        if h.0.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, Some(&mut pid));
        (pid != 0).then_some(pid)
    }
}
#[cfg(not(windows))]
pub fn foreground_pid() -> Option<u32> {
    None
}

/// The frame rate to show: the foreground app if it presents frames, else a process of a running game
pub fn pick(report: &Value, game_dirs: &[String]) -> Option<Value> {
    let apps = report["apps"].as_array()?;
    if apps.is_empty() {
        return None;
    }
    if let Some(fg) = foreground_pid() {
        if let Some(a) = apps.iter().find(|a| a["pid"].as_u64() == Some(fg as u64)) {
            return Some(a.clone());
        }
    }
    if game_dirs.is_empty() {
        return None;
    }
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let pids: Vec<sysinfo::Pid> = apps.iter().filter_map(|a| a["pid"].as_u64()).map(|p| sysinfo::Pid::from_u32(p as u32)).collect();
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&pids), true, ProcessRefreshKind::nothing().with_exe(UpdateKind::Always));
    apps.iter()
        .filter(|a| {
            let pid = sysinfo::Pid::from_u32(a["pid"].as_u64().unwrap_or(0) as u32);
            sys.process(pid).and_then(|p| p.exe()).map(|e| game_dirs.iter().any(|d| crate::util::common::is_inside(&e.to_string_lossy(), d))).unwrap_or(false)
        })
        .max_by_key(|a| a["frames"].as_u64().unwrap_or(0))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presentmon_csv() {
        assert_eq!(columns("Application,ProcessID,SwapChainAddress,Runtime,SyncInterval,PresentFlags,Dropped,TimeInSeconds,MsInPresentAPI,MsBetweenPresents"), Some((0, 1, 9)));
        assert_eq!(columns("Application,ProcessID,SwapChainAddress,PresentRuntime,SyncInterval,PresentFlags,AllowsTearing,PresentMode,FrameType,CPUStartTime,FrameTime,CPUBusy"), Some((0, 1, 10)));
        let mut a = Agg::default();
        let t0 = Instant::now();
        for i in 0..120 {
            a.push(42, "game.exe", if i == 60 { 50.0 } else { 8.0 }, t0 + Duration::from_micros(i * 8000));
        }
        let r = a.report(t0 + Duration::from_millis(960));
        assert_eq!(r.len(), 1);
        let fps = r[0]["fps"].as_f64().unwrap();
        assert!(fps > 110.0 && fps < 126.0, "{}", fps);
        assert_eq!(r[0]["low1"], 20.0); // the 50 ms hitch
    }
}
