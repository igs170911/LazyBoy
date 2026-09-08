//! GTK clipboard editor operated via Cua, for Linux driver builds without
//! clipboard_read/write support. No direct X11/AT-SPI/clipboard subprocesses.
use super::{CuaClient, CuaController, ListedWindow};
use crate::ControlError;
use serde_json::{Value, json};
use tokio::time::{Duration, sleep};

async fn front(
    client: &CuaClient,
    display: &str,
    window: &ListedWindow,
) -> Result<(), ControlError> {
    client
        .call(
            display,
            "bring_to_front",
            &json!({"pid":window.pid,"window_id":window.id}),
            &[],
        )
        .await?;
    Ok(())
}
async fn key(
    client: &CuaClient,
    display: &str,
    window: &ListedWindow,
    keys: &[&str],
) -> Result<(), ControlError> {
    client.call(display, "hotkey", &json!({"pid":window.pid,"window_id":window.id,"keys":keys,"delivery_mode":"foreground"}), &[]).await?;
    Ok(())
}
fn shortcut<'a>(window: &ListedWindow, letter: &'a str) -> Vec<&'a str> {
    if window.app_name.to_lowercase().contains("terminal") {
        vec!["ctrl", "shift", letter]
    } else {
        vec!["ctrl", letter]
    }
}
async fn active(client: &CuaClient, display: &str) -> Result<ListedWindow, ControlError> {
    CuaController::list_windows_now(client, display)
        .await?
        .into_iter()
        .max_by_key(|w| w.z)
        .ok_or(ControlError::TargetNotFound)
}
async fn editor(client: &CuaClient, display: &str) -> Result<(ListedWindow, Value), ControlError> {
    super::launch::run(
        client,
        display,
        &[
            "env".into(),
            format!("DISPLAY={display}"),
            "lazyboy-clipboard".into(),
        ],
    )
    .await?;
    let window = CuaController::list_windows_now(client, display)
        .await?
        .into_iter()
        .find(|w| w.title == "Clipboard · LazyBoy")
        .ok_or(ControlError::TargetNotFound)?;
    let state = client
        .call(
            display,
            "get_window_state",
            &json!({"pid":window.pid,"window_id":window.id,"include_screenshot":false}),
            &[],
        )
        .await?;
    Ok((window, state))
}
fn element<'a>(state: &'a Value, label: &str) -> Result<&'a Value, ControlError> {
    state
        .get("elements")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|item| {
            item.get("label")
                .and_then(Value::as_str)
                .is_some_and(|text| {
                    if label == "Clipboard text" {
                        text.starts_with("Clipboard text: ")
                    } else {
                        text == label
                    }
                })
        })
        .ok_or(ControlError::TargetNotFound)
}
async fn restore(
    client: &CuaClient,
    display: &str,
    editor: &ListedWindow,
    original: &ListedWindow,
) -> Result<(), ControlError> {
    key(client, display, editor, &["alt", "f4"]).await?;
    front(client, display, original).await
}

pub(super) async fn paste(
    client: &CuaClient,
    display: &str,
    text: &str,
) -> Result<(), ControlError> {
    let original = active(client, display).await?;
    let (window, state) = editor(client, display).await?;
    let result = async {
        let entry = element(&state, "Clipboard text")?;
        client.call(display, "set_value", &json!({"pid":window.pid,"window_id":window.id,"element_token":entry["element_token"],"value":text}), &[]).await?;
        // Fresh snapshot verifies exact content and supplies fresh action refs.
        let state = client.call(display, "get_window_state", &json!({"pid":window.pid,"window_id":window.id,"include_screenshot":false}), &[]).await?;
        if element(&state, "Clipboard text")?.get("label").and_then(Value::as_str) != Some(format!("Clipboard text: {text}").as_str()) {
            return Err(ControlError::internal("clipboard text did not roundtrip; nothing pasted"));
        }
        let copy = element(&state, "Copy")?;
        client.call(display, "click", &json!({"pid":window.pid,"window_id":window.id,"element_token":copy["element_token"],"delivery_mode":"foreground"}), &[]).await?;
        front(client, display, &original).await?;
        key(client, display, &original, &shortcut(&original,"v")).await?;
        sleep(Duration::from_millis(100)).await;
        Ok(())
    }.await;
    // Keep the editor alive until the destination consumes its clipboard.
    let restored = restore(client, display, &window, &original).await;
    result.and(restored)
}

pub(super) async fn copy(client: &CuaClient, display: &str) -> Result<String, ControlError> {
    let original = active(client, display).await?;
    front(client, display, &original).await?;
    key(client, display, &original, &shortcut(&original, "c")).await?;
    sleep(Duration::from_millis(100)).await;
    let (window, state) = editor(client, display).await?;
    let result = element(&state, "Clipboard text").map(|item| {
        item.get("label")
            .and_then(Value::as_str)
            .unwrap_or("")
            .strip_prefix("Clipboard text: ")
            .unwrap_or("")
            .to_string()
    });
    restore(client, display, &window, &original).await?;
    result
}
