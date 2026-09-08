use lazyboy_contracts::UiElement;
use serde_json::{Value, json};

use crate::normalize_display;
use crate::x11::{is_browser_title, parse_ui_elements};

const A11Y_PY: &str = include_str!("a11y.py");

#[derive(Debug, Clone, PartialEq, Default)]
pub struct A11yPage {
    pub ok: bool,
    pub error: Option<String>,
    pub elements: Vec<UiElement>,
}

pub fn a11y_command_on(display: &str, request: &Value) -> Vec<String> {
    let mut body = request.clone();
    if let Some(object) = body.as_object_mut() {
        object
            .entry("display")
            .or_insert_with(|| json!(normalize_display(display)));
    }
    vec![
        "env".into(),
        format!("DISPLAY={}", normalize_display(display)),
        "python3".into(),
        "-c".into(),
        A11Y_PY.into(),
        body.to_string(),
    ]
}

pub fn parse_a11y_page(raw: &str) -> A11yPage {
    let value: Value = serde_json::from_str(raw.trim()).unwrap_or(Value::Null);
    let ok = value.get("ok").and_then(Value::as_bool) == Some(true);
    A11yPage {
        ok,
        error: if ok {
            None
        } else {
            value
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| Some("atspi unavailable".into()))
        },
        elements: value
            .get("elements")
            .map(|items| parse_ui_elements(&items.to_string()))
            .unwrap_or_default(),
    }
}

/// Merge DOM (keep ids), then a11y, then native windows that are not already
/// covered by a control tree. Chromium's whole window is dropped when the
/// page snapshot returned elements.
pub fn merge_ui_elements(
    windows: Vec<UiElement>,
    page: &[UiElement],
    a11y: &[UiElement],
) -> Vec<UiElement> {
    let mut elements = page.to_vec();
    let mut next = elements.len() as u32 + 1;
    for mut control in a11y.iter().cloned() {
        control.id = next;
        next += 1;
        elements.push(control);
    }
    for mut window in windows {
        if !page.is_empty() && is_browser_title(&window.title) {
            continue;
        }
        if a11y_covers_window(a11y, &window) {
            continue;
        }
        window.id = next;
        next += 1;
        elements.push(window);
    }
    elements
}

fn a11y_covers_window(a11y: &[UiElement], window: &UiElement) -> bool {
    if a11y.is_empty() || window.w == 0 || window.h == 0 {
        return false;
    }
    a11y.iter().any(|control| center_inside(control, window))
}

fn center_inside(element: &UiElement, window: &UiElement) -> bool {
    let (x, y) = element.center();
    let area = u64::from(element.w.saturating_mul(element.h.max(1)));
    let window_area = u64::from(window.w.saturating_mul(window.h.max(1)));
    x >= window.x
        && y >= window.y
        && x < window.x.saturating_add(window.w)
        && y < window.y.saturating_add(window.h)
        && (window_area == 0 || area < window_area)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(id: u32, title: &str, x: u32, y: u32, w: u32, h: u32) -> UiElement {
        UiElement {
            id,
            title: title.into(),
            x,
            y,
            w,
            h,
            kind: Some("window".into()),
            ..UiElement::default()
        }
    }

    fn a11y(id: u32, title: &str, x: u32, y: u32, w: u32, h: u32) -> UiElement {
        UiElement {
            id,
            title: title.into(),
            x,
            y,
            w,
            h,
            kind: Some("a11y".into()),
            selector: Some(format!("0/{id}")),
            role: Some("push button".into()),
        }
    }

    #[test]
    fn command_injects_display_and_script() {
        let argv = a11y_command_on(":2", &json!({"action": "snapshot"}));
        assert!(argv.contains(&"DISPLAY=:2".into()));
        assert!(argv.iter().any(|item| item.contains("python3")));
        assert!(argv.last().unwrap().contains("\"display\":\":2\""));
        assert!(
            argv.iter()
                .any(|item| item.contains("Atspi") || item.contains("atspi"))
        );
    }

    #[test]
    fn parses_snapshot_elements() {
        let page = parse_a11y_page(
            r#"{"ok":true,"elements":[{"id":1,"title":"push button Open","role":"push button","kind":"a11y","selector":"0/2/1","x":10,"y":20,"w":80,"h":24}]}"#,
        );
        assert!(page.ok);
        assert_eq!(page.elements.len(), 1);
        assert_eq!(page.elements[0].kind.as_deref(), Some("a11y"));
        assert_eq!(page.elements[0].selector.as_deref(), Some("0/2/1"));
        assert_eq!(page.elements[0].role.as_deref(), Some("push button"));
    }

    #[test]
    fn merge_keeps_dom_ids_and_replaces_covered_windows() {
        let windows = vec![
            window(1, "Thunar", 0, 24, 800, 600),
            window(2, "Terminal", 800, 24, 480, 600),
            window(3, "Chromium", 0, 0, 1280, 800),
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
        let native = vec![a11y(1, "push button Open", 40, 40, 80, 24)];
        let merged = merge_ui_elements(windows, &page, &native);
        assert_eq!(merged[0].title, "Login");
        assert_eq!(merged[0].id, 1);
        assert_eq!(merged[1].id, 2);
        assert_eq!(merged[1].title, "push button Open");
        assert!(merged.iter().any(|element| element.title == "Terminal"));
        assert!(!merged.iter().any(|element| element.title == "Thunar"));
        assert!(!merged.iter().any(|element| element.title == "Chromium"));
    }

    #[test]
    fn merge_without_trees_keeps_windows() {
        let windows = vec![window(1, "Thunar", 0, 24, 800, 600)];
        let merged = merge_ui_elements(windows, &[], &[]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].title, "Thunar");
    }

    #[test]
    fn unavailable_tree_is_not_ok() {
        let page = parse_a11y_page(r#"{"ok":false,"error":"atspi unavailable"}"#);
        assert!(!page.ok);
        assert_eq!(page.error.as_deref(), Some("atspi unavailable"));
    }
}
