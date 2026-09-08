use lazyboy_contracts::{ComputerAction, PointerButton, PointerType, ScrollDirection};
use serde_json::{Value, json};

use crate::controller::ControlError;
use crate::{launch_argv_on, open_argv_on};

#[derive(Debug, Clone, PartialEq)]
pub enum TranslatedAction {
    Cua { tool: &'static str, payload: Value },
    Sleep { ms: u64 },
    Launch { argv: Vec<String> },
    FocusTitle { title: String },
}

pub fn translate_action(
    action: &ComputerAction,
    display: &str,
    profile: Option<&str>,
) -> Result<TranslatedAction, ControlError> {
    match action {
        ComputerAction::Wait { ms } => Ok(TranslatedAction::Sleep { ms: u64::from(*ms) }),
        ComputerAction::Open { path } => Ok(TranslatedAction::Launch {
            argv: open_argv_on(display, profile, path),
        }),
        ComputerAction::Launch { application, uri } => {
            let argv = launch_argv_on(display, profile, application, uri.as_deref())
                .ok_or(ControlError::Unsupported)?;
            Ok(TranslatedAction::Launch { argv })
        }
        ComputerAction::CopySelection | ComputerAction::Ref { .. } => {
            Err(ControlError::Unsupported)
        }
        ComputerAction::Focus { title } => Ok(TranslatedAction::FocusTitle {
            title: title.clone(),
        }),
        ComputerAction::Pointer {
            x,
            y,
            pointer_type,
            button,
        } => translate_pointer(*x, *y, pointer_type, *button),
        ComputerAction::Clipboard { text } => {
            tracing::info!(backend = "cua", tool = "type_text", length = text.len());
            Ok(TranslatedAction::Cua {
                tool: "type_text",
                payload: json!({
                    "text": text,
                    "scope": "desktop",
                }),
            })
        }
        ComputerAction::Key { key, modifiers } => Ok(translate_key(key, modifiers.as_deref())),
        ComputerAction::Scroll { direction, amount } => Ok(TranslatedAction::Cua {
            tool: "scroll",
            payload: json!({
                "direction": match direction {
                    ScrollDirection::Up => "up",
                    ScrollDirection::Down => "down",
                },
                "amount": amount.unwrap_or(12).clamp(1, 50),
                "by": "line",
                "scope": "desktop",
            }),
        }),
    }
}

fn translate_pointer(
    x: u32,
    y: u32,
    pointer_type: &PointerType,
    button: Option<PointerButton>,
) -> Result<TranslatedAction, ControlError> {
    let button = match button.unwrap_or(PointerButton::Left) {
        PointerButton::Left => "left",
        PointerButton::Middle => "middle",
        PointerButton::Right => "right",
    };
    match pointer_type {
        PointerType::Move => Ok(TranslatedAction::Cua {
            tool: "move_cursor",
            payload: json!({
                "x": x,
                "y": y,
                "scope": "desktop",
            }),
        }),
        PointerType::Click => Ok(TranslatedAction::Cua {
            tool: "click",
            payload: json!({
                "x": x,
                "y": y,
                "button": button,
                "scope": "desktop",
            }),
        }),
        // A held-button gesture collapses into one `drag` call before it gets
        // here; the CLI has no lease that keeps a button down between calls.
        PointerType::Down | PointerType::Up => Err(ControlError::Unsupported),
    }
}

fn translate_key(key: &str, modifiers: Option<&[String]>) -> TranslatedAction {
    // The public action DSL and mobile keyboard send chords as "ctrl+a".
    // Cua requires separate keys passed to hotkey, not a literal press_key.
    let mut keys: Vec<String> = modifiers
        .unwrap_or_default()
        .iter()
        .map(|item| map_key(item))
        .collect();
    if key.contains('+') && key.split('+').all(|part| !part.is_empty()) {
        keys.extend(key.split('+').map(map_key));
    } else {
        keys.push(map_key(key));
    }
    if keys.len() > 1 {
        TranslatedAction::Cua {
            tool: "hotkey",
            payload: json!({ "keys": keys, "scope": "desktop" }),
        }
    } else {
        TranslatedAction::Cua {
            tool: "press_key",
            payload: json!({ "key": keys[0], "scope": "desktop" }),
        }
    }
}

fn map_key(key: &str) -> String {
    match key.to_ascii_lowercase().as_str() {
        "enter" | "return" => "return".into(),
        "esc" | "escape" => "escape".into(),
        "cmd" | "command" | "super" | "meta" | "win" => "ctrl".into(),
        "control" | "ctl" => "ctrl".into(),
        "option" => "alt".into(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lazyboy_contracts::RefVerb;

    #[test]
    fn inline_shortcuts_use_hotkey_and_literal_plus_stays_a_key() {
        for (key, expected) in [
            ("ctrl+a", json!(["ctrl", "a"])),
            ("alt+Left", json!(["alt", "left"])),
            ("Control+Shift+Tab", json!(["ctrl", "shift", "tab"])),
        ] {
            let TranslatedAction::Cua { tool, payload } = translate_key(key, None) else {
                panic!("expected Cua")
            };
            assert_eq!(tool, "hotkey");
            assert_eq!(payload["keys"], expected);
        }
        let TranslatedAction::Cua { tool, payload } = translate_key("+", None) else {
            panic!("expected Cua")
        };
        assert_eq!(tool, "press_key");
        assert_eq!(payload["key"], "+");
    }

    #[test]
    fn pixel_click_is_desktop_cua_click() {
        let action = ComputerAction::Pointer {
            x: 40,
            y: 80,
            pointer_type: PointerType::Click,
            button: Some(PointerButton::Left),
        };
        let TranslatedAction::Cua { tool, payload } =
            translate_action(&action, ":1", None).unwrap()
        else {
            panic!("expected cua click");
        };
        assert_eq!(tool, "click");
        assert_eq!(payload["x"], 40);
        assert_eq!(payload["y"], 80);
        assert_eq!(payload["scope"], "desktop");
        // 0.23.2 rejects an explicit desktop target with invalid_action_target.
        assert!(payload.get("target").is_none());
    }

    #[test]
    fn desktop_actions_omit_the_rejected_target() {
        for action in [
            ComputerAction::Clipboard { text: "hi".into() },
            ComputerAction::Key {
                key: "return".into(),
                modifiers: None,
            },
            ComputerAction::Key {
                key: "c".into(),
                modifiers: Some(vec!["ctrl".into()]),
            },
            ComputerAction::Pointer {
                x: 1,
                y: 2,
                pointer_type: PointerType::Move,
                button: None,
            },
            ComputerAction::Scroll {
                direction: ScrollDirection::Down,
                amount: None,
            },
        ] {
            let TranslatedAction::Cua { payload, .. } =
                translate_action(&action, ":1", None).unwrap()
            else {
                panic!("every desktop action translates to a Cua tool");
            };
            assert!(payload.get("target").is_none(), "{action:?}");
        }
    }

    #[test]
    fn scroll_amount_stays_inside_the_driver_window() {
        let scroll = |amount| match translate_action(
            &ComputerAction::Scroll {
                direction: ScrollDirection::Down,
                amount: Some(amount),
            },
            ":1",
            None,
        )
        .unwrap()
        {
            TranslatedAction::Cua { payload, .. } => payload["amount"].as_i64().unwrap(),
            other => panic!("expected scroll, got {other:?}"),
        };
        assert_eq!(scroll(0), 1);
        assert_eq!(scroll(12), 12);
        assert_eq!(scroll(200), 50);
    }

    #[test]
    fn held_buttons_are_not_translated_one_by_one() {
        for pointer_type in [PointerType::Down, PointerType::Up] {
            let action = ComputerAction::Pointer {
                x: 1,
                y: 2,
                pointer_type,
                button: Some(PointerButton::Left),
            };
            assert!(matches!(
                translate_action(&action, ":1", None),
                Err(ControlError::Unsupported)
            ));
        }
    }

    #[test]
    fn typed_text_is_redacted_from_tool_name_only() {
        let action = ComputerAction::Clipboard {
            text: "secret-password".into(),
        };
        let TranslatedAction::Cua { tool, payload } =
            translate_action(&action, ":1", None).unwrap()
        else {
            panic!("expected type_text");
        };
        assert_eq!(tool, "type_text");
        assert_eq!(payload["text"], "secret-password");
        assert_eq!(payload["scope"], "desktop");
    }

    #[test]
    fn chord_uses_hotkey_and_maps_cmd_to_ctrl() {
        let action = ComputerAction::Key {
            key: "c".into(),
            modifiers: Some(vec!["cmd".into()]),
        };
        let TranslatedAction::Cua { tool, payload } =
            translate_action(&action, ":1", None).unwrap()
        else {
            panic!("expected hotkey");
        };
        assert_eq!(tool, "hotkey");
        assert_eq!(payload["keys"], json!(["ctrl", "c"]));
    }

    #[test]
    fn semantic_ref_is_not_translated_here() {
        let action = ComputerAction::Ref {
            verb: RefVerb::Click,
            target: "#go".into(),
            ref_kind: "dom".into(),
            text: None,
        };
        assert!(matches!(
            translate_action(&action, ":1", None),
            Err(ControlError::Unsupported)
        ));
    }
}
