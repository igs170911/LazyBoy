use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use lazyboy_control::{
    ActionRequest, BrowserRequest, ComputerController, ComputerDriver, ControlContext,
    ControlError, PRIMARY_DISPLAY, RecordingRequest, normalize_display,
    observation_to_control_json,
};

#[derive(Clone)]
struct App {
    token: String,
    controller: Arc<dyn ComputerController>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let token = std::env::var("LAZYBOY_CONTROL_TOKEN").unwrap_or_default();
    let driver = ComputerDriver::from_env();
    tracing::info!(backend = driver.as_str(), "computer controller");
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/controller/health", get(controller_health))
        .route("/observe", post(observe))
        .route("/act", post(act))
        .route("/browser", post(browser))
        .route("/recording/start", post(recording_start))
        .route("/recording/stop", post(recording_stop))
        .route("/recording/collect", post(recording_collect))
        .with_state(App {
            token,
            controller: driver.controller(),
        });
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
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
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

struct ControlFailure(StatusCode, String);

impl IntoResponse for ControlFailure {
    fn into_response(self) -> Response {
        (
            self.0,
            Json(serde_json::json!({ "ok": false, "error": self.1 })),
        )
            .into_response()
    }
}

impl From<StatusCode> for ControlFailure {
    fn from(status: StatusCode) -> Self {
        Self(status, status.to_string())
    }
}

fn status_for(error: &ControlError) -> ControlFailure {
    let status = if error.is_client_error() {
        StatusCode::BAD_REQUEST
    } else if matches!(error, ControlError::Timeout) {
        StatusCode::GATEWAY_TIMEOUT
    } else {
        tracing::error!(error = %error, "control failed");
        StatusCode::INTERNAL_SERVER_ERROR
    };
    ControlFailure(status, error.to_string())
}

async fn controller_health(
    State(app): State<App>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ControlFailure> {
    if !authorized(&headers, &app.token) {
        return Err(StatusCode::UNAUTHORIZED.into());
    }
    let ctx = ControlContext::new(display_of(&headers, None), None);
    let health = app
        .controller
        .health(&ctx)
        .await
        .map_err(|error| status_for(&error))?;
    Ok(Json(serde_json::json!({
        "backend": health.backend,
        "version": health.version,
        "healthy": health.healthy,
        "degraded": health.degraded,
        "details": health.details,
    })))
}

async fn observe(
    State(app): State<App>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ControlFailure> {
    if !authorized(&headers, &app.token) {
        return Err(StatusCode::UNAUTHORIZED.into());
    }
    let ctx = ControlContext::new(display_of(&headers, None), None);
    match app.controller.observe(&ctx).await {
        Ok(observation) => Ok(Json(observation_to_control_json(&observation))),
        Err(error) => Err(status_for(&error)),
    }
}

async fn act(
    State(app): State<App>,
    headers: HeaderMap,
    Json(request): Json<ActionRequest>,
) -> Result<Json<serde_json::Value>, ControlFailure> {
    if !authorized(&headers, &app.token) {
        return Err(StatusCode::UNAUTHORIZED.into());
    }
    let ctx = ControlContext::new(
        display_of(&headers, request.display.as_deref()),
        profile_of(&headers, request.profile_path.as_deref()),
    );
    match app.controller.act(&request, &ctx).await {
        Ok(result) => {
            let mut body = serde_json::json!({ "completed": result.completed, "clipboardText": result.clipboard_text });
            if let Some(observation) = result.observation
                && let serde_json::Value::Object(map) = observation_to_control_json(&observation)
            {
                body.as_object_mut().expect("object").extend(map);
            }
            Ok(Json(body))
        }
        Err(error) => Err(status_for(&error)),
    }
}

async fn browser(
    State(app): State<App>,
    headers: HeaderMap,
    Json(request): Json<BrowserRequest>,
) -> Result<Json<serde_json::Value>, ControlFailure> {
    if !authorized(&headers, &app.token) {
        return Err(StatusCode::UNAUTHORIZED.into());
    }
    let ctx = ControlContext::new(
        display_of(&headers, request.display.as_deref()),
        profile_of(&headers, request.profile_path.as_deref()),
    );
    match app.controller.browser(&request, &ctx).await {
        Ok(page) => Ok(Json(
            serde_json::to_value(&page).unwrap_or_else(|_| serde_json::json!({"ok": false})),
        )),
        Err(error) => Err(status_for(&error)),
    }
}

fn recording_ctx(headers: &HeaderMap, request: &RecordingRequest) -> ControlContext {
    ControlContext::new(
        display_of(headers, request.display.as_deref()),
        profile_of(headers, request.profile_path.as_deref()),
    )
}

async fn recording_start(
    State(app): State<App>,
    headers: HeaderMap,
    Json(request): Json<RecordingRequest>,
) -> Result<Json<serde_json::Value>, ControlFailure> {
    if !authorized(&headers, &app.token) {
        return Err(StatusCode::UNAUTHORIZED.into());
    }
    match app
        .controller
        .start_recording(&request, &recording_ctx(&headers, &request))
        .await
    {
        Ok(session) => Ok(Json(
            serde_json::to_value(&session).unwrap_or_else(|_| serde_json::json!({"ok": false})),
        )),
        Err(error) => Err(status_for(&error)),
    }
}

async fn recording_stop(
    State(app): State<App>,
    headers: HeaderMap,
    Json(request): Json<RecordingRequest>,
) -> Result<Json<serde_json::Value>, ControlFailure> {
    if !authorized(&headers, &app.token) {
        return Err(StatusCode::UNAUTHORIZED.into());
    }
    match app
        .controller
        .stop_recording(&request, &recording_ctx(&headers, &request))
        .await
    {
        Ok(()) => Ok(Json(serde_json::json!({ "ok": true }))),
        Err(error) => Err(status_for(&error)),
    }
}

async fn recording_collect(
    State(app): State<App>,
    headers: HeaderMap,
    Json(request): Json<RecordingRequest>,
) -> Result<Json<serde_json::Value>, ControlFailure> {
    if !authorized(&headers, &app.token) {
        return Err(StatusCode::UNAUTHORIZED.into());
    }
    match app
        .controller
        .collect_recording(&request, &recording_ctx(&headers, &request))
        .await
    {
        Ok(result) => Ok(Json(
            serde_json::to_value(&result).unwrap_or_else(|_| serde_json::json!({"events": []})),
        )),
        Err(error) => Err(status_for(&error)),
    }
}
