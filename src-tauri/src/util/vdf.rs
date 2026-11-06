//! Minimal parser for Valve KeyValues text files (.vdf / .acf)
use serde_json::{Map, Value};

pub fn parse_vdf(text: &str) -> Value {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut stack: Vec<(Option<String>, Map<String, Value>)> = vec![(None, Map::new())];
    let mut key: Option<String> = None;
    let mut i = 0;
    let is_ws = |c: char| c == ' ' || c == '\t' || c == '\r' || c == '\n' || c == '\u{feff}';
    while i < n {
        let c = chars[i];
        if c == '/' && i + 1 < n && chars[i + 1] == '/' {
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if is_ws(c) {
            i += 1;
            continue;
        }
        if c == '{' {
            stack.push((key.take(), Map::new()));
            i += 1;
            continue;
        }
        if c == '}' {
            if stack.len() > 1 {
                let (k, obj) = stack.pop().unwrap();
                if let Some(k) = k {
                    stack.last_mut().unwrap().1.insert(k, Value::Object(obj));
                }
            }
            key = None;
            i += 1;
            continue;
        }
        let mut tok = String::new();
        if c == '"' {
            i += 1;
            while i < n && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < n {
                    let nx = chars[i + 1];
                    tok.push(match nx {
                        'n' => '\n',
                        't' => '\t',
                        o => o,
                    });
                    i += 2;
                    continue;
                }
                tok.push(chars[i]);
                i += 1;
            }
            i += 1;
        } else {
            while i < n && !is_ws(chars[i]) && chars[i] != '{' && chars[i] != '}' && chars[i] != '"' {
                tok.push(chars[i]);
                i += 1;
            }
            if tok.starts_with('[') && tok.ends_with(']') {
                continue; // conditional like [$WIN32]
            }
        }
        match key.take() {
            None => key = Some(tok),
            Some(k) => {
                stack.last_mut().unwrap().1.insert(k, Value::String(tok));
            }
        }
    }
    while stack.len() > 1 {
        let (k, obj) = stack.pop().unwrap();
        if let Some(k) = k {
            stack.last_mut().unwrap().1.insert(k, Value::Object(obj));
        }
    }
    Value::Object(stack.pop().unwrap().1)
}

/// Case-insensitive property lookup
pub fn ci<'a>(obj: &'a Value, name: &str) -> Option<&'a Value> {
    let m = obj.as_object()?;
    if let Some(v) = m.get(name) {
        return Some(v);
    }
    let low = name.to_lowercase();
    m.iter().find(|(k, _)| k.to_lowercase() == low).map(|(_, v)| v)
}

pub fn ci_str(obj: &Value, name: &str) -> String {
    ci(obj, name).and_then(|v| v.as_str()).unwrap_or("").to_string()
}

pub fn ci_path<'a>(obj: &'a Value, names: &[&str]) -> Option<&'a Value> {
    let mut cur = obj;
    for n in names {
        cur = ci(cur, n)?;
    }
    Some(cur)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_nested_and_escapes() {
        let d = parse_vdf("\"libraryfolders\"\n{\n \"0\"\n {\n  \"path\"  \"D:\\\\SteamLibrary\"\n  \"apps\" { \"570\" \"123\" }\n }\n}");
        assert_eq!(d["libraryfolders"]["0"]["path"], "D:\\SteamLibrary");
        assert_eq!(d["libraryfolders"]["0"]["apps"]["570"], "123");
        assert_eq!(ci_str(&d["libraryfolders"]["0"], "PATH"), "D:\\SteamLibrary");
    }
}
