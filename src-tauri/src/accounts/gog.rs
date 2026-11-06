//! GOG: GOG Galaxy OAuth (as used by Heroic / Playnite), owned games and play time.
use super::common::{json_req, login_window};
use crate::app::app;
use crate::util::common::{now_ms, s};
use serde_json::{json, Value};

const CLIENT_ID: &str = "46899977096215655";
const CLIENT_SECRET: &str = "9d85c43b1482497dbbce61f6e4aa173a433796eeae2ca8c5f6129f2dc4de46d9";
const REDIRECT: &str = "https://embed.gog.com/on_login_success?origin=client";

fn token_url(params: &[(&str, &str)]) -> String {
    let mut q = vec![("client_id", CLIENT_ID), ("client_secret", CLIENT_SECRET)];
    q.extend_from_slice(params);
    format!("https://auth.gog.com/token?{}", q.iter().map(|(k, v)| format!("{}={}", k, urlencoding::encode(v))).collect::<Vec<_>>().join("&"))
}

fn to_token(t: &Value) -> Value {
    json!({ "access": t["access_token"], "refresh": t["refresh_token"], "expires": now_ms() + t["expires_in"].as_i64().unwrap_or(3600) * 1000 })
}

pub async fn login() -> Result<Value, String> {
    let auth = format!("https://auth.gog.com/auth?client_id={}&redirect_uri={}&response_type=code&layout=client2", CLIENT_ID, urlencoding::encode(REDIRECT));
    let code = login_window("gog", "Вход в GOG", &auth, None, |u, _| {
        u.starts_with("https://embed.gog.com/on_login_success").then(|| url::Url::parse(u).ok()?.query_pairs().find(|(k, _)| k == "code").map(|(_, v)| v.to_string())).flatten()
    })
    .await?;
    let t = json_req(app().http.get(token_url(&[("grant_type", "authorization_code"), ("code", &code), ("redirect_uri", REDIRECT)]))).await?;
    let token = to_token(&t);
    let name = json_req(app().http.get("https://embed.gog.com/userData.json").header("Authorization", format!("Bearer {}", s(&token, "access")))).await.map(|u| s(&u, "username")).unwrap_or_default();
    Ok(json!({ "id": t["user_id"].as_str().map(String::from).unwrap_or_else(|| t["user_id"].to_string()), "name": name, "mode": "oauth", "token": token }))
}

pub async fn ensure_token(acc: &Value) -> Result<(String, Option<Value>), String> {
    let t = &acc["token"];
    if !s(t, "access").is_empty() && t["expires"].as_i64().unwrap_or(0) > now_ms() + 120_000 {
        return Ok((s(t, "access"), None));
    }
    let rf = s(t, "refresh");
    if rf.is_empty() {
        return Err("Сессия GOG истекла — войдите снова".into());
    }
    let nt = to_token(&json_req(app().http.get(token_url(&[("grant_type", "refresh_token"), ("refresh_token", &rf)]))).await.map_err(|_| "Сессия GOG истекла — войдите снова")?);
    let mut upd = acc.clone();
    upd["token"] = nt.clone();
    Ok((s(&nt, "access"), Some(upd)))
}

pub async fn library(acc: &Value) -> Result<(Vec<Value>, Option<Value>), String> {
    let (access, upd) = ensure_token(acc).await?;
    let bearer = format!("Bearer {}", access);
    let playtime: std::collections::HashMap<String, i64> = crate::scanners::gog_time::read().into_iter().collect();
    let mut out = vec![];
    let mut total = 1;
    let mut page = 1;
    while page <= total && page <= 100 {
        let d = json_req(app().http.get(format!("https://embed.gog.com/account/getFilteredProducts?mediaType=1&sortBy=title&page={}", page)).header("Authorization", &bearer)).await?;
        total = d["totalPages"].as_i64().unwrap_or(1);
        for p in d["products"].as_array().cloned().unwrap_or_default() {
            if p["isGame"] == false {
                continue;
            }
            let id = p["id"].to_string();
            let img = s(&p, "image");
            let img = if img.is_empty() { String::new() } else { format!("https:{}", img) };
            out.push(json!({
                "id": format!("gog:{}", id), "name": p["title"], "installUri": format!("goggalaxy://openGameView/{}", id),
                "storeUrl": if s(&p, "url").is_empty() { String::new() } else { format!("https://www.gog.com{}", s(&p, "url")) },
                "coverUrl": if img.is_empty() { String::new() } else { format!("{}_392.jpg", img) },
                "heroUrl": if img.is_empty() { String::new() } else { format!("{}_product_card_v2_mobile_slider_639.jpg", img) },
                "importedPlaytime": playtime.get(&id).cloned().unwrap_or(0),
            }));
        }
        page += 1;
    }
    Ok((out, upd))
}
