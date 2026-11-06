//! Epic Games: the OAuth flow open-source launchers (Legendary / Heroic) use. Also reads play time.
use crate::util::common::nz;
use super::common::{cookie_header, json_req, login_window, UA};
use crate::app::app;
use crate::util::common::{now_ms, s};
use base64::Engine;
use serde_json::{json, Value};

const CLIENT_ID: &str = "34a02cf8f4414e29b15921876da36f9a";
const CLIENT_SECRET: &str = "daafbccc737745039dffe53d94fc76cf";
const OAUTH: &str = "https://account-public-service-prod03.ol.epicgames.com/account/api/oauth/token";
const LIBRARY: &str = "https://library-service.live.use1a.on.epicgames.com/library/api/public/items";
const CATALOG: &str = "https://catalog-public-service-prod06.ol.epicgames.com/catalog/api/shared/namespace";

fn redirect() -> String {
    format!("https://www.epicgames.com/id/api/redirect?clientId={}&responseType=code", CLIENT_ID)
}

async fn token_request(params: &[(&str, &str)]) -> Result<Value, String> {
    let basic = base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", CLIENT_ID, CLIENT_SECRET));
    let mut form: Vec<(&str, &str)> = params.to_vec();
    form.push(("token_type", "eg1"));
    json_req(app().http.post(OAUTH).header("Authorization", format!("Basic {}", basic)).form(&form)).await
}

fn parse_time(s: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s).map(|d| d.timestamp_millis()).unwrap_or(0)
}

fn token_to_account(t: &Value) -> Value {
    let e = parse_time(&s(t, "expires_at"));
    let expires = if e > 0 { e } else { now_ms() + 3_600_000 };
    json!({
        "id": t["account_id"], "name": s(t, "displayName"), "mode": "oauth",
        "token": { "access": t["access_token"], "refresh": t["refresh_token"], "expires": expires, "refreshExpires": parse_time(&s(t, "refresh_expires_at")) }
    })
}

pub async fn login() -> Result<Value, String> {
    let r = redirect();
    let url = format!("https://www.epicgames.com/id/login?redirectUrl={}", urlencoding::encode(&r));
    let cookies = login_window("epic", "Вход в Epic Games", &url, Some("https://www.epicgames.com"), |u, c| {
        (u.starts_with("https://www.epicgames.com/id/api/redirect") && !c.is_empty()).then(|| cookie_header(c))
    })
    .await?;
    // the redirect endpoint returns the one-time code for the signed-in session
    let d = json_req(app().http.get(&r).header("Cookie", cookies).header("User-Agent", UA)).await?;
    let code = d["authorizationCode"].as_str().ok_or("Epic не выдал код входа — попробуйте ещё раз")?.to_string();
    Ok(token_to_account(&token_request(&[("grant_type", "authorization_code"), ("code", &code)]).await?))
}

/// Fresh access token; Some(updated account) when it was refreshed
pub async fn ensure_token(acc: &Value) -> Result<(String, Option<Value>), String> {
    let t = &acc["token"];
    if !s(t, "access").is_empty() && t["expires"].as_i64().unwrap_or(0) > now_ms() + 120_000 {
        return Ok((s(t, "access"), None));
    }
    let rf = s(t, "refresh");
    if rf.is_empty() {
        return Err("Сессия Epic истекла — войдите снова".into());
    }
    let upd = token_to_account(&token_request(&[("grant_type", "refresh_token"), ("refresh_token", &rf)]).await.map_err(|_| "Сессия Epic истекла — войдите снова")?);
    Ok((s(&upd["token"], "access"), Some(upd)))
}

pub async fn library(acc: &Value) -> Result<(Vec<Value>, Option<Value>), String> {
    let (access, upd) = ensure_token(acc).await?;
    let bearer = format!("Bearer {}", access);
    let mut records: Vec<Value> = vec![];
    let mut cursor = String::new();
    for _ in 0..50 {
        let url = if cursor.is_empty() { format!("{}?includeMetadata=true", LIBRARY) } else { format!("{}?includeMetadata=true&cursor={}", LIBRARY, urlencoding::encode(&cursor)) };
        let d = json_req(app().http.get(url).header("Authorization", &bearer)).await?;
        records.extend(d["records"].as_array().cloned().unwrap_or_default());
        cursor = s(&d["responseMetadata"], "nextCursor");
        if cursor.is_empty() {
            break;
        }
    }
    let mut seen = std::collections::HashSet::new();
    let items: Vec<Value> = records
        .into_iter()
        .filter(|r| {
            let a = s(r, "appName");
            !a.is_empty() && !s(r, "catalogItemId").is_empty() && s(r, "namespace") != "ue" && (s(r, "sandboxType").is_empty() || s(r, "sandboxType") == "PUBLIC") && seen.insert(a)
        })
        .collect();
    // play time (Epic's own counter)
    let mut playtime = std::collections::HashMap::new();
    if let Ok(p) = json_req(app().http.get(format!("https://library-service.live.use1a.on.epicgames.com/library/api/public/playtime/account/{}/all", s(acc, "id"))).header("Authorization", &bearer)).await {
        for x in p.as_array().cloned().unwrap_or_default() {
            playtime.insert(s(&x, "artifactId"), x["totalTime"].as_i64().unwrap_or(0));
        }
    }
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(6));
    let mut tasks = vec![];
    for r in items {
        let sem = sem.clone();
        let bearer = bearer.clone();
        tasks.push(tauri::async_runtime::spawn(async move {
            let _p = sem.acquire().await;
            let url = format!("{}/{}/bulk/items?id={}&includeDLCDetails=true&includeMainGameDetails=true&country=RU&locale=ru", CATALOG, urlencoding::encode(&s(&r, "namespace")), urlencoding::encode(&s(&r, "catalogItemId")));
            let d = json_req(app().http.get(url).header("Authorization", bearer)).await.ok();
            (r.clone(), d.map(|d| d[s(&r, "catalogItemId")].clone()))
        }));
    }
    let mut out = vec![];
    for t in tasks {
        let Ok((r, Some(c))) = t.await else { continue };
        if c.is_null() || c.get("mainGameItem").map(|x| !x.is_null()).unwrap_or(false) {
            continue; // DLC
        }
        let cats: Vec<String> = c["categories"].as_array().map(|a| a.iter().map(|x| s(x, "path")).collect()).unwrap_or_default();
        if !cats.is_empty() && !cats.iter().any(|x| x == "games" || x == "applications") {
            continue;
        }
        if cats.iter().any(|x| x == "addons" || x == "digitalextras") {
            continue;
        }
        let img = |ty: &str| c["keyImages"].as_array().and_then(|a| a.iter().find(|k| k["type"] == ty)).map(|k| s(k, "url")).unwrap_or_default();
        let first = |v: [String; 3]| v.into_iter().find(|x| !x.is_empty()).unwrap_or_default();
        let (ns, item, app_name) = (s(&r, "namespace"), s(&r, "catalogItemId"), s(&r, "appName"));
        out.push(json!({
            "id": format!("epic:{}", app_name), "name": nz(s(&c, "title"), app_name.clone()),
            "installUri": format!("com.epicgames.launcher://apps/{}%3A{}%3A{}?action=install", urlencoding::encode(&ns), urlencoding::encode(&item), urlencoding::encode(&app_name)),
            "coverUrl": first([img("DieselGameBoxTall"), img("OfferImageTall"), img("Thumbnail")]),
            "heroUrl": first([img("DieselGameBox"), img("OfferImageWide"), String::new()]),
            "developer": s(&c, "developer"),
            "importedPlaytime": playtime.get(&app_name).cloned().unwrap_or(0),
        }));
    }
    Ok((out, upd))
}
