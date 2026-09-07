mod browser;
mod client;
mod native;
mod record;
mod translate;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use lazyboy_contracts::{
    ActiveWindow, ComputerAction, ComputerObservation, CursorPosition, PointerType, UiElement,
};
use serde_json::{Value, json};
use tokio::time::{Duration, sleep};

use crate::controller::{
    ComputerController, ComputerDriver, ControlContext, ControlError, ControllerHealth,
};
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
    screens: tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    native: tokio::sync::Mutex<HashMap<String, HashMap<String, native::NativeTarget>>>,
    browser: tokio::sync::Mutex<HashMap<String, browser::BrowserBind>>,
}

#[async_trait]
impl ComputerController for CuaController {
    fn backend(&self) -> ComputerDriver {
        ComputerDriver::Cua
    }

    async fn health(&self, ctx: &ControlContext) -> Result<ControllerHealth, ControlError> {
        let version = self.client.version().await.ok();
        let report = self
            .client
            .call(&ctx.display, "health_report", &json!({}), &[])
            .await;
        match report {
            Ok(report) => {
                let compatible = version.as_deref() == Some("cua-driver 0.23.2");
                let healthy =
                    compatible && report.get("overall").and_then(Value::as_str) == Some("ok");
                let mut details: Vec<String> = report
                    .get("checks")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter(|check| check["status"] == "fail")
                    .filter_map(|check| check["message"].as_str().map(str::to_string))
                    .collect();
                if !compatible {
                    details.push("expected pinned Cua Driver 0.23.2".into());
                }
                Ok(ControllerHealth {
                    backend: "cua".into(),
                    version,
                    healthy,
                    degraded: !healthy,
                    details,
                })
            }
            Err(error) => Ok(ControllerHealth {
                backend: "cua".into(),
                version,
                healthy: false,
                degraded: true,
                details: vec![error.to_string()],
            }),
        }
    }

    async fn observe(&self, ctx: &ControlContext) -> Result<ComputerObservation, ControlError> {
        let _screen = self.lock_screen(&ctx.display).await;
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
        let _screen = self.lock_screen(&ctx.display).await;
        let display = ctx.display.as_str();
        let profile = ctx.profile_path.as_deref();
        let key = normalize_display(display).to_string();
        self.browser.lock().await.remove(&key);
        // One snapshot for the whole batch. Taking the map per action made a
        // leading wait (or a second ref) look stale.
        let mut targets = self.native.lock().await.remove(&key).unwrap_or_default();
        match self
            .run_actions(request, display, profile, &mut targets)
            .await
        {
            Ok(completed) => {
                let observation = if request.observe {
                    Some(self.observe_display(display).await?)
                } else {
                    self.native.lock().await.insert(key, targets);
                    None
                };
                Ok(ActionResult {
                    completed,
                    observation,
                })
            }
            Err(error) => {
                self.native.lock().await.insert(key, targets);
                Err(error)
            }
        }
    }

    async fn browser(
        &self,
        request: &BrowserRequest,
        ctx: &ControlContext,
    ) -> Result<CdpPage, ControlError> {
        let _screen = self.lock_screen(&ctx.display).await;
        if !matches!(
            request.action.as_str(),
            "snapshot" | "wait" | "probe" | "ensure"
        ) {
            self.native
                .lock()
                .await
                .remove(normalize_display(&ctx.display));
        }
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
                        && matches!(request.action.as_str(), "snapshot" | "wait" | "ensure")
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
                    if let Some(mut current) = bind {
                        current.page = match &other {
                            Ok(page)
                                if page.ok
                                    && !matches!(request.action.as_str(), "probe" | "ensure") =>
                            {
                                Some(page.clone())
                            }
                            _ => None,
                        };
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
    async fn run_actions(
        &self,
        request: &ActionRequest,
        display: &str,
        profile: Option<&str>,
        targets: &mut HashMap<String, native::NativeTarget>,
    ) -> Result<usize, ControlError> {
        let mut completed = 0usize;
        while completed < request.actions.len() {
            let action = &request.actions[completed];
            if let Some(payload) = drag_payload(&request.actions[completed..]) {
                self.dispatch(
                    display,
                    TranslatedAction::Cua {
                        tool: "drag",
                        payload,
                    },
                )
                .await?;
                completed += 5;
                continue;
            }
            if matches!(
                action,
                ComputerAction::Pointer {
                    pointer_type: PointerType::Down | PointerType::Up,
                    ..
                }
            ) {
                return Err(ControlError::Unsupported);
            }
            if let ComputerAction::Ref {
                verb,
                target,
                ref_kind,
                text,
            } = action
            {
                if ref_kind != "a11y" {
                    return Err(ControlError::Unsupported);
                }
                let target = targets
                    .get(target)
                    .cloned()
                    .ok_or(ControlError::StaleReference)?;
                native::act(&self.client, display, target, *verb, text.as_deref()).await?;
            } else {
                let translated = translate_action(action, display, profile)?;
                self.dispatch(display, translated).await?;
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
        Ok(completed)
    }

    async fn lock_screen(&self, display: &str) -> tokio::sync::OwnedMutexGuard<()> {
        let lock = self
            .screens
            .lock()
            .await
            .entry(normalize_display(display).into())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone();
        lock.lock_owned().await
    }

    async fn observe_display(&self, display: &str) -> Result<ComputerObservation, ControlError> {
        self.native.lock().await.remove(normalize_display(display));
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
        let (mut elements, targets) =
            native::observe(&self.client, display, &windows, &png_path.to_string_lossy()).await?;
        elements.extend(
            windows
                .into_iter()
                .enumerate()
                .map(|(index, window)| UiElement {
                    id: (index + 1) as u32,
                    title: window.title.chars().take(80).collect(),
                    x: window.x.max(0) as u32,
                    y: window.y.max(0) as u32,
                    w: window.w,
                    h: window.h,
                    selector: None,
                    kind: Some("window".into()),
                    role: None,
                })
                .collect::<Vec<_>>(),
        );
        for (index, element) in elements.iter_mut().enumerate() {
            element.id = (index + 1) as u32;
        }
        self.native
            .lock()
            .await
            .insert(normalize_display(display).into(), targets);
        let mut observation = observation_with_elements(
            observation_from_png(png, width, height, cursor, active),
            elements,
        );
        observation.native_observation_complete = true;
        Ok(observation)
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
            TranslatedAction::LegacyArgv { argv } => {
                spawn_detached(&argv).await.map_err(ControlError::internal)
            }
            TranslatedAction::FocusTitle { title } => self.focus_title(display, &title).await,
            TranslatedAction::Cua { tool, mut payload } => {
                if tool == "drag" {
                    let x = payload["from_x"].as_f64().unwrap_or(0.0);
                    let y = payload["from_y"].as_f64().unwrap_or(0.0);
                    let windows = self.windows(display).await?;
                    let window = window_containing(&windows, x, y)
                        .cloned()
                        .ok_or(ControlError::TargetNotFound)?;
                    for (key, offset) in [
                        ("from_x", window.x),
                        ("to_x", window.x),
                        ("from_y", window.y),
                        ("to_y", window.y),
                    ] {
                        payload[key] = json!(payload[key].as_f64().unwrap_or(0.0) - offset as f64);
                    }
                    payload["pid"] = json!(window.pid);
                    payload["window_id"] = json!(window.id);
                    payload["delivery_mode"] = json!("foreground");
                }
                self.client.call(display, tool, &payload, &[]).await?;
                Ok(())
            }
        }
    }

    async fn focus_title(&self, display: &str, title: &str) -> Result<(), ControlError> {
        let windows = self.windows(display).await?;
        let window = window_matching_title(&windows, title).ok_or(ControlError::TargetNotFound)?;
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

// The public drag DSL expands into these five actions. Keep the gesture in
// one Cua call; separate CLI leases do not preserve held-button state.
fn drag_payload(actions: &[ComputerAction]) -> Option<Value> {
    let [
        ComputerAction::Pointer {
            x,
            y,
            pointer_type: PointerType::Down,
            button,
        },
        ComputerAction::Wait { ms: 40 },
        ComputerAction::Pointer {
            x: to_x,
            y: to_y,
            pointer_type: PointerType::Move,
            button: move_button,
        },
        ComputerAction::Wait { ms: 40 },
        ComputerAction::Pointer {
            x: up_x,
            y: up_y,
            pointer_type: PointerType::Up,
            button: up_button,
        },
        ..,
    ] = actions
    else {
        return None;
    };
    if (to_x, to_y, button) != (up_x, up_y, up_button) || button != move_button {
        return None;
    }
    Some(json!({"from_x": x, "from_y": y, "to_x": to_x, "to_y": to_y,
        "button": button.unwrap_or(lazyboy_contracts::PointerButton::Left), "duration_ms": 500}))
}

#[derive(Debug, Clone)]
pub(crate) struct ListedWindow {
    pub(crate) id: u64,
    pub(crate) pid: u64,
    pub(crate) title: String,
    pub(crate) app_name: String,
    pub(crate) x: i64,
    pub(crate) y: i64,
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
    let x = value
        .get("x")
        .or_else(|| bounds.and_then(|b| b.get("x")))?
        .as_f64()? as i64;
    let y = value
        .get("y")
        .or_else(|| bounds.and_then(|b| b.get("y")))?
        .as_f64()? as i64;
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
        x,
        y,
        w: w as u32,
        h: h as u32,
        z: number(value, "z_index").unwrap_or(0) as i64,
    })
}

fn window_containing(windows: &[ListedWindow], x: f64, y: f64) -> Option<&ListedWindow> {
    windows
        .iter()
        .filter(|window| {
            x >= window.x as f64
                && y >= window.y as f64
                && x < window.x as f64 + f64::from(window.w)
                && y < window.y as f64 + f64::from(window.h)
        })
        .min_by_key(|window| u64::from(window.w.saturating_mul(window.h.max(1))))
}

fn window_matching_title<'a>(windows: &'a [ListedWindow], title: &str) -> Option<&'a ListedWindow> {
    let needle = title.to_ascii_lowercase();
    windows
        .iter()
        .find(|window| window.title.eq_ignore_ascii_case(title))
        .or_else(|| {
            windows.iter().find(|window| {
                window.title.to_ascii_lowercase().contains(&needle)
                    && !window.app_name.to_ascii_lowercase().contains("chrom")
            })
        })
        .or_else(|| {
            windows
                .iter()
                .find(|window| window.title.to_ascii_lowercase().contains(&needle))
        })
}

fn ignored_window(window: &ListedWindow) -> bool {
    let title = window.title.to_ascii_lowercase();
    title.is_empty()
        || title == "desktop"
        || title == "xfce4-panel"
        || window.app_name.to_ascii_lowercase().contains("xfdesktop")
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
    fn normalized_drag_uses_one_driver_gesture() {
        let actions = crate::parse_computer_actions(
            &json!([{"kind": "drag", "x": 10, "y": 20, "x2": 30, "y2": 40}]),
        )
        .unwrap();
        let payload = drag_payload(&actions).unwrap();
        assert_eq!(payload["from_x"], 10);
        assert_eq!(payload["to_y"], 40);
        assert!(drag_payload(&actions[..4]).is_none());
    }

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

    fn listed(
        title: &str,
        app: &str,
        x: i64,
        y: i64,
        w: u32,
        h: u32,
        z: i64,
        id: u64,
    ) -> ListedWindow {
        ListedWindow {
            id,
            pid: id,
            title: title.into(),
            app_name: app.into(),
            x,
            y,
            w,
            h,
            z,
        }
    }

    #[test]
    fn drag_targets_the_smallest_containing_window() {
        let gtk = listed(
            "LazyBoy Cua Smoke",
            "lazyboy-cua-smoke-gtk",
            40,
            40,
            480,
            240,
            1,
            2,
        );
        let chrome = listed(
            "LazyBoy Cua Smoke - Chromium",
            "Chromium",
            0,
            0,
            1280,
            759,
            4,
            3,
        );
        let windows = [chrome.clone(), gtk.clone()];
        let hit = window_containing(&windows, 60.0, 70.0).unwrap();
        assert_eq!(hit.id, gtk.id);
    }

    #[test]
    fn focus_prefers_the_exact_native_title_over_chromium() {
        let gtk = listed(
            "LazyBoy Cua Smoke",
            "lazyboy-cua-smoke-gtk",
            40,
            40,
            480,
            240,
            1,
            2,
        );
        let chrome = listed(
            "LazyBoy Cua Smoke - Chromium",
            "Chromium",
            0,
            0,
            1280,
            759,
            4,
            3,
        );
        let windows = [chrome, gtk];
        let hit = window_matching_title(&windows, "LazyBoy Cua Smoke").unwrap();
        assert_eq!(hit.app_name, "lazyboy-cua-smoke-gtk");
    }
}
