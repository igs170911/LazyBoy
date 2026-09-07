use async_trait::async_trait;
use lazyboy_contracts::{ComputerAction, ComputerObservation, RefVerb};
use tokio::time::{Duration, sleep};

use crate::controller::{
    ComputerController, ComputerDriver, ControlContext, ControlError, ControllerHealth,
};
use crate::process::{
    capture_stdout, run_output, run_stdout_text, run_stdout_text_timeout, spawn_detached,
};
use crate::{
    ActionRequest, ActionResult, BrowserRequest, CdpPage, RecordingRequest, RecordingResult,
    RecordingSession, a11y_command_on, action_pause_ms, cdp_command_on, cdp_record_command_on,
    cdp_record_stop_command, devtools_port, launch_argv_on, observation_from_png,
    observation_with_elements, open_argv_on, parse_a11y_page, parse_cdp_page, parse_pointer_state,
    parse_ui_elements, pointer_state_command_on, screenshot_command_on, teach_recorder_output,
    teach_trajectory_dir, window_list_command_on, xdotool_argv_on,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct LegacyController;

#[async_trait]
impl ComputerController for LegacyController {
    fn backend(&self) -> ComputerDriver {
        ComputerDriver::Legacy
    }

    async fn health(&self, ctx: &ControlContext) -> Result<ControllerHealth, ControlError> {
        Ok(ControllerHealth {
            backend: ComputerDriver::Legacy.as_str().to_string(),
            version: None,
            healthy: true,
            degraded: false,
            details: vec![format!("display {}", ctx.display)],
        })
    }

    async fn observe(&self, ctx: &ControlContext) -> Result<ComputerObservation, ControlError> {
        observe_display(&ctx.display).await
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
            apply_action(display, profile, action).await?;
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
            Some(observe_display(display).await?)
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
        legacy_browser(request, ctx).await
    }

    async fn start_recording(
        &self,
        request: &RecordingRequest,
        ctx: &ControlContext,
    ) -> Result<RecordingSession, ControlError> {
        start_cdp_recorder(&ctx.display, ctx.profile_path.as_deref(), &request.skill_id).await?;
        Ok(RecordingSession {
            skill_id: request.skill_id.clone(),
            output_dir: teach_trajectory_dir(&request.skill_id),
        })
    }

    async fn stop_recording(
        &self,
        request: &RecordingRequest,
        _ctx: &ControlContext,
    ) -> Result<(), ControlError> {
        stop_cdp_recorder(&request.skill_id).await
    }

    async fn collect_recording(
        &self,
        request: &RecordingRequest,
        _ctx: &ControlContext,
    ) -> Result<RecordingResult, ControlError> {
        Ok(RecordingResult {
            events: collect_cdp_events(&request.skill_id).await,
        })
    }
}

async fn legacy_browser(
    request: &BrowserRequest,
    ctx: &ControlContext,
) -> Result<CdpPage, ControlError> {
    let mut body = serde_json::json!({
        "action": request.action,
        "ensure": request.ensure,
        "display": ctx.display,
        "port": devtools_port(&ctx.display),
    });
    if let Some(profile) = &ctx.profile_path {
        body["profile"] = serde_json::json!(profile);
    }
    if let Some(url) = &request.url {
        body["url"] = serde_json::json!(url);
    }
    if let Some(text) = &request.text {
        body["text"] = serde_json::json!(text);
    }
    if let Some(key) = &request.key {
        body["key"] = serde_json::json!(key);
    }
    if let Some(ms) = request.ms {
        body["ms"] = serde_json::json!(ms);
    }
    if let Some(selector) = &request.selector {
        body["selector"] = serde_json::json!(selector);
    }
    if let Some(wait_ms) = request.wait_ms {
        body["waitMs"] = serde_json::json!(wait_ms);
    }
    let timeout_ms = if request.action == "click" {
        request.wait_ms.unwrap_or(45_000).min(120_000) + 25_000
    } else {
        20_000
    };
    let argv = cdp_command_on(&ctx.display, ctx.profile_path.as_deref(), &body);
    let raw = run_stdout_text_timeout(&argv, timeout_ms)
        .await
        .map_err(ControlError::internal)?;
    Ok(parse_cdp_page(&raw))
}

pub async fn observe_display(display: &str) -> Result<ComputerObservation, ControlError> {
    let png = capture_stdout(&screenshot_command_on(display))
        .await
        .map_err(|_| ControlError::Internal("screenshot failed".into()))?;
    let ((cursor, window), elements) =
        tokio::join!(run_pointer_state(display), run_window_list(display));
    Ok(observation_with_elements(
        observation_from_png(png, 1280, 800, cursor, window),
        elements,
    ))
}

async fn run_window_list(display: &str) -> Vec<lazyboy_contracts::UiElement> {
    let Ok(raw) = run_stdout_text(&window_list_command_on(display)).await else {
        return Vec::new();
    };
    parse_ui_elements(&raw)
}

async fn run_pointer_state(
    display: &str,
) -> (
    Option<lazyboy_contracts::CursorPosition>,
    Option<lazyboy_contracts::ActiveWindow>,
) {
    let Ok(raw) = run_stdout_text(&pointer_state_command_on(display)).await else {
        return (None, None);
    };
    parse_pointer_state(&raw)
}

pub async fn apply_action(
    display: &str,
    profile: Option<&str>,
    action: &ComputerAction,
) -> Result<(), ControlError> {
    match action {
        ComputerAction::Wait { ms } => {
            sleep(Duration::from_millis(u64::from(*ms))).await;
            Ok(())
        }
        ComputerAction::Open { path } => spawn_detached(&open_argv_on(display, profile, path))
            .await
            .map_err(ControlError::internal),
        ComputerAction::Focus { .. } => run_xdotool(display, action).await,
        ComputerAction::Launch { application, uri } => {
            let argv = launch_argv_on(display, profile, application, uri.as_deref())
                .ok_or(ControlError::Unsupported)?;
            spawn_detached(&argv).await.map_err(ControlError::internal)
        }
        ComputerAction::Ref {
            verb,
            target,
            ref_kind,
            text,
        } => apply_ref(display, profile, *verb, target, ref_kind, text.as_deref()).await,
        other => run_xdotool(display, other).await,
    }
}

async fn run_xdotool(display: &str, action: &ComputerAction) -> Result<(), ControlError> {
    let argv = xdotool_argv_on(display, action).ok_or(ControlError::Unsupported)?;
    let output = run_output(&argv).await.map_err(ControlError::internal)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(ControlError::internal(String::from_utf8_lossy(
            &output.stderr,
        )))
    }
}

async fn apply_ref(
    display: &str,
    profile: Option<&str>,
    verb: RefVerb,
    target: &str,
    kind: &str,
    text: Option<&str>,
) -> Result<(), ControlError> {
    let action = match verb {
        RefVerb::Click => "click",
        RefVerb::SetValue => "type",
        RefVerb::Focus => "focus",
    };
    let mut request = serde_json::json!({
        "action": action,
        "selector": target,
        "display": display,
        "ensure": false,
    });
    if let Some(text) = text {
        request["text"] = serde_json::json!(text);
    }
    let argv = if kind == "dom" {
        cdp_command_on(display, profile, &request)
    } else {
        a11y_command_on(display, &request)
    };
    let raw = run_stdout_text(&argv)
        .await
        .map_err(ControlError::internal)?;
    let ok = if kind == "dom" {
        parse_cdp_page(&raw).ok
    } else {
        parse_a11y_page(&raw).ok
    };
    if ok {
        Ok(())
    } else {
        Err(ControlError::internal(raw))
    }
}

pub(crate) async fn start_cdp_recorder(
    display: &str,
    profile: Option<&str>,
    skill_id: &str,
) -> Result<(), ControlError> {
    if skill_id.trim().is_empty() {
        return Err(ControlError::InvalidAction(
            "recording needs a skill id".into(),
        ));
    }
    let argv = cdp_record_command_on(display, profile, skill_id);
    run_output(&argv).await.map_err(ControlError::internal)?;
    Ok(())
}

pub(crate) async fn stop_cdp_recorder(skill_id: &str) -> Result<(), ControlError> {
    let argv = cdp_record_stop_command(skill_id);
    let _ = run_output(&argv).await;
    Ok(())
}

pub(crate) async fn collect_cdp_events(skill_id: &str) -> Vec<serde_json::Value> {
    let path = teach_recorder_output(skill_id);
    let Ok(text) = tokio::fs::read_to_string(&path).await else {
        return Vec::new();
    };
    let _ = tokio::fs::remove_file(&path).await;
    text.lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|event| event.get("t").and_then(serde_json::Value::as_str) != Some("recorder"))
        .collect()
}
