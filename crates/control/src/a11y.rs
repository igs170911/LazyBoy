use crate::is_browser_title;
use lazyboy_contracts::UiElement;

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
}
