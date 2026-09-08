use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use chrono::DateTime;
use serde_json::{Value, json};

pub fn events_from_dir(dir: &Path) -> Vec<Value> {
    let mut turns = match fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.join("action.json").is_file())
            .collect::<Vec<PathBuf>>(),
        Err(_) => return Vec::new(),
    };
    turns.sort();
    turns
        .into_iter()
        .filter_map(|path| event_from_turn(&path))
        .collect()
}

fn event_from_turn(dir: &Path) -> Option<Value> {
    let raw = fs::read_to_string(dir.join("action.json")).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    let tool = tool_name(&value);
    if tool.is_empty() {
        return None;
    }
    let args = arguments(&value);
    let at = timestamp_ms(&value).unwrap_or_else(|| file_time_ms(&dir.join("action.json")));
    let el = element_from_args(args);
    let mut event = match tool.as_str() {
        "click" | "right_click" | "double_click" | "browser_click" => {
            json!({ "t": "click", "el": el, "at": at })
        }
        "type_text" | "set_value" | "browser_type" => {
            let name = element_label(&el);
            let role = el.get("role").and_then(Value::as_str).unwrap_or("");
            let raw_text = args
                .get("text")
                .or_else(|| args.get("value"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let value = if looks_secret(&name) || looks_secret(role) {
                "[已遮罩]"
            } else {
                raw_text
            };
            json!({ "t": "input", "el": el, "value": value, "at": at })
        }
        "press_key" | "hotkey" => {
            let key = args
                .get("key")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    args.get("keys").and_then(Value::as_array).map(|keys| {
                        keys.iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join("+")
                    })
                })
                .unwrap_or_default();
            json!({ "t": "key", "key": key, "el": el, "at": at })
        }
        "scroll" => json!({
            "t": "scroll",
            "y": args.get("amount").and_then(Value::as_i64).unwrap_or(0),
            "at": at
        }),
        "browser_navigate" => json!({
            "t": "navigate",
            "url": args.get("url").and_then(Value::as_str).unwrap_or(""),
            "at": at
        }),
        _ => return None,
    };
    if let Some(title) = window_title(dir) {
        event["window"] = json!(title);
    }
    Some(event)
}

fn tool_name(value: &Value) -> String {
    value
        .get("tool")
        .or_else(|| value.get("name"))
        .or_else(|| value.get("action"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn arguments(value: &Value) -> &Value {
    value
        .get("arguments")
        .or_else(|| value.get("input"))
        .or_else(|| value.get("args"))
        .unwrap_or(value)
}

fn element_from_args(args: &Value) -> Value {
    let name = pick(
        args,
        &[
            "name",
            "label",
            "ref",
            "selector",
            "element_token",
            "element_index",
        ],
    );
    let role = pick(args, &["role"]);
    json!({
        "name": name,
        "label": name,
        "role": role,
        "text": name,
    })
}

fn element_label(el: &Value) -> String {
    el.get("name")
        .or_else(|| el.get("label"))
        .or_else(|| el.get("text"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn pick(value: &Value, keys: &[&str]) -> String {
    for key in keys {
        if let Some(text) = value.get(*key).and_then(Value::as_str).map(str::trim)
            && !text.is_empty()
        {
            return text.to_string();
        }
        if let Some(number) = value.get(*key).and_then(Value::as_i64) {
            return number.to_string();
        }
    }
    String::new()
}

fn timestamp_ms(value: &Value) -> Option<i64> {
    if let Some(ms) = value.get("at").and_then(Value::as_i64) {
        return Some(ms);
    }
    let stamp = value
        .get("timestamp")
        .or_else(|| value.get("ts"))
        .or_else(|| value.get("time"))
        .and_then(Value::as_str)?;
    DateTime::parse_from_rfc3339(stamp)
        .ok()
        .map(|time| time.timestamp_millis())
}

fn file_time_ms(path: &Path) -> i64 {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

fn window_title(dir: &Path) -> Option<String> {
    for name in ["after_state.json", "app_state.json", "before_state.json"] {
        let Ok(raw) = fs::read_to_string(dir.join(name)) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        if let Some(title) = value
            .get("title")
            .or_else(|| value.get("window_title"))
            .or_else(|| value.pointer("/window/title"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|title| !title.is_empty())
        {
            return Some(title.to_string());
        }
    }
    None
}

pub fn looks_secret(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "pass",
        "pwd",
        "密碼",
        "token",
        "otp",
        "驗證碼",
        "secret",
        "cvv",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn write_turn(root: &Path, name: &str, action: &Value, after: Option<&Value>) {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("action.json"), action.to_string()).unwrap();
        if let Some(state) = after {
            fs::write(dir.join("after_state.json"), state.to_string()).unwrap();
        }
    }

    #[test]
    fn trajectory_turns_become_skill_events() {
        let root = std::env::temp_dir().join(format!(
            "lazyboy-traj-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        write_turn(
            &root,
            "turn-00001",
            &json!({
                "tool": "browser_click",
                "timestamp": "2026-09-07T00:00:01Z",
                "arguments": { "ref": "p1:1", "role": "button", "name": "Smoke Click" }
            }),
            Some(&json!({ "title": "LazyBoy Cua Smoke" })),
        );
        write_turn(
            &root,
            "turn-00002",
            &json!({
                "tool": "type_text",
                "timestamp": "2026-09-07T00:00:02Z",
                "arguments": { "text": "hunter2", "name": "Password", "role": "textbox" }
            }),
            None,
        );
        write_turn(
            &root,
            "turn-00003",
            &json!({
                "tool": "browser_navigate",
                "timestamp": "2026-09-07T00:00:03Z",
                "arguments": { "url": "https://example.com" }
            }),
            None,
        );
        let events = events_from_dir(&root);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0]["t"], "click");
        assert_eq!(events[0]["el"]["name"], "Smoke Click");
        assert_eq!(events[0]["window"], "LazyBoy Cua Smoke");
        assert_eq!(events[1]["t"], "input");
        assert_eq!(events[1]["value"], "[已遮罩]");
        assert_eq!(events[2]["t"], "navigate");
        assert_eq!(events[2]["url"], "https://example.com");
    }

    #[test]
    fn secret_labels_are_detected() {
        assert!(looks_secret("Password"));
        assert!(looks_secret("確認密碼"));
        assert!(!looks_secret("Search"));
    }
}
