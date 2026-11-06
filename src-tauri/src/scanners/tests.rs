use super::*;
use crate::util::common::file_name;
use crate::util::exefinder::find_game_exe;
use crate::util::winsys::RegKey;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

fn mk(p: &Path, content: &[u8]) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, content).unwrap();
}
fn big(p: &Path, kb: usize) {
    mk(p, &vec![0u8; kb * 1024]);
}
fn hm(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
}
fn ps(p: &Path) -> String {
    p.to_string_lossy().to_string()
}
/// Steam writes paths into .vdf with escaped backslashes ("C:\\Games")
fn vdf_esc(p: &Path) -> String {
    ps(p).replace('\\', "\\\\")
}

#[test]
fn exe_finder_picks_real_game() {
    let t = tempfile::tempdir().unwrap();
    let d = t.path().join("Hollow Knight");
    big(&d.join("unins000.exe"), 900);
    big(&d.join("UnityCrashHandler64.exe"), 1200);
    big(&d.join("_CommonRedist").join("vcredist_x64.exe"), 5000);
    big(&d.join("hollow_knight.exe"), 650);
    assert_eq!(file_name(&find_game_exe(&ps(&d), "Hollow Knight", 3).unwrap()), "hollow_knight.exe");
    let s = t.path().join("Stray");
    big(&s.join("Stray.exe"), 300);
    big(&s.join("Hk_project").join("Binaries").join("Win64").join("Stray-Win64-Shipping.exe"), 80000);
    assert_eq!(file_name(&find_game_exe(&ps(&s), "Stray", 3).unwrap()), "Stray.exe");
}

#[test]
fn full_scan() {
    let t = tempfile::tempdir().unwrap();
    let tmp = t.path();
    let pd = tmp.join("ProgramData");
    let steam = tmp.join("Steam");
    let lib2 = tmp.join("SteamLibrary");
    mk(&steam.join("steamapps").join("libraryfolders.vdf"), format!("\"libraryfolders\"{{\"0\"{{\"path\" \"{}\"}} \"1\"{{\"path\" \"{}\"}}}}", vdf_esc(&steam), vdf_esc(&lib2)).as_bytes());
    mk(&steam.join("steamapps").join("appmanifest_570.acf"), br#""AppState"{"appid" "570" "name" "Dota 2" "installdir" "dota 2 beta" "StateFlags" "4"}"#);
    mk(&steam.join("steamapps").join("appmanifest_228980.acf"), br#""AppState"{"appid" "228980" "name" "Steamworks Common Redistributables" "installdir" "x" "StateFlags" "4"}"#);
    mk(&lib2.join("steamapps").join("appmanifest_1091500.acf"), br#""AppState"{"appid" "1091500" "name" "Cyberpunk 2077" "installdir" "Cyberpunk 2077" "StateFlags" "4"}"#);
    mk(&lib2.join("steamapps").join("appmanifest_999.acf"), br#""AppState"{"appid" "999" "name" "Half" "installdir" "hd" "StateFlags" "1026"}"#);
    mk(&steam.join("userdata").join("42").join("config").join("localconfig.vdf"), br#""UserLocalConfigStore"{"Software"{"Valve"{"Steam"{"apps"{"570"{"Playtime" "600" "LastPlayed" "1700000000"}}}}}}"#);
    mk(&steam.join("appcache").join("librarycache").join("570").join("abc").join("library_600x900.jpg"), b"img");
    // Epic
    let eg = tmp.join("Epic").join("Fortnite");
    big(&eg.join("FortniteGame").join("Binaries").join("Win64").join("FortniteLauncher.exe"), 100);
    let man = pd.join("Epic").join("EpicGamesLauncher").join("Data").join("Manifests");
    mk(&man.join("A.item"), serde_json::to_string(&json!({ "DisplayName": "Fortnite", "AppName": "Fortnite", "MainGameAppName": "Fortnite", "CatalogNamespace": "fn", "CatalogItemId": "4fe7", "InstallLocation": ps(&eg), "LaunchExecutable": "FortniteGame/Binaries/Win64/FortniteLauncher.exe", "AppCategories": ["public", "games"] })).unwrap().as_bytes());
    mk(&man.join("B.item"), serde_json::to_string(&json!({ "DisplayName": "DLC", "AppName": "Dlc1", "MainGameAppName": "Fortnite", "InstallLocation": ps(&eg), "AppCategories": ["addons"] })).unwrap().as_bytes());
    // registry launchers
    let gog = tmp.join("GOG").join("Witcher 3");
    big(&gog.join("bin").join("x64").join("witcher3.exe"), 2000);
    let ubi = tmp.join("Ubisoft").join("Far Cry 5");
    big(&ubi.join("bin").join("FarCry5.exe"), 2000);
    let ea = tmp.join("EA").join("Apex");
    big(&ea.join("r5apex.exe"), 3000);
    mk(&ea.join("__Installer").join("installerdata.xml"), br"<DiPManifest><runtime><launcher><filePath>[HKEY_LOCAL_MACHINE\SOFTWARE\Respawn\Apex\Install Dir]r5apex.exe</filePath></launcher></runtime></DiPManifest>");
    let bnet = tmp.join("Battle.net");
    big(&bnet.join("Battle.net.exe"), 100);
    let ow = tmp.join("Overwatch");
    big(&ow.join("_retail_").join("Overwatch.exe"), 3000);
    let bsg = tmp.join("BsgLauncher");
    big(&bsg.join("BsgLauncher.exe"), 100);
    let eft = tmp.join("EFT");
    big(&eft.join("EscapeFromTarkov.exe"), 600);
    let u = r"HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";
    let mut reg = RegSnapshot::new();
    reg.insert(r"HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\GOG.com\Games".into(), RegKey { values: HashMap::new(), subkeys: vec![
        ("1207664663".into(), hm(&[("gameID", "1207664663"), ("gameName", "The Witcher 3: Wild Hunt"), ("path", &ps(&gog)), ("exe", &ps(&gog.join("bin").join("x64").join("witcher3.exe")))])),
        ("1640424747".into(), hm(&[("gameID", "1640424747"), ("gameName", "W3 DLC"), ("path", &ps(&gog)), ("dependsOn", "1207664663")])),
    ] });
    reg.insert(r"HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs".into(), RegKey { values: HashMap::new(), subkeys: vec![("4311".into(), hm(&[("InstallDir", &format!("{}/", ps(&ubi)))]))] });
    reg.insert(u.into(), RegKey { values: HashMap::new(), subkeys: vec![
        ("Uplay Install 4311".into(), hm(&[("DisplayName", "Far Cry 5")])),
        ("{EA-APEX}".into(), hm(&[("DisplayName", "Apex Legends"), ("Publisher", "Electronic Arts"), ("InstallLocation", &ps(&ea)), ("UninstallString", r"EAInstaller\Cleanup.exe")])),
        ("Battle.net".into(), hm(&[("DisplayName", "Battle.net"), ("InstallLocation", &ps(&bnet))])),
        ("Overwatch".into(), hm(&[("DisplayName", "Overwatch"), ("InstallLocation", &ps(&ow)), ("UninstallString", r#""C:\ProgramData\Battle.net\Agent\Blizzard Uninstaller.exe" --lang=enUS --uid=prometheus --displayname="Overwatch""#)])),
        ("BsgLauncher".into(), hm(&[("DisplayName", "BsgLauncher"), ("Publisher", "Battlestate Games"), ("InstallLocation", &ps(&bsg))])),
        ("EscapeFromTarkov".into(), hm(&[("DisplayName", "Escape from Tarkov"), ("Publisher", "Battlestate Games"), ("InstallLocation", &ps(&eft))])),
        ("Notepad++".into(), hm(&[("DisplayName", "Notepad++"), ("InstallLocation", &ps(&tmp.join("npp")))])),
    ] });
    // torrent folder
    let torr = tmp.join("Torrents");
    big(&torr.join("Hades.v1.38-GOG").join("Hades.exe"), 400);
    big(&torr.join("Hades.v1.38-GOG").join("x64").join("Hades.exe"), 9000);
    big(&torr.join("Elden Ring [FitGirl Repack]").join("setup.exe"), 4000);
    big(&torr.join("Lies of P (DODI Repack)").join("LOP.exe"), 300);
    big(&torr.join("Lies of P (DODI Repack)").join("LiesofP").join("Binaries").join("Win64").join("LOP-Win64-Shipping.exe"), 90000);
    big(&lib2.join("steamapps").join("common").join("Cyberpunk 2077").join("bin").join("x64").join("Cyberpunk2077.exe"), 50000);

    let mut ctx = Ctx::from_registry(reg);
    ctx.program_data = ps(&pd);
    ctx.steam_path = Some(ps(&steam));
    let settings = json!({ "sources": {}, "scanFolders": [ps(&torr), ps(&lib2.join("steamapps").join("common"))] });
    let (games, status) = scan_all(&settings, &[], Some(ctx));
    let by: HashMap<String, &Value> = games.iter().map(|g| (g["id"].as_str().unwrap().to_string(), g)).collect();
    // steam
    assert!(by.contains_key("steam:570") && by.contains_key("steam:1091500"));
    assert!(!by.contains_key("steam:228980") && !by.contains_key("steam:999"));
    assert_eq!(by["steam:570"]["importedPlaytime"], 36000);
    assert!(by["steam:570"]["localArt"]["cover"].as_str().unwrap().contains("library_600x900"));
    // epic
    assert!(by["epic:Fortnite"]["launch"]["uri"].as_str().unwrap().starts_with("com.epicgames.launcher://apps/fn%3A4fe7%3AFortnite"));
    assert_eq!(status["epic"]["count"], 1);
    // gog
    assert!(by["gog:1207664663"]["launch"]["path"].as_str().unwrap().ends_with("witcher3.exe"));
    assert_eq!(status["gog"]["count"], 1);
    // ubisoft
    assert_eq!(by["ubisoft:4311"]["name"], "Far Cry 5");
    assert_eq!(by["ubisoft:4311"]["launch"]["uri"], "uplay://launch/4311/0");
    // ea
    assert!(games.iter().any(|g| g["source"] == "ea" && g["exePath"].as_str().unwrap().ends_with("r5apex.exe")));
    // battle.net
    let ow = games.iter().find(|g| g["source"] == "battlenet").unwrap();
    assert_eq!(ow["launch"]["args"], "--exec=\"launch Pro\"");
    // battlestate
    let b = games.iter().find(|g| g["source"] == "battlestate").unwrap();
    assert!(b["launch"]["path"].as_str().unwrap().ends_with("BsgLauncher.exe"));
    assert_eq!(b["trackDir"], ps(&eft));
    assert_eq!(status["battlestate"]["count"], 1);
    // folders: clean names, unpacked repack skipped, steam dupes skipped
    let mut f: Vec<String> = games.iter().filter(|g| g["source"] == "folder").map(|g| g["name"].as_str().unwrap().to_string()).collect();
    f.sort();
    assert_eq!(f, vec!["Hades", "Lies of P"]);
    let lop = games.iter().find(|g| g["name"] == "Lies of P").unwrap();
    assert!(lop["exePath"].as_str().unwrap().ends_with("LOP.exe"));
}
