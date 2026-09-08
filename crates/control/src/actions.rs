use lazyboy_contracts::{
    ComputerAction, PointerButton, PointerType, RefVerb, ScrollDirection, UiElement,
};
use serde_json::{Value, json};
use thiserror::Error;

pub const MAX_COMPUTER_ACTIONS: usize = 24;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ActionError {
    #[error("computer_act requires at least one action")]
    Empty,
    #[error("computer_act accepts at most {MAX_COMPUTER_ACTIONS} actions")]
    TooMany,
    #[error("computer_act expands to more than {MAX_COMPUTER_ACTIONS} actions; split the batch")]
    ExpandedTooMany,
    #[error("computer action must be an object")]
    NotObject,
    #[error("unsupported computer action {0}")]
    Unsupported(String),
    #[error("computer action {0} must be a non-negative coordinate")]
    BadCoordinate(&'static str),
    #[error("computer action element {0} is not on the current screen")]
    UnknownElement(u32),
    #[error(
        "element {0} is outside the browser viewport, so it has no screen position. Use browser {{\"action\":\"click\",\"element\":{0}}} (it scrolls into view) or scroll the page first."
    )]
    OffscreenElement(u32),
}

/// Models write element ids as `3`, `3.0`, `"3"` or `"[3]"`; accept them all
/// instead of failing the click.
pub fn element_id(value: Option<&Value>) -> Option<u64> {
    match value? {
        Value::Number(number) => number.as_u64().or_else(|| {
            number
                .as_f64()
                .filter(|f| *f >= 0.0)
                .map(|f| f.round() as u64)
        }),
        Value::String(text) => text
            .trim()
            .trim_start_matches(['#', '['])
            .trim_end_matches(']')
            .parse()
            .ok(),
        _ => None,
    }
}

/// Resolve a numbered on-screen element. DOM/a11y refs stay semantic (no
/// pre-filled coordinates). Window-level targets still become center pixels.
pub fn apply_element_targets(value: &mut Value, elements: &[UiElement]) -> Result<(), ActionError> {
    let Some(items) = value.as_array_mut() else {
        return Ok(());
    };
    for raw in items {
        let Some(action) = raw.as_object_mut() else {
            continue;
        };
        let Some(id) = element_id(action.get("element")) else {
            continue;
        };
        let Some(element) = elements.iter().find(|element| u64::from(element.id) == id) else {
            return Err(ActionError::UnknownElement(id as u32));
        };
        let kind = action
            .get("kind")
            .and_then(Value::as_str)
            .or_else(|| action.get("type").and_then(Value::as_str))
            .unwrap_or("");
        if element.has_ref() && matches!(kind, "click" | "type") {
            if let Some(selector) = element.selector.clone() {
                action.insert("target".into(), json!(selector));
                action.insert(
                    "refKind".into(),
                    json!(element.kind.clone().unwrap_or_else(|| "a11y".into())),
                );
            }
            continue;
        }
        if element.is_offscreen() {
            return Err(ActionError::OffscreenElement(id as u32));
        }
        let (x, y) = element.center();
        action.insert("x".into(), json!(x));
        action.insert("y".into(), json!(y));
    }
    Ok(())
}

/// Fingerprint of the first click-like action, used to refuse repeating a miss.
pub fn click_fingerprint(actions: &Value) -> Option<String> {
    let items = actions.as_array()?;
    for raw in items {
        let Some(action) = raw.as_object() else {
            continue;
        };
        let kind = action
            .get("kind")
            .and_then(Value::as_str)
            .or_else(|| action.get("type").and_then(Value::as_str))
            .unwrap_or("");
        if !matches!(kind, "click" | "down" | "drag") {
            continue;
        }
        if let Some(id) = element_id(action.get("element")) {
            return Some(format!("e{id}"));
        }
        match (
            action.get("x").and_then(Value::as_f64),
            action.get("y").and_then(Value::as_f64),
        ) {
            (Some(x), Some(y)) if x.is_finite() && y.is_finite() => {
                return Some(format!("p{},{}", x.round() as i64, y.round() as i64));
            }
            _ => {}
        }
    }
    None
}

pub fn should_block_stale_click(miss_streak: u32, last: Option<&str>, next: Option<&str>) -> bool {
    miss_streak >= 2 && next.is_some() && next == last
}

pub fn parse_computer_actions(value: &Value) -> Result<Vec<ComputerAction>, ActionError> {
    let Value::Array(items) = value else {
        return Err(ActionError::Empty);
    };
    if items.is_empty() {
        return Err(ActionError::Empty);
    }
    if items.len() > MAX_COMPUTER_ACTIONS {
        return Err(ActionError::TooMany);
    }
    let mut actions = Vec::new();
    for raw in items {
        let Value::Object(action) = raw else {
            return Err(ActionError::NotObject);
        };
        let kind = action
            .get("kind")
            .and_then(Value::as_str)
            .or_else(|| action.get("type").and_then(Value::as_str))
            .unwrap_or_default();
        match kind {
            "click" | "move" | "down" | "up" => {
                if kind == "click"
                    && let Some(target) = ref_target(action)
                {
                    let pointer = ComputerAction::Ref {
                        verb: RefVerb::Click,
                        target,
                        ref_kind: ref_kind(action),
                        text: None,
                    };
                    let doubled = action.get("double").and_then(Value::as_bool) == Some(true);
                    actions.push(pointer.clone());
                    if doubled {
                        actions.push(ComputerAction::Wait { ms: 70 });
                        actions.push(pointer);
                    }
                    continue;
                }
                let x = coordinate(action.get("x"), "x")?;
                let y = coordinate(action.get("y"), "y")?;
                let pointer_type = match kind {
                    "click" => PointerType::Click,
                    "move" => PointerType::Move,
                    "down" => PointerType::Down,
                    _ => PointerType::Up,
                };
                let button = match action.get("button").and_then(Value::as_str) {
                    Some("right") => PointerButton::Right,
                    Some("middle") => PointerButton::Middle,
                    _ => PointerButton::Left,
                };
                let pointer = ComputerAction::Pointer {
                    x,
                    y,
                    pointer_type,
                    button: Some(button),
                };
                let doubled =
                    action.get("double").and_then(Value::as_bool) == Some(true) && kind == "click";
                actions.push(pointer.clone());
                if doubled {
                    actions.push(ComputerAction::Wait { ms: 70 });
                    actions.push(pointer);
                }
            }
            "hover" => {
                let x = coordinate(action.get("x"), "x")?;
                let y = coordinate(action.get("y"), "y")?;
                actions.push(ComputerAction::Pointer {
                    x,
                    y,
                    pointer_type: PointerType::Move,
                    button: Some(PointerButton::Left),
                });
                actions.push(ComputerAction::Wait { ms: 90 });
            }
            "drag" => {
                let x = coordinate(action.get("x"), "x")?;
                let y = coordinate(action.get("y"), "y")?;
                let to_x = coordinate(
                    action
                        .get("x2")
                        .or_else(|| action.get("toX"))
                        .or_else(|| action.get("to_x")),
                    "x2",
                )?;
                let to_y = coordinate(
                    action
                        .get("y2")
                        .or_else(|| action.get("toY"))
                        .or_else(|| action.get("to_y")),
                    "y2",
                )?;
                let button = if action.get("button").and_then(Value::as_str) == Some("right") {
                    PointerButton::Right
                } else {
                    PointerButton::Left
                };
                actions.push(ComputerAction::Pointer {
                    x,
                    y,
                    pointer_type: PointerType::Down,
                    button: Some(button),
                });
                actions.push(ComputerAction::Wait { ms: 40 });
                actions.push(ComputerAction::Pointer {
                    x: to_x,
                    y: to_y,
                    pointer_type: PointerType::Move,
                    button: Some(button),
                });
                actions.push(ComputerAction::Wait { ms: 40 });
                actions.push(ComputerAction::Pointer {
                    x: to_x,
                    y: to_y,
                    pointer_type: PointerType::Up,
                    button: Some(button),
                });
            }
            "type" => {
                let text = action
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if let Some(target) = ref_target(action) {
                    actions.push(ComputerAction::Ref {
                        verb: RefVerb::SetValue,
                        target,
                        ref_kind: ref_kind(action),
                        text: Some(text),
                    });
                } else {
                    actions.push(ComputerAction::Clipboard { text });
                }
            }
            "key" => {
                let key = action
                    .get("key")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let modifiers = action
                    .get("modifiers")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    });
                actions.push(ComputerAction::Key { key, modifiers });
            }
            "scroll" => {
                if action.get("x").is_some() || action.get("y").is_some() {
                    let x = coordinate(action.get("x"), "x")?;
                    let y = coordinate(action.get("y"), "y")?;
                    actions.push(ComputerAction::Pointer {
                        x,
                        y,
                        pointer_type: PointerType::Move,
                        button: Some(PointerButton::Left),
                    });
                }
                let direction = if action.get("direction").and_then(Value::as_str) == Some("up") {
                    ScrollDirection::Up
                } else {
                    ScrollDirection::Down
                };
                actions.push(ComputerAction::Scroll {
                    direction,
                    amount: Some(bounded(action.get("amount"), 1, 40, 12)),
                });
            }
            "focus" => {
                let title = action
                    .get("title")
                    .or_else(|| action.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                actions.push(ComputerAction::Focus { title });
            }
            "wait" => {
                actions.push(ComputerAction::Wait {
                    ms: bounded(action.get("ms"), 0, 1_200, 80),
                });
            }
            other => {
                return Err(ActionError::Unsupported(if other.is_empty() {
                    "(missing)".to_string()
                } else {
                    other.to_string()
                }));
            }
        }
    }
    if actions.len() > MAX_COMPUTER_ACTIONS {
        return Err(ActionError::ExpandedTooMany);
    }
    Ok(actions)
}

fn ref_target(action: &serde_json::Map<String, Value>) -> Option<String> {
    action
        .get("target")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn ref_kind(action: &serde_json::Map<String, Value>) -> String {
    action
        .get("refKind")
        .or_else(|| action.get("ref_kind"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("a11y")
        .to_string()
}

fn coordinate(value: Option<&Value>, name: &'static str) -> Result<u32, ActionError> {
    let number = value.and_then(Value::as_f64).unwrap_or(f64::NAN).round();
    if !number.is_finite() || !(0.0..=100_000.0).contains(&number) {
        return Err(ActionError::BadCoordinate(name));
    }
    Ok(number as u32)
}

fn bounded(value: Option<&Value>, min: u32, max: u32, fallback: u32) -> u32 {
    let Some(number) = value.and_then(Value::as_f64) else {
        return fallback;
    };
    if !number.is_finite() {
        return fallback;
    }
    (number.round() as i64).clamp(min as i64, max as i64) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn element_ids_accept_common_model_spellings() {
        assert_eq!(element_id(Some(&json!(3))), Some(3));
        assert_eq!(element_id(Some(&json!(3.0))), Some(3));
        assert_eq!(element_id(Some(&json!("3"))), Some(3));
        assert_eq!(element_id(Some(&json!("#12"))), Some(12));
        assert_eq!(element_id(Some(&json!("[7]"))), Some(7));
        assert_eq!(element_id(Some(&json!("Learn more"))), None);
        assert_eq!(element_id(Some(&json!(-1))), None);
        assert_eq!(element_id(None), None);
    }

    #[test]
    fn parses_click_and_double_click() {
        let actions = parse_computer_actions(&json!([
            {"kind": "click", "x": 10, "y": 20},
            {"kind": "click", "x": 10, "y": 20, "double": true}
        ]))
        .unwrap();
        assert_eq!(actions.len(), 4);
    }

    #[test]
    fn accepts_type_as_kind_alias() {
        let actions = parse_computer_actions(&json!([{"type": "click", "x": 4, "y": 8}])).unwrap();
        assert_eq!(actions.len(), 1);
    }

    #[test]
    fn drag_expands_to_down_move_up() {
        let actions = parse_computer_actions(&json!([
            {"kind": "drag", "x": 10, "y": 10, "x2": 80, "y2": 90}
        ]))
        .unwrap();
        assert_eq!(actions.len(), 5);
        assert!(matches!(
            actions[0],
            ComputerAction::Pointer {
                pointer_type: PointerType::Down,
                ..
            }
        ));
        assert!(matches!(
            actions[4],
            ComputerAction::Pointer {
                pointer_type: PointerType::Up,
                x: 80,
                y: 90,
                ..
            }
        ));
    }

    #[test]
    fn scroll_defaults_to_a_real_page_chunk() {
        let actions =
            parse_computer_actions(&json!([{"kind": "scroll", "direction": "down"}])).unwrap();
        match &actions[0] {
            ComputerAction::Scroll { amount, .. } => assert_eq!(*amount, Some(12)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn rejects_empty_and_overflow() {
        assert_eq!(
            parse_computer_actions(&json!([])).unwrap_err(),
            ActionError::Empty
        );
        let too_many: Vec<_> = (0..25).map(|_| json!({"kind": "wait", "ms": 1})).collect();
        assert_eq!(
            parse_computer_actions(&json!(too_many)).unwrap_err(),
            ActionError::TooMany
        );
    }

    #[test]
    fn element_id_fills_click_coordinates() {
        let mut actions = json!([{"kind": "click", "element": 1}]);
        let elements = vec![UiElement {
            id: 1,
            title: "Terminal".into(),
            x: 10,
            y: 20,
            w: 100,
            h: 40,
            ..UiElement::default()
        }];
        apply_element_targets(&mut actions, &elements).unwrap();
        assert_eq!(actions[0]["x"], 60);
        assert_eq!(actions[0]["y"], 40);
        let parsed = parse_computer_actions(&actions).unwrap();
        assert!(matches!(
            parsed[0],
            ComputerAction::Pointer { x: 60, y: 40, .. }
        ));
    }

    #[test]
    fn unknown_element_errors() {
        let mut actions = json!([{"kind": "click", "element": 9}]);
        assert_eq!(
            apply_element_targets(&mut actions, &[]).unwrap_err(),
            ActionError::UnknownElement(9)
        );
    }

    #[test]
    fn offscreen_page_elements_cannot_be_pixel_clicked() {
        let below_fold = UiElement {
            id: 3,
            title: "Next [below viewport]".into(),
            selector: Some("[data-lazyboy=\"3\"]".into()),
            kind: Some("dom".into()),
            ..UiElement::default()
        };
        let mut actions = json!([{"kind": "hover", "element": 3}]);
        assert_eq!(
            apply_element_targets(&mut actions, &[below_fold]).unwrap_err(),
            ActionError::OffscreenElement(3)
        );
    }

    fn a11y_button() -> UiElement {
        UiElement {
            id: 4,
            title: "push button Open".into(),
            selector: Some("0/2/1".into()),
            kind: Some("a11y".into()),
            role: Some("push button".into()),
            x: 10,
            y: 20,
            w: 80,
            h: 24,
        }
    }

    #[test]
    fn a11y_click_stays_semantic() {
        let mut actions = json!([{"kind": "click", "element": 4}]);
        apply_element_targets(&mut actions, &[a11y_button()]).unwrap();
        assert!(actions[0].get("x").is_none());
        assert_eq!(actions[0]["target"], "0/2/1");
        assert_eq!(actions[0]["refKind"], "a11y");
        let parsed = parse_computer_actions(&actions).unwrap();
        assert!(matches!(
            parsed[0],
            ComputerAction::Ref {
                verb: RefVerb::Click,
                ..
            }
        ));
    }

    #[test]
    fn a11y_type_is_set_value() {
        let mut actions = json!([{"kind": "type", "element": 4, "text": "report.pdf"}]);
        apply_element_targets(&mut actions, &[a11y_button()]).unwrap();
        let parsed = parse_computer_actions(&actions).unwrap();
        match &parsed[0] {
            ComputerAction::Ref {
                verb: RefVerb::SetValue,
                text,
                ..
            } => assert_eq!(text.as_deref(), Some("report.pdf")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn dom_click_does_not_fill_coordinates() {
        let mut actions = json!([{"kind": "click", "element": 1}]);
        let elements = vec![UiElement {
            id: 1,
            title: "Submit".into(),
            selector: Some("[data-lazyboy=\"1\"]".into()),
            kind: Some("dom".into()),
            x: 10,
            y: 20,
            w: 80,
            h: 24,
            ..UiElement::default()
        }];
        apply_element_targets(&mut actions, &elements).unwrap();
        assert!(actions[0].get("x").is_none());
        let parsed = parse_computer_actions(&actions).unwrap();
        match &parsed[0] {
            ComputerAction::Ref { ref_kind, verb, .. } => {
                assert_eq!(ref_kind, "dom");
                assert_eq!(*verb, RefVerb::Click);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn stale_repeat_blocks_the_third_same_click() {
        assert!(!should_block_stale_click(1, Some("e3"), Some("e3")));
        assert!(should_block_stale_click(2, Some("e3"), Some("e3")));
        assert!(!should_block_stale_click(2, Some("e3"), Some("e4")));
        assert_eq!(
            click_fingerprint(&json!([{"kind":"click","element":3}])).as_deref(),
            Some("e3")
        );
        assert_eq!(
            click_fingerprint(&json!([{"kind":"click","x":10,"y":20}])).as_deref(),
            Some("p10,20")
        );
    }
}
