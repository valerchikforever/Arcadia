//! Store accounts: optional sign-in per launcher, owned-games sync, store pages.
pub mod common;
pub mod epic;
pub mod gog;
pub mod steam;

use crate::app::app;
use crate::util::common::{now_ms, s};
use common::{profile_dir, seal, unseal};
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct Service {
    pub library: bool,
    pub home: &'static str,
    pub launcher: &'static str,
    pub login: &'static str,
    pub links: &'static [(&'static str, &'static str)],
}

pub fn services() -> Vec<(&'static str, Service)> {
    vec![
        ("steam", Service { library: true, home: "https://store.steampowered.com/", launcher: "steam://open/main", login: "", links: &[
            ("Магазин", "https://store.steampowered.com/"), ("Скидки", "https://store.steampowered.com/specials"), ("Список желаемого", "https://store.steampowered.com/wishlist/"),
            ("Профиль", "https://steamcommunity.com/my/"), ("Друзья", "https://steamcommunity.com/my/friends/"), ("Сообщество", "https://steamcommunity.com/"), ("Торговая площадка", "https://steamcommunity.com/market/"),
        ] }),
        ("epic", Service { library: true, home: "https://store.epicgames.com/ru/", launcher: "com.epicgames.launcher://store", login: "", links: &[
            ("Магазин", "https://store.epicgames.com/ru/"), ("Бесплатные игры", "https://store.epicgames.com/ru/free-games"), ("Список желаемого", "https://store.epicgames.com/ru/wishlist"), ("Аккаунт", "https://www.epicgames.com/account/personal"),
        ] }),
        ("gog", Service { library: true, home: "https://www.gog.com/ru/games", launcher: "goggalaxy://openStoreView", login: "", links: &[
            ("Магазин", "https://www.gog.com/ru/games"), ("Скидки", "https://www.gog.com/ru/games?discounted=true"), ("Мои игры", "https://www.gog.com/ru/account"), ("Список желаемого", "https://www.gog.com/ru/account/wishlist"),
        ] }),
        ("ea", Service { library: false, home: "https://www.ea.com/ru/games", launcher: "origin2://library/open", login: "https://myaccount.ea.com/cp-ui/aboutme/index", links: &[("Игры", "https://www.ea.com/ru/games"), ("Аккаунт", "https://myaccount.ea.com/cp-ui/aboutme/index")] }),
        ("ubisoft", Service { library: false, home: "https://store.ubisoft.com/ru/home", launcher: "uplay://open", login: "https://account.ubisoft.com/", links: &[("Магазин", "https://store.ubisoft.com/ru/home"), ("Аккаунт", "https://account.ubisoft.com/")] }),
        ("battlenet", Service { library: false, home: "https://shop.battle.net/ru-ru", launcher: "battlenet://", login: "https://account.battle.net/", links: &[("Магазин", "https://shop.battle.net/ru-ru"), ("Аккаунт", "https://account.battle.net/")] }),
        ("battlestate", Service { library: false, home: "https://www.escapefromtarkov.com/", launcher: "", login: "https://www.escapefromtarkov.com/profile", links: &[
            ("Escape from Tarkov", "https://www.escapefromtarkov.com/"), ("Профиль", "https://www.escapefromtarkov.com/profile"), ("Arena", "https://arena.tarkov.com/"),
        ] }),
    ]
}

pub fn service(key: &str) -> Option<Service> {
    services().into_iter().find(|(k, _)| *k == key).map(|(_, s)| s)
}

#[derive(Default)]
pub struct Accounts {
    pub busy: HashMap<String, String>,
    pub errors: HashMap<String, String>,
    local_cache: Option<(i64, Option<(String, String)>)>,
}

fn data() -> Value {
    app().store.lock().unwrap().settings["accounts"].clone()
}

/// Account with decrypted secrets (main process only)
fn account(key: &str) -> Option<Value> {
    let a = data()[key].clone();
    if !a.is_object() {
        return None;
    }
    let mut a = a;
    if let Some(sec) = a["secret"].as_str().and_then(unseal) {
        if let Some(t) = sec.get("token") {
            a["token"] = t.clone();
        }
    }
    Some(a)
}

fn save_account(key: &str, acc: &Value) {
    let mut st = app().store.lock().unwrap();
    let prev = st.settings["accounts"][key].clone();
    let mut pubv = acc.clone();
    if let Some(m) = pubv.as_object_mut() {
        m.remove("token");
    }
    let mut merged = if prev.is_object() { prev.clone() } else { json!({}) };
    for (k, v) in pubv.as_object().cloned().unwrap_or_default() {
        merged[k] = v;
    }
    if let Some(t) = acc.get("token") {
        merged["secret"] = json!(seal(&json!({ "token": t })));
    }
    st.settings["accounts"][key] = merged;
    st.save();
}

fn set_busy(key: &str, text: &str) {
    let mut a = app().accounts.lock().unwrap();
    if text.is_empty() {
        a.busy.remove(key);
    } else {
        a.busy.insert(key.into(), text.into());
    }
    drop(a);
    app().push();
}

fn set_error(key: &str, e: &str) {
    app().accounts.lock().unwrap().errors.insert(key.into(), e.into());
}

pub fn local_steam() -> Option<(String, String)> {
    let mut a = app().accounts.lock().unwrap();
    if a.local_cache.as_ref().map(|c| now_ms() - c.0 > 60_000).unwrap_or(true) {
        a.local_cache = Some((now_ms(), steam::local_account()));
    }
    a.local_cache.as_ref().unwrap().1.clone()
}

pub fn state() -> Value {
    let d = data();
    let local = local_steam();
    let api_key = !s(&app().store.lock().unwrap().settings, "steamApiKey").is_empty();
    let acc = app().accounts.lock().unwrap();
    let mut out = serde_json::Map::new();
    for (key, sv) in services() {
        let a = &d[key];
        let account = if a.is_object() {
            json!({ "name": s(a, "name"), "id": s(a, "id"), "avatar": s(a, "avatar"), "mode": a["mode"], "count": a["count"].as_i64().unwrap_or(0), "updatedAt": a["updatedAt"].as_i64().unwrap_or(0), "web": a["web"] == true })
        } else {
            Value::Null
        };
        let local_steam = match (&local, key) {
            (Some((id, name)), "steam") if s(a, "id") != *id => json!({ "id": id, "name": name }),
            _ => Value::Null,
        };
        out.insert(key.into(), json!({
            "library": sv.library, "links": sv.links.iter().map(|(l, u)| json!([l, u])).collect::<Vec<_>>(), "home": sv.home, "hasLauncher": !sv.launcher.is_empty(),
            "account": account, "busy": acc.busy.get(key).cloned().unwrap_or_default(), "error": acc.errors.get(key).cloned().unwrap_or_default(),
            "localSteam": local_steam, "hasApiKey": key == "steam" && api_key,
        }));
    }
    Value::Object(out)
}

pub async fn login(key: &str) -> Result<Value, String> {
    let sv = service(key).ok_or("Неизвестный сервис")?;
    app().accounts.lock().unwrap().errors.remove(key);
    set_busy(key, "Ожидаем вход в окне…");
    if !sv.library {
        // web-only services: keep a signed-in session for their pages in the built-in browser
        let _ = common::login_window(key, "Вход — закройте окно, когда войдёте", sv.login, None, |_, _| None).await;
        set_busy(key, "");
        let site: String = sv.login.split('/').take(3).collect::<Vec<_>>().join("/");
        let c = common::fresh_cookies(key, &site).await.unwrap_or_default();
        let signed = c.iter().filter(|x| x.expires().is_some()).count() >= 3;
        if signed {
            save_account(key, &json!({ "web": true, "mode": "web" }));
        }
        app().push();
        return Ok(json!({ "ok": true, "web": signed }));
    }
    let r = match key {
        "steam" => steam::login().await,
        "epic" => epic::login().await,
        "gog" => gog::login().await,
        _ => Err("Неизвестный сервис".into()),
    };
    set_busy(key, "");
    let mut acc = match r {
        Ok(a) => a,
        Err(e) => {
            if !e.contains("закрыто") {
                set_error(key, &e);
            }
            return Err(e);
        }
    };
    if key == "steam" {
        let (n, av) = steam::profile(&s(&acc, "id")).await;
        acc["name"] = json!(n);
        acc["avatar"] = json!(av);
    }
    save_account(key, &acc);
    sync(key).await
}

pub async fn use_local_steam() -> Result<Value, String> {
    let (id, name) = steam::local_account().ok_or("Клиент Steam не найден")?;
    let (n, av) = steam::profile(&id).await;
    save_account("steam", &json!({ "id": id, "name": if n.is_empty() { name } else { n }, "avatar": av, "mode": "local" }));
    sync("steam").await
}

pub async fn sync(key: &str) -> Result<Value, String> {
    let acc = account(key).filter(|a| !s(a, "id").is_empty()).ok_or("Сначала войдите в аккаунт")?;
    app().accounts.lock().unwrap().errors.remove(key);
    set_busy(key, "Загружаем купленные игры…");
    let api_key = s(&app().store.lock().unwrap().settings, "steamApiKey");
    let r: Result<(Vec<Value>, Option<Value>), String> = match key {
        "steam" => steam::library(&acc, &api_key).await.map(|l| (l, None)),
        "epic" => epic::library(&acc).await,
        "gog" => gog::library(&acc).await,
        _ => Err("Этот сервис не даёт список игр".into()),
    };
    set_busy(key, "");
    match r {
        Ok((list, upd)) => {
            if let Some(u) = upd {
                save_account(key, &u);
            }
            let added = app().store.lock().unwrap().merge_owned(key, &list);
            save_account(key, &json!({ "count": list.len(), "updatedAt": now_ms() }));
            app().push();
            Ok(json!({ "ok": true, "count": list.len(), "added": added }))
        }
        Err(e) => {
            set_error(key, &e);
            app().log(&format!("account sync {}: {}", key, e));
            app().push();
            Err(e)
        }
    }
}

pub async fn sync_all() {
    for key in ["steam", "epic", "gog"] {
        if account(key).map(|a| !s(&a, "id").is_empty()).unwrap_or(false) {
            let _ = sync(key).await;
        }
    }
}

pub fn logout(key: &str) {
    {
        let mut st = app().store.lock().unwrap();
        if let Some(m) = st.settings["accounts"].as_object_mut() {
            m.remove(key);
        }
        st.remove_owned(key);
    }
    app().accounts.lock().unwrap().errors.remove(key);
    crate::browser::close_if_key(key);
    // wipe the store's browser profile (cookies, cache); WebView2 may still hold it for a moment
    let dir = profile_dir(key);
    std::thread::spawn(move || {
        for _ in 0..10 {
            if std::fs::remove_dir_all(&dir).is_ok() || !dir.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    });
    app().push();
}

pub fn launcher_uri(key: &str) -> String {
    service(key).map(|s| s.launcher.to_string()).unwrap_or_default()
}
