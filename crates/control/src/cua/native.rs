use std::collections::HashMap;

use lazyboy_contracts::{RefVerb, UiElement};
use serde_json::{Value, json};

use super::{ListedWindow, client::CuaClient};
use crate::ControlError;

#[derive(Debug, Clone)]
pub(super) struct NativeTarget {
    pid: u64,
    window_id: u64,
    token: String,
}

#[derive(Debug, Default)]
pub(super) struct NativeObservation {
    pub(super) elements: Vec<UiElement>,
    pub(super) targets: HashMap<String, NativeTarget>,
    /// False when any window answered with a degraded tree, which means the
    /// element list is partial and a second opinion is still worth taking.
    pub(super) complete: bool,
}

pub(super) async fn observe(
    client: &CuaClient,
    display: &str,
    windows: &[ListedWindow],
    snapshot_id: &str,
) -> NativeObservation {
    let mut observed = NativeObservation {
        complete: true,
        ..NativeObservation::default()
    };
    for window in windows
        .iter()
        .filter(|window| !window.app_name.to_ascii_lowercase().contains("chrom"))
    {
        let state = match client
            .call(
                display,
                "get_window_state",
                &json!({
                    "pid": window.pid, "window_id": window.id, "include_screenshot": false,
                    "max_elements": 200, "max_depth": 16,
                }),
                &[],
            )
            .await
        {
            Ok(state) => state,
            Err(_) => {
                observed.complete = false;
                continue;
            }
        };
        // A degraded reply is window metadata plus a root node: usable for
        // discovery, not as an element tree, and never as proof of coverage.
        if state.get("degraded").and_then(Value::as_bool) == Some(true) {
            observed.complete = false;
            continue;
        }
        for item in state
            .get("elements")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(token) = item.get("element_token").and_then(Value::as_str) else {
                continue;
            };
            if item.get("enabled").and_then(Value::as_bool) == Some(false) {
                continue;
            }
            let selector = format!("cua:{snapshot_id}:{}:{token}", window.id);
            let title = item.get("label").and_then(Value::as_str).unwrap_or("");
            let role = item.get("role").and_then(Value::as_str).unwrap_or("");
            let frame = item.get("frame").unwrap_or(&Value::Null);
            let coordinate = |key| {
                frame
                    .get(key)
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0)
                    .clamp(0.0, u32::MAX as f64) as u32
            };
            observed.elements.push(UiElement {
                id: 0,
                title: if title.is_empty() {
                    role.into()
                } else {
                    title.into()
                },
                x: coordinate("x"),
                y: coordinate("y"),
                w: coordinate("w"),
                h: coordinate("h"),
                selector: Some(selector.clone()),
                kind: Some("a11y".into()),
                role: Some(role.into()),
            });
            observed.targets.insert(
                selector,
                NativeTarget {
                    pid: window.pid,
                    window_id: window.id,
                    token: token.into(),
                },
            );
        }
    }
    observed
}

pub(super) async fn act(
    client: &CuaClient,
    display: &str,
    target: NativeTarget,
    verb: RefVerb,
    text: Option<&str>,
) -> Result<(), ControlError> {
    client
        .call(
            display,
            "bring_to_front",
            &json!({
                "pid": target.pid, "window_id": target.window_id,
            }),
            &[],
        )
        .await?;
    let mut payload =
        json!({"pid": target.pid, "window_id": target.window_id, "element_token": target.token});
    let tool = match verb {
        RefVerb::Click => {
            payload["delivery_mode"] = json!("foreground");
            "click"
        }
        RefVerb::SetValue => {
            payload["value"] = json!(text.unwrap_or(""));
            "set_value"
        }
        RefVerb::Focus => return Err(ControlError::Unsupported),
    };
    client.call(display, tool, &payload, &[]).await?;
    Ok(())
}
