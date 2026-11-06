//! Covers, banners and descriptions: local Steam cache -> Steam store (no key) -> SteamGridDB (optional key).
use crate::util::common::nz;
use crate::app::app;
use crate::util::common::{exists, hash_id, match_key, now_ms, s, similarity};
use crate::util::winsys::{ps_quote, run_ps};
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::sync::Mutex;

const STEAM_CDN: &str = "https://shared.cloudflare.steamstatic.com/store_item_assets/";
const UA: &str = "ArcadiaLauncher/2.0";

#[derive(Default)]
pub struct Meta {
    queue: VecDeque<(String, Option<String>, bool)>,
    in_queue: HashSet<String>,
    active: usize,
    icons: Vec<(String, String, String)>, // id, exe, dest
}

static META: Lazy<Mutex<Meta>> = Lazy::new(|| Mutex::new(Meta::default()));
static LAST_STEAM: Lazy<tokio::sync::Mutex<i64>> = Lazy::new(|| tokio::sync::Mutex::new(0));

pub fn pending() -> usize {
    let m = META.lock().unwrap();
    m.queue.len() + m.active
}

pub fn enqueue(id: &str, term: Option<String>, force: bool) {
    {
        let mut m = META.lock().unwrap();
        if m.in_queue.contains(id) && !force {
            return;
        }
        m.in_queue.insert(id.to_string());
        if force {
            m.queue.push_front((id.to_string(), term, force));
        } else {
            m.queue.push_back((id.to_string(), term, force));
        }
    }
    pump();
}

pub fn enqueue_missing() {
    let ids: Vec<String> = {
        let st = app().store.lock().unwrap();
        st.games
            .values()
            .filter(|g| g["missing"] != true && g["installed"] != false)
            .filter(|g| g["metaTried"] != true || (s(g, "coverPath").is_empty() && s(g, "iconPath").is_empty()))
            .map(|g| s(g, "id"))
            .collect()
    };
    for id in ids {
        enqueue(&id, None, false);
    }
}

fn pump() {
    loop {
        let job = {
            let mut m = META.lock().unwrap();
            if m.active >= 3 {
                None
            } else if let Some(j) = m.queue.pop_front() {
                m.active += 1;
                Some(j)
            } else {
                if m.active == 0 && !m.icons.is_empty() {
                    let batch: Vec<_> = m.icons.drain(..).collect();
                    tauri::async_runtime::spawn_blocking(move || extract_icons(batch));
                }
                None
            }
        };
        let Some((id, term, force)) = job else { break };
        tauri::async_runtime::spawn(async move {
            if let Err(e) = process(&id, term, force).await {
                app().log(&format!("meta {}: {}", id, e));
            }
            {
                let mut m = META.lock().unwrap();
                m.active -= 1;
                m.in_queue.remove(&id);
            }
            app().push();
            pump();
        });
    }
    app().push();
}

/// One PowerShell call for many exe icons (starting PowerShell per file would be slow)
fn extract_icons(batch: Vec<(String, String, String)>) {
    if !cfg!(windows) || batch.is_empty() {
        return;
    }
    let items: Vec<String> = batch.iter().map(|(_, e, d)| format!("@({},{})", ps_quote(e), ps_quote(d))).collect();
    let script = format!(
        "Add-Type -AssemblyName System.Drawing\n$items=@({})\nforeach($i in $items){{ try {{ $ic=[System.Drawing.Icon]::ExtractAssociatedIcon($i[0]); $b=$ic.ToBitmap(); $b.Save($i[1],[System.Drawing.Imaging.ImageFormat]::Png); Write-Output ('OK|'+$i[1]) }} catch {{}} }}",
        items.join(",")
    );
    let out = run_ps(&script).unwrap_or_default();
    let ok: HashSet<&str> = out.lines().filter_map(|l| l.trim().strip_prefix("OK|")).collect();
    let mut st = app().store.lock().unwrap();
    for (id, _, dest) in &batch {
        if ok.contains(dest.as_str()) {
            st.update(id, json!({ "iconPath": dest, "mediaVersion": now_ms() }));
        }
    }
    drop(st);
    app().push();
}

async fn get_json(url: &str, headers: &[(&str, String)]) -> Result<Value, String> {
    let mut rq = app().http.get(url).header("User-Agent", UA);
    for (k, v) in headers {
        rq = rq.header(*k, v);
    }
    let r = rq.send().await.map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("HTTP {}", r.status().as_u16()));
    }
    r.json::<Value>().await.map_err(|e| e.to_string())
}

async fn steam_throttle() {
    let mut last = LAST_STEAM.lock().await;
    let wait = *last + 350 - now_ms();
    *last = now_ms().max(*last + 350);
    drop(last);
    if wait > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(wait as u64)).await;
    }
}

async fn download(url: &str, dest: &str) -> Option<String> {
    let r = app().http.get(url).header("User-Agent", UA).send().await.ok()?;
    if !r.status().is_success() {
        return None;
    }
    let ct = r.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    if !ct.starts_with("image/") {
        return None;
    }
    let bytes = r.bytes().await.ok()?;
    if bytes.len() < 1024 {
        return None;
    }
    let ext = if ct.contains("png") { ".png" } else if ct.contains("webp") { ".webp" } else { ".jpg" };
    let file = format!("{}{}", dest, ext);
    std::fs::write(&file, &bytes).ok()?;
    Some(file)
}

fn copy_local(src: &str, dest: &str) -> Option<String> {
    let ext = Path::new(src).extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let file = format!("{}{}", dest, ext);
    std::fs::copy(src, &file).ok()?;
    Some(file)
}

pub fn decode_entities(s: &str) -> String {
    static BR: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)<br\s*/?>").unwrap());
    static TAG: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[^>]+>").unwrap());
    let t = BR.replace_all(s, "\n");
    TAG.replace_all(&t, "")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .trim()
        .to_string()
}

pub async fn steam_search(term: &str) -> Option<String> {
    steam_throttle().await;
    let d = get_json(&format!("https://store.steampowered.com/api/storesearch/?term={}&l=english&cc=US", urlencoding::encode(term)), &[]).await.ok()?;
    let items = d["items"].as_array()?;
    let key = match_key(term);
    if let Some(e) = items.iter().find(|i| match_key(&s(i, "name")) == key) {
        return Some(e["id"].to_string());
    }
    let best = items.iter().take(5).map(|i| (similarity(term, &s(i, "name")), i)).max_by(|a, b| a.0.partial_cmp(&b.0).unwrap())?;
    if best.0 >= 0.6 { Some(best.1["id"].to_string()) } else { None }
}

async fn steam_assets(appid: &str) -> (Option<String>, Option<String>) {
    steam_throttle().await;
    let input = json!({ "ids": [{ "appid": appid.parse::<i64>().unwrap_or(0) }], "context": { "language": "russian", "country_code": "RU" }, "data_request": { "include_assets": true } });
    let url = format!("https://api.steampowered.com/IStoreBrowseService/GetItems/v1/?input_json={}", urlencoding::encode(&input.to_string()));
    let Ok(d) = get_json(&url, &[]).await else { return (None, None) };
    let a = &d["response"]["store_items"][0]["assets"];
    let fmt = s(a, "asset_url_format");
    if fmt.is_empty() {
        return (None, None);
    }
    let u = |k1: &str, k2: &str| {
        let f = nz(s(a, k1), s(a, k2));
        if f.is_empty() { None } else { Some(format!("{}{}", STEAM_CDN, fmt.replace("${FILENAME}", &f))) }
    };
    (u("library_capsule_2x", "library_capsule"), u("library_hero_2x", "library_hero"))
}

pub async fn steam_details(appid: &str) -> Option<Value> {
    steam_throttle().await;
    let d = get_json(&format!("https://store.steampowered.com/api/appdetails?appids={}&l=russian", appid), &[]).await.ok()?;
    if d[appid]["success"] == true { Some(d[appid]["data"].clone()) } else { None }
}

async fn sgdb(name: &str, appid: Option<&str>) -> (Option<String>, Option<String>) {
    let key = s(&app().store.lock().unwrap().settings, "sgdbKey");
    if key.is_empty() {
        return (None, None);
    }
    let h = [("Authorization", format!("Bearer {}", key))];
    let base = "https://www.steamgriddb.com/api/v2";
    let mut gid = None;
    if let Some(a) = appid {
        gid = get_json(&format!("{}/games/steam/{}", base, a), &h).await.ok().and_then(|g| g["data"]["id"].as_i64());
    }
    if gid.is_none() {
        gid = get_json(&format!("{}/search/autocomplete/{}", base, urlencoding::encode(name)), &h).await.ok().and_then(|g| g["data"][0]["id"].as_i64());
    }
    let Some(gid) = gid else { return (None, None) };
    let grids = get_json(&format!("{}/grids/game/{}?dimensions=600x900,342x482,660x930&types=static", base, gid), &h).await.ok();
    let heroes = get_json(&format!("{}/heroes/game/{}?types=static", base, gid), &h).await.ok();
    (
        grids.and_then(|g| g["data"][0]["url"].as_str().map(String::from)),
        heroes.and_then(|g| g["data"][0]["url"].as_str().map(String::from)),
    )
}

async fn process(id: &str, term: Option<String>, force: bool) -> Result<(), String> {
    let (g, media) = {
        let st = app().store.lock().unwrap();
        (st.games.get(id).cloned().ok_or("нет игры")?, st.media_dir.clone())
    };
    let name = term.clone().unwrap_or_else(|| nz(s(&g, "customName"), s(&g, "name")));
    let base = media.join(hash_id(id)).to_string_lossy().to_string();
    let mut patch = json!({ "metaTried": true });
    let keep_cover = g["userCover"] == true && !force;
    let keep_hero = g["userHero"] == true && !force;
    let (mut cover, mut hero): (Option<String>, Option<String>) = (None, None);
    // 1) Steam's own library cache
    if term.is_none() {
        let la = &g["localArt"];
        if !keep_cover && exists(&s(la, "cover")) {
            cover = copy_local(&s(la, "cover"), &format!("{}-cover", base));
        }
        if !keep_hero && exists(&s(la, "hero")) {
            hero = copy_local(&s(la, "hero"), &format!("{}-hero", base));
        }
    }
    // 2) Steam store
    let mut appid = if term.is_some() { None } else { [s(&g, "steamAppId"), s(&g, "metaAppId")].into_iter().find(|x| !x.is_empty()) };
    if appid.is_none() {
        appid = steam_search(&name).await;
    }
    if let Some(a) = &appid {
        patch["metaAppId"] = json!(a);
        if (cover.is_none() && !keep_cover) || (hero.is_none() && !keep_hero) {
            let (ac, ah) = steam_assets(a).await;
            let old = |f: &str| format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{}/{}", a, f);
            if cover.is_none() && !keep_cover {
                for u in [ac.clone(), Some(old("library_600x900_2x.jpg")), Some(old("library_600x900.jpg"))].into_iter().flatten() {
                    cover = download(&u, &format!("{}-cover", base)).await;
                    if cover.is_some() {
                        break;
                    }
                }
            }
            if hero.is_none() && !keep_hero {
                for u in [ah.clone(), Some(old("library_hero.jpg"))].into_iter().flatten() {
                    hero = download(&u, &format!("{}-hero", base)).await;
                    if hero.is_some() {
                        break;
                    }
                }
            }
        }
        if let Some(det) = steam_details(a).await {
            patch["description"] = json!(decode_entities(&s(&det, "short_description")));
            patch["developer"] = json!(det["developers"].as_array().map(|d| d.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default());
            patch["releaseDate"] = json!(s(&det["release_date"], "date"));
            patch["genres"] = json!(det["genres"].as_array().map(|d| d.iter().filter_map(|x| x["description"].as_str()).take(5).collect::<Vec<_>>()).unwrap_or_default());
            if cover.is_none() && !keep_cover && !s(&det, "header_image").is_empty() {
                cover = download(&s(&det, "header_image"), &format!("{}-cover", base)).await;
            }
        }
    }
    // 3) SteamGridDB
    if (cover.is_none() && !keep_cover) || (hero.is_none() && !keep_hero) || term.is_some() {
        let (sc, sh) = sgdb(&name, appid.as_deref()).await;
        if let (Some(u), false) = (sc, keep_cover) {
            if cover.is_none() || term.is_some() {
                cover = download(&u, &format!("{}-cover", base)).await.or(cover);
            }
        }
        if let (Some(u), false) = (sh, keep_hero) {
            if hero.is_none() || term.is_some() {
                hero = download(&u, &format!("{}-hero", base)).await.or(hero);
            }
        }
    }
    if let Some(c) = cover {
        patch["coverPath"] = json!(c);
        patch["userCover"] = json!(false);
    }
    if let Some(h) = hero {
        patch["heroPath"] = json!(h);
        patch["userHero"] = json!(false);
    }
    // 4) exe icon as a last-resort visual (batched)
    let exe = s(&g, "exePath");
    if s(&g, "iconPath").is_empty() && exists(&exe) {
        META.lock().unwrap().icons.push((id.to_string(), exe, format!("{}-icon.png", base)));
    }
    patch["mediaVersion"] = json!(now_ms());
    app().store.lock().unwrap().update(id, patch);
    Ok(())
}

/// The user picked an image file or pasted a link
pub async fn set_image(id: &str, kind: &str, file: Option<String>, url: Option<String>) -> Result<(), String> {
    let media = app().store.lock().unwrap().media_dir.clone();
    let dest = media.join(format!("{}-{}-user-{:x}", hash_id(id), kind, now_ms())).to_string_lossy().to_string();
    let out = match (file, url) {
        (Some(f), _) => copy_local(&f, &dest),
        (None, Some(u)) => download(&u, &dest).await,
        _ => None,
    }
    .ok_or("Не удалось загрузить изображение")?;
    let patch = if kind == "hero" { json!({ "heroPath": out, "userHero": true }) } else { json!({ "coverPath": out, "userCover": true }) };
    let mut st = app().store.lock().unwrap();
    st.update(id, patch);
    st.update(id, json!({ "mediaVersion": now_ms() }));
    Ok(())
}
