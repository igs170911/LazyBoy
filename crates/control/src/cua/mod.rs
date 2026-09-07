mod browser;
mod client;
mod record;
mod translate;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use lazyboy_contracts::{ActiveWindow, ComputerObservation, CursorPosition, UiElement};
use serde_json::{Value, json};
use tokio::time::{Duration, sleep};

use crate::controller::{
    ComputerController, ComputerDriver, ControlContext, ControlError, ControllerHealth,
};
use crate::legacy::apply_action as legacy_apply_action;
use crate::process::spawn_detached;
use crate::{
    ActionRequest, ActionResult, BrowserRequest, CdpPage, RecordingRequest, RecordingResult,
    RecordingSession, action_pause_ms, normalize_display, observation_from_png,
    observation_with_elements, teach_trajectory_dir,
};
use client::first_array_of_objects;

pub use client::CuaClient;
pub use translate::{TranslatedAction, translate_action};

#[derive(Debug, Default)]
pub struct CuaController {
    client: CuaClient,
    browser: tokio::sync::Mutex<HashMap<String, browser::BrowserBind>>,
}

#[async_trait]
impl ComputerController for CuaController {
    fn backend(&self) -> ComputerDriver {
        ComputerDriver::Cua
    }

    async fn health(&self, ctx: &ControlContext) -> Result<ControllerHealth, ControlError> {
        let version = self.client.version().await.ok();
        match self.client.status(&ctx.display).await {
            Ok(status) => Ok(ControllerHealth {
                backend: ComputerDriver::Cua.as_str().to_string(),
                version,
                healthy: true,
                degraded: false,
                details: vec![status.trim().to_string()],
            }),
            Err(error) => Ok(ControllerHealth {
                backend: ComputerDriver::Cua.as_str().to_string(),
                version,
                healthy: false,
                degraded: true,
                details: vec![error.to_string()],
            }),
        }
    }

    async fn observe(&self, ctx: &ControlContext) -> Result<ComputerObservation, ControlError> {
        self.browser
            .lock()
            .await
            .remove(normalize_display(&ctx.display));
        self.observe_display(&ctx.display).await
    }

    async fn act(
        &self,
        request: &ActionRequest,
        ctx: &ControlContext,
    ) -> Result<ActionResult, ControlError> {
        let display = ctx.display.as_str();
        let profile = ctx.profile_path.as_deref();
        let mut completed = 0usize;
        for action in &request.actions {
            match translate_action(action, display, profile) {
                Ok(translated) => self.dispatch(display, translated).await?,
                Err(ControlError::Unsupported) => {
                    legacy_apply_action(display, profile, action).await?;
                }
                Err(error) => return Err(error),
            }
            let pause = action_pause_ms(action);
            if pause > 0 {
                sleep(Duration::from_millis(pause)).await;
            }
            completed += 1;
        }
        if request.settle_ms > 0 {
            sleep(Duration::from_millis(u64::from(request.settle_ms))).await;
        }
        let observation = if request.observe {
            Some(self.observe_display(display).await?)
        } else {
            None
        };
        Ok(ActionResult {
            completed,
            observation,
        })
    }

    async fn browser(
        &self,
        request: &BrowserRequest,
        ctx: &ControlContext,
    ) -> Result<CdpPage, ControlError> {
        let key = normalize_display(&ctx.display).to_string();
        let mut windows = self.windows(&ctx.display).await.unwrap_or_default();
        let mut cache = self.browser.lock().await;
        for attempt in 0..2 {
            let mut bind = cache.remove(&key);
            let had_bind = bind.is_some();
            match browser::run(
                &self.client,
                &ctx.display,
                ctx.profile_path.as_deref(),
                request,
                &windows,
                &mut bind,
            )
            .await
            {
                Err(ControlError::BrowserUnavailable) if !had_bind => {
                    return Ok(CdpPage {
                        ok: false,
                        error: Some("cdp unavailable".into()),
                        ..CdpPage::default()
                    });
                }
                Err(error)
                    if attempt == 0
                        && had_bind
                        && matches!(
                            error,
                            ControlError::BrowserUnavailable
                                | ControlError::StaleReference
                                | ControlError::TargetNotFound
                        ) =>
                {
                    windows = self.windows(&ctx.display).await.unwrap_or_default();
                }
                other => {
                    if let Some(current) = bind {
                        cache.insert(key, current);
                    }
                    return map_browser_unavailable(other);
                }
            }
        }
        map_browser_unavailable(Err(ControlError::BrowserUnavailable))
    }

    async fn start_recording(
        &self,
        request: &RecordingRequest,
        ctx: &ControlContext,
    ) -> Result<RecordingSession, ControlError> {
        if request.skill_id.trim().is_empty() {
            return Err(ControlError::InvalidAction(
                "recording needs a skill id".into(),
            ));
        }
        let output_dir = teach_trajectory_dir(&request.skill_id);
        let _ = tokio::fs::create_dir_all(&output_dir).await;
        let _ = self
            .client
            .call(&ctx.display, "stop_recording", &json!({}), &[])
            .await;
        if let Err(error) = self
            .client
            .call(
                &ctx.display,
                "start_recording",
                &json!({ "output_dir": output_dir, "record_video": false }),
                &[],
            )
            .await
        {
            tracing::warn!(error = %error, "cua start_recording failed");
        }
        if let Err(error) = crate::legacy::start_cdp_recorder(
            &ctx.display,
            ctx.profile_path.as_deref(),
            &request.skill_id,
        )
        .await
        {
            tracing::warn!(error = %error, "cdp recorder start failed");
        }
        Ok(RecordingSession {
            skill_id: request.skill_id.clone(),
            output_dir,
        })
    }

    async fn stop_recording(
        &self,
        request: &RecordingRequest,
        ctx: &ControlContext,
    ) -> Result<(), ControlError> {
        let _ = self
            .client
            .call(&ctx.display, "stop_recording", &json!({}), &[])
            .await;
        crate::legacy::stop_cdp_recorder(&request.skill_id).await
    }

    async fn collect_recording(
        &self,
        request: &RecordingRequest,
        _ctx: &ControlContext,
    ) -> Result<RecordingResult, ControlError> {
        let mut events = crate::legacy::collect_cdp_events(&request.skill_id).await;
        let dir = teach_trajectory_dir(&request.skill_id);
        events.extend(record::events_from_dir(std::path::Path::new(&dir)));
        events.sort_by_key(|event| event.get("at").and_then(Value::as_i64).unwrap_or(0));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        Ok(RecordingResult { events })
    }
}

impl CuaController {
    async fn observe_display(&self, display: &str) -> Result<ComputerObservation, ControlError> {
        let png_path = observe_png_path(display);
        let _ = tokio::fs::remove_file(&png_path).await;
        self.client
            .call(
                display,
                "get_desktop_state",
                &json!({ "screenshot_out_file": png_path.to_string_lossy() }),
                &["--screenshot-out-file", &png_path.to_string_lossy()],
            )
            .await?;
        let png = tokio::fs::read(&png_path)
            .await
            .map_err(|_| ControlError::Internal("screenshot failed".into()))?;
        let _ = tokio::fs::remove_file(&png_path).await;
        if png.is_empty() {
            return Err(ControlError::Internal("screenshot failed".into()));
        }
        let (width, height) = png_dimensions(&png);
        let cursor = self.cursor(display).await;
        let windows = self.windows(display).await.unwrap_or_default();
        let active = windows
            .iter()
            .max_by_key(|window| window.z)
            .map(|window| ActiveWindow {
                id: window.id.to_string(),
                title: Some(window.title.clone()).filter(|title| !title.is_empty()),
            });
        let elements: Vec<UiElement> = windows
            .into_iter()
            .enumerate()
            .map(|(index, window)| UiElement {
                id: (index + 1) as u32,
                title: window.title.chars().take(80).collect(),
                x: window.x,
                y: window.y,
                w: window.w,
                h: window.h,
                selector: None,
                kind: Some("window".into()),
                role: None,
            })
            .collect();
        Ok(observation_with_elements(
            observation_from_png(png, width, height, cursor, active),
            elements,
        ))
    }

    async fn cursor(&self, display: &str) -> Option<CursorPosition> {
        let value = self
            .client
            .call(display, "get_cursor_position", &json!({}), &[])
            .await
            .ok()?;
        cursor_from_value(&value)
    }

    async fn windows(&self, display: &str) -> Result<Vec<ListedWindow>, ControlError> {
        Self::list_windows_now(&self.client, display).await
    }

    pub(crate) async fn list_windows_now(
        client: &CuaClient,
        display: &str,
    ) -> Result<Vec<ListedWindow>, ControlError> {
        let value = client
            .call(
                display,
                "list_windows",
                &json!({ "on_screen_only": true }),
                &[],
            )
            .await?;
        Ok(parse_listed_windows(&value))
    }

    async fn dispatch(
        &self,
        display: &str,
        translated: TranslatedAction,
    ) -> Result<(), ControlError> {
        match translated {
            TranslatedAction::Sleep { ms } => {
                sleep(Duration::from_millis(ms)).await;
                Ok(())
            }
            TranslatedAction::LegacyArgv { argv, detached } => {
                if detached {
                    spawn_detached(&argv).await.map_err(ControlError::internal)
                } else {
                    crate::process::run_output(&argv)
                        .await
                        .map_err(ControlError::internal)
                        .and_then(|output| {
                            if output.status.success() {
                                Ok(())
                            } else {
                                Err(ControlError::internal(String::from_utf8_lossy(
                                    &output.stderr,
                                )))
                            }
                        })
                }
            }
            TranslatedAction::FocusTitle { title } => self.focus_title(display, &title).await,
            TranslatedAction::Cua { tool, payload } => {
                let payload = self.with_window_target(display, tool, payload).await?;
                self.client.call(display, tool, &payload, &[]).await?;
                Ok(())
            }
        }
    }

    async fn with_window_target(
        &self,
        display: &str,
        tool: &str,
        mut payload: Value,
    ) -> Result<Value, ControlError> {
        if tool != "mouse_button_down" && tool != "mouse_button_up" {
            return Ok(payload);
        }
        if payload.get("pid").is_some() && payload.get("window_id").is_some() {
            return Ok(payload);
        }
        let window = self
            .windows(display)
            .await?
            .into_iter()
            .max_by_key(|window| window.z)
            .ok_or(ControlError::TargetNotFound)?;
        if let Some(object) = payload.as_object_mut() {
            object.insert("pid".into(), json!(window.pid));
            object.insert("window_id".into(), json!(window.id));
        }
        Ok(payload)
    }

    async fn focus_title(&self, display: &str, title: &str) -> Result<(), ControlError> {
        let needle = title.to_ascii_lowercase();
        let windows = self.windows(display).await?;
        let window = windows
            .into_iter()
            .find(|window| window.title.to_ascii_lowercase().contains(&needle))
            .ok_or(ControlError::TargetNotFound)?;
        self.client
            .call(
                display,
                "bring_to_front",
                &json!({ "pid": window.pid, "window_id": window.id }),
                &[],
            )
            .await?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ListedWindow {
    pub(crate) id: u64,
    pub(crate) pid: u64,
    pub(crate) title: String,
    pub(crate) app_name: String,
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) w: u32,
    pub(crate) h: u32,
    pub(crate) z: i64,
}

fn map_browser_unavailable(result: Result<CdpPage, ControlError>) -> Result<CdpPage, ControlError> {
    match result {
        Err(ControlError::BrowserUnavailable) => Ok(CdpPage {
            ok: false,
            error: Some("cdp unavailable".into()),
            ..CdpPage::default()
        }),
        other => other,
    }
}

fn cursor_from_value(value: &Value) -> Option<CursorPosition> {
    let mut nodes = Vec::new();
    client::walk(value, &mut nodes);
    for node in nodes {
        if let (Some(x), Some(y)) = (
            node.get("x").and_then(Value::as_i64),
            node.get("y").and_then(Value::as_i64),
        ) {
            return Some(CursorPosition {
                x: x as i32,
                y: y as i32,
            });
        }
    }
    None
}

fn parse_listed_windows(value: &Value) -> Vec<ListedWindow> {
    first_array_of_objects(value, "window_id")
        .into_iter()
        .filter_map(listed_window)
        .filter(|window| !ignored_window(window))
        .collect()
}

fn listed_window(value: &Value) -> Option<ListedWindow> {
    let bounds = value.get("bounds");
    let x = number(value, "x").or_else(|| bounds.and_then(|bounds| number(bounds, "x")))?;
    let y = number(value, "y").or_else(|| bounds.and_then(|bounds| number(bounds, "y")))?;
    let w = number(value, "width")
        .or_else(|| number(value, "w"))
        .or_else(|| bounds.and_then(|bounds| number(bounds, "width")))?;
    let h = number(value, "height")
        .or_else(|| number(value, "h"))
        .or_else(|| bounds.and_then(|bounds| number(bounds, "height")))?;
    if w < 32 || h < 16 {
        return None;
    }
    Some(ListedWindow {
        id: number(value, "window_id")?,
        pid: number(value, "pid").unwrap_or(0),
        title: value
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        app_name: value
            .get("app_name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        x: x as u32,
        y: y as u32,
        w: w as u32,
        h: h as u32,
        z: number(value, "z_index").unwrap_or(0) as i64,
    })
}

fn ignored_window(window: &ListedWindow) -> bool {
    let title = window.title.to_ascii_lowercase();
    title.is_empty() || title == "desktop" || title == "xfce4-panel" || title.contains("xfdesktop")
}

fn number(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64).or_else(|| {
        value
            .get(key)
            .and_then(Value::as_f64)
            .filter(|n| *n >= 0.0)
            .map(|n| n as u64)
    })
}

fn png_dimensions(bytes: &[u8]) -> (u32, u32) {
    image::load_from_memory(bytes)
        .map(|image| (image.width(), image.height()))
        .unwrap_or((1280, 800))
}

fn observe_png_path(display: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    PathBuf::from(format!(
        "/tmp/lazyboy/cua-obs-{}-{nanos}.png",
        display.trim_start_matches(':')
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_list_skips_panel_and_numbers_from_one() {
        let raw = json!([
            {
                "window_id": 1,
                "pid": 8,
                "title": "xfce4-panel",
                "x": 0,
                "y": 759,
                "width": 1280,
                "height": 41,
                "z_index": 3
            },
            {
                "window_id": 9,
                "pid": 20,
                "title": "終端機",
                "app_name": "xfce4-terminal",
                "bounds": { "x": 53, "y": 55, "width": 753, "height": 699 },
                "z_index": 1
            }
        ]);
        let windows = parse_listed_windows(&raw);
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].title, "終端機");
        assert_eq!(windows[0].app_name, "xfce4-terminal");
        assert_eq!(windows[0].w, 753);
    }
}
