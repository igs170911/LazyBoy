//! Launch and foreground applications using Cua on the shared desktop.
use serde_json::json;
use tokio::time::{Duration, Instant, sleep};

use super::{CuaClient, CuaController};
use crate::ControlError;

pub(super) async fn run(
    client: &CuaClient,
    display: &str,
    argv: &[String],
) -> Result<(), ControlError> {
    let (program, arguments) = argv
        .split_first()
        .ok_or_else(|| ControlError::InvalidAction("empty application command".into()))?;
    let before = CuaController::list_windows_now(client, display).await?;
    let result = client
        .call(
            display,
            "launch_app",
            &json!({
                "name": program, "additional_arguments": arguments,
            }),
            &[],
        )
        .await?;
    let pid = result.get("pid").and_then(serde_json::Value::as_u64);
    let browser = arguments.iter().any(|arg| arg == "lazyboy-browser");
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let windows = CuaController::list_windows_now(client, display).await?;
        let window = windows
            .iter()
            .find(|window| Some(window.pid) == pid)
            .or_else(|| {
                windows.iter().find(|window| {
                    !before
                        .iter()
                        .any(|old| old.id == window.id && old.title == window.title)
                })
            })
            .or_else(|| {
                if browser {
                    super::browser::chromium_window(&windows)
                } else {
                    None
                }
            });
        if let Some(window) = window {
            client
                .call(
                    display,
                    "bring_to_front",
                    &json!({"pid":window.pid,"window_id":window.id}),
                    &[],
                )
                .await?;
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(ControlError::TargetNotFound);
        }
        sleep(Duration::from_millis(100)).await;
    }
}
