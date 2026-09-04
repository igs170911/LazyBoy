use lazyboy_contracts::{ComputerAction, PointerButton, PointerType, ScrollDirection, UiElement};
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
}

/// Fill x/y from a numbered on-screen element so the model can click by id.
pub fn apply_element_targets(value: &mut Value, elements: &[UiElement]) -> Result<(), ActionError> {
    let Some(items) = value.as_array_mut() else {
        return Ok(());
    };
    for raw in items {
        let Some(action) = raw.as_object_mut() else {
            continue;
        };
        let Some(id) = action.get("element").and_then(Value::as_u64) else {
            continue;
        };
        let Some(element) = elements.iter().find(|element| u64::from(element.id) == id) else {
            return Err(ActionError::UnknownElement(id as u32));
        };
        let (x, y) = element.center();
        action.insert("x".into(), json!(x));
        action.insert("y".into(), json!(y));
    }
    Ok(())
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
                actions.push(ComputerAction::Clipboard { text });
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

fn coordinate(value: Option<&Value>, name: &'static str) -> Result<u32, ActionError> {
    let number = value.and_then(Value::as_f64).unwrap_or(f64::NAN).round();
    if !number.is_finite() || number < 0.0 || number > 100_000.0 {
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
}
