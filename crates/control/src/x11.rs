use lazyboy_contracts::{ComputerAction, PointerButton, PointerType, ScrollDirection};

use crate::screen::{normalize_display, PRIMARY_DISPLAY};

pub const DISPLAY: &str = PRIMARY_DISPLAY;
pub const HOME: &str = "/home/lazyboy";

fn display_env(display: &str) -> String {
    format!("DISPLAY={}", normalize_display(display))
}

pub fn xdotool_argv(action: &ComputerAction) -> Option<Vec<String>> {
    xdotool_argv_on(PRIMARY_DISPLAY, action)
}

pub fn xdotool_argv_on(display: &str, action: &ComputerAction) -> Option<Vec<String>> {
    let mut argv = vec!["env".into(), display_env(display), "xdotool".into()];
    match action {
        ComputerAction::Pointer {
            x,
            y,
            pointer_type,
            button,
        } => {
            let button_n = match button.unwrap_or(PointerButton::Left) {
                PointerButton::Left => "1",
                PointerButton::Middle => "2",
                PointerButton::Right => "3",
            };
            match pointer_type {
                PointerType::Move => {
                    argv.extend([
                        "mousemove".into(),
                        "--sync".into(),
                        "--".into(),
                        x.to_string(),
                        y.to_string(),
                    ]);
                }
                PointerType::Click => {
                    argv.extend([
                        "mousemove".into(),
                        "--sync".into(),
                        "--".into(),
                        x.to_string(),
                        y.to_string(),
                        "click".into(),
                        "--delay".into(),
                        "40".into(),
                        button_n.into(),
                    ]);
                }
                PointerType::Down => {
                    argv.extend([
                        "mousemove".into(),
                        "--".into(),
                        x.to_string(),
                        y.to_string(),
                        "mousedown".into(),
                        button_n.into(),
                    ]);
                }
                PointerType::Up => {
                    argv.extend([
                        "mousemove".into(),
                        "--".into(),
                        x.to_string(),
                        y.to_string(),
                        "mouseup".into(),
                        button_n.into(),
                    ]);
                }
            }
        }
        ComputerAction::Key { key, modifiers } => {
            let combo = match modifiers {
                Some(items) if !items.is_empty() => format!("{}+{key}", items.join("+")),
                _ => key.clone(),
            };
            argv.extend(["key".into(), "--clearmodifiers".into(), combo]);
        }
        ComputerAction::Clipboard { text } => {
            let quoted = shell_single_quote(text);
            if looks_like_typed_ascii(text) {
                argv.extend(["type".into(), "--delay".into(), "16".into(), "--".into(), text.clone()]);
            } else {
                return Some(vec![
                    "env".into(),
                    display_env(display),
                    "bash".into(),
                    "-lc".into(),
                    format!(
                        "printf %s {quoted} | xclip -selection clipboard && xdotool key --clearmodifiers ctrl+v"
                    ),
                ]);
            }
        }
        ComputerAction::Scroll { direction, amount } => {
            let button = match direction {
                ScrollDirection::Up => "4",
                ScrollDirection::Down => "5",
            };
            argv.extend([
                "click".into(),
                "--repeat".into(),
                amount.unwrap_or(12).to_string(),
                "--delay".into(),
                "15".into(),
                button.into(),
            ]);
        }
        ComputerAction::Focus { title } => {
            let quoted = shell_single_quote(title);
            return Some(vec![
                "env".into(),
                display_env(display),
                "bash".into(),
                "-lc".into(),
                format!(
                    "wmctrl -a {quoted} || xdotool search --name {quoted} windowactivate --sync windowfocus"
                ),
            ]);
        }
        ComputerAction::Wait { .. } | ComputerAction::Open { .. } | ComputerAction::Launch { .. } => {
            return None;
        }
    }
    Some(argv)
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn looks_like_typed_ascii(text: &str) -> bool {
    text.len() <= 48
        && text.chars().all(|ch| ch.is_ascii() && (!ch.is_control() || ch == '\n' || ch == '\t'))
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
        ComputerAction::Wait { .. } => 0,
    }
}

pub fn pointer_state_command() -> Vec<String> {
    pointer_state_command_on(PRIMARY_DISPLAY)
}

pub fn pointer_state_command_on(display: &str) -> Vec<String> {
    vec![
        "env".into(),
        display_env(display),
        "python3".into(),
        "-c".into(),
        r#"
import json, subprocess
def out(args):
    try:
        return subprocess.check_output(args, stderr=subprocess.DEVNULL, text=True).strip()
    except Exception:
        return ""
vals = {}
for line in out(["xdotool", "getmouselocation", "--shell"]).splitlines():
    if "=" in line:
        key, value = line.split("=", 1)
        vals[key] = value
wid = out(["xdotool", "getactivewindow"])
title = out(["xdotool", "getwindowname", wid]) if wid else ""
print(json.dumps({"x": int(vals.get("X") or 0), "y": int(vals.get("Y") or 0), "id": wid, "title": title}))
"#
        .into(),
    ]
}

pub fn parse_pointer_state(raw: &str) -> (Option<lazyboy_contracts::CursorPosition>, Option<lazyboy_contracts::ActiveWindow>) {
    let value: serde_json::Value = serde_json::from_str(raw.trim()).unwrap_or(serde_json::Value::Null);
    let cursor = match (value.get("x").and_then(serde_json::Value::as_i64), value.get("y").and_then(serde_json::Value::as_i64)) {
        (Some(x), Some(y)) => Some(lazyboy_contracts::CursorPosition {
            x: x as i32,
            y: y as i32,
        }),
        _ => None,
    };
    let window = value.get("id").and_then(serde_json::Value::as_str).filter(|id| !id.is_empty()).map(|id| {
        lazyboy_contracts::ActiveWindow {
            id: id.to_string(),
            title: value
                .get("title")
                .and_then(serde_json::Value::as_str)
                .filter(|title| !title.is_empty())
                .map(str::to_string),
        }
    });
    (cursor, window)
}

pub fn open_argv(path: &str) -> Vec<String> {
    open_argv_on(PRIMARY_DISPLAY, None, path)
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

pub fn launch_argv(application: &str, uri: Option<&str>) -> Option<Vec<String>> {
    launch_argv_on(PRIMARY_DISPLAY, None, application, uri)
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
            let mut argv = vec!["env".into(), display_env(display), "lazyboy-terminal".into()];
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

pub fn screenshot_command() -> Vec<String> {
    screenshot_command_on(PRIMARY_DISPLAY)
}

pub fn screenshot_command_on(display: &str) -> Vec<String> {
    vec![
        "bash".into(),
        "-lc".into(),
        format!(
            "DISPLAY={} xwd -root -silent | convert xwd:- png:-",
            normalize_display(display)
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_maps_to_xdotool() {
        let argv = xdotool_argv(&ComputerAction::Pointer {
            x: 12,
            y: 40,
            pointer_type: PointerType::Click,
            button: Some(PointerButton::Left),
        })
        .unwrap();
        assert!(argv.contains(&"click".into()));
        assert!(argv.contains(&"12".into()));
    }

    #[test]
    fn pointer_up_moves_before_release() {
        let argv = xdotool_argv(&ComputerAction::Pointer {
            x: 80,
            y: 90,
            pointer_type: PointerType::Up,
            button: Some(PointerButton::Left),
        })
        .unwrap();
        assert!(argv.contains(&"mousemove".into()));
        assert!(argv.contains(&"80".into()));
        assert!(argv.contains(&"mouseup".into()));
    }

    #[test]
    fn extra_display_is_injected_into_input_commands() {
        let argv = xdotool_argv_on(
            ":2",
            &ComputerAction::Pointer {
                x: 4,
                y: 8,
                pointer_type: PointerType::Move,
                button: None,
            },
        )
        .unwrap();
        assert!(argv.contains(&"DISPLAY=:2".into()));
        let shot = screenshot_command_on(":3");
        assert!(shot.last().unwrap().contains("DISPLAY=:3"));
        let browser = launch_argv_on(":2", Some("/home/lazyboy/.browser-profiles/bots/a"), "browser", None)
            .unwrap();
        assert!(browser.contains(&"DISPLAY=:2".into()));
        assert!(browser.iter().any(|item| item.contains("bots/a")));
    }
}
