//! Starting games. ShellExecute handles exes that need administrator rights, shortcuts and URIs.
use crate::util::common::{exists, parent, s};
use crate::util::winsys::{open, shell_exec};
use serde_json::Value;

/// The launch method in effect: the user's override, or what the scanner found
pub fn launch_spec(g: &Value) -> Value {
    let o = &g["launchOverride"];
    if o.is_object() && (!s(o, "path").is_empty() || !s(o, "uri").is_empty()) {
        o.clone()
    } else {
        g["launch"].clone()
    }
}

pub fn run_spec(l: &Value) -> Result<(), String> {
    match s(l, "type").as_str() {
        "uri" => open(&s(l, "uri")),
        _ => {
            let path = s(l, "path");
            if path.is_empty() || !exists(&path) {
                return Err(format!("Файл не найден: {}", if path.is_empty() { "—" } else { &path }));
            }
            let cwd = { let c = s(l, "cwd"); if !c.is_empty() && exists(&c) { c } else { parent(&path) } };
            shell_exec("open", &path, &s(l, "args"), &cwd, true, false)
        }
    }
}

pub fn launch_game(g: &Value) -> Result<(), String> {
    let l = launch_spec(g);
    if l.is_null() {
        return Err("Не указан способ запуска".into());
    }
    run_spec(&l)
}
