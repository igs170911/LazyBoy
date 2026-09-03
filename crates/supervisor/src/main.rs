mod docker;

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use docker::DockerHost;
use lazyboy_control::{
    normalize_workspace_path, ActionRequest, CommandRequest, EnsureScreenRequest, ScreenTarget, HOME,
};
use serde::{Deserialize, Serialize};
use tracing_subscriber::EnvFilter;

#[derive(Clone)]
struct App {
    token: String,
    docker: Arc<DockerHost>,
}

#[derive(Deserialize)]
struct ProvisionBody {
    #[serde(rename = "homeKey")]
    home_key: String,
    #[serde(rename = "homePath")]
    home_path: String,
    #[serde(rename = "spaceId")]
    space_id: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();
    let token = std::env::var("SANDBOX_SUPERVISOR_TOKEN").unwrap_or_else(|_| "dev-token".into());
    let image = std::env::var("LAZYBOY_COMPUTER_IMAGE").unwrap_or_else(|_| "lazyboy/computer:local".into());
    let docker = DockerHost::connect(image, token.clone())
        .await
        .expect("docker");
    let app = App {
        token,
        docker: Arc::new(docker),
    };
    let router = Router::new()
        .route("/health", get(|| async { Json(serde_json::json!({"ok": true})) }))
        .route("/computers", post(provision))
        .route("/computers/{id}/exec", post(exec))
        .route("/computers/{id}/observe", post(observe))
        .route("/computers/{id}/act", post(act))
        .route("/computers/{id}/screens", post(ensure_screen))
        .route("/computers/{id}/screen-mode", post(screen_mode))
        .route("/computers/{id}/files", get(list_files).post(write_file))
        .route("/computers/{id}/read", post(read_file))
        .route("/computers/{id}/stop", post(stop))
        .route("/computers/{id}", delete(destroy))
        .with_state(app);
    let bind = std::env::var("SUPERVISOR_BIND").unwrap_or_else(|_| "0.0.0.0:7091".into());
    let listener = tokio::net::TcpListener::bind(&bind).await.expect("bind");
    tracing::info!("supervisor listening on {bind}");
    axum::serve(listener, router).await.expect("serve");
}

fn require_token(headers: &HeaderMap, token: &str) -> Result<(), StatusCode> {
    let supplied = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if supplied == format!("Bearer {token}") {
        Ok(())
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

fn header_str<'a>(headers: &'a HeaderMap, name: &'static str) -> Option<&'a str> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
}

fn screen_target(headers: &HeaderMap) -> ScreenTarget {
    ScreenTarget::from_parts(
        header_str(headers, "x-lazyboy-display"),
        header_str(headers, "x-lazyboy-profile"),
        header_str(headers, "x-lazyboy-screen-slot").and_then(|value| value.parse().ok()),
    )
}

async fn provision(
    State(app): State<App>,
    headers: HeaderMap,
    Json(body): Json<ProvisionBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    require_token(&headers, &app.token)?;
    let created = app
        .docker
        .provision(&body.home_key, &body.home_path, &body.space_id)
        .await
        .map_err(|error| {
            tracing::error!("provision: {error}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(serde_json::json!({
        "id": created.id,
        "resumed": created.resumed,
        "screenUrl": created.screen_url,
    })))
}

async fn exec(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<CommandRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    require_token(&headers, &app.token)?;
    let result = app
        .docker
        .exec_on(&id, body, &screen_target(&headers))
        .await
        .map_err(|error| {
            tracing::error!("exec: {error}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(serde_json::json!({
        "stdout": result.stdout,
        "stderr": result.stderr,
        "code": result.code,
    })))
}

async fn observe(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    require_token(&headers, &app.token)?;
    let payload = app
        .docker
        .observe_payload(&id, &screen_target(&headers))
        .await
        .map_err(|error| {
            tracing::error!("observe: {error}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(payload.json))
}

async fn act(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ActionRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    require_token(&headers, &app.token)?;
    let mut body = body;
    let target = screen_target(&headers);
    if body.display.is_none() {
        body.display = Some(target.display.clone());
    }
    if body.profile_path.is_none() {
        body.profile_path = target.profile_path.clone();
    }
    let result = app.docker.act(&id, body).await.map_err(|error| {
        tracing::error!("act: {error}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(result))
}

#[derive(Deserialize)]
struct ScreenModeBody {
    interactive: bool,
    #[serde(default)]
    slot: Option<u32>,
}

async fn screen_mode(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ScreenModeBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    require_token(&headers, &app.token)?;
    let slot = body.slot.or(screen_target(&headers).slot.into()).unwrap_or(0);
    let url = app
        .docker
        .screen_url_for(&id, slot, body.interactive)
        .await
        .map_err(|error| {
            tracing::error!("screen: {error}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(serde_json::json!({ "screenUrl": url })))
}

async fn ensure_screen(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<EnsureScreenRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    require_token(&headers, &app.token)?;
    let result = app.docker.ensure_screen(&id, body).await.map_err(|error| {
        tracing::error!("ensure screen: {error}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(serde_json::json!({
        "slot": result.slot,
        "display": result.display,
        "viewPort": result.view_port,
    })))
}

#[derive(Deserialize)]
struct PathQuery {
    path: Option<String>,
}

async fn list_files(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<PathQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    require_token(&headers, &app.token)?;
    let path = query.path.unwrap_or_default();
    let entries = app.docker.list_files(&id, &path).await.map_err(|error| {
        tracing::error!("list: {error}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(serde_json::json!(entries)))
}

#[derive(Deserialize)]
struct FileBody {
    path: String,
    content: Option<String>,
}

async fn write_file(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<FileBody>,
) -> Result<StatusCode, StatusCode> {
    require_token(&headers, &app.token)?;
    let relative = normalize_workspace_path(&body.path).map_err(|_| StatusCode::BAD_REQUEST)?;
    app.docker
        .write_file(&id, &relative, body.content.unwrap_or_default().as_bytes())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn read_file(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<FileBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    require_token(&headers, &app.token)?;
    let relative = normalize_workspace_path(&body.path).map_err(|_| StatusCode::BAD_REQUEST)?;
    let bytes = app
        .docker
        .read_file(&id, &relative)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({
        "path": relative,
        "content": String::from_utf8_lossy(&bytes),
    })))
}

async fn stop(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    require_token(&headers, &app.token)?;
    app.docker.stop(&id).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn destroy(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    require_token(&headers, &app.token)?;
    app.docker.destroy(&id).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
#[allow(dead_code)]
struct _Home(&'static str);

fn _assert_home() {
    let _ = HOME;
}
