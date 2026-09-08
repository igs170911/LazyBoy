//! Observability surface for a run: the activity trail the chat bubble reads,
//! and the plain-language reading of a failure that turns into an action item.
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use crate::db::Actor;
use crate::state::AppState;

type ApiError = (StatusCode, Json<Value>);

/// Longest a single string inside an activity payload may get. The trail is a
/// glanceable summary, not a second copy of the transcript.
const MAX_STRING_CHARS: usize = 512;
const DEFAULT_ACTIVITY_LIMIT: i32 = 60;
const MAX_ACTIVITY_LIMIT: i32 = 200;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/runs/{id}/activity", get(activity))
        .route("/api/runs/{id}/retry", post(retry))
}

/// Append one line to the run's trail. Diagnostics never fail a run: a write
/// that cannot land is logged and dropped.
pub async fn record(state: &AppState, run_id: &str, kind: &str, payload: Value) {
    let result = sqlx::query("INSERT INTO run_activity (run_id,kind,payload) VALUES ($1,$2,$3)")
        .bind(run_id)
        .bind(kind)
        .bind(clamp_strings(&payload, MAX_STRING_CHARS))
        .execute(state.pool())
        .await;
    if let Err(error) = result {
        tracing::warn!(run_id, kind, "failed to record run activity: {error}");
    }
}

/// One line of text for the trail: single line, bounded, never an image.
pub fn snippet(text: &str, max_chars: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max_chars {
        return flat;
    }
    let mut head: String = flat.chars().take(max_chars).collect();
    head.push('…');
    head
}

/// Bound every string leaf so one chatty tool result cannot bloat the trail.
pub fn clamp_strings(value: &Value, max_chars: usize) -> Value {
    match value {
        Value::String(text) if text.chars().count() > max_chars => {
            let mut head: String = text.chars().take(max_chars).collect();
            head.push('…');
            Value::String(head)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| clamp_strings(item, max_chars))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, item)| (key.clone(), clamp_strings(item, max_chars)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// What the user is told, and what they can do about it. `code` is the stable
/// contract the chat card renders from; the Chinese strings are the fallback
/// body that also lives in the message history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunFailure {
    pub code: &'static str,
    pub headline: &'static str,
    pub action: &'static str,
    pub retryable: bool,
    pub needs_settings: bool,
    pub needs_computer: bool,
}

impl RunFailure {
    fn new(
        code: &'static str,
        headline: &'static str,
        action: &'static str,
        retryable: bool,
        needs_settings: bool,
        needs_computer: bool,
    ) -> Self {
        Self {
            code,
            headline,
            action,
            retryable,
            needs_settings,
            needs_computer,
        }
    }
}

/// Turn a raw run error into a classified, human-readable failure. Ordering is
/// the policy: the most specific and most actionable cause wins, so a 429 that
/// also mentions a timeout is reported as a quota problem, not a slow network.
pub fn classify_run_error(error: &str) -> RunFailure {
    let text = error.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|needle| text.contains(needle));
    if has(&["worker interrupted"]) {
        return RunFailure::new(
            "interrupted",
            "我剛剛被打斷了，最後那個動作可能已經做下去了。",
            "先按重試；不確定的話，打開它的畫面確認現狀再繼續。",
            true,
            false,
            false,
        );
    }
    if text.contains("tool ") && text.contains("timed out") {
        return RunFailure::new(
            "tool_timeout",
            "有一個動作卡超過 150 秒，我無法確定它有沒有做完。",
            "先打開它的畫面看一眼，再按重試；我不會重複已經成功的步驟。",
            true,
            false,
            false,
        );
    }
    if has(&[
        "invalid api key",
        "incorrect api key",
        "unauthorized",
        "authentication",
        "forbidden",
        "401",
        "403",
    ]) {
        return RunFailure::new(
            "model_key",
            "模型不認得這個 API 金鑰。",
            "到「設定 → 模型」重新貼一次金鑰，再按重試。",
            false,
            true,
            false,
        );
    }
    if has(&[
        "429",
        "rate limit",
        "ratelimit",
        "too many requests",
        "insufficient",
        "quota",
        "credit",
        "billing",
    ]) {
        return RunFailure::new(
            "model_quota",
            "模型那邊限流了，或是額度已經用完。",
            "等一下再按重試；如果是額度用完，到「設定 → 模型」換一個或加值。",
            true,
            true,
            false,
        );
    }
    if has(&[
        "model not found",
        "unknown model",
        "invalid model",
        "does not exist",
        "404",
    ]) {
        return RunFailure::new(
            "model_unknown",
            "這個模型名稱找不到，可能已下架或打錯了。",
            "到「設定 → 模型」選一個存在的模型，再按重試。",
            false,
            true,
            false,
        );
    }
    if has(&["timed out", "timeout", "逾時"]) {
        return RunFailure::new(
            "model_timeout",
            "模型太久沒有回話（超過 165 秒）。",
            "先按重試；每次都發生的話，到「設定 → 模型」換一個快一點的模型。",
            true,
            true,
            false,
        );
    }
    if has(&[
        "error sending request",
        "failed to connect",
        "connection refused",
        "dns",
        "host not found",
        "certificate",
        "tls",
        "network",
    ]) {
        return RunFailure::new(
            "network",
            "連不到模型服務，網路或位址不對。",
            "確認網路與「設定 → 模型」的 Base URL，再按重試。",
            true,
            true,
            false,
        );
    }
    if has(&[
        "no such container",
        "container",
        "docker",
        "computer not found",
        "computer is not running",
    ]) {
        return RunFailure::new(
            "computer_gone",
            "它的電腦不見了或已經停止。",
            "先把電腦啟動，再按重試。",
            true,
            false,
            true,
        );
    }
    if text.contains("lease") {
        return RunFailure::new(
            "lease_lost",
            "這份工作被另一邊接手過，我手上這份已經失效。",
            "按重試重新開始這一步就好。",
            true,
            false,
            false,
        );
    }
    RunFailure::new(
        "unknown",
        "我卡住了，這一輪沒有完成。",
        "按重試看看；需要的話把下面的記錄複製起來給我。",
        true,
        false,
        false,
    )
}

/// The sentence that goes into the chat: what happened, how far the run got,
/// and the one thing the human can do next.
pub fn failure_message(failure: &RunFailure, last_step: Option<&str>, turn: Option<i64>) -> String {
    let progress = match (last_step, turn) {
        (Some(step), Some(n)) if n > 0 && !step.trim().is_empty() => {
            format!("我最後在做：{}（第 {n} 輪）。", snippet(step, 80))
        }
        (Some(step), _) if !step.trim().is_empty() => {
            format!("我最後在做：{}。", snippet(step, 80))
        }
        _ => "我還沒有開始動手。".to_string(),
    };
    format!("{} {} {}", failure.headline, progress, failure.action)
}

#[derive(Debug, serde::Deserialize)]
struct ActivityQuery {
    after: Option<i64>,
    limit: Option<i32>,
}

#[derive(Debug, sqlx::FromRow)]
struct RunRow {
    status: String,
    step: Option<String>,
    turn: Option<i64>,
    turn_limit: Option<i64>,
    error: Option<String>,
    step_at: Option<DateTime<Utc>>,
    started_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
}

async fn actor(state: &AppState) -> Result<Actor, ApiError> {
    state.bootstrap().await.map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"message": error.to_string()})),
        )
    })
}

/// The bubble's whole payload: where the run stands right now plus its trail.
/// `after` pages forward; the first read returns the newest window, oldest
/// first, so the UI renders it and then tails from the last id.
async fn activity(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<ActivityQuery>,
) -> Result<Json<Value>, ApiError> {
    let actor = actor(&state).await?;
    let run = sqlx::query_as::<_, RunRow>(
        "SELECT status, checkpoint->>'step' AS step, (checkpoint->>'turn')::bigint AS turn,
                (checkpoint->>'turnLimit')::bigint AS turn_limit, error, (checkpoint->>'stepAt')::timestamptz AS step_at, started_at, completed_at
         FROM runs WHERE id=$1 AND space_id=$2 AND user_id=$3",
    )
    .bind(&id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .fetch_optional(state.pool())
    .await
    .map_err(internal)?;
    let Some(run) = run else {
        return Err(not_found("run not found"));
    };
    let limit = query
        .limit
        .unwrap_or(DEFAULT_ACTIVITY_LIMIT)
        .clamp(1, MAX_ACTIVITY_LIMIT);
    let rows = sqlx::query_as::<_, (i64, String, Value, DateTime<Utc>)>(
        "SELECT id, kind, payload, created_at FROM (
             SELECT id, kind, payload, created_at FROM run_activity
             WHERE run_id=$1 AND ($2::bigint IS NULL OR id>$2)
             ORDER BY id DESC LIMIT $3
         ) recent ORDER BY id ASC",
    )
    .bind(&id)
    .bind(query.after)
    .bind(limit)
    .fetch_all(state.pool())
    .await
    .map_err(internal)?;
    let error = run
        .error
        .as_deref()
        .filter(|raw| !raw.trim().is_empty())
        .map(|raw| {
            let failure = classify_run_error(raw);
            json!({"code": failure.code, "headline": failure.headline, "action": failure.action, "raw": raw})
        });
    let elapsed = run.started_at.map(|started| {
        (run.completed_at.unwrap_or_else(Utc::now) - started)
            .num_milliseconds()
            .max(0) as u64
    });
    Ok(Json(json!({
        "runId": id,
        "status": run.status,
        "turn": run.turn,
        "turnLimit": run.turn_limit,
        "step": run.step,
        "elapsedMs": elapsed,
        "stepAt": run.step_at,
        "error": error,
        "activity": rows.into_iter().map(|(row_id, kind, payload, created_at)| {
            let mut entry = payload;
            if let Some(map) = entry.as_object_mut() {
                map.insert("id".to_string(), json!(row_id));
                map.insert("kind".to_string(), json!(kind));
                map.insert("createdAt".to_string(), json!(created_at.to_rfc3339()));
            }
            entry
        }).collect::<Vec<_>>(),
    })))
}

/// Re-queue a run that stopped on an error, keeping its harness checkpoint so
/// it continues where it stopped instead of starting the task over.
async fn retry(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let actor = actor(&state).await?;
    let exists: Option<String> =
        sqlx::query_scalar("SELECT status FROM runs WHERE id=$1 AND space_id=$2 AND user_id=$3")
            .bind(&id)
            .bind(&actor.space_id)
            .bind(&actor.user_id)
            .fetch_optional(state.pool())
            .await
            .map_err(internal)?;
    if exists.is_none() {
        return Err(not_found("run not found"));
    }
    let queued: Option<String> = sqlx::query_scalar(
        "UPDATE runs SET status='queued', retry_count=0, error=NULL, completed_at=NULL,
                lease_owner=NULL, lease_expires_at=NULL, updated_at=now()
         WHERE id=$1 AND space_id=$2 AND user_id=$3
           AND status IN ('failed','cancelled')
           AND NOT EXISTS (
               SELECT 1 FROM runs a
               WHERE a.bot_id=runs.bot_id AND a.id<>runs.id
                 AND a.status IN ('leased','running','waiting_input','waiting_takeover')
                 AND (a.lease_expires_at IS NULL OR a.lease_expires_at >= now())
           )
         RETURNING thread_id",
    )
    .bind(&id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .fetch_optional(state.pool())
    .await
    .map_err(internal)?;
    if queued.is_none() {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"message": "run is not retryable"})),
        ));
    }
    record(&state, &id, "run", json!({"event": "retry"})).await;
    tracing::info!(run_id = id.as_str(), "run re-queued from the chat");
    Ok(Json(json!({"ok": true, "runId": id})))
}

fn not_found(message: &str) -> ApiError {
    (StatusCode::NOT_FOUND, Json(json!({"message": message})))
}

fn internal(error: sqlx::Error) -> ApiError {
    tracing::error!("monitor: {error}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"message": "internal error"})),
    )
}

#[cfg(test)]
mod tests {
    use super::{RunFailure, clamp_strings, classify_run_error, failure_message, snippet};
    use serde_json::json;

    fn code(error: &str) -> String {
        classify_run_error(error).code.to_string()
    }

    #[test]
    fn every_failure_offers_an_action_and_a_code() {
        for error in [
            "boom",
            "HTTP 401",
            "model request timed out after 165 seconds",
            "no such container",
        ] {
            let failure = classify_run_error(error);
            assert!(!failure.code.is_empty(), "{error}");
            assert!(!failure.action.is_empty(), "{error}");
            assert!(!failure.headline.is_empty(), "{error}");
        }
    }

    #[test]
    fn auth_quota_and_unknown_model_are_told_apart() {
        assert_eq!(
            code("Api error 401 Unauthorized: invalid api key"),
            "model_key"
        );
        assert_eq!(code("HTTP status 403 Forbidden"), "model_key");
        assert_eq!(code("status 429 Too Many Requests"), "model_quota");
        assert_eq!(code("insufficient quota for this project"), "model_quota");
        assert_eq!(code("The model gpt-9 does not exist"), "model_unknown");
        assert_eq!(code("HTTP status 404 Not Found"), "model_unknown");
    }

    #[test]
    fn the_specific_timeouts_win_over_the_generic_one() {
        assert_eq!(
            code("tool browser timed out after 150 seconds"),
            "tool_timeout"
        );
        assert_eq!(
            code("model request timed out after 165 seconds"),
            "model_timeout"
        );
        assert_eq!(code("AI 回應逾時（150 秒）"), "model_timeout");
    }

    #[test]
    fn infrastructure_and_network_failures_are_separate_from_the_model() {
        assert_eq!(code("no such container: lazyboy-bot-1"), "computer_gone");
        assert_eq!(code("computer not found"), "computer_gone");
        assert_eq!(code("error sending request: dns failure"), "network");
        assert_eq!(code("run lease was lost"), "lease_lost");
        assert_eq!(
            code("Worker interrupted after tool execution; inspect current state"),
            "interrupted"
        );
        assert_eq!(code("something odd happened"), "unknown");
    }

    #[test]
    fn a_retryable_answer_never_promises_a_setting_that_cannot_help() {
        assert!(!classify_run_error("401 invalid api key").retryable);
        assert!(classify_run_error("401 invalid api key").needs_settings);
        assert!(classify_run_error("no such container").needs_computer);
        assert!(!classify_run_error("no such container").needs_settings);
        assert!(classify_run_error("status 429 rate limit").retryable);
    }

    #[test]
    fn a_failure_sentence_names_the_last_step_and_turn() {
        let failure = RunFailure::new("tool_timeout", "卡住了。", "按重試。", true, false, false);
        let said = failure_message(&failure, Some("browser: click #12"), Some(7));
        assert!(said.contains("卡住了。"), "{said}");
        assert!(said.contains("browser: click #12"), "{said}");
        assert!(said.contains("第 7 輪"), "{said}");
        assert!(said.contains("按重試。"), "{said}");
    }

    #[test]
    fn a_failure_before_any_work_says_so_instead_of_inventing_a_step() {
        let failure = RunFailure::new("model_key", "金鑰不對。", "改金鑰。", false, true, false);
        assert!(failure_message(&failure, None, None).contains("我還沒有開始動手。"));
        assert!(failure_message(&failure, Some("  "), Some(0)).contains("我還沒有開始動手。"));
        assert!(failure_message(&failure, Some("思考中"), None).contains("思考中"));
    }

    #[test]
    fn snippets_stay_single_line_and_bounded() {
        assert_eq!(snippet("  多行\n文字\t在這裡  ", 40), "多行 文字 在這裡");
        let long = "字".repeat(500);
        let cut = snippet(&long, 200);
        assert_eq!(cut.chars().count(), 201);
        assert!(cut.ends_with('…'));
    }

    #[test]
    fn payloads_are_clamped_at_every_string_leaf() {
        let clamped = clamp_strings(
            &json!({"error": "x".repeat(900), "nested": {"raw": "y".repeat(900)}, "n": 7}),
            50,
        );
        assert_eq!(clamped["error"].as_str().unwrap().chars().count(), 51);
        assert_eq!(
            clamped["nested"]["raw"].as_str().unwrap().chars().count(),
            51
        );
        assert_eq!(clamped["n"].as_i64(), Some(7));
    }
}
