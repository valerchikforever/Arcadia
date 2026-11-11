//! Hardware sensors in one format for the Monitor page and the in-game overlay:
//! [{ name, kind: cpu|gpu|ram|disk|net, groups: { Temperatures: [{name, value, max}], Load, Clocks, Powers, … } }]
//! (the same shape LibreHardwareMonitor's tree is converted to).
//!
//! Windows: LibreHardwareMonitor (module) when it runs; otherwise a basic set (CPU load, memory, NVIDIA GPU).
//! Linux: read natively — /proc, hwmon (k10temp/coretemp/nvme/drivetemp), amdgpu sysfs, nvidia-smi. No module, no root.
use once_cell::sync::Lazy;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

// ---------------- helpers ----------------
fn fmt(v: f64, unit: &str) -> String {
    match unit {
        "°C" | "%" | "W" => format!("{:.1} {}", v, unit),
        "GB" => format!("{:.1} GB", v),
        "MB/s" => if v >= 100.0 { format!("{:.0} MB/s", v) } else { format!("{:.1} MB/s", v) },
        _ => format!("{:.0} {}", v, unit),
    }
}

/// Number from a sensor string ("54,5 °C" -> 54.5)
pub fn num(v: &Value) -> Option<f64> {
    let t = v.as_str()?.replace(',', ".");
    let t: String = t.chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-').collect();
    t.parse().ok()
}

static MAXES: Lazy<Mutex<HashMap<String, f64>>> = Lazy::new(|| Mutex::new(HashMap::new()));

struct Hw {
    name: String,
    kind: &'static str,
    groups: Map<String, Value>,
}
impl Hw {
    fn new(name: &str, kind: &'static str) -> Hw {
        Hw { name: name.to_string(), kind, groups: Map::new() }
    }
    fn add(&mut self, group: &str, name: &str, v: f64, unit: &str) {
        if !v.is_finite() {
            return;
        }
        let key = format!("{}|{}|{}", self.name, group, name);
        let max = { let mut m = MAXES.lock().unwrap(); let e = m.entry(key).or_insert(v); if v > *e { *e = v; } *e };
        let list = self.groups.entry(group.to_string()).or_insert_with(|| json!([]));
        list.as_array_mut().unwrap().push(json!({ "name": name, "value": fmt(v, unit), "max": fmt(max, unit) }));
    }
    fn done(self, out: &mut Vec<Value>) {
        if !self.groups.is_empty() {
            out.push(json!({ "name": self.name, "kind": self.kind, "groups": self.groups }));
        }
    }
}

// ---------------- NVIDIA: nvidia-smi in loop mode (one process instead of one per second) ----------------
struct Smi {
    child: Option<std::process::Child>,
    lines: Vec<String>,
    used: Instant,
    missing: bool,
}
static SMI: Lazy<Mutex<Smi>> = Lazy::new(|| Mutex::new(Smi { child: None, lines: vec![], used: Instant::now(), missing: false }));

const SMI_Q: &str = "name,temperature.gpu,utilization.gpu,clocks.gr,clocks.mem,memory.used,memory.total,power.draw,fan.speed";

fn smi_exe() -> Option<String> {
    if cfg!(windows) {
        let sys = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        [format!(r"{}\System32\nvidia-smi.exe", sys), r"C:\Program Files\NVIDIA Corporation\NVSMI\nvidia-smi.exe".to_string()].into_iter().find(|p| std::path::Path::new(p).exists())
    } else {
        crate::util::linux::which("nvidia-smi")
    }
}

/// Latest nvidia-smi lines (one per GPU); starts the reader on first use, stops it after 20 s without use
fn nvidia_lines() -> Vec<String> {
    use std::io::BufRead;
    let mut s = SMI.lock().unwrap();
    s.used = Instant::now();
    if s.missing {
        return vec![];
    }
    let alive = s.child.as_mut().map(|c| c.try_wait().ok().flatten().is_none()).unwrap_or(false);
    if !alive {
        let Some(exe) = smi_exe() else { s.missing = true; return vec![] };
        let child = crate::util::winsys::quiet(&exe)
            .args([&format!("--query-gpu={}", SMI_Q), "--format=csv,noheader,nounits", "-lms", "1000"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn();
        let Ok(mut child) = child else { s.missing = true; return vec![] };
        let out = child.stdout.take().unwrap();
        s.child = Some(child);
        std::thread::spawn(move || {
            // one line per GPU each tick: keep the latest line of each GPU (in first-seen order)
            for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                let name = line.split(',').next().unwrap_or("").trim().to_string();
                let mut s = SMI.lock().unwrap();
                if let Some(slot) = s.lines.iter_mut().find(|l| l.split(',').next().unwrap_or("").trim() == name) {
                    *slot = line;
                } else {
                    s.lines.push(line);
                }
            }
        });
        // idle watchdog
        std::thread::spawn(|| loop {
            std::thread::sleep(std::time::Duration::from_secs(5));
            let mut s = SMI.lock().unwrap();
            if s.child.is_none() {
                return;
            }
            if s.used.elapsed().as_secs() > 20 {
                if let Some(mut c) = s.child.take() {
                    let _ = c.kill();
                }
                s.lines.clear();
                return;
            }
        });
    }
    s.lines.clone()
}

fn nvidia(out: &mut Vec<Value>) {
    for l in nvidia_lines() {
        let f: Vec<&str> = l.split(',').map(|x| x.trim()).collect();
        if f.len() < 9 {
            continue;
        }
        let n = |i: usize| f[i].parse::<f64>().unwrap_or(f64::NAN);
        let mut h = Hw::new(f[0], "gpu");
        h.add("Temperatures", "GPU Core", n(1), "°C");
        h.add("Load", "GPU Core", n(2), "%");
        h.add("Clocks", "GPU Core", n(3), "MHz");
        h.add("Clocks", "GPU Memory", n(4), "MHz");
        h.add("SmallData", "GPU Memory Used", n(5), "MB");
        h.add("SmallData", "GPU Memory Total", n(6), "MB");
        h.add("Powers", "GPU Package", n(7), "W");
        h.add("Fans", "GPU Fan", n(8), "%");
        h.done(out);
    }
}

// ---------------- CPU load / memory (sysinfo, both systems) ----------------
/// CPU load is the difference between two samples: the counter is primed once at start and sampled
/// at most every 400 ms — two samples close together (two readers at once) read as 100 %
struct Cpu {
    sys: sysinfo::System,
    at: Instant,
    load: f64,
}
static SYS: Lazy<Mutex<Cpu>> = Lazy::new(|| {
    let mut sys = sysinfo::System::new_with_specifics(sysinfo::RefreshKind::nothing().with_cpu(sysinfo::CpuRefreshKind::everything()).with_memory(sysinfo::MemoryRefreshKind::everything()));
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    sys.refresh_cpu_usage();
    let load = sys.global_cpu_usage() as f64;
    Mutex::new(Cpu { sys, at: Instant::now(), load })
});

fn cpu_name() -> String {
    static N: Lazy<String> = Lazy::new(|| {
        let mut s = sysinfo::System::new();
        s.refresh_cpu_all();
        s.cpus().first().map(|c| c.brand().trim().to_string()).filter(|x| !x.is_empty()).unwrap_or_else(|| "CPU".into())
    });
    N.clone()
}

fn basic_cpu_ram(cpu: &mut Hw, out_ram: &mut Hw) {
    let mut c = SYS.lock().unwrap();
    if c.at.elapsed() >= std::time::Duration::from_millis(400) {
        c.sys.refresh_cpu_usage();
        c.sys.refresh_cpu_frequency();
        c.load = c.sys.global_cpu_usage() as f64;
        c.at = Instant::now();
    }
    c.sys.refresh_memory();
    let load = c.load;
    let s = &c.sys;
    cpu.add("Load", "CPU Total", load, "%");
    let fr: Vec<f64> = s.cpus().iter().map(|c| c.frequency() as f64).filter(|f| *f > 0.0).collect();
    if !fr.is_empty() {
        cpu.add("Clocks", "Cores (Average)", fr.iter().sum::<f64>() / fr.len() as f64, "MHz");
    }
    let gb = |b: u64| b as f64 / 1073741824.0;
    let (total, avail) = (s.total_memory(), s.available_memory());
    if total > 0 {
        out_ram.add("Load", "Memory", (total - avail) as f64 / total as f64 * 100.0, "%");
        out_ram.add("Data", "Memory Used", gb(total - avail), "GB");
        out_ram.add("Data", "Memory Available", gb(avail), "GB");
    }
}

// ---------------- Linux sysfs ----------------
fn rd(p: &str) -> Option<String> {
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}
fn rdf(p: &str) -> Option<f64> {
    rd(p).and_then(|s| s.parse().ok())
}

/// hwmon devices: (name, dir)
fn hwmons() -> Vec<(String, String)> {
    let mut v = vec![];
    if let Ok(rd_) = std::fs::read_dir("/sys/class/hwmon") {
        for e in rd_.flatten() {
            let d = e.path().to_string_lossy().to_string();
            v.push((rd(&format!("{}/name", d)).unwrap_or_default(), d));
        }
    }
    v
}

/// temp*_input of a hwmon dir with labels
fn temps(dir: &str) -> Vec<(String, f64)> {
    let mut v = vec![];
    for i in 1..=16 {
        if let Some(t) = rdf(&format!("{}/temp{}_input", dir, i)) {
            v.push((rd(&format!("{}/temp{}_label", dir, i)).unwrap_or_else(|| format!("temp{}", i)), t / 1000.0));
        }
    }
    v
}

#[derive(Default)]
struct Prev {
    at: Option<Instant>,
    disks: HashMap<String, (u64, u64)>,
    net: (u64, u64),
    rapl: Option<u64>,
}
static PREV: Lazy<Mutex<Prev>> = Lazy::new(|| Mutex::new(Prev::default()));

fn linux_extra(cpu: &mut Hw, out: &mut Vec<Value>) {
    let mons = hwmons();
    // CPU temperature: AMD k10temp/zenpower (Tctl/Tdie), Intel coretemp (Package id 0)
    for (name, dir) in &mons {
        if matches!(name.as_str(), "k10temp" | "zenpower" | "coretemp") {
            let t = temps(dir);
            let main = t.iter().find(|(l, _)| l.starts_with("Tdie") || l.starts_with("Package")).or_else(|| t.iter().find(|(l, _)| l.starts_with("Tctl"))).or(t.first());
            if let Some((l, v)) = main {
                cpu.add("Temperatures", if l.starts_with("Package") { "CPU Package" } else { "Core (Tctl/Tdie)" }, *v, "°C");
            }
            for (l, v) in t.iter().filter(|(l, _)| l.starts_with("Core ") || l.starts_with("Tccd")) {
                cpu.add("Temperatures", l, *v, "°C");
            }
            break;
        }
    }
    let now = Instant::now();
    let mut prev = PREV.lock().unwrap();
    let dt = prev.at.map(|a| now.duration_since(a).as_secs_f64()).unwrap_or(0.0);
    // CPU package power (RAPL; readable without root on many systems)
    if let Some(e) = rd("/sys/class/powercap/intel-rapl:0/energy_uj").and_then(|x| x.parse::<u64>().ok()) {
        if let (Some(p), true) = (prev.rapl, dt > 0.2) {
            if e >= p {
                cpu.add("Powers", "CPU Package", (e - p) as f64 / 1e6 / dt, "W");
            }
        }
        prev.rapl = Some(e);
    }
    // AMD GPUs (amdgpu sysfs)
    if let Ok(cards) = std::fs::read_dir("/sys/class/drm") {
        for c in cards.flatten() {
            let n = c.file_name().to_string_lossy().to_string();
            if !(n.starts_with("card") && n[4..].chars().all(|x| x.is_ascii_digit())) {
                continue;
            }
            let dev = format!("/sys/class/drm/{}/device", n);
            if rd(&format!("{}/vendor", dev)).as_deref() != Some("0x1002") {
                continue;
            }
            let slot = rd(&format!("{}/uevent", dev)).and_then(|u| u.lines().find_map(|l| l.strip_prefix("PCI_SLOT_NAME=").map(String::from))).unwrap_or_default();
            let gname = crate::util::linux::output("lspci", &["-mm", "-s", &slot])
                .and_then(|o| o.split('"').nth(5).map(|x| x.to_string()))
                .map(|x| format!("AMD {}", x.split('[').nth(1).map(|y| y.trim_end_matches(']').to_string()).unwrap_or(x)))
                .unwrap_or_else(|| "AMD Radeon".into());
            let mut h = Hw::new(&gname, "gpu");
            if let Some(b) = rdf(&format!("{}/gpu_busy_percent", dev)) {
                h.add("Load", "GPU Core", b, "%");
            }
            if let Some(hw) = std::fs::read_dir(format!("{}/hwmon", dev)).ok().and_then(|mut r| r.next()).and_then(|e| e.ok()).map(|e| e.path().to_string_lossy().to_string()) {
                for (l, v) in temps(&hw) {
                    h.add("Temperatures", match l.as_str() { "edge" => "GPU Core", "junction" => "GPU Hot Spot", "mem" => "GPU Memory", x => x }, v, "°C");
                }
                if let Some(f) = rdf(&format!("{}/freq1_input", hw)) {
                    h.add("Clocks", "GPU Core", f / 1e6, "MHz");
                }
                if let Some(f) = rdf(&format!("{}/freq2_input", hw)) {
                    h.add("Clocks", "GPU Memory", f / 1e6, "MHz");
                }
                if let Some(p) = rdf(&format!("{}/power1_average", hw)).or_else(|| rdf(&format!("{}/power1_input", hw))) {
                    h.add("Powers", "GPU Package", p / 1e6, "W");
                }
                if let Some(r) = rdf(&format!("{}/fan1_input", hw)) {
                    h.add("Fans", "GPU Fan", r, "RPM");
                }
            }
            if let (Some(u), Some(t)) = (rdf(&format!("{}/mem_info_vram_used", dev)), rdf(&format!("{}/mem_info_vram_total", dev))) {
                h.add("SmallData", "GPU Memory Used", u / 1048576.0, "MB");
                h.add("SmallData", "GPU Memory Total", t / 1048576.0, "MB");
            }
            h.done(out);
        }
    }
    // disks: throughput from /proc/diskstats, temperature from nvme / drivetemp hwmon
    let disk_temp: HashMap<String, f64> = mons
        .iter()
        .filter(|(n, _)| n == "nvme" || n == "drivetemp")
        .filter_map(|(_, d)| {
            let link = std::fs::canonicalize(format!("{}/device", d)).ok()?.to_string_lossy().to_string();
            let t = temps(d).first()?.1;
            let key = link.rsplit('/').find(|p| p.starts_with("nvme") || p.contains(':')).unwrap_or("").to_string();
            Some((key, t))
        })
        .collect();
    if let Some(stats) = rd("/proc/diskstats") {
        for l in stats.lines() {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 10 {
                continue;
            }
            let dev = f[2];
            if !std::path::Path::new(&format!("/sys/block/{}/device", dev)).exists() || dev.starts_with("loop") {
                continue;
            }
            let (r, w) = (f[5].parse::<u64>().unwrap_or(0) * 512, f[9].parse::<u64>().unwrap_or(0) * 512);
            let model = rd(&format!("/sys/block/{}/device/model", dev)).unwrap_or_else(|| dev.to_string());
            let mut h = Hw::new(&format!("{} ({})", model, dev), "disk");
            let ctrl = dev.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end_matches('n').to_string(); // nvme0n1 -> nvme0
            if let Some(t) = disk_temp.get(&ctrl).or_else(|| {
                let dl = std::fs::canonicalize(format!("/sys/block/{}/device", dev)).ok()?.to_string_lossy().rsplit('/').next()?.to_string();
                disk_temp.get(&dl)
            }) {
                h.add("Temperatures", "Temperature", *t, "°C");
            }
            if let (Some((pr, pw)), true) = (prev.disks.get(dev).cloned(), dt > 0.2) {
                h.add("Throughput", "Read Rate", r.saturating_sub(pr) as f64 / 1048576.0 / dt, "MB/s");
                h.add("Throughput", "Write Rate", w.saturating_sub(pw) as f64 / 1048576.0 / dt, "MB/s");
            }
            prev.disks.insert(dev.to_string(), (r, w));
            h.done(out);
        }
    }
    // network: all interfaces except loopback
    if let Some(t) = rd("/proc/net/dev") {
        let (mut rx, mut tx) = (0u64, 0u64);
        for l in t.lines().skip(2) {
            let Some((ifc, rest)) = l.split_once(':') else { continue };
            if ifc.trim() == "lo" {
                continue;
            }
            let f: Vec<u64> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            if f.len() >= 9 {
                rx += f[0];
                tx += f[8];
            }
        }
        if dt > 0.2 && prev.net.0 > 0 {
            let mut h = Hw::new("Сеть", "net");
            h.add("Throughput", "Download Speed", rx.saturating_sub(prev.net.0) as f64 / 1048576.0 / dt, "MB/s");
            h.add("Throughput", "Upload Speed", tx.saturating_sub(prev.net.1) as f64 / 1048576.0 / dt, "MB/s");
            h.done(out);
        }
        prev.net = (rx, tx);
    }
    prev.at = Some(now);
}

/// Native readings (blocking, a few ms)
pub fn native() -> Vec<Value> {
    let mut out = vec![];
    let mut cpu = Hw::new(&cpu_name(), "cpu");
    let mut ram = Hw::new("Память", "ram");
    basic_cpu_ram(&mut cpu, &mut ram);
    let mut rest = vec![];
    if !cfg!(windows) {
        linux_extra(&mut cpu, &mut rest);
    }
    cpu.done(&mut out);
    nvidia(&mut out);
    ram.done(&mut out);
    out.extend(rest);
    out
}

/// LHM's web server (Windows module)
pub const LHM_PORT: u16 = 8085;
static LOCAL: Lazy<reqwest::Client> = Lazy::new(|| reqwest::Client::builder().no_proxy().timeout(std::time::Duration::from_millis(800)).build().unwrap());

/// One reading shared by everyone who asks within 400 ms (overlay + Monitor page)
static LAST: Lazy<tokio::sync::Mutex<(Option<Instant>, Value)>> = Lazy::new(|| tokio::sync::Mutex::new((None, Value::Null)));

/// Full readings: { ok, hardware, source: "lhm" | "native", error? }
pub async fn read() -> Value {
    let mut last = LAST.lock().await;
    if let (Some(at), false) = (last.0, last.1.is_null()) {
        if at.elapsed() < std::time::Duration::from_millis(400) {
            return last.1.clone();
        }
    }
    let v = read_now().await;
    *last = (Some(Instant::now()), v.clone());
    v
}

async fn read_now() -> Value {
    if cfg!(windows) {
        if let Ok(r) = LOCAL.get(format!("http://127.0.0.1:{}/data.json", LHM_PORT)).send().await {
            if let Ok(t) = r.json::<Value>().await {
                let hw = crate::extras::parse_lhm(&t);
                if !hw.is_empty() {
                    return json!({ "ok": true, "hardware": hw, "source": "lhm" });
                }
            }
        }
    }
    let hw = tauri::async_runtime::spawn_blocking(native).await.unwrap_or_default();
    json!({ "ok": !hw.is_empty(), "hardware": hw, "source": "native", "error": if hw.is_empty() { "Датчики недоступны" } else { "" } })
}

/// The numbers the overlay shows, picked from the readings
pub fn summary(hw: &[Value]) -> Value {
    let find = |kind: &str| -> Vec<&Value> { hw.iter().filter(|h| h["kind"] == kind).collect() };
    let pick = |h: &Value, group: &str, re: &str| -> Option<f64> {
        let re = regex::Regex::new(re).unwrap();
        h["groups"][group].as_array()?.iter().find(|s| re.is_match(s["name"].as_str().unwrap_or(""))).and_then(|s| num(&s["value"]))
    };
    let mut o = json!({});
    if let Some(c) = find("cpu").first() {
        o["cpu"] = json!(pick(c, "Load", r"(?i)^cpu total$").or_else(|| pick(c, "Load", r"(?i)total")).or_else(|| {
            let l: Vec<f64> = c["groups"]["Load"].as_array()?.iter().filter(|s| regex::Regex::new(r"(?i)^cpu core #\d+$").unwrap().is_match(s["name"].as_str().unwrap_or(""))).filter_map(|s| num(&s["value"])).collect();
            (!l.is_empty()).then(|| l.iter().sum::<f64>() / l.len() as f64)
        }));
        o["cputemp"] = json!(pick(c, "Temperatures", r"(?i)package|tctl|tdie").or_else(|| pick(c, "Temperatures", r"(?i)core|cpu")));
        o["cpuclk"] = json!(pick(c, "Clocks", r"(?i)average").or_else(|| {
            let l: Vec<f64> = c["groups"]["Clocks"].as_array()?.iter().filter(|s| s["name"].as_str().unwrap_or("").contains("Core #")).filter_map(|s| num(&s["value"])).collect();
            (!l.is_empty()).then(|| l.iter().sum::<f64>() / l.len() as f64)
        }));
        o["cpupower"] = json!(pick(c, "Powers", r"(?i)package|cpu"));
    }
    // the discrete GPU: the one with its own memory, not the integrated one
    let gpus = find("gpu");
    let gpu = gpus.iter().find(|g| !regex::Regex::new(r"(?i)intel|uhd|iris|radeon\(tm\) graphics|vega \d+ graphics").unwrap().is_match(g["name"].as_str().unwrap_or(""))).or(gpus.first());
    if let Some(g) = gpu {
        o["gpuName"] = g["name"].clone();
        o["gpu"] = json!(pick(g, "Load", r"(?i)gpu core|^core|d3d 3d"));
        o["gputemp"] = json!(pick(g, "Temperatures", r"(?i)gpu core|^core|edge|gpu$"));
        o["gpuhot"] = json!(pick(g, "Temperatures", r"(?i)hot ?spot|junction"));
        o["gpuclk"] = json!(pick(g, "Clocks", r"(?i)gpu core|^core"));
        o["gpupower"] = json!(pick(g, "Powers", r"(?i)package|board|gpu|total"));
        let used = pick(g, "SmallData", r"(?i)memory used|dedicated memory used").or_else(|| pick(g, "Data", r"(?i)memory used").map(|x| x * 1024.0));
        let total = pick(g, "SmallData", r"(?i)memory total|dedicated memory total").or_else(|| pick(g, "Data", r"(?i)memory total").map(|x| x * 1024.0));
        o["vram"] = json!(used);
        o["vramTotal"] = json!(total);
        o["gpufan"] = json!(pick(g, "Fans", r"."));
    }
    if let Some(r) = find("ram").first() {
        o["ram"] = json!(pick(r, "Load", r"(?i)memory"));
        let used = pick(r, "Data", r"(?i)memory used|used memory");
        let avail = pick(r, "Data", r"(?i)memory available|available memory");
        o["ramUsed"] = json!(used);
        o["ramTotal"] = json!(used.zip(avail).map(|(a, b)| a + b));
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn summary_picks() {
        let hw = vec![
            json!({ "name": "AMD Ryzen 7", "kind": "cpu", "groups": { "Load": [{ "name": "CPU Total", "value": "12,0 %" }], "Temperatures": [{ "name": "Core (Tctl/Tdie)", "value": "54.5 °C" }] } }),
            json!({ "name": "AMD Radeon(TM) Graphics", "kind": "gpu", "groups": { "Load": [{ "name": "GPU Core", "value": "3 %" }] } }),
            json!({ "name": "NVIDIA GeForce RTX 3070", "kind": "gpu", "groups": { "Load": [{ "name": "GPU Core", "value": "97 %" }], "SmallData": [{ "name": "GPU Memory Used", "value": "6120 MB" }, { "name": "GPU Memory Total", "value": "8192 MB" }] } }),
            json!({ "name": "RAM", "kind": "ram", "groups": { "Load": [{ "name": "Memory", "value": "40 %" }], "Data": [{ "name": "Memory Used", "value": "12.8 GB" }, { "name": "Memory Available", "value": "19.2 GB" }] } }),
        ];
        let s = summary(&hw);
        assert_eq!(s["cpu"], 12.0);
        assert_eq!(s["cputemp"], 54.5);
        assert_eq!(s["gpu"], 97.0);
        assert_eq!(s["vram"], 6120.0);
        assert_eq!(s["ramTotal"], 32.0);
    }
    #[test]
    fn native_runs() {
        let a = native();
        std::thread::sleep(std::time::Duration::from_millis(300));
        let b = native();
        assert!(!a.is_empty() && !b.is_empty());
        assert!(b.iter().any(|h| h["kind"] == "cpu"));
    }
}

