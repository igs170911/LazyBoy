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

pub(super) async fn observe(
    client: &CuaClient,
    display: &str,
    windows: &[ListedWindow],
    generation: &str,
) -> Result<(Vec<UiElement>, HashMap<String, NativeTarget>), ControlError> {
    let mut elements = Vec::new();
    let mut targets = HashMap::new();
    for window in windows
        .iter()
        .filter(|window| !window.app_name.to_ascii_lowercase().contains("chrom"))
    {
        let state = client
            .call(
                display,
                "get_window_state",
                &json!({
                    "pid": window.pid, "window_id": window.id, "include_screenshot": false,
                    "max_elements": 200, "max_depth": 16,
                }),
                &[],
            )
            .await?;
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
            let selector = format!("cua:{generation}:{}:{token}", window.id);
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
            elements.push(UiElement {
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
            targets.insert(
                selector,
                NativeTarget {
                    pid: window.pid,
                    window_id: window.id,
                    token: token.into(),
                },
            );
        }
    }
    Ok((elements, targets))
}

pub(super) async fn act(
    client: &CuaClient,
    display: &str,
    target: NativeTarget,
    verb: RefVerb,
    text: Option<&str>,
) -> Result<(), ControlError> {
    let mut payload =
        json!({"pid": target.pid, "window_id": target.window_id, "element_token": target.token});
    let tool = match verb {
        RefVerb::Click => "click",
        RefVerb::SetValue => {
            payload["value"] = json!(text.unwrap_or(""));
            "set_value"
        }
        RefVerb::Focus => return Err(ControlError::Unsupported),
    };
    client.call(display, tool, &payload, &[]).await?;
    Ok(())
}
