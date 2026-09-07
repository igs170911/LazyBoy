use lazyboy_contracts::{ComputerAction, PointerButton, PointerType, ScrollDirection};
use serde_json::{Value, json};

use crate::controller::ControlError;
use crate::{launch_argv_on, open_argv_on};

#[derive(Debug, Clone, PartialEq)]
pub enum TranslatedAction {
    Cua { tool: &'static str, payload: Value },
    Sleep { ms: u64 },
    LegacyArgv { argv: Vec<String>, detached: bool },
    FocusTitle { title: String },
}

pub fn translate_action(
    action: &ComputerAction,
    display: &str,
    profile: Option<&str>,
) -> Result<TranslatedAction, ControlError> {
    match action {
        ComputerAction::Wait { ms } => Ok(TranslatedAction::Sleep { ms: u64::from(*ms) }),
        ComputerAction::Open { path } => Ok(TranslatedAction::LegacyArgv {
            argv: open_argv_on(display, profile, path),
            detached: true,
        }),
        ComputerAction::Launch { application, uri } => {
            let argv = launch_argv_on(display, profile, application, uri.as_deref())
                .ok_or(ControlError::Unsupported)?;
            Ok(TranslatedAction::LegacyArgv {
                argv,
                detached: true,
            })
        }
        ComputerAction::Ref { .. } => Err(ControlError::Unsupported),
        ComputerAction::Focus { title } => Ok(TranslatedAction::FocusTitle {
            title: title.clone(),
        }),
        ComputerAction::Pointer {
            x,
            y,
            pointer_type,
            button,
        } => Ok(translate_pointer(*x, *y, pointer_type, *button)),
        ComputerAction::Clipboard { text } => {
            tracing::info!(backend = "cua", tool = "type_text", length = text.len());
            Ok(TranslatedAction::Cua {
                tool: "type_text",
                payload: json!({
                    "text": text,
                    "scope": "desktop",
                    "target": { "kind": "desktop", "display_id": "primary" },
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
                "amount": amount.unwrap_or(12),
                "by": "line",
                "scope": "desktop",
                "target": { "kind": "desktop", "display_id": "primary" },
            }),
        }),
    }
}

fn translate_pointer(
    x: u32,
    y: u32,
    pointer_type: &PointerType,
    button: Option<PointerButton>,
) -> TranslatedAction {
    let button = match button.unwrap_or(PointerButton::Left) {
        PointerButton::Left => "left",
        PointerButton::Middle => "middle",
        PointerButton::Right => "right",
    };
    match pointer_type {
        PointerType::Move => TranslatedAction::Cua {
            tool: "move_cursor",
            payload: json!({
                "x": x,
                "y": y,
                "scope": "desktop",
                "target": { "kind": "desktop", "display_id": "primary" },
            }),
        },
        PointerType::Click => TranslatedAction::Cua {
            tool: "click",
            payload: json!({
                "x": x,
                "y": y,
                "button": button,
                "scope": "desktop",
                "target": { "kind": "desktop", "display_id": "primary" },
            }),
        },
        PointerType::Down => TranslatedAction::Cua {
            tool: "mouse_button_down",
            payload: json!({ "x": x, "y": y, "button": button }),
        },
        PointerType::Up => TranslatedAction::Cua {
            tool: "mouse_button_up",
            payload: json!({ "x": x, "y": y, "button": button }),
        },
    }
}

fn translate_key(key: &str, modifiers: Option<&[String]>) -> TranslatedAction {
    let key = map_key(key);
    match modifiers {
        Some(items) if !items.is_empty() => {
            let mut keys: Vec<String> = items.iter().map(|item| map_key(item)).collect();
            keys.push(key);
            TranslatedAction::Cua {
                tool: "hotkey",
                payload: json!({
                    "keys": keys,
                    "scope": "desktop",
                    "target": { "kind": "desktop", "display_id": "primary" },
                }),
            }
        }
        _ => TranslatedAction::Cua {
            tool: "press_key",
            payload: json!({
                "key": key,
                "scope": "desktop",
                "target": { "kind": "desktop", "display_id": "primary" },
            }),
        },
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
        assert_eq!(payload["target"]["display_id"], "primary");
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
