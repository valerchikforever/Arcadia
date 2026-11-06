//! Steam: sign in on steamcommunity.com (or use the account of the local Steam client) and read owned games.
use crate::util::common::nz;
use super::common::{cookie_header, decode_entities, fresh_cookies, json_req, login_window, Cookies};
use crate::app::app;
use crate::scanners::steam::steam_path;
use crate::util::common::join;
use crate::util::vdf::{ci, ci_str, parse_vdf};
use serde_json::{json, Value};

pub fn steamid_from_cookies(c: &Cookies) -> Option<String> {
    let v = c.iter().find(|x| x.name() == "steamLoginSecure" && !x.value().is_empty())?;
    let d = urlencoding::decode(v.value()).map(|x| x.to_string()).unwrap_or_default();
    let id: String = d.chars().take_while(|c| c.is_ascii_digit()).collect();
    (id.len() == 17).then_some(id)
}

pub async fn login() -> Result<Value, String> {
    let id = login_window("steam", "Вход в Steam", "https://steamcommunity.com/login/home/?goto=", Some("https://steamcommunity.com"), |_u, c| steamid_from_cookies(c)).await?;
    Ok(json!({ "id": id, "mode": "web" }))
}

/// The most recent account of the installed Steam client (no password, public data only)
pub fn local_account() -> Option<(String, String)> {
    let steam = steam_path()?;
    let t = std::fs::read_to_string(join(&join(&steam, "config"), "loginusers.vdf")).ok()?;
    let d = parse_vdf(&t);
    let users = ci(&d, "users")?.as_object()?;
    let mut best: Option<(String, String, f64)> = None;
    for (id, u) in users {
        let ts: f64 = ci_str(u, "Timestamp").parse().unwrap_or(0.0);
        let rank = if ci_str(u, "MostRecent") == "1" { f64::INFINITY } else { ts };
        let name = nz(ci_str(u, "PersonaName"), ci_str(u, "AccountName"));
        if best.as_ref().map(|b| rank > b.2).unwrap_or(true) {
            best = Some((id.clone(), if name.is_empty() { id.clone() } else { name }, rank));
        }
    }
    best.map(|b| (b.0, b.1))
}

pub async fn profile(id: &str) -> (String, String) {
    let Ok(r) = app().http.get(format!("https://steamcommunity.com/profiles/{}/?xml=1", id)).send().await else { return Default::default() };
    let x = r.text().await.unwrap_or_default();
    let tag = |t: &str| {
        let re = regex::Regex::new(&format!(r"<{t}><!\[CDATA\[([\s\S]*?)\]\]></{t}>|<{t}>([^<]*)</{t}>")).unwrap();
        re.captures(&x).map(|c| c.get(1).or(c.get(2)).map(|m| m.as_str().trim().to_string()).unwrap_or_default()).unwrap_or_default()
    };
    (tag("steamID"), tag("avatarMedium"))
}

pub fn parse_games_page(html: &str) -> Option<Vec<Value>> {
    let re = regex::Regex::new(r#"data-profile-gameslist="([^"]+)""#).unwrap();
    if let Some(c) = re.captures(html) {
        let d: Value = serde_json::from_str(&decode_entities(&c[1])).ok()?;
        return Some(d["rgGames"].as_array().cloned().unwrap_or_default());
    }
    let old = regex::Regex::new(r"var rgGames = (\[[\s\S]*?\]);\s*\n").unwrap();
    old.captures(html).and_then(|c| serde_json::from_str(&c[1]).ok())
}

pub async fn library(acc: &Value, api_key: &str) -> Result<Vec<Value>, String> {
    let id = acc["id"].as_str().unwrap_or("");
    let games: Vec<Value> = if !api_key.is_empty() {
        let d = json_req(app().http.get(format!("https://api.steampowered.com/IPlayerService/GetOwnedGames/v1/?key={}&steamid={}&include_appinfo=1&include_played_free_games=1", urlencoding::encode(api_key), id))).await?;
        d["response"]["games"].as_array().cloned().unwrap_or_default()
    } else {
        let url = format!("https://steamcommunity.com/profiles/{}/games/?tab=all&l=russian", id);
        let mut rq = app().http.get(&url).header("User-Agent", super::common::UA);
        if acc["mode"] == "web" {
            if let Ok(c) = fresh_cookies("steam", &url).await {
                rq = rq.header("Cookie", cookie_header(&c));
            }
        }
        let html = rq.send().await.map_err(|e| e.to_string())?.text().await.map_err(|e| e.to_string())?;
        let g = parse_games_page(&html).ok_or("Steam не отдал список игр. Войдите через «Войти» или откройте список игр в настройках приватности профиля")?;
        if g.is_empty() && acc["mode"] != "web" {
            return Err("Список игр скрыт настройками приватности. Войдите в аккаунт через «Войти»".into());
        }
        g
    };
    Ok(games
        .iter()
        .map(|g| {
            let a = g["appid"].to_string();
            json!({
                "id": format!("steam:{}", a), "name": g["name"].as_str().map(String::from).unwrap_or(format!("App {}", a)), "steamAppId": a,
                "installUri": format!("steam://install/{}", a), "storeUrl": format!("https://store.steampowered.com/app/{}/", a),
                "coverUrl": format!("https://shared.cloudflare.steamstatic.com/store_item_assets/steam/apps/{}/library_600x900.jpg", a),
                "heroUrl": format!("https://shared.cloudflare.steamstatic.com/store_item_assets/steam/apps/{}/library_hero.jpg", a),
                "importedPlaytime": g["playtime_forever"].as_i64().unwrap_or(0) * 60, "importedLastPlayed": g["rtime_last_played"].as_i64().unwrap_or(0) * 1000,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn games_page() {
        let j = json!({ "rgGames": [{ "appid": 570, "name": "Dota 2" }, { "appid": 730, "name": "CS2" }] }).to_string();
        let html = format!("<template data-profile-gameslist=\"{}\"></template>", j.replace('&', "&amp;").replace('"', "&quot;"));
        assert_eq!(parse_games_page(&html).unwrap().len(), 2);
        assert!(parse_games_page("<html></html>").is_none());
    }
}
