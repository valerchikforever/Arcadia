//! Play time from GOG Galaxy's local database (works without signing in, like Playnite does).
//! %ProgramData%\GOG.com\Galaxy\storage\galaxy-2.0.db: GameTimes(releaseKey "gog_<id>", minutesInGame)
use rusqlite::{Connection, OpenFlags};

pub fn db_path() -> String {
    let pd = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
    format!(r"{}\GOG.com\Galaxy\storage\galaxy-2.0.db", pd)
}

/// (gog game id, seconds)
pub fn read_from(path: &str) -> Vec<(String, i64)> {
    if !std::path::Path::new(path).exists() {
        return vec![];
    }
    // immutable: Galaxy keeps the file open; we only read a snapshot
    let uri = format!("file:{}?mode=ro&immutable=1", path.replace('\\', "/"));
    let Ok(c) = Connection::open_with_flags(uri, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI) else { return vec![] };
    let Ok(mut st) = c.prepare("SELECT releaseKey, SUM(minutesInGame) FROM GameTimes WHERE releaseKey LIKE 'gog_%' GROUP BY releaseKey") else { return vec![] };
    st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .map(|rows| rows.flatten().map(|(k, m)| (k.trim_start_matches("gog_").to_string(), m * 60)).collect())
        .unwrap_or_default()
}

pub fn read() -> Vec<(String, i64)> {
    read_from(&db_path())
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_galaxy_db() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("galaxy-2.0.db");
        let c = rusqlite::Connection::open(&p).unwrap();
        c.execute_batch("CREATE TABLE GameTimes(userId INTEGER, releaseKey TEXT, minutesInGame INTEGER); INSERT INTO GameTimes VALUES(1,'gog_1207664663',90),(1,'steam_570',5),(2,'gog_1207664663',30);").unwrap();
        drop(c);
        let r = super::read_from(&p.to_string_lossy());
        assert_eq!(r, vec![("1207664663".to_string(), 7200)]);
    }
}
