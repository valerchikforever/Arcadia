//! Shared helpers: path normalisation, name cleaning, matching.
use fancy_regex::Regex as FRegex;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;
use sha1::{Digest, Sha1};
use std::path::{Path, PathBuf};

/// Comparison key for paths (case-insensitive, slash-agnostic)
pub fn norm_path(p: &str) -> String {
    static SL: Lazy<Regex> = Lazy::new(|| Regex::new(r"[\\/]+").unwrap());
    let s = SL.replace_all(p, "/").to_string();
    s.trim_end_matches('/').to_lowercase()
}

pub fn is_inside(child: &str, parent: &str) -> bool {
    let c = norm_path(child);
    let p = norm_path(parent);
    !p.is_empty() && (c == p || c.starts_with(&(p + "/")))
}

pub fn exists(p: &str) -> bool {
    !p.is_empty() && Path::new(p).exists()
}

pub fn is_dir(p: &str) -> bool {
    !p.is_empty() && Path::new(p).is_dir()
}

pub fn hash_id(s: &str) -> String {
    let mut h = Sha1::new();
    h.update(s.as_bytes());
    let out = h.finalize();
    out.iter().map(|b| format!("{:02x}", b)).collect::<String>()[..16].to_string()
}

/// Normalize Windows-style paths coming from registries / manifests
pub fn fix_path(p: &str) -> String {
    let s = p.trim().trim_matches('"');
    if s.is_empty() {
        return String::new();
    }
    let s = if cfg!(windows) { s.replace('/', "\\") } else { s.to_string() };
    let t = s.trim_end_matches(['\\', '/']);
    if t.is_empty() { s } else { t.to_string() }
}

pub fn join(a: &str, b: &str) -> String {
    PathBuf::from(a).join(b).to_string_lossy().to_string()
}

pub fn file_name(p: &str) -> String {
    Path::new(p).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

pub fn parent(p: &str) -> String {
    Path::new(p).parent().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

pub fn stem(p: &str) -> String {
    Path::new(p).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

const SCENE_TAGS: &[&str] = &[
    "repack", "fitgirl", "dodi", "xatab", "mechanics", "механики", r"r\.?g\.?", "elamigos", "steamrip", "codex", "plaza",
    "skidrow", "empress", "flt", "tenoke", "rune", "razor1911", "goldberg", "portable", r"multi\d*", "eng", "rus", "ru",
    "en", r"by\s+\S+", "gog", r"drm[- ]?free", "p2p", "kaos", "decepticon", "chovka", "catalyst", "online-fix", "onlinefix",
    r"repack by \S+", "lossless",
];

/// Turn "Cyberpunk.2077.v2.1-FitGirl" / "Hollow Knight [GOG] (v1.5)" into a readable name
pub fn clean_name(raw: &str) -> String {
    static BRACKETS: Lazy<Regex> = Lazy::new(|| Regex::new(r"\[[^\]]*\]|\([^)]*\)|\{[^}]*\}").unwrap());
    static GROUP: Lazy<Regex> = Lazy::new(|| Regex::new(r"-[A-Za-z0-9]+$").unwrap());
    static VER: Lazy<FRegex> =
        Lazy::new(|| FRegex::new(r"(?i)(?<![\p{L}\d])(?:v|ver|version|build|b)\s?\.?\d[\w.\-]*").unwrap());
    static TAGS: Lazy<FRegex> =
        Lazy::new(|| FRegex::new(&format!(r"(?i)(?<![\p{{L}}\d])(?:{})(?![\p{{L}}\d])", SCENE_TAGS.join("|"))).unwrap());
    static DOTS: Lazy<Regex> = Lazy::new(|| Regex::new(r"[._]+").unwrap());
    static UNDERS: Lazy<Regex> = Lazy::new(|| Regex::new(r"_+").unwrap());
    static TRAIL: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+[-–—+&|]+\s*$").unwrap());
    static LEAD: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*[-–—+&|]+\s+").unwrap());
    static SPACES: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s{2,}").unwrap());
    if raw.is_empty() {
        return String::new();
    }
    let scene = !raw.chars().any(char::is_whitespace) && (raw.contains('.') || raw.contains('_'));
    let mut s = BRACKETS.replace_all(raw, " ").to_string();
    if scene {
        s = GROUP.replace(&s, "").to_string();
    }
    s = VER.replace_all(&s, " ").to_string();
    s = if scene { DOTS.replace_all(&s, " ").to_string() } else { UNDERS.replace_all(&s, " ").to_string() };
    s = TAGS.replace_all(&s, " ").to_string();
    s = TRAIL.replace(&s, "").to_string();
    s = LEAD.replace(&s, "").to_string();
    s = SPACES.replace_all(&s, " ").trim().to_string();
    if s.is_empty() { raw.trim().to_string() } else { s }
}

/// Loose normalization for matching names between sources
pub fn match_key(s: &str) -> String {
    static P: Lazy<Regex> = Lazy::new(|| Regex::new(r#"[™®©:’'`"!?,.\-–—_()\[\]]"#).unwrap());
    static WS: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());
    let s = P.replace_all(&s.to_lowercase(), " ").to_string();
    WS.replace_all(&s, " ").trim().to_string()
}

pub fn similarity(a: &str, b: &str) -> f64 {
    let ka = match_key(a);
    let kb = match_key(b);
    let ta: std::collections::HashSet<&str> = ka.split(' ').filter(|x| !x.is_empty()).collect();
    let tb: std::collections::HashSet<&str> = kb.split(' ').filter(|x| !x.is_empty()).collect();
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    let inter = ta.intersection(&tb).count() as f64;
    inter / (ta.len() as f64 + tb.len() as f64 - inter)
}

/// Splits a command-line string, honouring double quotes
pub fn parse_args(s: &str) -> Vec<String> {
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r#""([^"]*)"|(\S+)"#).unwrap());
    RE.captures_iter(s)
        .map(|c| c.get(1).or_else(|| c.get(2)).map(|m| m.as_str().to_string()).unwrap_or_default())
        .collect()
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// `s`, or the fallback when it is empty
pub fn nz(s: String, d: impl Into<String>) -> String {
    if s.is_empty() { d.into() } else { s }
}

// ---- serde_json helpers ----
pub fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
}
pub fn b(v: &Value, k: &str) -> bool {
    v.get(k).and_then(|x| x.as_bool()).unwrap_or(false)
}
pub fn i(v: &Value, k: &str) -> i64 {
    v.get(k).and_then(|x| x.as_i64().or_else(|| x.as_f64().map(|f| f as i64))).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clean_names() {
        assert_eq!(clean_name("Cyberpunk.2077.v2.1-FitGirl"), "Cyberpunk 2077");
        assert_eq!(clean_name("Hollow Knight [GOG] (v1.5.78)"), "Hollow Knight");
        assert_eq!(clean_name("Ведьмак 3 [R.G. Механики]"), "Ведьмак 3");
        assert_eq!(clean_name("Elden Ring - DODI Repack"), "Elden Ring");
        assert_eq!(clean_name("S.T.A.L.K.E.R. 2"), "S.T.A.L.K.E.R. 2");
        assert_eq!(clean_name("Red_Dead_Redemption_2"), "Red Dead Redemption 2");
    }
    #[test]
    fn sim() {
        assert!(similarity("The Witcher 3: Wild Hunt", "The Witcher® 3: Wild Hunt") > 0.9);
        assert_eq!(parse_args(r#"-w "C:\My Games\x" -x"#), vec!["-w", r"C:\My Games\x", "-x"]);
    }
}
