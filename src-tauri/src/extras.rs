//! Extra features: HowLongToBeat, zapret strategy auto-pick, store news (Epic free games, Steam wishlist sales),
//! qBittorrent downloads, hardware monitoring module (LibreHardwareMonitor).
use crate::app::app;
use crate::util::common::{is_inside, join, now_ms, s, similarity};
use crate::util::winsys::{ps_quote, quiet, run_elevated, shell_exec};
use once_cell::sync::Lazy;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

pub static AUTOPICK_STOP: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
pub struct Extras {
    pub autopick: Value,
    pub torrents: Vec<Value>,
    pub torrent_error: String,
    pub free_games: Vec<Value>,
    pub monitor_installing: String,
    pub monitor_error: String,
}

pub static EX: Lazy<Mutex<Extras>> = Lazy::new(|| Mutex::new(Extras { autopick: json!({ "running": false }), ..Default::default() }));

/// Local services (qBittorrent, the monitor) must never go through the VPN proxy
static LOCAL: Lazy<reqwest::Client> = Lazy::new(|| reqwest::Client::builder().no_proxy().cookie_store(true).timeout(std::time::Duration::from_secs(6)).build().unwrap());

pub fn state() -> Value {
    let e = EX.lock().unwrap();
    json!({
        "autopick": e.autopick, "torrents": e.torrents, "torrentError": e.torrent_error, "freeGames": e.free_games,
        "monitor": {
            "installed": monitor_exe().is_some(), "running": monitor_running(), "installing": e.monitor_installing, "error": e.monitor_error,
            "native": !cfg!(windows), "fps": pm_installed(), "helper": helper_alive(), "task": task_cached(),
        },
        "osd": { "visible": crate::osd::visible(), "mangohud": crate::osd::mangohud_path().is_some() },
    })
}

/// schtasks takes ~50 ms: checked at most every 30 s for the state snapshot
fn task_cached() -> bool {
    static C: Lazy<Mutex<(bool, i64)>> = Lazy::new(|| Mutex::new((false, 0)));
    let mut c = C.lock().unwrap();
    if now_ms() - c.1 > 30_000 {
        *c = (task_ok(), now_ms());
    }
    c.0
}

fn settings() -> Value {
    app().store.lock().unwrap().settings.clone()
}

// ---------------- HowLongToBeat ----------------
const HLTB: &str = "https://howlongtobeat.com/";
const HLTB_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36";
static HLTB_AUTH: Lazy<tokio::sync::Mutex<Option<(String, Value, i64)>>> = Lazy::new(|| tokio::sync::Mutex::new(None));

/// The search path is hidden in the site's JS and changes now and then; the token comes from <path>/init
async fn hltb_auth() -> Result<(String, Value), String> {
    let mut g = HLTB_AUTH.lock().await;
    if let Some((p, t, at)) = g.as_ref() {
        if now_ms() - at < 10 * 60_000 {
            return Ok((p.clone(), t.clone()));
        }
    }
    let http = &app().http;
    let mut path = "api/s".to_string();
    if let Ok(r) = http.get(HLTB).header("User-Agent", HLTB_UA).send().await {
        let html = r.text().await.unwrap_or_default();
        let scr = regex::Regex::new(r#"src="(/_next/static/chunks/[^"]+\.js)""#).unwrap();
        let api = regex::Regex::new(r#"fetch\s*\(\s*["']/api/([a-zA-Z0-9_/]+)[^"']*["']\s*,\s*\{[^}]*method:\s*["']POST["']"#).unwrap();
        'outer: for c in scr.captures_iter(&html) {
            if let Ok(r) = http.get(format!("https://howlongtobeat.com{}", &c[1])).header("User-Agent", HLTB_UA).send().await {
                let js = r.text().await.unwrap_or_default();
                if let Some(m) = api.captures(&js) {
                    path = format!("api/{}", m[1].trim_end_matches('/'));
                    break 'outer;
                }
            }
        }
    }
    let tok: Value = http
        .get(format!("{}{}/init?t={}", HLTB, path, now_ms()))
        .header("User-Agent", HLTB_UA)
        .header("Referer", HLTB)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|_| "HowLongToBeat не выдал ключ поиска")?;
    *g = Some((path.clone(), tok.clone(), now_ms()));
    Ok((path, tok))
}

pub async fn hltb(id: &str) -> Result<Value, String> {
    let name = {
        let st = app().store.lock().unwrap();
        let g = st.games.get(id).ok_or("Игра не найдена")?;
        let c = s(g, "customName");
        if c.is_empty() { s(g, "name") } else { c }
    };
    let clean = regex::Regex::new(r"[™®©:]").unwrap().replace_all(&name, " ").to_string();
    let (path, tok) = hltb_auth().await?;
    let (mut key, mut val) = (String::new(), String::new());
    for (k, v) in tok.as_object().cloned().unwrap_or_default() {
        let l = k.to_lowercase();
        if l.contains("key") { key = v.as_str().map(String::from).unwrap_or_else(|| v.to_string()); }
        else if l.contains("val") { val = v.as_str().map(String::from).unwrap_or_else(|| v.to_string()); }
    }
    let mut body = json!({
        "searchType": "games", "searchTerms": clean.split_whitespace().collect::<Vec<_>>(), "searchPage": 1, "size": 20,
        "searchOptions": {
            "games": { "userId": 0, "platform": "", "sortCategory": "popular", "rangeCategory": "main", "rangeTime": { "min": null, "max": null },
                "gameplay": { "perspective": "", "flow": "", "genre": "", "difficulty": "" }, "rangeYear": { "max": "", "min": "" }, "modifier": "" },
            "users": { "sortCategory": "postcount" }, "lists": { "sortCategory": "follows" }, "filter": "", "sort": 0, "randomizer": 0
        },
        "useCache": true
    });
    if !key.is_empty() {
        body[key.as_str()] = json!(val);
    }
    let mut rq = app().http.post(format!("{}{}", HLTB, path)).header("User-Agent", HLTB_UA).header("Referer", HLTB).header("Origin", "https://howlongtobeat.com").json(&body);
    if let Some(t) = tok["token"].as_str() {
        rq = rq.header("x-auth-token", t).header("x-hp-key", &key).header("x-hp-val", &val);
    }
    let r = rq.send().await.map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        *HLTB_AUTH.lock().await = None;
        return Err(format!("HowLongToBeat ответил ошибкой {} — сайт мог сменить формат, укажите время вручную", r.status().as_u16()));
    }
    let d: Value = r.json().await.map_err(|e| e.to_string())?;
    let list = d["data"].as_array().cloned().unwrap_or_default();
    let best = list.iter().max_by(|a, b| similarity(&name, &s(a, "game_name")).partial_cmp(&similarity(&name, &s(b, "game_name"))).unwrap()).ok_or("Игра не найдена на HowLongToBeat")?;
    if similarity(&name, &s(best, "game_name")) < 0.34 {
        return Err("Игра не найдена на HowLongToBeat".into());
    }
    let h = |k: &str| (best[k].as_f64().unwrap_or(0.0) / 360.0).round() / 10.0;
    let v = json!({ "main": h("comp_main"), "plus": h("comp_plus"), "all": h("comp_100"), "name": best["game_name"], "url": format!("https://howlongtobeat.com/game/{}", best["game_id"]), "at": now_ms() });
    app().store.lock().unwrap().update(id, json!({ "hltb": v.clone() }));
    app().push();
    Ok(json!({ "ok": true, "hltb": v }))
}

// ---------------- zapret: auto-pick a strategy ----------------
const TEST_URLS: &[&str] = &[
    "https://www.youtube.com/generate_204",
    "https://i.ytimg.com/generate_204",
    "https://redirector.googlevideo.com/generate_204",
    "https://discord.com/api/v9/gateway",
    "https://cdn.discordapp.com/",
];

/// Tries every strategy in ONE elevated PowerShell run (one UAC prompt), measures which test sites open,
/// keeps the best one running. Progress goes to a file the app watches.
pub async fn zapret_autopick() {
    AUTOPICK_STOP.store(false, Ordering::SeqCst);
    let cfg = settings()["zapret"].clone();
    let dir = s(&cfg, "dir");
    let gf = cfg["gameFilter"] == true;
    let list = crate::net::zapret::list_strategies(&dir);
    let parsed: Vec<(String, String, String, String)> = list
        .iter()
        .filter_map(|f| crate::net::zapret::parse_bat(&join(&dir, f), gf).ok().map(|(e, a, c)| (f.clone(), e, a, c)))
        .collect();
    if parsed.is_empty() {
        EX.lock().unwrap().autopick = json!({ "running": false, "error": "Нет стратегий для проверки — укажите папку zapret" });
        app().push();
        return;
    }
    let tmp = std::env::temp_dir();
    let progress = tmp.join(format!("arcadia-autopick-{}.txt", now_ms()));
    let stopflag = tmp.join("arcadia-autopick.stop");
    let _ = std::fs::remove_file(&stopflag);
    let _ = std::fs::write(&progress, "");
    let items: Vec<String> = parsed.iter().map(|(n, e, a, c)| format!("@{{n={};e={};a={};c={}}}", ps_quote(n), ps_quote(e), ps_quote(a), ps_quote(c))).collect();
    let urls: Vec<String> = TEST_URLS.iter().map(|u| ps_quote(u)).collect();
    let script = format!(
        r#"$list=@({items})
$urls=@({urls})
$out={out}
$stop={stop}
$best=$null; $bestOk=-1; $bestT=99999
foreach($s in $list){{
  if(Test-Path -LiteralPath $stop){{ break }}
  Stop-Process -Name winws -Force -ErrorAction SilentlyContinue
  Start-Sleep -Milliseconds 500
  Start-Process -FilePath $s.e -ArgumentList $s.a -WorkingDirectory $s.c -WindowStyle Hidden
  Start-Sleep -Seconds 3
  $ok=0; $t=0.0
  foreach($u in $urls){{
    $r = & curl.exe -s -o NUL -m 6 -w '%{{http_code}} %{{time_total}}' $u 2>$null
    $p = "$r".Split(' ')
    $code = 0; [int]::TryParse($p[0], [ref]$code) | Out-Null
    if($code -ge 200 -and $code -lt 500){{ $ok++; $t += [double]::Parse($p[1], [Globalization.CultureInfo]::InvariantCulture) }} else {{ $t += 6 }}
  }}
  Add-Content -LiteralPath $out -Value ("R|{{0}}|{{1}}|{{2}}" -f $s.n,$ok,[Math]::Round($t,2)) -Encoding UTF8
  if($ok -gt $bestOk -or ($ok -eq $bestOk -and $t -lt $bestT)){{ $best=$s; $bestOk=$ok; $bestT=$t }}
}}
Stop-Process -Name winws -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 500
if($best -ne $null -and $bestOk -gt 0){{
  Start-Process -FilePath $best.e -ArgumentList $best.a -WorkingDirectory $best.c -WindowStyle Hidden
  Add-Content -LiteralPath $out -Value ("BEST|" + $best.n) -Encoding UTF8
}}
Add-Content -LiteralPath $out -Value "DONE" -Encoding UTF8"#,
        items = items.join(","),
        urls = urls.join(","),
        out = ps_quote(&progress.to_string_lossy()),
        stop = ps_quote(&stopflag.to_string_lossy()),
    );
    EX.lock().unwrap().autopick = json!({ "running": true, "total": parsed.len(), "done": 0, "results": [], "best": "" });
    app().push();
    if let Err(e) = tauri::async_runtime::spawn_blocking(move || run_elevated(&script, false)).await.unwrap_or(Err("ошибка".into())) {
        EX.lock().unwrap().autopick = json!({ "running": false, "error": e });
        app().push();
        return;
    }
    let started = now_ms();
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        if AUTOPICK_STOP.load(Ordering::SeqCst) {
            let _ = std::fs::write(&stopflag, "1");
        }
        let txt = std::fs::read(&progress).map(|b| String::from_utf8_lossy(&b).replace('\u{feff}', "")).unwrap_or_default();
        let mut results = vec![];
        let mut best = String::new();
        let mut done = false;
        for l in txt.lines() {
            let p: Vec<&str> = l.trim().split('|').collect();
            match p.first() {
                Some(&"R") if p.len() == 4 => results.push(json!({ "name": p[1], "ok": p[2].parse::<i64>().unwrap_or(0), "of": TEST_URLS.len(), "time": p[3].replace(',', ".").parse::<f64>().unwrap_or(0.0) })),
                Some(&"BEST") if p.len() == 2 => best = p[1].to_string(),
                Some(&"DONE") => done = true,
                _ => {}
            }
        }
        let n = results.len();
        let timeout = now_ms() - started > (parsed.len() as i64 * 45_000).max(120_000);
        EX.lock().unwrap().autopick = json!({ "running": !(done || timeout), "total": parsed.len(), "done": n, "results": results, "best": best, "error": if timeout && !done { json!("Проверка не завершилась — возможно, отклонён запрос прав администратора") } else { json!(null) } });
        if done || timeout {
            if !best.is_empty() {
                app().store.lock().unwrap().set_settings(json!({ "zapret": { "strategy": best, "lastStarted": best } }));
                app().zapret.lock().unwrap().running = true;
                crate::actions::notify("zapret", "Стратегия подобрана", &format!("Включена {}", best.trim_end_matches(".bat")));
            }
            let _ = std::fs::remove_file(&progress);
            app().push();
            break;
        }
        app().push();
    }
}

// ---------------- store news ----------------
pub async fn check_store_news() {
    let n = settings()["notify"].clone();
    // Epic: free games of the week
    if let Ok(r) = app().http.get("https://store-site-backend-static-ipv4.ak.epicgames.com/freeGamesPromotions?locale=ru&country=RU&allowCountries=RU").send().await {
        if let Ok(d) = r.json::<Value>().await {
            let now = chrono::Utc::now();
            let mut free = vec![];
            for e in d["data"]["Catalog"]["searchStore"]["elements"].as_array().cloned().unwrap_or_default() {
                let offers = e["promotions"]["promotionalOffers"].as_array().cloned().unwrap_or_default();
                let cur = offers.iter().flat_map(|o| o["promotionalOffers"].as_array().cloned().unwrap_or_default()).find(|p| {
                    p["discountSetting"]["discountPercentage"] == 0
                        && chrono::DateTime::parse_from_rfc3339(p["startDate"].as_str().unwrap_or("")).map(|d| d <= now).unwrap_or(false)
                        && chrono::DateTime::parse_from_rfc3339(p["endDate"].as_str().unwrap_or("")).map(|d| d > now).unwrap_or(false)
                });
                let Some(p) = cur else { continue };
                let slug = [s(&e, "productSlug"), s(&e["catalogNs"]["mappings"][0], "pageSlug"), s(&e["offerMappings"][0], "pageSlug")].into_iter().find(|x| !x.is_empty() && x != "[]").unwrap_or_default();
                let img = e["keyImages"].as_array().and_then(|a| a.iter().find(|k| k["type"] == "OfferImageTall" || k["type"] == "Thumbnail")).map(|k| s(k, "url")).unwrap_or_default();
                free.push(json!({ "id": e["id"], "title": e["title"], "until": p["endDate"], "url": if slug.is_empty() { "https://store.epicgames.com/ru/free-games".to_string() } else { format!("https://store.epicgames.com/ru/p/{}", slug.trim_end_matches("/home")) }, "image": img }));
            }
            let seen: Vec<String> = settings()["notified"]["epicFree"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
            let new: Vec<&Value> = free.iter().filter(|f| !seen.contains(&s(f, "id"))).collect();
            if !new.is_empty() && n["freeGames"] != false {
                crate::actions::notify("freeGames", "Бесплатно в Epic Games", &new.iter().map(|f| s(f, "title")).collect::<Vec<_>>().join(", "));
            }
            let ids: Vec<String> = free.iter().map(|f| s(f, "id")).collect();
            app().store.lock().unwrap().set_settings(json!({ "notified": { "epicFree": ids } }));
            EX.lock().unwrap().free_games = free;
        }
    }
    // Steam: discounts on the wishlist (public wishlist of the signed-in account)
    let steam_id = s(&settings()["accounts"]["steam"], "id");
    if !steam_id.is_empty() && n["wishlist"] != false {
        if let Ok(r) = app().http.get(format!("https://api.steampowered.com/IWishlistService/GetWishlist/v1/?steamid={}", steam_id)).send().await {
            let d: Value = r.json().await.unwrap_or(Value::Null);
            let ids: Vec<i64> = d["response"]["items"].as_array().map(|a| a.iter().filter_map(|x| x["appid"].as_i64()).collect()).unwrap_or_default();
            let mut sales = vec![];
            for chunk in ids.chunks(50) {
                let input = json!({ "ids": chunk.iter().map(|a| json!({ "appid": a })).collect::<Vec<_>>(), "context": { "language": "russian", "country_code": "RU" }, "data_request": { "include_basic_info": true } });
                let url = format!("https://api.steampowered.com/IStoreBrowseService/GetItems/v1/?input_json={}", urlencoding::encode(&input.to_string()));
                let Ok(r) = app().http.get(url).send().await else { continue };
                let d: Value = r.json().await.unwrap_or(Value::Null);
                for it in d["response"]["store_items"].as_array().cloned().unwrap_or_default() {
                    let o = &it["best_purchase_option"];
                    let pct = o["discount_pct"].as_i64().unwrap_or(0);
                    if pct > 0 {
                        sales.push((it["appid"].as_i64().unwrap_or(0), s(&it, "name"), pct, s(o, "formatted_final_price")));
                    }
                }
            }
            let prev = settings()["notified"]["wishlist"].clone();
            let new: Vec<&(i64, String, i64, String)> = sales.iter().filter(|(a, _, p, _)| prev[a.to_string()].as_i64() != Some(*p)).collect();
            if !new.is_empty() {
                let body = new.iter().take(4).map(|(_, nm, p, pr)| format!("{} −{}% ({})", nm, p, pr)).collect::<Vec<_>>().join(", ");
                crate::actions::notify("wishlist", "Скидки в списке желаемого Steam", &body);
            }
            let map: serde_json::Map<String, Value> = sales.iter().map(|(a, _, p, _)| (a.to_string(), json!(p))).collect();
            app().store.lock().unwrap().set_settings(json!({ "notified": { "wishlist": map } }));
        }
    }
    app().push();
}

// ---------------- qBittorrent ----------------
async fn qbit_get(base: &str, user: &str, pass: &str, path: &str) -> Result<Value, String> {
    let url = format!("{}{}", base.trim_end_matches('/'), path);
    let r = LOCAL.get(&url).header("Referer", base).send().await.map_err(|_| "qBittorrent не отвечает — включите веб-интерфейс (Настройки → Веб-интерфейс)".to_string())?;
    if r.status().as_u16() == 403 {
        let l = LOCAL.post(format!("{}/api/v2/auth/login", base.trim_end_matches('/'))).header("Referer", base).form(&[("username", user), ("password", pass)]).send().await.map_err(|e| e.to_string())?;
        let t = l.text().await.unwrap_or_default();
        if !t.contains("Ok") {
            return Err("qBittorrent: неверный логин или пароль веб-интерфейса".into());
        }
        let r2 = LOCAL.get(&url).header("Referer", base).send().await.map_err(|e| e.to_string())?;
        return r2.json().await.map_err(|e| e.to_string());
    }
    r.json().await.map_err(|e| e.to_string())
}

pub async fn torrent_loop() {
    let mut prev: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    loop {
        let t = settings()["torrent"].clone();
        let visible = crate::app::main_window().map(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false)).unwrap_or(false);
        if t["enabled"] == true {
            match qbit_get(&s(&t, "url"), &s(&t, "user"), &s(&t, "pass"), "/api/v2/torrents/info").await {
                Ok(list) => {
                    let mut out = vec![];
                    let folders: Vec<String> = settings()["scanFolders"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
                    let mut rescan = false;
                    for x in list.as_array().cloned().unwrap_or_default() {
                        let hash = s(&x, "hash");
                        let progress = x["progress"].as_f64().unwrap_or(0.0);
                        let content = { let c = s(&x, "content_path"); if c.is_empty() { s(&x, "save_path") } else { c } };
                        let in_lib = folders.iter().any(|f| is_inside(&content, f));
                        if let Some(p) = prev.get(&hash) {
                            if *p < 1.0 && progress >= 1.0 {
                                if in_lib {
                                    rescan = true;
                                }
                                crate::actions::notify("torrents", "Торрент скачан", &format!("{}{}", s(&x, "name"), if in_lib { " — игра появится в библиотеке" } else { "" }));
                            }
                        }
                        prev.insert(hash.clone(), progress);
                        let added = x["added_on"].as_i64().unwrap_or(0) * 1000;
                        let recent_done = progress >= 1.0 && now_ms() - x["completion_on"].as_i64().unwrap_or(0) * 1000 < 24 * 3_600_000;
                        if progress < 1.0 || recent_done {
                            out.push(json!({ "hash": hash, "name": x["name"], "progress": progress, "speed": x["dlspeed"], "eta": x["eta"], "state": x["state"], "size": x["size"], "path": content, "inLibrary": in_lib, "added": added }));
                        }
                    }
                    out.sort_by_key(|x| -(x["added"].as_i64().unwrap_or(0)));
                    let changed = { let mut e = EX.lock().unwrap(); let c = e.torrents != out || !e.torrent_error.is_empty(); e.torrents = out; e.torrent_error.clear(); c };
                    if rescan {
                        crate::app::run_scan();
                    }
                    if changed {
                        app().push();
                    }
                }
                Err(err) => {
                    let changed = { let mut e = EX.lock().unwrap(); let c = e.torrent_error != err; e.torrent_error = err; e.torrents.clear(); c };
                    if changed {
                        app().push();
                    }
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(if t["enabled"] != true { 10 } else if visible { 3 } else { 15 })).await;
    }
}

// ---------------- hardware monitor module ----------------
// Windows: LibreHardwareMonitor (sensors) + PresentMon (FPS), downloaded on demand into modules/monitor and run by
// the elevated helper (see fps.rs). Linux: sensors are read natively (sensors.rs) — nothing to install.
pub fn monitor_dir() -> std::path::PathBuf {
    app().data_dir.join("modules").join("monitor")
}

fn monitor_exe() -> Option<String> {
    if !cfg!(windows) {
        return Some(String::new());
    }
    let p = monitor_dir().join(crate::fps::LHM_EXE);
    p.exists().then(|| p.to_string_lossy().to_string())
}

fn pm_installed() -> bool {
    monitor_dir().join(crate::fps::PM_EXE).exists()
}

fn lhm_running() -> bool {
    crate::util::winsys::process_names().contains("librehardwaremonitor.exe")
}

fn helper_alive() -> bool {
    crate::fps::read(&monitor_dir()).is_some()
}

fn monitor_running() -> bool {
    !cfg!(windows) || lhm_running() || helper_alive()
}

const LHM_PORT: u16 = crate::sensors::LHM_PORT;

/// LHM settings: web server on, start minimized to tray, sensors every 0.5 s (it is our sensor backend)
fn configure_monitor() {
    let f = monitor_dir().join("LibreHardwareMonitor.config");
    let mut xml = std::fs::read_to_string(&f).unwrap_or_else(|_| "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<configuration>\n  <appSettings>\n  </appSettings>\n</configuration>\n".into());
    for (k, v) in [("runWebServerMenuItem", "true"), ("listenerPort", &LHM_PORT.to_string()[..]), ("startMinMenuItem", "true"), ("minTrayMenuItem", "true"), ("minCloseMenuItem", "true"), ("updateIntervalMenuItem", "1")] {
        let re = regex::Regex::new(&format!(r#"<add key="{}" value="[^"]*"\s*/>"#, k)).unwrap();
        let line = format!(r#"<add key="{}" value="{}" />"#, k, v);
        if re.is_match(&xml) {
            xml = re.replace(&xml, line.as_str()).to_string();
        } else {
            xml = xml.replacen("</appSettings>", &format!("    {}\n  </appSettings>", line), 1);
        }
    }
    let _ = std::fs::write(&f, xml);
}

async fn gh_json(url: &str) -> Result<Value, String> {
    app().http.get(url).header("User-Agent", "ArcadiaLauncher").header("Accept", "application/vnd.github+json").send().await.map_err(|e| e.to_string())?.json().await.map_err(|e| e.to_string())
}

async fn gh_download(url: &str) -> Result<Vec<u8>, String> {
    let r = app().http.get(url).header("User-Agent", "ArcadiaLauncher").send().await.map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("Не удалось скачать ({})", r.status().as_u16()));
    }
    r.bytes().await.map(|b| b.to_vec()).map_err(|e| e.to_string())
}

fn set_installing(t: &str) {
    EX.lock().unwrap().monitor_installing = t.into();
    app().push();
}

/// PresentMon console build (one exe) from GameTechDev/PresentMon releases
async fn install_presentmon() -> Result<(), String> {
    set_installing("Ищем счётчик FPS (PresentMon)…");
    let rels = gh_json("https://api.github.com/repos/GameTechDev/PresentMon/releases?per_page=10").await?;
    let re = regex::Regex::new(r"(?i)^PresentMon-[\d.]+-x64\.exe$").unwrap();
    let (rel, a) = rels
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["prerelease"] != true)
        .find_map(|r| r["assets"].as_array()?.iter().find(|a| re.is_match(&s(a, "name"))).map(|a| (r.clone(), a.clone())))
        .ok_or("Не найдена консольная версия PresentMon")?;
    set_installing(&format!("Скачиваем PresentMon {}…", s(&rel, "tag_name")));
    let bytes = gh_download(&s(&a, "browser_download_url")).await?;
    let dir = monitor_dir();
    let _ = std::fs::create_dir_all(&dir);
    let tmp = dir.join("PresentMon.exe.new");
    std::fs::write(&tmp, &bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, dir.join(crate::fps::PM_EXE)).map_err(|e| format!("{} (остановите датчики и повторите)", e))?;
    Ok(())
}

async fn install_lhm() -> Result<(), String> {
    set_installing("Ищем последнюю версию LibreHardwareMonitor…");
    let rel = gh_json("https://api.github.com/repos/LibreHardwareMonitor/LibreHardwareMonitor/releases/latest").await?;
    let assets = rel["assets"].as_array().cloned().unwrap_or_default();
    let pick = |re: &str| { let re = regex::Regex::new(re).unwrap(); assets.iter().find(|a| re.is_match(&s(a, "name"))).cloned() };
    let a = pick(r"(?i)net4\d*\.zip$").or_else(|| pick(r"(?i)^LibreHardwareMonitor[^/]*\.zip$")).ok_or("Не найден архив LibreHardwareMonitor")?;
    set_installing(&format!("Скачиваем LibreHardwareMonitor {}…", s(&rel, "tag_name")));
    let bytes = gh_download(&s(&a, "browser_download_url")).await?;
    set_installing("Распаковываем…");
    if lhm_running() {
        let _ = monitor_stop();
    }
    let dir = monitor_dir();
    let _ = std::fs::create_dir_all(&dir);
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    // the archive may keep everything in one top folder — flatten it
    let names: Vec<String> = (0..zip.len()).filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string())).collect();
    let prefix = names.iter().find(|n| n.to_lowercase().ends_with("librehardwaremonitor.exe")).map(|n| n[..n.len() - "LibreHardwareMonitor.exe".len()].to_string()).unwrap_or_default();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = f.name().to_string();
        let Some(rel) = name.strip_prefix(&prefix) else { continue };
        if rel.is_empty() || name.ends_with('/') {
            continue;
        }
        let dest = dir.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        if let Some(p) = dest.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let mut out = std::fs::File::create(&dest).map_err(|e| e.to_string())?;
        std::io::copy(&mut f, &mut out).map_err(|e| e.to_string())?;
    }
    if monitor_exe().is_none() {
        return Err("В архиве нет LibreHardwareMonitor.exe".into());
    }
    configure_monitor();
    Ok(())
}

/// Installs what is missing: sensors (LHM) and the FPS counter (PresentMon)
pub async fn monitor_install() -> Result<(), String> {
    EX.lock().unwrap().monitor_error.clear();
    let r: Result<(), String> = async {
        if !cfg!(windows) {
            return Ok(());
        }
        if monitor_exe().is_none() {
            install_lhm().await?;
        }
        if !pm_installed() {
            // sensors work without it, so a failure here is only a note
            if let Err(e) = install_presentmon().await {
                EX.lock().unwrap().monitor_error = format!("Датчики установлены, но счётчик FPS не скачался: {}", e);
            }
        }
        Ok(())
    }
    .await;
    EX.lock().unwrap().monitor_installing.clear();
    if let Err(e) = &r {
        EX.lock().unwrap().monitor_error = e.clone();
    }
    app().push();
    r
}

const TASK: &str = r"\Arcadia\Sensors";

fn exe_path() -> String {
    std::env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default()
}

/// Is the "no UAC" scheduled task registered for this Arcadia.exe?
fn task_ok() -> bool {
    cfg!(windows) && s(&settings()["monitor"], "taskExe") == exe_path() && quiet("schtasks.exe").args(["/query", "/tn", TASK]).output().map(|o| o.status.success()).unwrap_or(false)
}

/// One UAC prompt now, none later: a scheduled task with highest privileges that starts the helper
pub fn monitor_task(enable: bool) -> Result<(), String> {
    if !cfg!(windows) {
        return Ok(());
    }
    let script = if enable {
        let args = format!("--monitor-helper \"{}\" 0", monitor_dir().display());
        format!(
            "$a = New-ScheduledTaskAction -Execute {} -Argument {} -WorkingDirectory {}\n\
             $p = New-ScheduledTaskPrincipal -UserId \"$env:USERDOMAIN\\$env:USERNAME\" -LogonType Interactive -RunLevel Highest\n\
             $s = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew\n\
             Register-ScheduledTask -TaskName 'Sensors' -TaskPath '\\Arcadia\\' -Action $a -Principal $p -Settings $s -Force | Out-Null",
            ps_quote(&exe_path()), ps_quote(&args), ps_quote(&monitor_dir().to_string_lossy())
        )
    } else {
        "Unregister-ScheduledTask -TaskName 'Sensors' -TaskPath '\\Arcadia\\' -Confirm:$false".to_string()
    };
    run_elevated(&script, true)?;
    app().store.lock().unwrap().set_settings(json!({ "monitor": { "taskExe": if enable { exe_path() } else { String::new() } } }));
    if enable && !task_ok() {
        return Err("Задача не создалась — датчики будут запрашивать права при каждом запуске".into());
    }
    app().push();
    Ok(())
}

pub fn monitor_start() -> Result<(), String> {
    if !cfg!(windows) || helper_alive() {
        return Ok(());
    }
    monitor_exe().ok_or("Модуль мониторинга не установлен")?;
    configure_monitor();
    let dir = monitor_dir();
    let _ = std::fs::remove_file(dir.join("stop.flag"));
    if task_ok() && quiet("schtasks.exe").args(["/run", "/tn", TASK]).output().map(|o| o.status.success()).unwrap_or(false) {
        return Ok(());
    }
    // one UAC prompt: the helper runs LHM and PresentMon with administrator rights
    let params = format!("--monitor-helper \"{}\" {}", dir.display(), std::process::id());
    shell_exec("runas", &exe_path(), &params, &dir.to_string_lossy(), false, false)
}

/// At game start: start sensors only if that needs no prompt
pub fn monitor_autostart_quiet() {
    if cfg!(windows) && !helper_alive() && monitor_exe().is_some() && task_ok() {
        let _ = monitor_start();
    }
}

pub fn monitor_stop() -> Result<(), String> {
    if !cfg!(windows) {
        return Ok(());
    }
    let dir = monitor_dir();
    if helper_alive() {
        let _ = std::fs::write(dir.join("stop.flag"), "1");
        for _ in 0..10 {
            std::thread::sleep(std::time::Duration::from_millis(400));
            if !helper_alive() && !lhm_running() {
                return Ok(());
            }
        }
    }
    if lhm_running() {
        let _ = quiet("taskkill.exe").args(["/IM", "LibreHardwareMonitor.exe"]).output();
        std::thread::sleep(std::time::Duration::from_millis(800));
        if lhm_running() {
            run_elevated("Stop-Process -Name LibreHardwareMonitor -Force", true)?;
        }
    }
    Ok(())
}

/// LHM's data.json tree -> [{name, kind, groups: {Temperatures: [{name, value, min, max}], …}}]
pub fn parse_lhm(tree: &Value) -> Vec<Value> {
    fn kind(img: &str) -> &'static str {
        let i = img.to_lowercase();
        if i.contains("cpu") { "cpu" } else if i.contains("nvidia") || i.contains("ati") || i.contains("amd") || i.contains("intel") && i.contains("gpu") || i.contains("gpu") { "gpu" }
        else if i.contains("ram") { "ram" } else if i.contains("hdd") || i.contains("ssd") || i.contains("nvme") { "disk" } else if i.contains("mainboard") || i.contains("chip") { "board" }
        else if i.contains("nic") || i.contains("ethernet") || i.contains("network") { "net" } else { "other" }
    }
    fn walk(n: &Value, out: &mut Vec<Value>) {
        let img = s(n, "ImageURL");
        let children = n["Children"].as_array().cloned().unwrap_or_default();
        let is_hw = !img.is_empty() && !img.contains("computer") && children.iter().any(|c| c["Children"].as_array().map(|a| !a.is_empty()).unwrap_or(false) && c.get("Value").map(|v| v.as_str().map(|x| x.is_empty()).unwrap_or(true)).unwrap_or(true));
        if is_hw && !matches!(kind(&img), "other" | "board") || (is_hw && kind(&img) == "board" && children.iter().any(|c| !s(c, "Text").is_empty() && c["Children"].as_array().map(|a| a.iter().any(|x| !s(x, "Value").is_empty())).unwrap_or(false))) {
            let mut groups = serde_json::Map::new();
            for g in &children {
                let sensors: Vec<Value> = g["Children"].as_array().cloned().unwrap_or_default().iter().filter(|x| !s(x, "Value").is_empty()).map(|x| json!({ "name": x["Text"], "value": x["Value"], "min": x["Min"], "max": x["Max"] })).collect();
                if !sensors.is_empty() {
                    groups.insert(s(g, "Text"), json!(sensors));
                }
            }
            if !groups.is_empty() {
                // memory modules (SPD: capacity, timings) are not the "Memory" usage sensor
                let mut k = kind(&img);
                if k == "ram" && !groups.contains_key("Load") && !groups.contains_key("Data") {
                    k = "dimm";
                }
                out.push(json!({ "name": n["Text"], "kind": k, "groups": groups }));
            }
        }
        for c in &children {
            walk(c, out);
        }
    }
    let mut out = vec![];
    walk(tree, &mut out);
    out
}

pub async fn monitor_data() -> Value {
    let mut r = crate::sensors::read().await;
    if cfg!(windows) {
        if let Some(f) = crate::fps::read(&monitor_dir()) {
            r["fps"] = f;
        }
        if r["source"] != "lhm" && monitor_running() {
            r["note"] = json!("Датчики запускаются…");
        }
    }
    r
}

// ---------------- commands ----------------
pub async fn dispatch(ch: &str, a: &[Value]) -> Value {
    let arg = |i: usize| a.get(i).cloned().unwrap_or(Value::Null);
    let res = |r: Result<(), String>| match r { Ok(_) => json!({ "ok": true }), Err(e) => json!({ "ok": false, "error": e }) };
    match ch {
        "notify-test" => { crate::actions::notify("test", "Arcadia", "Уведомления работают"); json!({ "ok": true }) }
        "store-news" => { check_store_news().await; json!({ "ok": true }) }
        "discord-test" => {
            let id = s(&settings()["discord"], "clientId");
            let r = tauri::async_runtime::spawn_blocking(move || crate::discord::set_activity(&id, "Проверка Arcadia", now_ms(), None)).await.unwrap_or(Err("ошибка".into()));
            if r.is_ok() {
                tauri::async_runtime::spawn(async { tokio::time::sleep(std::time::Duration::from_secs(15)).await; crate::discord::clear(); });
            }
            res(r)
        }
        "torrent-test" => {
            let t = arg(0);
            match qbit_get(&s(&t, "url"), &s(&t, "user"), &s(&t, "pass"), "/api/v2/app/version").await {
                Ok(v) => json!({ "ok": true, "version": v }),
                Err(e) => {
                    // /app/version returns plain text, not JSON: a decode error still means we are connected
                    if e.contains("decoding") || e.contains("expected") { json!({ "ok": true }) } else { json!({ "ok": false, "error": e }) }
                }
            }
        }
        "monitor-install" => res(monitor_install().await),
        "monitor-start" => res(tauri::async_runtime::spawn_blocking(monitor_start).await.unwrap_or(Err("ошибка".into()))),
        "monitor-stop" => res(tauri::async_runtime::spawn_blocking(monitor_stop).await.unwrap_or(Err("ошибка".into()))),
        "monitor-data" => monitor_data().await,
        "monitor-task" => {
            let on = arg(0) == true;
            let r = tauri::async_runtime::spawn_blocking(move || monitor_task(on)).await.unwrap_or(Err("ошибка".into()));
            res(r)
        }
        "osd-toggle" => { crate::osd::toggle(); json!({ "ok": true, "visible": crate::osd::visible() }) }
        "osd-refresh" => { crate::osd::refresh(); crate::osd::register_hotkey_again(); json!({ "ok": true }) }
        "mangohud-config" => json!({ "ok": true, "config": crate::osd::mangohud_config(&crate::osd::settings()), "installed": crate::osd::mangohud_path().is_some() }),
        "monitor-folder" => { let _ = crate::util::winsys::open(&monitor_dir().to_string_lossy()); json!({ "ok": true }) }
        _ => json!({ "ok": false, "error": format!("Неизвестная команда {}", ch) }),
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lhm_tree() {
        let t = json!({ "id": 0, "Text": "Sensor", "Children": [{ "id": 1, "Text": "PC", "ImageURL": "images_icon/computer.png", "Children": [
            { "id": 2, "Text": "AMD Ryzen 7 5800X", "ImageURL": "images_icon/cpu.png", "Value": "", "Children": [
                { "id": 3, "Text": "Temperatures", "ImageURL": "images_icon/temperature.png", "Value": "", "Children": [{ "id": 4, "Text": "Core (Tctl/Tdie)", "Value": "54,5 °C", "Min": "40 °C", "Max": "80 °C", "Children": [] }] },
                { "id": 5, "Text": "Load", "Value": "", "Children": [{ "id": 6, "Text": "CPU Total", "Value": "12,0 %", "Min": "", "Max": "", "Children": [] }] } ] },
            { "id": 7, "Text": "NVIDIA GeForce RTX 3070", "ImageURL": "images_icon/nvidia.png", "Value": "", "Children": [
                { "id": 8, "Text": "Temperatures", "Value": "", "Children": [{ "id": 9, "Text": "GPU Core", "Value": "61,0 °C", "Children": [] }] } ] } ] }] });
        let hw = parse_lhm(&t);
        assert_eq!(hw.len(), 2);
        assert_eq!(hw[0]["kind"], "cpu");
        assert_eq!(hw[0]["groups"]["Temperatures"][0]["value"], "54,5 °C");
        assert_eq!(hw[1]["kind"], "gpu");
    }
}
