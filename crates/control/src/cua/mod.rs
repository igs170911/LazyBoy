mod browser;
mod client;
mod clipboard;
mod launch;
mod native;
mod record;
mod translate;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
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
use crate::{
    ActionRequest, ActionResult, ActionVerdict, BrowserPage, BrowserRequest, RecordingRequest,
    RecordingResult, RecordingSession, action_pause_ms, image_dimensions, normalize_display,
    observation_from_png, observation_with_elements, teach_trajectory_dir,
};
use client::{action_verdict, first_array_of_objects};

pub use client::CuaClient;

/// Last resort when neither the screenshot nor the driver reports a mode.
const FALLBACK_SCREEN: (u32, u32) = (1280, 800);

/// `image/computer/Dockerfile` pins `CUA_DRIVER_RS_VERSION`; only that tool
/// surface is guaranteed. Patch releases stay compatible, a new minor does not.
const PINNED_DRIVER: (u32, u32) = (0, 23);

/// `cua-driver --version` prints `cua-driver 0.23.2`, and some builds append a
/// target suffix (`cua-driver 0.23.2 (x86_64-linux)`), so only the leading
/// numeric version is trusted.
fn driver_release(version: &str) -> Option<(u32, u32)> {
    let digits = version.find(|character: char| character.is_ascii_digit())?;
    let mut parts = version[digits..].split('.');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

/// CLI surface LazyBoy spawns. `call` writes screenshots to a file because a
/// base64 frame in a pipe cannot survive a busy desktop.
const REQUIRED_CLI_ARGS: &[(&str, &[&str])] = &[
    ("call", &["--socket", "--screenshot-out-file"]),
    ("status", &["--socket"]),
];

/// Every driver tool the harness dispatches. Naming them turns a pin failure
/// into "this driver cannot do X" instead of a bare "unhealthy".
const REQUIRED_TOOLS: &[&str] = &[
    "bring_to_front",
    "browser_click",
    "browser_navigate",
    "browser_prepare",
    "browser_type",
    "click",
    "drag",
    "get_browser_state",
    "get_cursor_position",
    "get_desktop_state",
    "get_screen_size",
    "get_window_state",
    "health_report",
    "hotkey",
    "launch_app",
    "list_windows",
    "move_cursor",
    "press_key",
    "scroll",
    "set_agent_cursor_motion",
    "set_value",
    "start_recording",
    "start_session",
    "stop_recording",
    "type_text",
];

/// `subcommands: [{ "name", "args": [{ "name" }] }]` from `cua-driver manifest`.
fn advertised_flags(manifest: &Value) -> HashMap<&str, Vec<&str>> {
    manifest
        .get("subcommands")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|verb| Some((verb["name"].as_str()?, verb)))
        .map(|(name, verb)| {
            (
                name,
                verb.get("args")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|arg| arg["name"].as_str())
                    .collect(),
            )
        })
        .collect()
}

/// Required `verb --flag` pairs this driver does not advertise.
fn cli_contract_gap(manifest: &Value) -> Vec<String> {
    let advertised = advertised_flags(manifest);
    let mut gap = Vec::new();
    for (verb, flags) in REQUIRED_CLI_ARGS {
        let offered: &[&str] = advertised.get(verb).map(Vec::as_slice).unwrap_or_default();
        gap.extend(
            flags
                .iter()
                .filter(|flag| !offered.contains(flag))
                .map(|flag| format!("{verb} {flag}")),
        );
    }
    gap
}

/// Required tools the driver cannot dispatch.
fn missing_tools(advertised: &[String]) -> Vec<&'static str> {
    REQUIRED_TOOLS
        .iter()
        .copied()
        .filter(|tool| !advertised.iter().any(|named| named == tool))
        .collect()
}

/// What a driver that failed the pin is actually missing. Both probes are
/// read-only and degrade to silence on drivers that predate them.
async fn capability_gap(client: &CuaClient) -> Vec<String> {
    let mut gap = Vec::new();
    if let Some(manifest) = client.manifest().await {
        let missing = cli_contract_gap(&manifest);
        if !missing.is_empty() {
            gap.push(format!("driver CLI is missing: {}", missing.join(", ")));
        }
    }
    if let Some(tools) = client.tool_names().await {
        let missing = missing_tools(&tools);
        gap.push(if missing.is_empty() {
            format!(
                "all {} driver tools LazyBoy needs are present, so only the version series differs",
                REQUIRED_TOOLS.len()
            )
        } else {
            format!("driver tools are missing: {}", missing.join(", "))
        });
    }
    gap
}

pub use translate::{TranslatedAction, translate_action};

#[derive(Debug, Default)]
pub struct CuaController {
    client: CuaClient,
    screens: tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    native: tokio::sync::Mutex<HashMap<String, HashMap<String, native::NativeTarget>>>,
    browser: tokio::sync::Mutex<HashMap<String, browser::BrowserBind>>,
    snapshots: AtomicU64,
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
                let compatible = version
                    .as_deref()
                    .is_some_and(|text| driver_release(text) == Some(PINNED_DRIVER));
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
                    details.push(format!(
                        "Cua Driver {} is not the pinned {}.{} series",
                        version.as_deref().unwrap_or("unknown"),
                        PINNED_DRIVER.0,
                        PINNED_DRIVER.1
                    ));
                    // Only the failing path pays for the probes; a healthy
                    // desktop is never asked about its own surface again.
                    details.extend(capability_gap(&self.client).await);
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
        // A desktop screenshot does not change the browser binding. The next
        // browser snapshot refreshes page refs; keep the session attachment warm.
        self.observe_display(&ctx.display).await
    }

    async fn act(
        &self,
        request: &ActionRequest,
        ctx: &ControlContext,
    ) -> Result<ActionResult, ControlError> {
        let _screen = self.lock_screen(&ctx.display).await;
        if matches!(request.actions.as_slice(), [ComputerAction::CopySelection]) {
            let text = clipboard::copy(&self.client, &ctx.display).await?;
            return Ok(ActionResult {
                completed: 1,
                clipboard_text: Some(text),
                observation: None,
                verdict: None,
            });
        }
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
            Ok((completed, verdict)) => {
                let observation = if request.observe {
                    Some(self.observe_display(display).await?)
                } else {
                    self.native.lock().await.insert(key, targets);
                    None
                };
                Ok(ActionResult {
                    clipboard_text: None,
                    completed,
                    observation,
                    verdict,
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
    ) -> Result<BrowserPage, ControlError> {
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
        // The screen lock serializes this display. Never hold the shared cache
        // lock across driver calls or waits on behalf of other displays.
        let mut bind = self.browser.lock().await.remove(&key);
        for attempt in 0..2 {
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
                    return Ok(BrowserPage {
                        ok: false,
                        error: Some("Cua browser unavailable".into()),
                        ..BrowserPage::default()
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
                    bind = None;
                    windows = self.windows(&ctx.display).await.unwrap_or_default();
                }
                other => {
                    if let Some(mut current) = bind {
                        update_browser_page(&mut current, &request.action, &other);
                        self.browser.lock().await.insert(key, current);
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
        self.client
            .call(
                &ctx.display,
                "start_recording",
                &json!({ "output_dir": output_dir, "record_video": false }),
                &[],
            )
            .await?;
        Ok(RecordingSession {
            skill_id: request.skill_id.clone(),
            output_dir,
        })
    }

    async fn stop_recording(
        &self,
        _request: &RecordingRequest,
        ctx: &ControlContext,
    ) -> Result<(), ControlError> {
        self.client
            .call(&ctx.display, "stop_recording", &json!({}), &[])
            .await?;
        Ok(())
    }

    async fn collect_recording(
        &self,
        request: &RecordingRequest,
        _ctx: &ControlContext,
    ) -> Result<RecordingResult, ControlError> {
        let dir = teach_trajectory_dir(&request.skill_id);
        let mut events = record::events_from_dir(std::path::Path::new(&dir));
        events.sort_by_key(|event| event.get("at").and_then(Value::as_i64).unwrap_or(0));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        Ok(RecordingResult { events })
    }
}

fn update_browser_page(
    bind: &mut browser::BrowserBind,
    action: &str,
    result: &Result<BrowserPage, ControlError>,
) {
    // These checks neither observe nor mutate the page. Keep the refs returned
    // by the previous snapshot usable for the next click/type.
    if matches!(action, "probe" | "ensure") && result.is_ok() {
        return;
    }
    bind.page = match result {
        Ok(page) if page.ok => Some(page.clone()),
        _ => None,
    };
}

impl CuaController {
    async fn run_actions(
        &self,
        request: &ActionRequest,
        display: &str,
        profile: Option<&str>,
        targets: &mut HashMap<String, native::NativeTarget>,
    ) -> Result<(usize, Option<ActionVerdict>), ControlError> {
        let mut completed = 0usize;
        // One batch, one answer: keep the most urgent verdict the driver gave.
        let mut verdict = None;
        while completed < request.actions.len() {
            let action = &request.actions[completed];
            if let Some(payload) = drag_payload(&request.actions[completed..]) {
                merge_verdict(
                    &mut verdict,
                    self.dispatch(
                        display,
                        TranslatedAction::Cua {
                            tool: "drag",
                            payload,
                        },
                    )
                    .await?,
                );
                completed += 5;
                continue;
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
                merge_verdict(
                    &mut verdict,
                    native::act(&self.client, display, target, *verb, text.as_deref()).await?,
                );
            } else {
                let translated = translate_action(action, display, profile)?;
                merge_verdict(&mut verdict, self.dispatch(display, translated).await?);
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
        Ok((completed, verdict))
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
        let (width, height) = match image_dimensions(&png) {
            Some(dimensions) => dimensions,
            None => self.screen_size(display).await,
        };
        // Cursor position and window list are two independent reads of the same
        // frame: run them as two CLI processes at once instead of end to end.
        let (cursor, windows) = tokio::join!(self.cursor(display), self.windows(display));
        // A window list that failed is not a desktop with no windows. Telling
        // those apart is what keeps a driver hiccup from reaching the model as
        // "the screen is empty".
        let (windows, windows_listed) = match windows {
            Ok(windows) => (windows, true),
            Err(_) => (Vec::new(), false),
        };
        let active = windows
            .iter()
            .max_by_key(|window| window.z)
            .map(|window| ActiveWindow {
                id: window.id.to_string(),
                title: Some(window.title.clone()).filter(|title| !title.is_empty()),
            });
        // Selectors only need to be short and unique: a process-local counter
        // keeps stale handles from resolving without pasting a temp path into
        // every identifier the Agent echoes back.
        let snapshot = self.snapshots.fetch_add(1, Ordering::Relaxed);
        let native::NativeObservation {
            mut elements,
            targets,
            complete,
        } = native::observe(&self.client, display, &windows, &format!("s{snapshot}")).await;
        elements.extend(
            windows
                .into_iter()
                .enumerate()
                .map(|(index, window)| UiElement {
                    id: (index + 1) as u32,
                    title: window.title.chars().take(256).collect(),
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
        observation.native_observation_complete = complete && windows_listed;
        Ok(observation)
    }

    /// `scroll` on the desktop plane is aimed at a point and the action DSL
    /// does not carry one, so aim at the pointer; the screen centre is the
    /// next best guess when the pointer cannot be read.
    async fn scroll_point(&self, display: &str) -> (u32, u32) {
        if let Some(cursor) = self.cursor(display).await
            && cursor.x >= 0
            && cursor.y >= 0
        {
            return (cursor.x as u32, cursor.y as u32);
        }
        let (width, height) = self.screen_size(display).await;
        (width / 2, height / 2)
    }

    async fn screen_size(&self, display: &str) -> (u32, u32) {
        self.client
            .call(display, "get_screen_size", &json!({}), &[])
            .await
            .ok()
            .and_then(|value| {
                Some((
                    value.get("width").and_then(Value::as_u64)? as u32,
                    value.get("height").and_then(Value::as_u64)? as u32,
                ))
            })
            .unwrap_or(FALLBACK_SCREEN)
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
    ) -> Result<Option<ActionVerdict>, ControlError> {
        match translated {
            TranslatedAction::Sleep { ms } => {
                sleep(Duration::from_millis(ms)).await;
                Ok(None)
            }
            TranslatedAction::Launch { argv } => {
                launch::run(&self.client, display, &argv).await?;
                Ok(None)
            }
            TranslatedAction::FocusTitle { title } => self.focus_title(display, &title).await,
            TranslatedAction::Cua { tool, mut payload } => {
                if tool == "type_text"
                    && let Some(text) = payload.get("text").and_then(Value::as_str)
                    && (!text.is_ascii() || text.contains(['\n', '\r']))
                {
                    clipboard::paste(&self.client, display, text).await?;
                    return Ok(None);
                }

                if tool == "scroll" && payload.get("x").is_none() {
                    let (x, y) = self.scroll_point(display).await;
                    payload["x"] = json!(x);
                    payload["y"] = json!(y);
                }
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
                let reply = self.client.call(display, tool, &payload, &[]).await?;
                Ok(action_verdict(&reply))
            }
        }
    }

    async fn focus_title(
        &self,
        display: &str,
        title: &str,
    ) -> Result<Option<ActionVerdict>, ControlError> {
        let windows = self.windows(display).await?;
        let window = window_matching_title(&windows, title).ok_or(ControlError::TargetNotFound)?;
        let reply = self
            .client
            .call(
                display,
                "bring_to_front",
                &json!({ "pid": window.pid, "window_id": window.id }),
                &[],
            )
            .await?;
        Ok(action_verdict(&reply))
    }
}

/// The more urgent verdict wins: one suspected no-op makes the whole batch
/// unproven, and the model has to hear about that one, not about the steps
/// that happened to report cleanly.
fn merge_verdict(current: &mut Option<ActionVerdict>, step: Option<ActionVerdict>) {
    if let Some(step) = step
        && current
            .as_ref()
            .is_none_or(|best| step.decision > best.decision)
    {
        *current = Some(step);
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

fn map_browser_unavailable(
    result: Result<BrowserPage, ControlError>,
) -> Result<BrowserPage, ControlError> {
    match result {
        Err(ControlError::BrowserUnavailable) => Ok(BrowserPage {
            ok: false,
            error: Some("Cua browser unavailable".into()),
            ..BrowserPage::default()
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
    fn browser_checks_preserve_refs_but_failed_actions_invalidate_them() {
        let mut bind = browser::BrowserBind {
            pid: 1,
            window_id: 2,
            target_id: "target".into(),
            tab_id: "tab".into(),
            page: Some(BrowserPage {
                ok: true,
                title: "original snapshot".into(),
                ..BrowserPage::default()
            }),
        };
        for action in ["probe", "ensure"] {
            update_browser_page(
                &mut bind,
                action,
                &Ok(BrowserPage {
                    ok: true,
                    ..BrowserPage::default()
                }),
            );
            assert_eq!(bind.page.as_ref().unwrap().title, "original snapshot");
        }
        update_browser_page(&mut bind, "click", &Err(ControlError::StaleReference));
        assert!(bind.page.is_none());
        update_browser_page(
            &mut bind,
            "snapshot",
            &Ok(BrowserPage {
                ok: true,
                title: "fresh".into(),
                ..BrowserPage::default()
            }),
        );
        assert_eq!(bind.page.as_ref().unwrap().title, "fresh");
    }

    #[test]
    fn driver_release_reads_the_pinned_minor_series() {
        assert_eq!(driver_release("cua-driver 0.23.2"), Some((0, 23)));
        assert_eq!(
            driver_release("cua-driver 0.23.2 (x86_64-linux)"),
            Some((0, 23))
        );
        assert_eq!(driver_release("cua-driver"), None);
        assert_eq!(driver_release(""), None);
    }

    #[test]
    fn a_new_minor_series_is_not_compatible() {
        assert_ne!(driver_release("cua-driver 0.24.0"), Some(PINNED_DRIVER));
        assert_eq!(driver_release("cua-driver 0.23.9"), Some(PINNED_DRIVER));
    }

    #[test]
    fn an_older_driver_is_named_by_the_verb_flag_it_lacks() {
        let manifest = json!({"subcommands": [
            {"name": "call", "args": [{"name": "tool"}, {"name": "--socket"}]},
            {"name": "status", "args": [{"name": "--socket"}]},
        ]});
        assert_eq!(cli_contract_gap(&manifest), ["call --screenshot-out-file"]);
    }

    #[test]
    fn the_pinned_driver_surface_passes_the_cli_contract() {
        let manifest = json!({"subcommands": [
            {"name": "call", "args": [{"name": "tool"}, {"name": "json-args"},
                {"name": "--screenshot-out-file"}, {"name": "--socket"}]},
            {"name": "status", "args": [{"name": "--socket"}]},
        ]});
        assert!(cli_contract_gap(&manifest).is_empty());
    }

    #[test]
    fn a_missing_tool_is_named_instead_of_a_bare_unhealthy() {
        let all: Vec<String> = REQUIRED_TOOLS.iter().map(|tool| tool.to_string()).collect();
        assert!(missing_tools(&all).is_empty());
        let advertised: Vec<String> = all
            .into_iter()
            .filter(|tool| tool != "get_window_state")
            .collect();
        assert_eq!(missing_tools(&advertised), ["get_window_state"]);
    }

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
