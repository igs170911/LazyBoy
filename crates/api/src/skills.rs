//! Skills taught by demonstration.
//!
//! Flow: the human presses "teach it a task", takes control of the bot's
//! desktop and does the task once. Meanwhile a CDP recorder inside the
//! desktop logs *semantic* browser events (which control was clicked, what
//! text went into which field, which URL loaded) and a poller keeps window
//! titles plus a few keyframes. When the human stops, a model distils the
//! trace into an intent-level playbook (goal, inputs, steps described by
//! meaning, how to verify). Later runs get the playbook and execute it with the
//! ordinary tools, locating controls on the live screen instead of replaying
//! coordinates.

use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use base64::Engine;
use chrono::{DateTime, TimeDelta, Utc};
use lazyboy_control::{
    AdapterContext, CommandRequest, ComputerRef, cdp_record_command_on, cdp_record_stop_command,
    frame_signature, signatures_similar, teach_recorder_output,
};
use rig_core::completion::message::{
    AssistantContent, ImageDetail, ImageMediaType, Message, UserContent,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::computer;
use crate::db::Actor;
use crate::state::AppState;

const TEACH_TTL_MINUTES: i64 = 20;
const POLL_MS: u64 = 1500;
const MAX_FRAMES: usize = 60;
const MAX_EVENTS: usize = 800;
const DISTILL_FRAMES: usize = 8;

type ApiError = (StatusCode, Json<Value>);

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/bots/{id}/skills", get(list_skills))
        .route("/api/bots/{id}/skills/start", post(start_skill))
        .route("/api/bots/{id}/skills/stop", post(stop_skill))
        .route("/api/bots/{id}/skills/cancel", post(cancel_skill))
        .route("/api/bots/{id}/skills/import", post(import_skill))
        .route("/api/skills/{id}", patch(update_skill).delete(delete_skill))
        .route("/api/skills/{id}/export", get(export_skill))
        .route("/api/skills/{id}/test", post(test_skill))
}

#[derive(Debug, Clone, FromRow)]
pub struct SkillRow {
    pub id: String,
    pub bot_id: String,
    pub thread_id: Option<String>,
    pub name: String,
    pub goal: String,
    pub status: String,
    pub playbook: Value,
    pub recording: Value,
    pub error: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub stopped_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub id: String,
    pub bot_id: String,
    pub thread_id: Option<String>,
    pub name: String,
    pub goal: String,
    pub status: String,
    pub playbook: Value,
    pub error: Option<String>,
    pub started_at: Option<String>,
    pub expires_at: Option<String>,
    pub stopped_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub event_count: usize,
    pub frame_count: usize,
}

impl From<SkillRow> for Skill {
    fn from(row: SkillRow) -> Self {
        let count = |key: &str| {
            row.recording
                .get(key)
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0)
        };
        Skill {
            event_count: count("events"),
            frame_count: count("frames"),
            id: row.id,
            bot_id: row.bot_id,
            thread_id: row.thread_id,
            name: row.name,
            goal: row.goal,
            status: row.status,
            playbook: row.playbook,
            error: row.error,
            started_at: row.started_at.map(|at| at.to_rfc3339()),
            expires_at: row.expires_at.map(|at| at.to_rfc3339()),
            stopped_at: row.stopped_at.map(|at| at.to_rfc3339()),
            created_at: row.created_at.to_rfc3339(),
            updated_at: row.updated_at.to_rfc3339(),
        }
    }
}

const COLUMNS: &str = "id, bot_id, thread_id, name, goal, status, playbook, recording, error,
                       started_at, expires_at, stopped_at, created_at, updated_at";
const SELECT: &str = "SELECT id, bot_id, thread_id, name, goal, status, playbook, recording, error,
                      started_at, expires_at, stopped_at, created_at, updated_at FROM taught_skills";

async fn actor(state: &AppState) -> Result<Actor, ApiError> {
    state
        .bootstrap()
        .await
        .map_err(|error| internal(error.to_string()))
}

fn internal(message: String) -> ApiError {
    tracing::error!("skills: {message}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "message": message })),
    )
}

fn bad_request(message: impl Into<String>) -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "message": message.into() })),
    )
}

fn conflict(message: impl Into<String>) -> ApiError {
    (
        StatusCode::CONFLICT,
        Json(json!({ "message": message.into() })),
    )
}

fn not_found() -> ApiError {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "message": "not found" })),
    )
}

async fn load_skill(state: &AppState, actor: &Actor, skill_id: &str) -> Result<SkillRow, ApiError> {
    sqlx::query_as::<_, SkillRow>(&format!(
        "{SELECT} WHERE id = $1 AND space_id = $2 AND user_id = $3"
    ))
    .bind(skill_id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .fetch_optional(state.pool())
    .await
    .map_err(|error| internal(error.to_string()))?
    .ok_or_else(not_found)
}

pub async fn recording_skill(pool: &PgPool, bot_id: &str) -> Option<SkillRow> {
    sqlx::query_as::<_, SkillRow>(&format!(
        "{SELECT} WHERE bot_id = $1 AND status = 'recording'"
    ))
    .bind(bot_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
}

async fn list_skills(
    State(state): State<AppState>,
    Path(bot_id): Path<String>,
) -> Result<Json<Vec<Skill>>, ApiError> {
    let actor = actor(&state).await?;
    let rows = sqlx::query_as::<_, SkillRow>(&format!(
        "{SELECT} WHERE bot_id = $1 AND space_id = $2 AND user_id = $3
         ORDER BY CASE status WHEN 'recording' THEN 0 WHEN 'drafting' THEN 1 WHEN 'draft' THEN 2 ELSE 3 END,
                  updated_at DESC"
    ))
    .bind(&bot_id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .fetch_all(state.pool())
    .await
    .map_err(|error| internal(error.to_string()))?;
    Ok(Json(rows.into_iter().map(Skill::from).collect()))
}

#[derive(Deserialize)]
struct StartBody {
    goal: String,
}

async fn start_skill(
    State(state): State<AppState>,
    Path(bot_id): Path<String>,
    Json(body): Json<StartBody>,
) -> Result<Json<Skill>, ApiError> {
    let actor = actor(&state).await?;
    let goal = body.goal.trim().to_string();
    if goal.is_empty() {
        return Err(bad_request("goal required"));
    }
    let bot = state
        .db
        .get_bot(&actor, &bot_id)
        .await
        .map_err(|error| internal(error.to_string()))?
        .ok_or_else(not_found)?;
    if recording_skill(state.pool(), &bot_id).await.is_some() {
        return Err(conflict("already recording"));
    }
    crate::runs::cancel_active_runs(&state, &bot_id)
        .await
        .map_err(internal)?;
    computer::boot(&state, &actor, &bot_id)
        .await
        .map_err(|error| bad_request(format!("boot: {error}")))?;
    computer::takeover(&state, &actor, &bot_id)
        .await
        .map_err(|error| bad_request(format!("takeover: {error}")))?;
    let computer = state
        .db
        .get_computer(bot.computer_id.as_deref().unwrap_or(""))
        .await
        .map_err(|error| internal(error.to_string()))?
        .ok_or_else(|| bad_request("computer not found"))?;
    let computer_ref =
        computer::computer_ref(&computer).ok_or_else(|| bad_request("computer is not running"))?;
    let screen = state
        .db
        .get_screen(&computer.id, &bot_id)
        .await
        .map_err(|error| internal(error.to_string()))?;
    let thread_id = crate::sessions::default_session_for_bot(&state, &actor, &bot_id)
        .await
        .map_err(|error| internal(error.to_string()))?;

    let skill_id = Uuid::new_v4().to_string();
    let now = Utc::now();
    let expires = now + TimeDelta::minutes(TEACH_TTL_MINUTES);
    let row = sqlx::query_as::<_, SkillRow>(&format!(
        "INSERT INTO taught_skills (id, space_id, user_id, bot_id, thread_id, goal, status, started_at, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, 'recording', $7, $8)
         RETURNING {COLUMNS}"
    ))
    .bind(&skill_id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .bind(&bot_id)
    .bind(&thread_id)
    .bind(&goal)
    .bind(now)
    .bind(expires)
    .fetch_one(state.pool())
    .await
    .map_err(|error| {
        if error.to_string().contains("taught_skills_one_recording") {
            conflict("already recording")
        } else {
            internal(error.to_string())
        }
    })?;

    let ctx = computer::adapter_context_for(&actor, &bot_id, "teach", screen.as_ref(), None);
    let display = ctx.display.clone().unwrap_or_else(|| ":1".into());
    let argv = cdp_record_command_on(&display, ctx.profile_path.as_deref(), &skill_id);
    if let Err(error) = state
        .sandbox
        .execute(
            &computer_ref,
            CommandRequest {
                argv,
                cwd: None,
                timeout_ms: Some(10_000),
            },
            &ctx,
        )
        .await
    {
        tracing::warn!("teach {skill_id}: recorder start failed: {error}");
    }

    if let Some(thread_id) = &thread_id {
        let _ = crate::runs::append_bot_message(
            &state,
            thread_id,
            &skill_id,
            &bot_id,
            &format!(
                "開始學習：{goal}\n畫面交給你了，請直接在上面示範一次。我會記錄你點了哪些控制項、輸入了什麼、去了哪些頁面（密碼欄位不會記錄）。做完請按「完成示範」。"
            ),
        )
        .await;
    }

    tokio::spawn(record_loop(
        state.clone(),
        actor.clone(),
        skill_id.clone(),
        bot_id.clone(),
        computer_ref,
        ctx,
    ));
    Ok(Json(row.into()))
}

async fn stop_skill(
    State(state): State<AppState>,
    Path(bot_id): Path<String>,
) -> Result<Json<Skill>, ApiError> {
    let actor = actor(&state).await?;
    let row = recording_skill(state.pool(), &bot_id)
        .await
        .ok_or_else(|| bad_request("not recording"))?;
    let (computer_ref, ctx) = teach_target(&state, &actor, &bot_id)
        .await
        .map_err(bad_request)?;
    // Distillation takes a model round-trip (tens of seconds); the UI polls
    // for the `drafting → draft` transition instead of holding the request.
    tokio::spawn({
        let state = state.clone();
        let actor = actor.clone();
        let row = row.clone();
        async move { finalize(&state, &actor, &row, computer_ref, ctx, false).await }
    });
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Ok(row) = load_skill(&state, &actor, &row.id).await
            && row.status != "recording"
        {
            return Ok(Json(row.into()));
        }
    }
    let row = load_skill(&state, &actor, &row.id).await?;
    Ok(Json(row.into()))
}

async fn cancel_skill(
    State(state): State<AppState>,
    Path(bot_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let actor = actor(&state).await?;
    let Some(row) = recording_skill(state.pool(), &bot_id).await else {
        return Ok(Json(json!({ "ok": true })));
    };
    sqlx::query("UPDATE taught_skills SET status = 'cancelled', stopped_at = now(), updated_at = now() WHERE id = $1")
        .bind(&row.id)
        .execute(state.pool())
        .await
        .map_err(|error| internal(error.to_string()))?;
    if let Ok((computer_ref, ctx)) = teach_target(&state, &actor, &bot_id).await {
        stop_recorder(&state, &computer_ref, &ctx, &row.id).await;
    }
    let _ = computer::release(&state, &actor, &bot_id).await;
    let _ = tokio::fs::remove_dir_all(frames_dir(&state, &row.id)).await;
    sqlx::query("DELETE FROM taught_skills WHERE id = $1")
        .bind(&row.id)
        .execute(state.pool())
        .await
        .map_err(|error| internal(error.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateBody {
    name: Option<String>,
    playbook: Option<Value>,
    #[serde(default)]
    save: bool,
}

async fn update_skill(
    State(state): State<AppState>,
    Path(skill_id): Path<String>,
    Json(body): Json<UpdateBody>,
) -> Result<Json<Skill>, ApiError> {
    let actor = actor(&state).await?;
    let row = load_skill(&state, &actor, &skill_id).await?;
    if !matches!(row.status.as_str(), "draft" | "saved") {
        return Err(conflict("skill is not ready yet"));
    }
    let mut playbook = body.playbook.unwrap_or(row.playbook.clone());
    let name = body
        .name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| Some(row.name.clone()).filter(|value| !value.is_empty()))
        .or_else(|| {
            playbook
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| row.goal.chars().take(30).collect());
    if let Some(object) = playbook.as_object_mut() {
        object.insert("name".into(), json!(name));
    }
    let status = if body.save {
        "saved"
    } else {
        row.status.as_str()
    };
    let row = sqlx::query_as::<_, SkillRow>(&format!(
        "UPDATE taught_skills SET name = $2, playbook = $3, status = $4, updated_at = now()
         WHERE id = $1 RETURNING {COLUMNS}"
    ))
    .bind(&skill_id)
    .bind(&name)
    .bind(&playbook)
    .bind(status)
    .fetch_one(state.pool())
    .await
    .map_err(|error| internal(error.to_string()))?;
    Ok(Json(row.into()))
}

async fn delete_skill(
    State(state): State<AppState>,
    Path(skill_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let actor = actor(&state).await?;
    let row = load_skill(&state, &actor, &skill_id).await?;
    if row.status == "recording" {
        return Err(conflict("stop or cancel the recording first"));
    }
    sqlx::query("DELETE FROM taught_skills WHERE id = $1")
        .bind(&skill_id)
        .execute(state.pool())
        .await
        .map_err(|error| internal(error.to_string()))?;
    let _ = tokio::fs::remove_dir_all(frames_dir(&state, &skill_id)).await;
    Ok(Json(json!({ "ok": true })))
}

async fn test_skill(
    State(state): State<AppState>,
    Path(skill_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let actor = actor(&state).await?;
    let row = load_skill(&state, &actor, &skill_id).await?;
    if !matches!(row.status.as_str(), "draft" | "saved") {
        return Err(conflict("skill is not ready yet"));
    }
    let name = if row.name.is_empty() {
        row.playbook
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&row.goal)
            .to_string()
    } else {
        row.name.clone()
    };
    let thread_id = crate::sessions::default_session_for_bot(&state, &actor, &row.bot_id)
        .await
        .map_err(|error| internal(error.to_string()))?
        .ok_or_else(not_found)?;
    let prompt = format!("試跑技能「{name}」：照剛學到的流程做一遍，做完回報結果。");
    let result = crate::runs::send(
        &state,
        &actor,
        &row.bot_id,
        &thread_id,
        &prompt,
        None,
        &[],
        &[],
    )
    .await
    .map_err(bad_request)?;
    Ok(Json(result))
}

/// Portable JSON a human can download after a demo and load onto another bot.
pub const SKILL_FILE_KIND: &str = "lazyboy.skill";
pub const SKILL_FILE_VERSION: u32 = 1;
const SKILL_NAME_MAX: usize = 40;

pub fn skill_file(name: &str, goal: &str, playbook: &Value) -> Value {
    json!({
        "kind": SKILL_FILE_KIND,
        "version": SKILL_FILE_VERSION,
        "name": name,
        "goal": goal,
        "playbook": playbook,
    })
}

fn clip_name(name: &str) -> String {
    name.chars().take(SKILL_NAME_MAX).collect()
}

pub fn unique_skill_name(existing: &[String], wanted: &str) -> String {
    let wanted = clip_name(wanted.trim());
    let clash = |candidate: &str| {
        existing
            .iter()
            .any(|have| have.eq_ignore_ascii_case(candidate))
    };
    if !clash(&wanted) {
        return wanted;
    }
    for n in 2..1000 {
        let suffix = format!(" ({n})");
        let budget = SKILL_NAME_MAX.saturating_sub(suffix.chars().count());
        let base: String = wanted.chars().take(budget).collect();
        let candidate = format!("{base}{suffix}");
        if !clash(&candidate) {
            return candidate;
        }
    }
    wanted
}

fn sanitize_steps(value: Option<&Value>) -> Result<Vec<Value>, String> {
    let steps = value
        .and_then(Value::as_array)
        .ok_or_else(|| "steps required".to_string())?;
    let steps: Vec<Value> = steps
        .iter()
        .filter_map(|step| match step {
            Value::String(text) => {
                let text = text.trim();
                if text.is_empty() {
                    None
                } else {
                    Some(json!({ "do": text, "expect": "", "note": "" }))
                }
            }
            Value::Object(obj) => {
                let action = obj
                    .get("do")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())?;
                Some(json!({
                    "do": action,
                    "expect": obj.get("expect").and_then(Value::as_str).unwrap_or(""),
                    "note": obj.get("note").and_then(Value::as_str).unwrap_or(""),
                }))
            }
            _ => None,
        })
        .collect();
    if steps.is_empty() {
        return Err("steps required".into());
    }
    Ok(steps)
}

fn sanitize_inputs(value: Option<&Value>) -> Vec<Value> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let name = item
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|name| !name.is_empty())?;
                    Some(json!({
                        "name": name,
                        "description": item.get("description").and_then(Value::as_str).unwrap_or(""),
                        "example": item.get("example").and_then(Value::as_str).unwrap_or(""),
                    }))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn sanitize_playbook(playbook: &Value, name: &str) -> Result<Value, String> {
    let text = |key: &str| {
        playbook
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("")
    };
    Ok(json!({
        "name": name,
        "whenToUse": text("whenToUse"),
        "intent": text("intent"),
        "inputs": sanitize_inputs(playbook.get("inputs")),
        "preconditions": strings(playbook.get("preconditions")),
        "steps": sanitize_steps(playbook.get("steps"))?,
        "howToCheck": text("howToCheck"),
        "whatToReturn": text("whatToReturn"),
        "cautions": strings(playbook.get("cautions")),
    }))
}

/// Accepts the envelope we export, or a bare playbook object with `name` + `steps`.
pub fn parse_skill_file(value: &Value) -> Result<(String, String, Value), String> {
    if !value.is_object() {
        return Err("skill file must be a JSON object".into());
    }
    if let Some(kind) = value.get("kind").and_then(Value::as_str)
        && kind != SKILL_FILE_KIND
    {
        return Err(format!("unsupported skill kind: {kind}"));
    }
    let playbook = match value.get("playbook") {
        Some(inner) if inner.is_object() => inner,
        _ => value,
    };
    let name = value
        .get("name")
        .or_else(|| playbook.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "name required".to_string())?;
    let name = clip_name(name);
    let goal = value
        .get("goal")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|goal| !goal.is_empty())
        .map(str::to_string)
        .or_else(|| {
            playbook
                .get("intent")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|intent| !intent.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| name.clone());
    let playbook = sanitize_playbook(playbook, &name)?;
    Ok((name, goal, playbook))
}

fn export_filename_ascii(name: &str) -> String {
    let slug: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(40)
        .collect();
    if slug.is_empty() {
        "skill.json".into()
    } else {
        format!("{slug}.json")
    }
}

async fn export_skill(
    State(state): State<AppState>,
    Path(skill_id): Path<String>,
) -> Result<(HeaderMap, Json<Value>), ApiError> {
    let actor = actor(&state).await?;
    let row = load_skill(&state, &actor, &skill_id).await?;
    if !matches!(row.status.as_str(), "draft" | "saved") {
        return Err(conflict("skill is not ready yet"));
    }
    let name = if row.name.trim().is_empty() {
        row.playbook
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(&row.goal)
            .to_string()
    } else {
        row.name.clone()
    };
    let payload = skill_file(&name, &row.goal, &row.playbook);
    let mut headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(&format!(
        "attachment; filename=\"{}\"",
        export_filename_ascii(&name)
    )) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    Ok((headers, Json(payload)))
}

async fn import_skill(
    State(state): State<AppState>,
    Path(bot_id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Skill>, ApiError> {
    let actor = actor(&state).await?;
    state
        .db
        .get_bot(&actor, &bot_id)
        .await
        .map_err(|error| internal(error.to_string()))?
        .ok_or_else(not_found)?;
    let (name, goal, mut playbook) = parse_skill_file(&body).map_err(bad_request)?;
    let existing = sqlx::query_scalar::<_, String>(
        "SELECT name FROM taught_skills
         WHERE bot_id = $1 AND space_id = $2 AND user_id = $3
           AND status IN ('saved','draft','drafting','recording')",
    )
    .bind(&bot_id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .fetch_all(state.pool())
    .await
    .map_err(|error| internal(error.to_string()))?;
    let name = unique_skill_name(&existing, &name);
    if let Some(object) = playbook.as_object_mut() {
        object.insert("name".into(), json!(name));
    }
    let thread_id = crate::sessions::default_session_for_bot(&state, &actor, &bot_id)
        .await
        .map_err(|error| internal(error.to_string()))?;
    let skill_id = Uuid::new_v4().to_string();
    let row = sqlx::query_as::<_, SkillRow>(&format!(
        "INSERT INTO taught_skills (id, space_id, user_id, bot_id, thread_id, name, goal, status, playbook)
         VALUES ($1, $2, $3, $4, $5, $6, $7, 'saved', $8)
         RETURNING {COLUMNS}"
    ))
    .bind(&skill_id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .bind(&bot_id)
    .bind(&thread_id)
    .bind(&name)
    .bind(&goal)
    .bind(&playbook)
    .fetch_one(state.pool())
    .await
    .map_err(|error| internal(error.to_string()))?;
    Ok(Json(row.into()))
}

async fn teach_target(
    state: &AppState,
    actor: &Actor,
    bot_id: &str,
) -> Result<(ComputerRef, AdapterContext), String> {
    let bot = state
        .db
        .get_bot(actor, bot_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "bot not found".to_string())?;
    let computer = state
        .db
        .get_computer(bot.computer_id.as_deref().unwrap_or(""))
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "computer not found".to_string())?;
    let computer_ref =
        computer::computer_ref(&computer).ok_or_else(|| "computer is not running".to_string())?;
    let screen = state
        .db
        .get_screen(&computer.id, bot_id)
        .await
        .map_err(|error| error.to_string())?;
    let ctx = computer::adapter_context_for(actor, bot_id, "teach", screen.as_ref(), None);
    Ok((computer_ref, ctx))
}

fn frames_dir(state: &AppState, skill_id: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(&state.data_dir)
        .join("teach")
        .join(skill_id)
}

async fn append_recording(pool: &PgPool, skill_id: &str, key: &str, item: Value) {
    let _ = sqlx::query(
        "UPDATE taught_skills
         SET recording = jsonb_set(recording, ARRAY[$2], COALESCE(recording->$2, '[]'::jsonb) || $3::jsonb),
             updated_at = now()
         WHERE id = $1",
    )
    .bind(skill_id)
    .bind(key)
    .bind(json!([item]))
    .execute(pool)
    .await;
}

/// Watches the desktop while the human demonstrates: window titles and
/// changed frames become part of the trace. Exits when the row leaves
/// `recording`, and auto-finalises when the session expires.
async fn record_loop(
    state: AppState,
    actor: Actor,
    skill_id: String,
    bot_id: String,
    computer_ref: ComputerRef,
    ctx: AdapterContext,
) {
    let dir = frames_dir(&state, &skill_id);
    let _ = tokio::fs::create_dir_all(&dir).await;
    let mut last_frame: Option<Vec<u8>> = None;
    let mut last_title: Option<String> = None;
    let mut frames = 0usize;
    loop {
        tokio::time::sleep(Duration::from_millis(POLL_MS)).await;
        let row: Option<(String, Option<DateTime<Utc>>)> =
            sqlx::query_as("SELECT status, expires_at FROM taught_skills WHERE id = $1")
                .bind(&skill_id)
                .fetch_optional(state.pool())
                .await
                .ok()
                .flatten();
        let Some((status, expires_at)) = row else {
            break;
        };
        if status != "recording" {
            break;
        }
        if expires_at.is_some_and(|at| at < Utc::now()) {
            if let Some(row) = recording_skill(state.pool(), &bot_id).await {
                finalize(
                    &state,
                    &actor,
                    &row,
                    computer_ref.clone(),
                    ctx.clone(),
                    true,
                )
                .await;
            }
            break;
        }
        let Ok(observation) = state.sandbox.observe(&computer_ref, &ctx).await else {
            continue;
        };
        let at = Utc::now().timestamp_millis();
        let title = observation
            .active_window
            .as_ref()
            .and_then(|window| window.title.clone())
            .filter(|title| !title.is_empty());
        if title.is_some() && title != last_title {
            append_recording(
                state.pool(),
                &skill_id,
                "events",
                json!({ "t": "window", "title": title, "at": at }),
            )
            .await;
            last_title = title.clone();
        }
        // Keyframes only: a byte hash would flip on every panel-clock tick and
        // exhaust MAX_FRAMES during an idle minute, so compare coarse thumbnails.
        let signature = frame_signature(&observation.image);
        let changed = match (&signature, &last_frame) {
            (Some(now), Some(before)) => !signatures_similar(now, before),
            (Some(_), None) => true,
            (None, _) => false,
        };
        if changed && frames < MAX_FRAMES {
            frames += 1;
            let file = format!("{frames:03}.jpg");
            if tokio::fs::write(dir.join(&file), &observation.image)
                .await
                .is_ok()
            {
                append_recording(
                    state.pool(),
                    &skill_id,
                    "frames",
                    json!({ "n": frames, "file": file, "at": at, "title": title }),
                )
                .await;
            }
            last_frame = signature;
        }
    }
}

async fn stop_recorder(
    state: &AppState,
    computer_ref: &ComputerRef,
    ctx: &AdapterContext,
    skill_id: &str,
) {
    let _ = state
        .sandbox
        .execute(
            computer_ref,
            CommandRequest {
                argv: cdp_record_stop_command(skill_id),
                cwd: None,
                timeout_ms: Some(5_000),
            },
            ctx,
        )
        .await;
}

async fn collect_browser_events(
    state: &AppState,
    computer_ref: &ComputerRef,
    ctx: &AdapterContext,
    skill_id: &str,
) -> Vec<Value> {
    let out = teach_recorder_output(skill_id);
    let result = state
        .sandbox
        .execute(
            computer_ref,
            CommandRequest {
                argv: vec![
                    "sh".into(),
                    "-c".into(),
                    "cat \"$0\" 2>/dev/null; rm -f \"$0\"".into(),
                    out,
                ],
                cwd: None,
                timeout_ms: Some(10_000),
            },
            ctx,
        )
        .await;
    let Ok(result) = result else {
        return Vec::new();
    };
    result
        .stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|event| event.get("t").and_then(Value::as_str) != Some("recorder"))
        .collect()
}

/// Stop recording, gather the trace, hand the desktop back and distil the
/// playbook. Idempotent: only the caller that flips `recording → drafting`
/// does the work.
async fn finalize(
    state: &AppState,
    actor: &Actor,
    row: &SkillRow,
    computer_ref: ComputerRef,
    ctx: AdapterContext,
    expired: bool,
) {
    let claimed = sqlx::query(
        "UPDATE taught_skills SET status = 'drafting', stopped_at = now(), updated_at = now()
         WHERE id = $1 AND status = 'recording'",
    )
    .bind(&row.id)
    .execute(state.pool())
    .await
    .map(|result| result.rows_affected() == 1)
    .unwrap_or(false);
    if !claimed {
        return;
    }
    stop_recorder(state, &computer_ref, &ctx, &row.id).await;
    let mut events = collect_browser_events(state, &computer_ref, &ctx, &row.id).await;

    // One last look at the result screen: it is the strongest hint for "how
    // to check" and it is what the human was satisfied with.
    let dir = frames_dir(state, &row.id);
    if let Ok(observation) = state.sandbox.observe(&computer_ref, &ctx).await {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT jsonb_array_length(COALESCE(recording->'frames','[]'::jsonb)) FROM taught_skills WHERE id = $1",
        )
        .bind(&row.id)
        .fetch_one(state.pool())
        .await
        .unwrap_or(0) as usize;
        let n = count + 1;
        let file = format!("{n:03}.jpg");
        if tokio::fs::write(dir.join(&file), &observation.image)
            .await
            .is_ok()
        {
            let title = observation
                .active_window
                .as_ref()
                .and_then(|window| window.title.clone());
            append_recording(
                state.pool(),
                &row.id,
                "frames",
                json!({ "n": n, "file": file, "at": Utc::now().timestamp_millis(), "title": title, "final": true }),
            )
            .await;
        }
    }
    let _ = computer::release(state, actor, &row.bot_id).await;

    let fresh = sqlx::query_as::<_, SkillRow>(&format!("{SELECT} WHERE id = $1"))
        .bind(&row.id)
        .fetch_one(state.pool())
        .await
        .ok();
    let recording = fresh
        .as_ref()
        .map(|row| row.recording.clone())
        .unwrap_or_else(|| json!({}));
    let desktop_events = recording
        .get("events")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let frames = recording
        .get("frames")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    events.extend(desktop_events);
    events.sort_by_key(|event| event.get("at").and_then(Value::as_i64).unwrap_or(0));
    let events = compact_events(events);
    let _ = sqlx::query(
        "UPDATE taught_skills SET recording = jsonb_set(recording, '{events}', $2::jsonb), updated_at = now() WHERE id = $1",
    )
    .bind(&row.id)
    .bind(json!(events))
    .execute(state.pool())
    .await;

    if let Some(thread_id) = &row.thread_id {
        let note = if expired {
            "示範時間到，我先收尾了。正在把剛才的流程整理成技能…"
        } else {
            "收到，示範結束。正在把剛才的流程整理成技能（不是記座標，而是理解你想做什麼、怎麼做）…"
        };
        let _ = crate::runs::append_bot_message(state, thread_id, &row.id, &row.bot_id, note).await;
    }

    let bot = state.db.get_bot(actor, &row.bot_id).await.ok().flatten();
    let distilled = match bot {
        Some(bot) => distill(state, actor, &bot, &row.goal, &events, &frames, &dir).await,
        None => Err("bot not found".to_string()),
    };
    let (playbook, error) = match distilled {
        Ok(playbook) => (playbook, None),
        Err(error) => {
            tracing::warn!("teach {}: distill failed: {error}", row.id);
            (fallback_playbook(&row.goal, &events), Some(error))
        }
    };
    let name = playbook
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| row.goal.chars().take(30).collect());
    let _ = sqlx::query(
        "UPDATE taught_skills SET status = 'draft', name = $2, playbook = $3, error = $4, updated_at = now() WHERE id = $1",
    )
    .bind(&row.id)
    .bind(&name)
    .bind(&playbook)
    .bind(&error)
    .execute(state.pool())
    .await;

    if let Some(thread_id) = &row.thread_id {
        let mut body = format!("我學會了「{name}」。\n{}", summarize_playbook(&playbook));
        if error.is_some() {
            body.push_str("\n\n（模型整理失敗，這是依事件直接列出的版本，建議先修改再儲存。）");
        }
        body.push_str("\n\n確認名稱後按「儲存」，之後跟我說「執行");
        body.push_str(&name);
        body.push_str("」就會照這個流程做；或先「試跑」看看。");
        let _ =
            crate::runs::append_bot_message(state, thread_id, &row.id, &row.bot_id, &body).await;
    }
}

/// Drop noise that carries no intent: repeated scrolls, `page` echoes of a
/// navigation, and title flaps.
fn compact_events(events: Vec<Value>) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut last_url = String::new();
    for event in events {
        let kind = event.get("t").and_then(Value::as_str).unwrap_or("");
        match kind {
            "scroll" => {
                if out
                    .last()
                    .and_then(|prev| prev.get("t"))
                    .and_then(Value::as_str)
                    == Some("scroll")
                {
                    out.pop();
                }
                out.push(event);
            }
            "page" => {
                let url = event.get("url").and_then(Value::as_str).unwrap_or("");
                if url == last_url {
                    continue;
                }
                last_url = url.to_string();
                out.push(event);
            }
            "navigate" => {
                let url = event.get("url").and_then(Value::as_str).unwrap_or("");
                if url == last_url {
                    continue;
                }
                last_url = url.to_string();
                out.push(event);
            }
            "title" => {
                let title = event.get("title").and_then(Value::as_str).unwrap_or("");
                let same = out.iter().rev().take(3).any(|prev| {
                    prev.get("t").and_then(Value::as_str) == Some("title")
                        && prev.get("title").and_then(Value::as_str) == Some(title)
                });
                if title.is_empty() || same {
                    continue;
                }
                out.push(event);
            }
            _ => out.push(event),
        }
    }
    if out.len() > MAX_EVENTS {
        let skip = out.len() - MAX_EVENTS;
        out.drain(0..skip);
    }
    out
}

fn element_name(el: Option<&Value>) -> String {
    let Some(el) = el else { return String::new() };
    let pick = |key: &str| {
        el.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let label = pick("label")
        .or_else(|| pick("text"))
        .or_else(|| pick("name"))
        .or_else(|| pick("id"));
    let tag = pick("tag").unwrap_or_default();
    let role = pick("role").or_else(|| pick("type"));
    let mut name = match label {
        Some(label) => format!("「{}」", label.chars().take(60).collect::<String>()),
        None => String::new(),
    };
    match role {
        Some(role) => name.push_str(&format!(" ({tag}/{role})")),
        None if !tag.is_empty() => name.push_str(&format!(" ({tag})")),
        None => {}
    }
    if let Some(href) = pick("href") {
        name.push_str(&format!(" → {href}"));
    }
    name.trim().to_string()
}

fn looks_secret(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "pass",
        "pwd",
        "密碼",
        "token",
        "otp",
        "驗證碼",
        "secret",
        "cvv",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn describe_event(event: &Value, t0: i64) -> Option<String> {
    let at = event.get("at").and_then(Value::as_i64).unwrap_or(t0);
    let stamp = format!("[+{:.1}s]", (at - t0).max(0) as f64 / 1000.0);
    let kind = event.get("t").and_then(Value::as_str)?;
    let text = match kind {
        "navigate" | "page" => format!(
            "前往 {}{}",
            event.get("url").and_then(Value::as_str).unwrap_or("?"),
            event
                .get("title")
                .and_then(Value::as_str)
                .filter(|title| !title.is_empty())
                .map(|title| format!("（{title}）"))
                .unwrap_or_default()
        ),
        "title" => format!(
            "頁面標題變為「{}」",
            event.get("title").and_then(Value::as_str).unwrap_or("")
        ),
        "window" => format!(
            "作用中視窗：「{}」",
            event.get("title").and_then(Value::as_str).unwrap_or("")
        ),
        "click" => format!("點擊 {}", element_name(event.get("el"))),
        "input" => {
            let name = element_name(event.get("el"));
            let value = event.get("value").and_then(Value::as_str).unwrap_or("");
            let value = if looks_secret(&name) {
                "[已遮罩]"
            } else {
                value
            };
            format!("在 {name} 輸入「{value}」")
        }
        "key" => format!(
            "按下 {}（焦點：{}）",
            event.get("key").and_then(Value::as_str).unwrap_or("?"),
            element_name(event.get("el"))
        ),
        "submit" => format!(
            "送出表單 {}",
            event
                .get("form")
                .and_then(|form| form.get("name").or(form.get("action")))
                .and_then(Value::as_str)
                .unwrap_or("")
        ),
        "scroll" => format!(
            "捲動頁面到 y={}",
            event.get("y").and_then(Value::as_i64).unwrap_or(0)
        ),
        "frame" => format!(
            "[截圖 #{}]",
            event.get("n").and_then(Value::as_i64).unwrap_or(0)
        ),
        _ => return None,
    };
    Some(format!("{stamp} {text}"))
}

fn pick_frames(frames: &[Value]) -> Vec<Value> {
    if frames.len() <= DISTILL_FRAMES {
        return frames.to_vec();
    }
    let mut picked = Vec::with_capacity(DISTILL_FRAMES);
    for index in 0..DISTILL_FRAMES {
        let position = index * (frames.len() - 1) / (DISTILL_FRAMES - 1);
        picked.push(frames[position].clone());
    }
    picked
}

const DISTILL_SYSTEM: &str = "You turn a human's one-time screen demonstration into a reusable skill for a computer-use agent that controls the same Linux desktop (Chromium via a DOM snapshot/click/type tool, plus screenshots and xdotool for native windows).

You receive: the human's stated goal, a timeline of what they did (semantic browser events: which control was clicked by its label/text, what text was typed into which field, which URLs loaded, active window titles) and a few screenshots taken along the way. The trace is noisy: ignore mis-clicks, corrections, tab switches and anything unrelated to the goal.

Produce a playbook that captures INTENT and PROCESS, never pixel positions:
- Describe each step by what it achieves and which control to use, named by its visible label/role/page (e.g. \"在 Wikipedia 首頁的搜尋框輸入 <主題> 並按 Enter\"), so the agent can find it on a slightly different layout.
- Generalise values the user will likely change into inputs (search terms, names, dates, amounts). Keep values that are part of the procedure itself (a fixed URL, a fixed menu path). Reference inputs in steps as <input name>.
- For each step say what should be visible when it worked (\"expect\"). Add short notes for waits (videos, loading), choices, or alternate paths you saw.
- Note preconditions (logged in, file present) and cautions (never submit payments, stop and ask on login/2FA/CAPTCHA).
- Write in the same language as the goal (Traditional Chinese if the goal is Chinese). Keep it concise; 3-15 steps.

Return ONLY a JSON object with exactly these keys:
{\"name\": string (<= 30 chars, a short verb phrase),
 \"whenToUse\": string (one sentence: what the user might say to trigger this),
 \"intent\": string (1-3 sentences: what the human wants and why),
 \"inputs\": [{\"name\": string, \"description\": string, \"example\": string}],
 \"preconditions\": [string],
 \"steps\": [{\"do\": string, \"expect\": string, \"note\": string}],
 \"howToCheck\": string,
 \"whatToReturn\": string,
 \"cautions\": [string]}";

async fn distill(
    state: &AppState,
    actor: &Actor,
    bot: &crate::db::BotRow,
    goal: &str,
    events: &[Value],
    frames: &[Value],
    dir: &std::path::Path,
) -> Result<Value, String> {
    let (model, vision) = crate::runs::bot_model(state, actor, bot).await?;
    let t0 = events
        .iter()
        .chain(frames.iter())
        .filter_map(|event| event.get("at").and_then(Value::as_i64))
        .min()
        .unwrap_or(0);
    let picked = if vision {
        pick_frames(frames)
    } else {
        Vec::new()
    };
    let mut timeline: Vec<(i64, String)> = events
        .iter()
        .filter_map(|event| {
            describe_event(event, t0)
                .map(|line| (event.get("at").and_then(Value::as_i64).unwrap_or(t0), line))
        })
        .collect();
    for frame in &picked {
        let at = frame.get("at").and_then(Value::as_i64).unwrap_or(t0);
        let n = frame.get("n").and_then(Value::as_i64).unwrap_or(0);
        timeline.push((
            at,
            format!(
                "[+{:.1}s] [截圖 #{n} 附在下方]",
                (at - t0).max(0) as f64 / 1000.0
            ),
        ));
    }
    timeline.sort_by_key(|(at, _)| *at);
    let lines: Vec<String> = timeline.into_iter().map(|(_, line)| line).collect();
    let mut content = vec![UserContent::text(format!(
        "Goal stated by the human:\n{goal}\n\nDemonstration timeline ({} events):\n{}\n\nReturn the playbook JSON now.",
        lines.len(),
        if lines.is_empty() {
            "(no events were captured; build the playbook from the goal alone and mark steps as assumptions)".to_string()
        } else {
            lines.join("\n")
        }
    ))];
    for frame in &picked {
        let Some(file) = frame.get("file").and_then(Value::as_str) else {
            continue;
        };
        let Ok(bytes) = tokio::fs::read(dir.join(file)).await else {
            continue;
        };
        let n = frame.get("n").and_then(Value::as_i64).unwrap_or(0);
        content.push(UserContent::text(format!("截圖 #{n}:")));
        content.push(UserContent::image_base64(
            base64::engine::general_purpose::STANDARD.encode(bytes),
            Some(ImageMediaType::JPEG),
            Some(ImageDetail::High),
        ));
    }
    let reply =
        crate::runs::complete_once(&model, Message::User { content }, DISTILL_SYSTEM, &[], &[])
            .await?;
    let text: String = reply
        .into_iter()
        .filter_map(|part| match part {
            AssistantContent::Text(text) => Some(text.text),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    parse_playbook_json(&text).ok_or_else(|| {
        format!(
            "model did not return JSON: {}",
            text.chars().take(200).collect::<String>()
        )
    })
}

pub fn parse_playbook_json(text: &str) -> Option<Value> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end <= start {
        return None;
    }
    let value: Value = serde_json::from_str(&text[start..=end]).ok()?;
    value.get("steps")?.as_array()?;
    Some(value)
}

/// Used when the model is unavailable: a literal transcript of the trace,
/// still expressed by control names rather than coordinates.
pub fn fallback_playbook(goal: &str, events: &[Value]) -> Value {
    let t0 = events
        .iter()
        .filter_map(|event| event.get("at").and_then(Value::as_i64))
        .min()
        .unwrap_or(0);
    let steps: Vec<Value> = events
        .iter()
        .filter(|event| {
            matches!(
                event.get("t").and_then(Value::as_str),
                Some("navigate") | Some("click") | Some("input") | Some("key") | Some("submit")
            )
        })
        .filter_map(|event| describe_event(event, t0))
        .map(|line| {
            let text = line.splitn(2, ' ').nth(1).unwrap_or(&line).to_string();
            json!({ "do": text, "expect": "", "note": "" })
        })
        .take(40)
        .collect();
    json!({
        "name": goal.chars().take(30).collect::<String>(),
        "whenToUse": format!("使用者要求：{goal}"),
        "intent": goal,
        "inputs": [],
        "preconditions": [],
        "steps": steps,
        "howToCheck": "完成後畫面應顯示目標達成的結果；不確定時回報你看到的內容。",
        "whatToReturn": "一句話說明完成了什麼。",
        "cautions": ["遇到登入、2FA、驗證碼或付款時停止並 request_takeover。"]
    })
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| match item {
                    Value::String(text) => Some(text.clone()),
                    Value::Object(_) => Some(
                        item.get("do")
                            .or(item.get("name"))
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    ),
                    _ => None,
                })
                .filter(|text| !text.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

pub fn summarize_playbook(playbook: &Value) -> String {
    let mut out = String::new();
    if let Some(intent) = playbook.get("intent").and_then(Value::as_str) {
        out.push_str(&format!("意圖：{intent}\n"));
    }
    let inputs = playbook
        .get("inputs")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("name").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("、")
        })
        .unwrap_or_default();
    if !inputs.is_empty() {
        out.push_str(&format!("可變輸入：{inputs}\n"));
    }
    out.push_str("步驟：\n");
    for (index, step) in strings(playbook.get("steps")).iter().enumerate() {
        out.push_str(&format!("{}. {step}\n", index + 1));
    }
    if let Some(check) = playbook.get("howToCheck").and_then(Value::as_str) {
        out.push_str(&format!("驗證：{check}"));
    }
    out.trim_end().to_string()
}

#[derive(Debug, Clone)]
pub struct SavedSkill {
    #[allow(dead_code)]
    pub id: String,
    pub name: String,
    pub goal: String,
    pub playbook: Value,
}

pub async fn saved_skills(pool: &PgPool, bot_id: &str) -> Vec<SavedSkill> {
    let rows: Vec<(String, String, String, Value)> = sqlx::query_as(
        "SELECT id, name, goal, playbook FROM taught_skills WHERE bot_id = $1 AND status = 'saved' ORDER BY updated_at DESC LIMIT 30",
    )
    .bind(bot_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    rows.into_iter()
        .map(|(id, name, goal, playbook)| SavedSkill {
            id,
            name,
            goal,
            playbook,
        })
        .collect()
}

async fn saved_or_draft(pool: &PgPool, bot_id: &str) -> Vec<SavedSkill> {
    let rows: Vec<(String, String, String, Value)> = sqlx::query_as(
        "SELECT id, name, goal, playbook FROM taught_skills WHERE bot_id = $1 AND status IN ('saved','draft') ORDER BY updated_at DESC LIMIT 30",
    )
    .bind(bot_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    rows.into_iter()
        .map(|(id, name, goal, playbook)| SavedSkill {
            id,
            name,
            goal,
            playbook,
        })
        .collect()
}

/// Draft skills count for test runs: the message names the skill explicitly.
pub async fn skill_for_prompt(pool: &PgPool, bot_id: &str, prompt: &str) -> Option<SavedSkill> {
    let skills = saved_or_draft(pool, bot_id).await;
    skill_mentioned(&skills, prompt).cloned()
}

pub fn skills_preamble(skills: &[SavedSkill]) -> Option<String> {
    if skills.is_empty() {
        return None;
    }
    let mut out = String::from(
        "Taught skills (the human demonstrated these once; each has a playbook of intent-level steps):",
    );
    for skill in skills {
        let when = skill
            .playbook
            .get("whenToUse")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .unwrap_or(&skill.goal);
        out.push_str(&format!("\n- {}: {}", skill.name, when));
    }
    out.push_str("\nWhen a request matches one of these, call use_skill {\"name\":\"...\"} first (unless its playbook is already in the message) and follow the playbook: locate each control on the CURRENT screen by its label/text/role via browser snapshot or computer_observe, never by remembered coordinates; verify each step's expectation before moving on; if the layout differs, look for the equivalent control; take input values from the user's message.");
    Some(out)
}

pub fn skill_mentioned<'a>(skills: &'a [SavedSkill], prompt: &str) -> Option<&'a SavedSkill> {
    let lower = prompt.to_lowercase();
    skills
        .iter()
        .filter(|skill| skill.name.chars().count() >= 2)
        .filter(|skill| lower.contains(&skill.name.to_lowercase()))
        .max_by_key(|skill| skill.name.chars().count())
}

pub fn find_skill<'a>(skills: &'a [SavedSkill], name: &str) -> Option<&'a SavedSkill> {
    let wanted = name.trim().to_lowercase();
    skills
        .iter()
        .find(|skill| skill.name.to_lowercase() == wanted)
        .or_else(|| {
            skills.iter().find(|skill| {
                let have = skill.name.to_lowercase();
                have.contains(&wanted) || wanted.contains(&have)
            })
        })
}

pub fn format_playbook_for_run(skill: &SavedSkill) -> String {
    let playbook = &skill.playbook;
    let text = |key: &str| {
        playbook
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let mut out = format!(
        "Taught skill「{}」playbook (learned from the human's demonstration; follow the intent and process, not pixel positions):",
        skill.name
    );
    if let Some(intent) = text("intent") {
        out.push_str(&format!("\nIntent: {intent}"));
    }
    if let Some(when) = text("whenToUse") {
        out.push_str(&format!("\nWhen to use: {when}"));
    }
    if let Some(inputs) = playbook
        .get("inputs")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
    {
        out.push_str("\nInputs (take values from the user's message; if missing, use the example or ask in one short question):");
        for input in inputs {
            out.push_str(&format!(
                "\n- {}: {}{}",
                input.get("name").and_then(Value::as_str).unwrap_or("?"),
                input
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                input
                    .get("example")
                    .and_then(Value::as_str)
                    .filter(|example| !example.is_empty())
                    .map(|example| format!(" (example: {example})"))
                    .unwrap_or_default()
            ));
        }
    }
    let preconditions = strings(playbook.get("preconditions"));
    if !preconditions.is_empty() {
        out.push_str(&format!("\nPreconditions: {}", preconditions.join("; ")));
    }
    out.push_str("\nSteps:");
    if let Some(steps) = playbook.get("steps").and_then(Value::as_array) {
        for (index, step) in steps.iter().enumerate() {
            let action = step
                .get("do")
                .and_then(Value::as_str)
                .or_else(|| step.as_str())
                .unwrap_or("");
            out.push_str(&format!("\n{}. {action}", index + 1));
            if let Some(expect) = step
                .get("expect")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
            {
                out.push_str(&format!(" → expect: {expect}"));
            }
            if let Some(note) = step
                .get("note")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
            {
                out.push_str(&format!(" (note: {note})"));
            }
        }
    }
    if let Some(check) = text("howToCheck") {
        out.push_str(&format!("\nHow to check: {check}"));
    }
    if let Some(ret) = text("whatToReturn") {
        out.push_str(&format!("\nWhat to return: {ret}"));
    }
    let cautions = strings(playbook.get("cautions"));
    if !cautions.is_empty() {
        out.push_str(&format!("\nCautions: {}", cautions.join("; ")));
    }
    out.push_str("\nExecution rules: observe or snapshot before each step; find controls by their label/text/role on the current page and click by element id (controls tagged [below viewport] are clickable too); if a step's expectation is not met, wait and re-observe once, then try the equivalent control; never replay coordinates from memory; if login, 2FA, CAPTCHA or payment appears, request_takeover and say what the human must do. Keep going until \"How to check\" is satisfied — a course with 24 pages means 24 Next clicks. When finished, report in one or two sentences what was done and what you saw.");
    out
}

/// The completion criterion the run loop uses to push a model that stops
/// early back to work.
pub(crate) fn skill_check_hint(skill: &SavedSkill) -> String {
    skill
        .playbook
        .get("howToCheck")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|check| !check.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            format!(
                "the goal of「{}」is visibly achieved on the current screen",
                skill.name
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(name: &str) -> SavedSkill {
        SavedSkill {
            id: name.into(),
            name: name.into(),
            goal: format!("goal {name}"),
            playbook: json!({"steps": [{"do": "open", "expect": "page"}], "intent": "x"}),
        }
    }

    #[test]
    fn mentions_pick_longest_match() {
        let skills = vec![skill("維基"), skill("維基搜尋")];
        let hit = skill_mentioned(&skills, "幫我執行維基搜尋 台灣").unwrap();
        assert_eq!(hit.name, "維基搜尋");
        assert!(skill_mentioned(&skills, "今天天氣").is_none());
    }

    #[test]
    fn parses_fenced_json() {
        let text = "Here you go:\n```json\n{\"name\":\"a\",\"steps\":[{\"do\":\"x\"}]}\n```";
        let value = parse_playbook_json(text).unwrap();
        assert_eq!(value["name"], "a");
        assert!(parse_playbook_json("{\"name\":\"no steps\"}").is_none());
    }

    #[test]
    fn compact_drops_duplicate_navigation_and_scrolls() {
        let events = vec![
            json!({"t":"navigate","url":"https://a","at":1}),
            json!({"t":"page","url":"https://a","at":2}),
            json!({"t":"scroll","y":10,"at":3}),
            json!({"t":"scroll","y":20,"at":4}),
            json!({"t":"click","el":{"tag":"a","text":"Go"},"at":5}),
        ];
        let out = compact_events(events);
        assert_eq!(out.len(), 3);
        assert_eq!(out[1]["y"], 20);
    }

    #[test]
    fn describe_masks_secret_fields() {
        let event =
            json!({"t":"input","el":{"tag":"input","label":"密碼"},"value":"hunter2","at":1000});
        let line = describe_event(&event, 0).unwrap();
        assert!(line.contains("[已遮罩]"));
        assert!(!line.contains("hunter2"));
    }

    #[test]
    fn playbook_prompt_lists_steps_without_coordinates() {
        let text = format_playbook_for_run(&skill("demo"));
        assert!(text.contains("1. open → expect: page"));
        assert!(text.contains("never replay coordinates"));
    }

    fn sample_playbook() -> Value {
        json!({
            "name": "完成 STAR 訓練",
            "whenToUse": "要把指定課程看完",
            "intent": "把 STAR 課程的影片看完並通過測驗",
            "inputs": [{"name": "courseName", "description": "課程名稱", "example": "Workplace"}],
            "preconditions": ["已登入"],
            "steps": [{"do": "點 Next", "expect": "頁碼加一", "note": "鎖住就等"}],
            "howToCheck": "課程顯示 completed",
            "whatToReturn": "課程名稱與結果",
            "cautions": ["遇到驗證碼就停下"],
            "noise": "drop me"
        })
    }

    #[test]
    fn skill_file_roundtrip_drops_unknown_keys() {
        let playbook = sample_playbook();
        let file = skill_file("完成 STAR 訓練", "示範目標", &playbook);
        assert_eq!(file["kind"], SKILL_FILE_KIND);
        assert_eq!(file["version"], SKILL_FILE_VERSION);
        let (name, goal, parsed) = parse_skill_file(&file).unwrap();
        assert_eq!(name, "完成 STAR 訓練");
        assert_eq!(goal, "示範目標");
        assert_eq!(parsed["steps"][0]["do"], "點 Next");
        assert_eq!(parsed["inputs"][0]["name"], "courseName");
        assert!(parsed.get("noise").is_none());
    }

    #[test]
    fn import_accepts_bare_playbook() {
        let (name, goal, playbook) = parse_skill_file(&sample_playbook()).unwrap();
        assert_eq!(name, "完成 STAR 訓練");
        assert_eq!(goal, "把 STAR 課程的影片看完並通過測驗");
        assert_eq!(playbook["steps"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn import_rejects_wrong_kind_and_empty_steps() {
        assert!(
            parse_skill_file(&json!({"kind":"other","name":"a","steps":[{"do":"x"}]})).is_err()
        );
        assert!(parse_skill_file(&json!({"name":"a","steps":[]})).is_err());
        assert!(parse_skill_file(&json!({"steps":[{"do":"x"}]})).is_err());
    }

    #[test]
    fn unique_name_adds_suffix() {
        let have = vec!["完成 STAR 訓練".into(), "完成 STAR 訓練 (2)".into()];
        assert_eq!(
            unique_skill_name(&have, "完成 STAR 訓練"),
            "完成 STAR 訓練 (3)"
        );
        assert_eq!(unique_skill_name(&have, "新技能"), "新技能");
    }
}
