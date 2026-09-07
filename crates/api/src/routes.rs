use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::routing::{any, delete, get, post};
use axum::{Json, Router};
use lazyboy_contracts::{Bot, ComputerMode, CreateBotInput, UpdateBotInput};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::computer;
use crate::db::{Actor, parse_mode};
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .merge(crate::sessions::router())
        .merge(crate::memory::router())
        .merge(crate::monitor::router())
        .merge(crate::rooms::router())
        .merge(crate::mcp::router())
        .merge(crate::workspace::router())
        .merge(crate::voice::router())
        .merge(crate::skills::router())
        .route("/api/file-skills", get(file_skills))
        .merge(crate::vault::router())
        .merge(crate::schedules::router())
        .route("/api/bots", get(list_bots).post(create_bot))
        .route(
            "/api/bots/{id}",
            get(get_bot).patch(update_bot).delete(delete_bot),
        )
        .route("/api/bots/{id}/stop", post(stop_task))
        .route("/api/bots/{id}/inbox", post(update_inbox))
        .route("/api/environments/{id}", delete(delete_environment))
        .route(
            "/api/bots/{id}/messages",
            get(list_messages).post(send_message),
        )
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
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            crate::auth::require_auth,
        ))
        .with_state(state)
}

async fn file_skills(State(state): State<AppState>) -> Json<Vec<crate::file_skills::FileSkill>> {
    Json(crate::file_skills::list(&state.data_dir))
}

async fn actor(state: &AppState) -> Result<Actor, StatusCode> {
    state
        .bootstrap()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn list_bots(State(state): State<AppState>) -> Result<Json<Vec<Bot>>, StatusCode> {
    let actor = actor(&state).await?;
    let rows = state
        .db
        .list_bots(&actor)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(
        rows.into_iter()
            .map(|(bot, thread_id, computer)| Bot {
                id: bot.id,
                space_id: bot.space_id,
                name: bot.name,
                title: bot.title,
                description: bot.description,
                avatar_color: bot.avatar_color,
                avatar_shape: bot.avatar_shape,
                tags: bot.tags,
                pinned: bot.pinned,
                hidden: bot.hidden,
                group_name: bot.group_name,
                unread_count: bot.unread_count,
                last_message_at: bot.last_message_at,
                instructions: bot.instructions,
                thread_id,
                computer_id: computer.id,
                computer_mode: parse_mode(&computer.scope),
                model_provider: bot.model_provider.and_then(|value| value.parse().ok()),
                model_id: bot.model_id,
                memory_enabled: bot.memory_enabled,
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
            input.memory_enabled,
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(bot))
}

async fn get_bot(
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
    let thread_id = crate::sessions::default_session_for_bot(&state, &actor, &id)
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
            avatar_color: bot.avatar_color,
            avatar_shape: bot.avatar_shape,
            tags: bot.tags,
            pinned: bot.pinned,
            hidden: bot.hidden,
            group_name: bot.group_name,
            unread_count: bot.unread_count,
            last_message_at: bot.last_message_at,
            instructions: bot.instructions,
            thread_id,
            computer_id: computer.id,
            computer_mode: parse_mode(&computer.scope),
            model_provider: bot.model_provider.and_then(|value| value.parse().ok()),
            model_id: bot.model_id,
            memory_enabled: bot.memory_enabled,
        },
        "computer": status,
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InboxInput {
    action: String,
    group_name: Option<String>,
}

async fn update_inbox(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<InboxInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let actor = actor(&state)
        .await
        .map_err(|status| (status, Json(json!({"message":"actor"}))))?;
    let query = match input.action.as_str() {
        "read" => "UPDATE bots SET last_read_at=now() WHERE id=$1 AND space_id=$2 AND user_id=$3",
        "unread" => {
            "UPDATE bots SET last_read_at='1970-01-01' WHERE id=$1 AND space_id=$2 AND user_id=$3"
        }
        "pin" => "UPDATE bots SET pinned=TRUE WHERE id=$1 AND space_id=$2 AND user_id=$3",
        "unpin" => "UPDATE bots SET pinned=FALSE WHERE id=$1 AND space_id=$2 AND user_id=$3",
        "hide" => "UPDATE bots SET hidden=TRUE WHERE id=$1 AND space_id=$2 AND user_id=$3",
        "show" => "UPDATE bots SET hidden=FALSE WHERE id=$1 AND space_id=$2 AND user_id=$3",
        "group" => "UPDATE bots SET group_name=$4 WHERE id=$1 AND space_id=$2 AND user_id=$3",
        _ => return Err((StatusCode::BAD_REQUEST, Json(json!({"message":"未知操作"})))),
    };
    let mut statement = sqlx::query(query)
        .bind(&id)
        .bind(&actor.space_id)
        .bind(&actor.user_id);
    if input.action == "group" {
        statement = statement.bind(
            input
                .group_name
                .map(|v| v.trim().chars().take(30).collect::<String>())
                .filter(|v| !v.is_empty()),
        );
    }
    let result = statement
        .execute(state.pool())
        .await
        .map_err(internal_error)?;
    if result.rows_affected() != 1 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"message":"bot not found"})),
        ));
    }
    Ok(Json(json!({"ok":true})))
}

async fn update_bot(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateBotInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let actor = actor(&state)
        .await
        .map_err(|status| (status, Json(json!({"message":"actor"}))))?;
    let name = input.name.trim();
    let color_ok = input.avatar_color.len() == 7
        && input.avatar_color.starts_with('#')
        && input.avatar_color[1..]
            .bytes()
            .all(|c| c.is_ascii_hexdigit());
    let shape_ok = matches!(
        input.avatar_shape.as_str(),
        "round"
            | "organic"
            | "boxy"
            | "capsule"
            | "nub"
            | "cloud"
            | "droplet"
            | "hexagon"
            | "sun"
            | "triangle"
            | "blob"
            | "diamond"
            | "squircle"
            | "drop"
            | "organic-4"
            | "organic-5"
            | "organic-6"
            | "organic-7"
            | "organic-8"
            | "organic-9"
            | "organic-10"
            | "organic-11"
            | "cat"
            | "bunny"
            | "star"
            | "heart"
            | "egg"
            | "ghost"
            | "sprout"
            | "cactus"
            | "mushroom"
            | "paw"
    );
    if name.is_empty() || name.chars().count() > 80 || !color_ok || !shape_ok {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"message":"設定格式不正確"})),
        ));
    }
    let tags: Vec<String> = input
        .tags
        .into_iter()
        .map(|tag| tag.trim().to_string())
        .filter(|tag| !tag.is_empty())
        .take(6)
        .collect();
    let result = sqlx::query(
        "UPDATE bots SET name=$1,title=$2,description=$3,avatar_color=$4,avatar_shape=$5,tags=$6,
                         memory_enabled=COALESCE($7,memory_enabled),updated_at=now()
         WHERE id=$8 AND space_id=$9 AND user_id=$10",
    )
    .bind(name)
    .bind(input.title.trim())
    .bind(input.description.trim())
    .bind(input.avatar_color.to_uppercase())
    .bind(input.avatar_shape)
    .bind(tags)
    .bind(input.memory_enabled)
    .bind(&id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .execute(state.pool())
    .await
    .map_err(internal_error)?;
    if result.rows_affected() != 1 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"message":"bot not found"})),
        ));
    }
    Ok(Json(json!({"ok":true})))
}

async fn delete_bot(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let actor = actor(&state)
        .await
        .map_err(|status| (status, Json(json!({ "message": "actor" }))))?;
    let bot = state
        .db
        .get_bot(&actor, &id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "message": "bot not found" })),
            )
        })?;
    let computer = match bot.computer_id.as_deref() {
        Some(computer_id) => state
            .db
            .get_computer(computer_id)
            .await
            .map_err(internal_error)?,
        None => None,
    };

    // Dedicated computers belong to the bot. Team computers belong to the environment and survive.
    if let Some(computer) = computer
        .as_ref()
        .filter(|row| parse_mode(&row.scope) == ComputerMode::Dedicated)
        && let Some(computer_ref) = computer::computer_ref(computer)
    {
        state
            .sandbox
            .destroy(
                &computer_ref,
                &computer::adapter_context(&actor, &id, "delete-bot"),
            )
            .await
            .map_err(|error| bad_gateway(error.to_string()))?;
    }

    let mut tx = state.pool().begin().await.map_err(internal_error)?;
    sqlx::query("DELETE FROM computer_screens WHERE bot_id = $1")
        .bind(&id)
        .execute(&mut *tx)
        .await
        .map_err(internal_error)?;
    sqlx::query("DELETE FROM computer_profile_locks WHERE bot_id = $1")
        .bind(&id)
        .execute(&mut *tx)
        .await
        .map_err(internal_error)?;
    sqlx::query("DELETE FROM computer_execution_leases WHERE bot_id = $1")
        .bind(&id)
        .execute(&mut *tx)
        .await
        .map_err(internal_error)?;
    sqlx::query("DELETE FROM bots WHERE id = $1 AND space_id = $2 AND user_id = $3")
        .bind(&id)
        .bind(&actor.space_id)
        .bind(&actor.user_id)
        .execute(&mut *tx)
        .await
        .map_err(internal_error)?;
    if let Some(computer) = computer
        .as_ref()
        .filter(|row| parse_mode(&row.scope) == ComputerMode::Dedicated)
    {
        sqlx::query("DELETE FROM computers WHERE id = $1")
            .bind(&computer.id)
            .execute(&mut *tx)
            .await
            .map_err(internal_error)?;
    }
    tx.commit().await.map_err(internal_error)?;

    if let Some(computer) = computer.filter(|row| parse_mode(&row.scope) == ComputerMode::Dedicated)
    {
        remove_home(&state, &computer.home_key).await?;
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_environment(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let actor = actor(&state)
        .await
        .map_err(|status| (status, Json(json!({ "message": "actor" }))))?;
    if id != actor.space_id {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "message": "environment not found" })),
        ));
    }
    let computers: Vec<crate::db::ComputerRow> = sqlx::query_as(
        "SELECT id, space_id, user_id, scope, scope_key, home_key, home_revision, kind, provider_ref, state,
                control_holder, control_lease_id, control_lease_expires_at, control_bot_id, control_run_id,
                execution_run_id, execution_bot_id, execution_lease_expires_at, execution_fence,
                browser_profile_mode FROM computers WHERE space_id = $1 AND user_id = $2",
    )
    .bind(&id)
    .bind(&actor.user_id)
    .fetch_all(state.pool())
    .await
    .map_err(internal_error)?;

    for row in &computers {
        if let Some(computer_ref) = computer::computer_ref(row) {
            state
                .sandbox
                .destroy(
                    &computer_ref,
                    &computer::adapter_context(&actor, "environment", "delete-environment"),
                )
                .await
                .map_err(|error| bad_gateway(error.to_string()))?;
        }
    }
    let deleted = sqlx::query("DELETE FROM spaces WHERE id = $1 AND user_id = $2")
        .bind(&id)
        .bind(&actor.user_id)
        .execute(state.pool())
        .await
        .map_err(internal_error)?;
    if deleted.rows_affected() != 1 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "message": "environment not found" })),
        ));
    }
    for row in computers {
        remove_home(&state, &row.home_key).await?;
    }
    Ok(Json(json!({ "ok": true })))
}

fn internal_error(error: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    tracing::error!("delete: {error}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "message": "internal error" })),
    )
}

fn bad_gateway(message: String) -> (StatusCode, Json<Value>) {
    tracing::error!("delete sandbox: {message}");
    (StatusCode::BAD_GATEWAY, Json(json!({ "message": message })))
}

async fn remove_home(state: &AppState, home_key: &str) -> Result<(), (StatusCode, Json<Value>)> {
    let path = computer::home_path(&state.data_dir, home_key);
    match tokio::fs::remove_dir_all(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(internal_error(error)),
    }
}

#[derive(Deserialize)]
struct SendBody {
    text: String,
    #[serde(rename = "clientNonce")]
    client_nonce: Option<String>,
    #[serde(default)]
    blocks: Vec<Value>,
    #[serde(default)]
    attachments: Vec<lazyboy_contracts::SessionAttachment>,
}

async fn send_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SendBody>,
) -> Result<Json<Value>, StatusCode> {
    let actor = actor(&state).await?;
    let _ = state
        .db
        .get_bot(&actor, &id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let thread_id = crate::sessions::default_session_for_bot(&state, &actor, &id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    crate::runs::send(
        &state,
        &actor,
        &id,
        &thread_id,
        &body.text,
        body.client_nonce.as_deref(),
        &body.blocks,
        &body.attachments,
    )
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
    let thread_id = crate::sessions::default_session_for_bot(&state, &actor, &id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let rows = crate::sessions::messages_for_session(&state, &actor, &thread_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!(rows)))
}

async fn stop_task(
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
    crate::runs::cancel_active_runs(&state, &id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "ok": true })))
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

async fn boot(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
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

async fn restart(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let actor = actor(&state).await?;
    computer::restart(&state, &actor, &id)
        .await
        .map(|status| Json(serde_json::to_value(status).unwrap()))
        .map_err(|error| {
            tracing::error!("restart: {error}");
            StatusCode::BAD_REQUEST
        })
}

async fn stop(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let actor = actor(&state)
        .await
        .map_err(|status| (status, Json(json!({"message":"無法取得工作區"}))))?;
    computer::stop(&state, &actor, &id)
        .await
        .map(|status| Json(serde_json::to_value(status).unwrap()))
        .map_err(|error| (StatusCode::BAD_REQUEST, Json(json!({"message":error}))))
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
    let screen = match computer::ensure_bot_screen(&state, &actor, &id, &computer, None).await {
        Ok(bound) => bound.row,
        Err(error) => {
            tracing::warn!("screen url ensure: {error}");
            state.db.get_screen(&computer.id, &id).await.ok().flatten()
        }
    };
    let interactive = computer::user_has_screen_control(&computer, screen.as_ref(), &id);
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
        "url": format!("/view/{id}/vnc.html")
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
        Ok((lease_id, expires_at)) => Ok(Json(
            json!({ "leaseId": lease_id, "expiresAt": expires_at }),
        )),
        Err(error) => Err((StatusCode::BAD_REQUEST, Json(json!({ "message": error })))),
    }
}

async fn release(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let actor = actor(&state).await?;
    computer::release(&state, &actor, &id)
        .await
        .map(|_| Json(json!({ "ok": true })))
        .map_err(|_| StatusCode::BAD_REQUEST)
}

async fn heartbeat(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
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
    if body.kind == "clipboard" || body.kind == "copy" {
        let text = body.text.unwrap_or_default();
        if text.len() > 1024 * 1024 {
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }
        let context =
            computer::adapter_context_for(&actor, &id, "clipboard", screen.as_ref(), None);
        let mut argv =
            lazyboy_control::paste_command_on(context.display.as_deref().unwrap_or(":1"));
        if body.kind == "copy" {
            argv.push("copy".into());
        }
        let result = state
            .sandbox
            .execute(
                &computer_ref,
                lazyboy_control::CommandRequest {
                    argv,
                    cwd: None,
                    timeout_ms: Some(10_000),
                    stdin: Some(text),
                },
                &context,
            )
            .await
            .map_err(|_| StatusCode::BAD_GATEWAY)?;
        if result.code != 0 {
            return Err(StatusCode::BAD_GATEWAY);
        }
        return Ok(Json(
            json!({"ok":true,"text":if body.kind=="copy" {Some(result.stdout)} else {None}}),
        ));
    }
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
