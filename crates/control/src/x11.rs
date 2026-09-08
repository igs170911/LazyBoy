use lazyboy_contracts::{ComputerAction, PointerType};

pub fn is_browser_title(title: &str) -> bool {
    let title = title.to_lowercase();
    title.contains("chromium") || title.contains("chrome")
}

use crate::screen::normalize_display;

pub const HOME: &str = "/home/lazyboy";

fn display_env(display: &str) -> String {
    format!("DISPLAY={}", normalize_display(display))
}

fn looks_like_typed_ascii(text: &str) -> bool {
    text.len() <= 48
        && text
            .chars()
            .all(|ch| ch.is_ascii() && (!ch.is_control() || ch == '\n' || ch == '\t'))
}

pub fn action_pause_ms(action: &ComputerAction) -> u64 {
    match action {
        ComputerAction::Pointer { pointer_type, .. } => match pointer_type {
            PointerType::Move => 18,
            PointerType::Click => 55,
            PointerType::Down | PointerType::Up => 30,
        },
        ComputerAction::Scroll { .. } => 45,
        ComputerAction::Clipboard { text } if looks_like_typed_ascii(text) => 40,
        ComputerAction::Clipboard { .. } | ComputerAction::Key { .. } => 35,
        ComputerAction::Focus { .. } => 90,
        ComputerAction::Open { .. } | ComputerAction::Launch { .. } => 220,
        ComputerAction::Wait { .. } | ComputerAction::CopySelection => 0,
        ComputerAction::Ref { .. } => 55,
    }
}

pub fn parse_ui_elements(raw: &str) -> Vec<lazyboy_contracts::UiElement> {
    let value: serde_json::Value =
        serde_json::from_str(raw.trim()).unwrap_or(serde_json::Value::Null);
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            Some(lazyboy_contracts::UiElement {
                id: item.get("id")?.as_u64()? as u32,
                title: item.get("title")?.as_str()?.to_string(),
                x: item.get("x")?.as_u64()? as u32,
                y: item.get("y")?.as_u64()? as u32,
                w: item.get("w")?.as_u64()? as u32,
                h: item.get("h")?.as_u64()? as u32,
                selector: item
                    .get("selector")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string),
                kind: item
                    .get("kind")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string),
                role: item
                    .get("role")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string),
            })
        })
        .collect()
}

pub fn open_argv_on(display: &str, profile: Option<&str>, path: &str) -> Vec<String> {
    if path.starts_with("http://") || path.starts_with("https://") {
        return browser_argv(display, profile, Some(path));
    }
    vec![
        "env".into(),
        display_env(display),
        "xdg-open".into(),
        path.into(),
    ]
}

pub fn launch_argv_on(
    display: &str,
    profile: Option<&str>,
    application: &str,
    uri: Option<&str>,
) -> Option<Vec<String>> {
    match application {
        "browser" | "chrome" | "chromium" | "lazyboy-browser" => {
            Some(browser_argv(display, profile, uri))
        }
        "xterm" | "terminal" | "xfce4-terminal" | "lazyboy-terminal" => {
            let mut argv = vec![
                "env".into(),
                display_env(display),
                "lazyboy-terminal".into(),
            ];
            if let Some(uri) = uri {
                argv.push(uri.into());
            }
            Some(argv)
        }
        _ => None,
    }
}

fn browser_argv(display: &str, profile: Option<&str>, uri: Option<&str>) -> Vec<String> {
    let mut argv = vec!["env".into(), display_env(display)];
    if let Some(profile) = profile.filter(|value| !value.is_empty()) {
        argv.push(format!("LAZYBOY_BROWSER_PROFILE={profile}"));
    }
    argv.push("lazyboy-browser".into());
    if let Some(uri) = uri {
        argv.push(uri.into());
    }
    argv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_window_list_elements() {
        let elements =
            parse_ui_elements(r#"[{"id":1,"title":"Chromium","x":0,"y":24,"w":1280,"h":776}]"#);
        assert_eq!(elements.len(), 1);
        assert_eq!(elements[0].title, "Chromium");
        assert_eq!(elements[0].center(), (640, 412));
    }

    #[test]
    fn parses_dom_element_selector() {
        let elements = parse_ui_elements(
            r#"[{"id":1,"title":"Submit","selector":"[data-lazyboy=\"1\"]","kind":"dom","x":10,"y":20,"w":80,"h":24}]"#,
        );
        assert_eq!(elements[0].kind.as_deref(), Some("dom"));
        assert_eq!(
            elements[0].selector.as_deref(),
            Some("[data-lazyboy=\"1\"]")
        );
    }

    #[test]
    fn parses_a11y_role_and_path() {
        let elements = parse_ui_elements(
            r#"[{"id":2,"title":"push button Open","selector":"0/3/1","kind":"a11y","role":"push button","x":8,"y":9,"w":40,"h":16}]"#,
        );
        assert_eq!(elements[0].kind.as_deref(), Some("a11y"));
        assert_eq!(elements[0].role.as_deref(), Some("push button"));
        assert_eq!(elements[0].selector.as_deref(), Some("0/3/1"));
        assert!(elements[0].has_ref());
    }

    #[test]
    fn browser_titles_match_chromium() {
        assert!(is_browser_title("Example - Chromium"));
        assert!(is_browser_title("chrome"));
        assert!(!is_browser_title("Thunar"));
    }

    #[test]
    fn empty_or_junk_window_list_is_empty() {
        assert!(parse_ui_elements("").is_empty());
        assert!(parse_ui_elements("not-json").is_empty());
    }
}
