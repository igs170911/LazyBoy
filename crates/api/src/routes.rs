use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{any, get, post};
use axum::{Json, Router};
use lazyboy_contracts::{Bot, ComputerMode, CreateBotInput};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::computer;
use crate::db::{parse_mode, Actor};
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(|| async { Json(json!({"ok": true})) }))
        .route("/api/bots", get(list_bots).post(create_bot))
        .route("/api/bots/{id}", get(get_bot))
        .route("/api/bots/{id}/messages", get(list_messages).post(send_message))
        .route("/api/computer/{id}/status", get(computer_status))
        .route("/api/computer/{id}/boot", post(boot))
        .route("/api/computer/{id}/restart", post(restart))
        .route("/api/computer/{id}/stop", post(stop))
        .route("/api/computer/{id}/screen", get(screen_url))
        .route("/api/computer/{id}/takeover", post(takeover))
        .route("/api/computer/{id}/release", post(release))
        .route("/api/computer/{id}/heartbeat", post(heartbeat))
        .route("/api/computer/{id}/input", post(input))
        .route("/view/{id}/", any(crate::screen_proxy::view_root))
        .route("/view/{id}/{*rest}", any(crate::screen_proxy::view_path))
        .with_state(state)
}

async fn actor(state: &AppState) -> Result<Actor, StatusCode> {
    state.bootstrap().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn list_bots(State(state): State<AppState>) -> Result<Json<Vec<Bot>>, StatusCode> {
    let actor = actor(&state).await?;
    let rows = state.db.list_bots(&actor).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(
        rows.into_iter()
            .map(|(bot, thread_id, computer)| Bot {
                id: bot.id,
                space_id: bot.space_id,
                name: bot.name,
                title: bot.title,
                description: bot.description,
                instructions: bot.instructions,
                thread_id,
                computer_id: computer.id,
                computer_mode: parse_mode(&computer.scope),
                model_provider: bot.model_provider.and_then(|value| value.parse().ok()),
                model_id: bot.model_id,
            })
            .collect(),
    ))
}

async fn create_bot(
    State(state): State<AppState>,
    Json(input): Json<CreateBotInput>,
) -> Result<Json<Bot>, StatusCode> {
    let actor = actor(&state).await?;
    if input.name.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let bot = state
        .db
        .create_bot(
            &actor,
            input.name.trim(),
            &input.title,
            &input.description,
            &input.instructions,
            input.computer_mode,
            input.model_provider.map(|provider| provider.as_str()),
            input.model_id.as_deref(),
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(bot))
}

async fn get_bot(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, StatusCode> {
    let actor = actor(&state).await?;
    let bot = state
        .db
        .get_bot(&actor, &id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let thread_id = state
        .db
        .thread_id_for_bot(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let computer = state
        .db
        .get_computer(bot.computer_id.as_deref().unwrap_or(""))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let screen = state.db.get_screen(&computer.id, &id).await.ok().flatten();
    let status = computer::status_from(&id, &computer, screen.as_ref(), None);
    Ok(Json(json!({
        "bot": Bot {
            id: bot.id,
            space_id: bot.space_id,
            name: bot.name,
            title: bot.title,
            description: bot.description,
            instructions: bot.instructions,
            thread_id,
            computer_id: computer.id,
            computer_mode: parse_mode(&computer.scope),
            model_provider: bot.model_provider.and_then(|value| value.parse().ok()),
            model_id: bot.model_id,
        },
        "computer": status,
    })))
}

#[derive(Deserialize)]
struct SendBody {
    text: String,
}

async fn send_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SendBody>,
) -> Result<Json<Value>, StatusCode> {
    crate::runs::send(&state, &id, &body.text)
        .await
        .map_err(|error| {
            tracing::error!("send: {error}");
            StatusCode::BAD_REQUEST
        })
        .map(Json)
}

async fn list_messages(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let actor = actor(&state).await?;
    let _ = state
        .db
        .get_bot(&actor, &id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let thread_id = state
        .db
        .thread_id_for_bot(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let rows: Vec<(String, String, String, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT id, role, body, created_at FROM messages WHERE thread_id = $1 ORDER BY created_at ASC",
    )
    .bind(thread_id)
    .fetch_all(state.pool())
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!(rows
        .into_iter()
        .map(|(id, role, body, created_at)| json!({
            "id": id,
            "role": role,
            "body": body,
            "createdAt": created_at,
        }))
        .collect::<Vec<_>>())))
}

async fn computer_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let actor = actor(&state).await?;
    computer::current_status(&state, &actor, &id)
        .await
        .map(|status| Json(serde_json::to_value(status).unwrap()))
        .map_err(|_| StatusCode::NOT_FOUND)
}

async fn boot(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, StatusCode> {
    let actor = actor(&state).await?;
    computer::boot(&state, &actor, &id)
        .await
        .map(|status| Json(serde_json::to_value(status).unwrap()))
        .map_err(|error| {
            tracing::error!("boot: {error}");
            if error.contains("busy") {
                StatusCode::CONFLICT
            } else {
                StatusCode::BAD_REQUEST
            }
        })
}

async fn restart(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, StatusCode> {
    let actor = actor(&state).await?;
    computer::restart(&state, &actor, &id)
        .await
        .map(|status| Json(serde_json::to_value(status).unwrap()))
        .map_err(|error| {
            tracing::error!("restart: {error}");
            StatusCode::BAD_REQUEST
        })
}

async fn stop(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, StatusCode> {
    let actor = actor(&state).await?;
    computer::stop(&state, &actor, &id)
        .await
        .map(|status| Json(serde_json::to_value(status).unwrap()))
        .map_err(|_| StatusCode::BAD_REQUEST)
}

async fn screen_url(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let actor = actor(&state).await?;
    let bot = state
        .db
        .get_bot(&actor, &id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let computer = state
        .db
        .get_computer(bot.computer_id.as_deref().unwrap_or(""))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    if computer.state != "running" {
        return Ok(Json(json!({ "url": null })));
    }
    let Some(computer_ref) = computer::computer_ref(&computer) else {
        return Ok(Json(json!({ "url": null })));
    };
    let waiting = state
        .db
        .active_run(&id)
        .await
        .ok()
        .flatten()
        .and_then(|(_, status)| crate::db::parse_run_status(&status));
    let screen = match computer::ensure_bot_screen(&state, &actor, &id, &computer, None).await {
        Ok(bound) => bound.row,
        Err(error) => {
            tracing::warn!("screen url ensure: {error}");
            state.db.get_screen(&computer.id, &id).await.ok().flatten()
        }
    };
    let bot_driving = screen
        .as_ref()
        .and_then(|row| row.execution_run_id.as_ref())
        .is_some()
        && screen
            .as_ref()
            .and_then(|row| row.execution_lease_expires_at)
            .is_some_and(|expires| expires > chrono::Utc::now())
        && waiting != Some(lazyboy_contracts::RunStatus::WaitingTakeover);
    // Idle screens are clickable. View-only only while this bot is driving the GUI.
    let interactive = !bot_driving;
    let _ = state
        .sandbox
        .connect_screen(
            &computer_ref,
            interactive,
            &computer::adapter_context_for(&actor, &id, "screen", screen.as_ref(), None),
        )
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(Json(json!({
        "url": format!("/view/{id}/vnc.html?view_only={}", if interactive { "false" } else { "true" })
    })))
}

async fn takeover(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let actor = actor(&state)
        .await
        .map_err(|status| (status, Json(json!({"message": "actor"}))))?;
    match computer::takeover(&state, &actor, &id).await {
        Ok((lease_id, expires_at)) => Ok(Json(json!({ "leaseId": lease_id, "expiresAt": expires_at }))),
        Err(error) if error.contains("Stop the bot") => {
            Err((StatusCode::CONFLICT, Json(json!({ "message": error }))))
        }
        Err(error) => Err((StatusCode::BAD_REQUEST, Json(json!({ "message": error })))),
    }
}

async fn release(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, StatusCode> {
    let actor = actor(&state).await?;
    computer::release(&state, &actor, &id)
        .await
        .map(|_| Json(json!({ "ok": true })))
        .map_err(|_| StatusCode::BAD_REQUEST)
}

async fn heartbeat(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, StatusCode> {
    let actor = actor(&state).await?;
    computer::heartbeat(&state, &actor, &id)
        .await
        .map(|_| Json(json!({ "ok": true })))
        .map_err(|_| StatusCode::BAD_REQUEST)
}

#[derive(Deserialize)]
struct InputBody {
    kind: String,
    x: Option<u32>,
    y: Option<u32>,
    key: Option<String>,
    text: Option<String>,
}

async fn input(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<InputBody>,
) -> Result<Json<Value>, StatusCode> {
    let actor = actor(&state).await?;
    let bot = state
        .db
        .get_bot(&actor, &id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let computer = state
        .db
        .get_computer(bot.computer_id.as_deref().unwrap_or(""))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let screen = state.db.get_screen(&computer.id, &id).await.ok().flatten();
    if !computer::user_has_screen_control(&computer, screen.as_ref(), &id) {
        return Err(StatusCode::CONFLICT);
    }
    let computer_ref = computer::computer_ref(&computer).ok_or(StatusCode::BAD_REQUEST)?;
    let action = match body.kind.as_str() {
        "key" => lazyboy_contracts::ComputerAction::Key {
            key: body.key.unwrap_or_default(),
            modifiers: None,
        },
        "clipboard" => lazyboy_contracts::ComputerAction::Clipboard {
            text: body.text.unwrap_or_default(),
        },
        _ => lazyboy_contracts::ComputerAction::Pointer {
            x: body.x.unwrap_or(0),
            y: body.y.unwrap_or(0),
            pointer_type: lazyboy_contracts::PointerType::Click,
            button: Some(lazyboy_contracts::PointerButton::Left),
        },
    };
    state
        .sandbox
        .act(
            &computer_ref,
            lazyboy_control::ActionRequest {
                actions: vec![action],
                observe: false,
                settle_ms: 0,
                display: screen.as_ref().map(|row| row.display.clone()),
                profile_path: screen.as_ref().map(|row| row.profile_path.clone()),
            },
            &computer::adapter_context_for(&actor, &id, "input", screen.as_ref(), None),
        )
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(Json(json!({ "ok": true })))
}

fn _mode(mode: ComputerMode) {
    let _ = mode;
}
