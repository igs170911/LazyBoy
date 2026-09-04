use lazyboy_contracts::UiElement;
use serde_json::{Value, json};

use crate::x11::parse_ui_elements;
use crate::{PRIMARY_DISPLAY, normalize_display};

const CDP_PY: &str = include_str!("cdp.py");

pub fn devtools_port(display: &str) -> u16 {
    let number = normalize_display(display)
        .trim_start_matches(':')
        .parse::<u16>()
        .unwrap_or(1)
        .max(1);
    9221 + number
}

pub fn cdp_command_on(display: &str, profile: Option<&str>, request: &Value) -> Vec<String> {
    let mut body = request.clone();
    if let Some(object) = body.as_object_mut() {
        object
            .entry("display")
            .or_insert_with(|| json!(normalize_display(display)));
        object
            .entry("port")
            .or_insert_with(|| json!(devtools_port(display)));
        if let Some(profile) = profile.filter(|value| !value.is_empty()) {
            object.entry("profile").or_insert_with(|| json!(profile));
        }
    }
    vec![
        "env".into(),
        format!("DISPLAY={}", normalize_display(display)),
        "python3".into(),
        "-c".into(),
        CDP_PY.into(),
        body.to_string(),
    ]
}

pub fn cdp_command(request: &Value) -> Vec<String> {
    cdp_command_on(PRIMARY_DISPLAY, None, request)
}

/// Marker embedded in the recorder's argv so `pkill -f` can find exactly one
/// teaching session without touching other python processes.
pub fn teach_recorder_tag(skill_id: &str) -> String {
    format!("lazyboy-teach-{skill_id}")
}

pub fn teach_recorder_output(skill_id: &str) -> String {
    format!("/tmp/{}.jsonl", teach_recorder_tag(skill_id))
}

/// Detached, long-running CDP recorder for a human demonstration. The script
/// is handed to `sh` as a positional argument so no shell quoting touches it.
pub fn cdp_record_command_on(display: &str, profile: Option<&str>, skill_id: &str) -> Vec<String> {
    let mut request = json!({
        "action": "record",
        "ensure": true,
        "out": teach_recorder_output(skill_id),
        "tag": teach_recorder_tag(skill_id),
        "display": normalize_display(display),
        "port": devtools_port(display),
    });
    if let Some(profile) = profile.filter(|value| !value.is_empty()) {
        request["profile"] = json!(profile);
    }
    vec![
        "sh".into(),
        "-c".into(),
        "setsid nohup env DISPLAY=\"$0\" python3 -c \"$1\" \"$2\" >/dev/null 2>&1 </dev/null &".into(),
        normalize_display(display).to_string(),
        CDP_PY.into(),
        request.to_string(),
    ]
}

pub fn cdp_record_stop_command(skill_id: &str) -> Vec<String> {
    vec!["pkill".into(), "-f".into(), teach_recorder_tag(skill_id)]
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CdpPage {
    pub ok: bool,
    pub error: Option<String>,
    pub url: String,
    pub title: String,
    pub text: String,
    pub restarted: bool,
    pub elements: Vec<UiElement>,
}

pub fn parse_cdp_page(raw: &str) -> CdpPage {
    let value: Value = serde_json::from_str(raw.trim()).unwrap_or(Value::Null);
    if value.get("ok").and_then(Value::as_bool) != Some(true) {
        return CdpPage {
            ok: false,
            error: value
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| Some("cdp unavailable".into())),
            ..CdpPage::default()
        };
    }
    CdpPage {
        ok: true,
        error: None,
        url: value
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        title: value
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        text: value
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        restarted: value.get("restarted").and_then(Value::as_bool) == Some(true),
        elements: value
            .get("elements")
            .map(|items| parse_ui_elements(&items.to_string()))
            .unwrap_or_default(),
    }
}

pub fn merge_page_elements(windows: Vec<UiElement>, page: &[UiElement]) -> Vec<UiElement> {
    if page.is_empty() {
        return windows;
    }
    let mut elements = page.to_vec();
    let mut next = elements.len() as u32 + 1;
    for mut window in windows {
        let title = window.title.to_lowercase();
        if title.contains("chromium") || title.contains("chrome") {
            continue;
        }
        window.id = next;
        next += 1;
        elements.push(window);
    }
    elements
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_port_follows_the_display() {
        assert_eq!(devtools_port(":1"), 9222);
        assert_eq!(devtools_port(":2"), 9223);
        assert_eq!(devtools_port("3"), 9224);
    }

    #[test]
    fn command_passes_port_profile_and_script() {
        let argv = cdp_command_on(
            ":2",
            Some("/home/lazyboy/.browser-profiles/bots/a"),
            &json!({"action": "snapshot", "ensure": true}),
        );
        assert!(argv.contains(&"DISPLAY=:2".into()));
        assert!(argv.iter().any(|item| item.contains("python3")));
        let payload = argv.last().unwrap();
        assert!(payload.contains("\"port\":9223"));
        assert!(payload.contains("bots/a"));
        assert!(payload.contains("snapshot"));
    }

    #[test]
    fn parses_snapshot_elements() {
        let page = parse_cdp_page(
            r#"{"ok":true,"url":"https://example.com","title":"Example","text":"Hello","elements":[{"id":1,"title":"Submit","selector":"[data-lazyboy=\"1\"]","kind":"dom","x":10,"y":20,"w":80,"h":24}]}"#,
        );
        assert!(page.ok);
        assert_eq!(page.url, "https://example.com");
        assert_eq!(page.elements.len(), 1);
        assert_eq!(page.elements[0].title, "Submit");
        assert_eq!(
            page.elements[0].selector.as_deref(),
            Some("[data-lazyboy=\"1\"]")
        );
        assert_eq!(page.elements[0].center(), (50, 32));
    }

    #[test]
    fn merge_keeps_page_controls_and_native_dialogs() {
        let windows = vec![
            UiElement {
                id: 1,
                title: "Chromium".into(),
                x: 0,
                y: 0,
                w: 1280,
                h: 800,
                kind: Some("window".into()),
                ..UiElement::default()
            },
            UiElement {
                id: 2,
                title: "Open File".into(),
                x: 100,
                y: 100,
                w: 400,
                h: 300,
                kind: Some("window".into()),
                ..UiElement::default()
            },
        ];
        let page = vec![UiElement {
            id: 1,
            title: "Login".into(),
            selector: Some("[data-lazyboy=\"1\"]".into()),
            kind: Some("dom".into()),
            x: 40,
            y: 80,
            w: 60,
            h: 20,
            ..UiElement::default()
        }];
        let merged = merge_page_elements(windows, &page);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].title, "Login");
        assert_eq!(merged[1].id, 2);
        assert_eq!(merged[1].title, "Open File");
    }

    #[test]
    fn unavailable_page_is_not_ok() {
        let page = parse_cdp_page(r#"{"ok":false,"error":"cdp unavailable"}"#);
        assert!(!page.ok);
        assert_eq!(page.error.as_deref(), Some("cdp unavailable"));
    }
}
