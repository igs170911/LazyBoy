use std::process::Stdio;
use std::time::Duration;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use lazyboy_contracts::ComputerAction;
use lazyboy_control::{
    action_pause_ms, launch_argv_on, normalize_display, open_argv_on, parse_pointer_state,
    pointer_state_command_on, screenshot_command_on, xdotool_argv_on, ActionRequest, PRIMARY_DISPLAY,
};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::sleep;

#[derive(Clone)]
struct App {
    token: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let token = std::env::var("LAZYBOY_CONTROL_TOKEN").unwrap_or_default();
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/observe", post(observe))
        .route("/act", post(act))
        .with_state(App { token });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:7070")
        .await
        .expect("bind control port");
    tracing::info!("controld listening on 127.0.0.1:7070");
    axum::serve(listener, app).await.expect("serve");
}

fn authorized(headers: &HeaderMap, token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == format!("Bearer {token}"))
}

fn header_value<'a>(headers: &'a HeaderMap, name: &'static str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok()).filter(|value| !value.is_empty())
}

fn display_of(headers: &HeaderMap, fallback: Option<&str>) -> String {
    normalize_display(
        header_value(headers, "x-lazyboy-display")
            .or(fallback)
            .unwrap_or(PRIMARY_DISPLAY),
    )
    .to_string()
}

fn profile_of(headers: &HeaderMap, fallback: Option<&str>) -> Option<String> {
    header_value(headers, "x-lazyboy-profile")
        .or(fallback)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

async fn observe(State(app): State<App>, headers: HeaderMap) -> Result<Json<serde_json::Value>, StatusCode> {
    if !authorized(&headers, &app.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let display = display_of(&headers, None);
    let png = run_capture(&display)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(observation_json(&display, png).await))
}

async fn act(
    State(app): State<App>,
    headers: HeaderMap,
    Json(request): Json<ActionRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !authorized(&headers, &app.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let display = display_of(&headers, request.display.as_deref());
    let profile = profile_of(&headers, request.profile_path.as_deref());
    let mut completed = 0usize;
    for action in &request.actions {
        apply_action(&display, profile.as_deref(), action)
            .await
            .map_err(|_| StatusCode::BAD_REQUEST)?;
        let pause = action_pause_ms(action);
        if pause > 0 {
            sleep(Duration::from_millis(pause)).await;
        }
        completed += 1;
    }
    if request.settle_ms > 0 {
        sleep(Duration::from_millis(request.settle_ms as u64)).await;
    }
    let mut body = serde_json::json!({ "completed": completed });
    if request.observe {
        let png = run_capture(&display)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if let serde_json::Value::Object(map) = observation_json(&display, png).await {
            body.as_object_mut().unwrap().extend(map);
        }
    }
    Ok(Json(body))
}

async fn apply_action(display: &str, profile: Option<&str>, action: &ComputerAction) -> Result<(), String> {
    match action {
        ComputerAction::Wait { ms } => {
            sleep(Duration::from_millis(*ms as u64)).await;
            Ok(())
        }
        ComputerAction::Open { path } => spawn_detached(&open_argv_on(display, profile, path)).await,
        ComputerAction::Focus { .. } => {
            let argv = xdotool_argv_on(display, action).ok_or_else(|| "unsupported action".to_string())?;
            let output = Command::new(&argv[0])
                .args(&argv[1..])
                .output()
                .await
                .map_err(|error| error.to_string())?;
            if output.status.success() {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&output.stderr).into_owned())
            }
        }
        ComputerAction::Launch { application, uri } => {
            let argv = launch_argv_on(display, profile, application, uri.as_deref())
                .ok_or_else(|| "unknown application".to_string())?;
            spawn_detached(&argv).await
        }
        other => {
            let argv = xdotool_argv_on(display, other).ok_or_else(|| "unsupported action".to_string())?;
            let output = Command::new(&argv[0])
                .args(&argv[1..])
                .output()
                .await
                .map_err(|error| error.to_string())?;
            if output.status.success() {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&output.stderr).into_owned())
            }
        }
    }
}

async fn spawn_detached(argv: &[String]) -> Result<(), String> {
    Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    Ok(())
}

async fn observation_json(display: &str, png: Vec<u8>) -> serde_json::Value {
    use base64::Engine;
    let mut body = serde_json::json!({
        "png_base64": base64::engine::general_purpose::STANDARD.encode(png)
    });
    let (cursor, window) = run_pointer_state(display).await;
    if let Some(cursor) = cursor {
        body["cursor"] = serde_json::json!({ "x": cursor.x, "y": cursor.y });
    }
    if let Some(window) = window {
        body["activeWindow"] = serde_json::json!({ "id": window.id, "title": window.title });
    }
    body
}

async fn run_pointer_state(
    display: &str,
) -> (
    Option<lazyboy_contracts::CursorPosition>,
    Option<lazyboy_contracts::ActiveWindow>,
) {
    let argv = pointer_state_command_on(display);
    let output = Command::new(&argv[0]).args(&argv[1..]).output().await.ok();
    let Some(output) = output else {
        return (None, None);
    };
    parse_pointer_state(&String::from_utf8_lossy(&output.stdout))
}

async fn run_capture(display: &str) -> Result<Vec<u8>, String> {
    let argv = screenshot_command_on(display);
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let mut stdout = Vec::new();
    if let Some(mut pipe) = child.stdout.take() {
        pipe.read_to_end(&mut stdout)
            .await
            .map_err(|error| error.to_string())?;
    }
    let status = child.wait().await.map_err(|error| error.to_string())?;
    if !status.success() || stdout.is_empty() {
        return Err("screenshot failed".into());
    }
    Ok(stdout)
}
