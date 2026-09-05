//! Recurring bot runs. Chat or the computer panel can create them; a tick
//! loop turns due rows into ordinary queued runs.

use std::str::FromStr;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use cron::Schedule;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::FromRow;
use uuid::Uuid;

use crate::db::Actor;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleRow {
    pub id: String,
    pub bot_id: String,
    pub thread_id: Option<String>,
    pub name: String,
    pub cron: String,
    pub timezone: String,
    pub instructions: String,
    pub enabled: bool,
    pub last_run_at: Option<DateTime<Utc>>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSchedule {
    pub name: String,
    pub cron: String,
    #[serde(default)]
    pub timezone: String,
    pub instructions: String,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSchedule {
    pub name: Option<String>,
    pub cron: Option<String>,
    pub timezone: Option<String>,
    pub instructions: Option<String>,
    pub enabled: Option<bool>,
    pub thread_id: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/bots/{bot_id}/schedules",
            get(list_http).post(create_http),
        )
        .route(
            "/api/schedules/{id}",
            axum::routing::patch(update_http).delete(delete_http),
        )
        .route("/api/schedules/{id}/run", post(run_now_http))
}

async fn scoped_actor(state: &AppState, bot_id: &str) -> Result<Actor, StatusCode> {
    let actor = state
        .bootstrap()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    state
        .db
        .get_bot(&actor, bot_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(actor)
}

async fn list_http(
    State(state): State<AppState>,
    Path(bot_id): Path<String>,
) -> Result<Json<Vec<Value>>, StatusCode> {
    let actor = scoped_actor(&state, &bot_id).await?;
    let rows = list(&state, &actor, &bot_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(rows.into_iter().map(public_json).collect()))
}

async fn create_http(
    State(state): State<AppState>,
    Path(bot_id): Path<String>,
    Json(input): Json<CreateSchedule>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let actor = scoped_actor(&state, &bot_id)
        .await
        .map_err(|status| (status, Json(json!({"message":"bot not found"}))))?;
    create(&state, &actor, &bot_id, input)
        .await
        .map(|row| Json(public_json(row)))
        .map_err(|error| (StatusCode::BAD_REQUEST, Json(json!({"message": error}))))
}

async fn update_http(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateSchedule>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let actor = state.bootstrap().await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"message":"actor"})),
        )
    })?;
    update(&state, &actor, &id, input)
        .await
        .map_err(|error| (StatusCode::BAD_REQUEST, Json(json!({"message": error}))))?
        .map(|row| Json(public_json(row)))
        .ok_or((
            StatusCode::NOT_FOUND,
            Json(json!({"message":"schedule not found"})),
        ))
}

async fn delete_http(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let actor = state
        .bootstrap()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let deleted = sqlx::query("DELETE FROM schedules WHERE id=$1 AND space_id=$2 AND user_id=$3")
        .bind(&id)
        .bind(&actor.space_id)
        .bind(&actor.user_id)
        .execute(state.pool())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if deleted.rows_affected() == 0 {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn run_now_http(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let actor = state.bootstrap().await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"message":"actor"})),
        )
    })?;
    let row = get_row(&state, &actor, &id)
        .await
        .map_err(|error| (StatusCode::BAD_REQUEST, Json(json!({"message": error}))))?
        .ok_or((
            StatusCode::NOT_FOUND,
            Json(json!({"message":"schedule not found"})),
        ))?;
    enqueue(&state, &row, false)
        .await
        .map(|run_id| Json(json!({"ok": true, "runId": run_id})))
        .map_err(|error| (StatusCode::BAD_REQUEST, Json(json!({"message": error}))))
}

pub fn public_json(row: ScheduleRow) -> Value {
    json!({
        "id": row.id,
        "botId": row.bot_id,
        "threadId": row.thread_id,
        "name": row.name,
        "cron": row.cron,
        "timezone": row.timezone,
        "instructions": row.instructions,
        "enabled": row.enabled,
        "human": describe_cron(&row.cron),
        "lastRunAt": row.last_run_at,
        "nextRunAt": row.next_run_at,
        "createdAt": row.created_at,
        "updatedAt": row.updated_at,
    })
}

pub async fn list(
    state: &AppState,
    actor: &Actor,
    bot_id: &str,
) -> Result<Vec<ScheduleRow>, String> {
    sqlx::query_as(
        "SELECT id, bot_id, thread_id, name, cron, timezone, instructions, enabled,
                last_run_at, next_run_at, created_at, updated_at
         FROM schedules
         WHERE bot_id=$1 AND space_id=$2 AND user_id=$3
         ORDER BY enabled DESC, next_run_at ASC NULLS LAST, updated_at DESC",
    )
    .bind(bot_id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .fetch_all(state.pool())
    .await
    .map_err(|error| error.to_string())
}

pub async fn get_row(
    state: &AppState,
    actor: &Actor,
    id: &str,
) -> Result<Option<ScheduleRow>, String> {
    sqlx::query_as(
        "SELECT id, bot_id, thread_id, name, cron, timezone, instructions, enabled,
                last_run_at, next_run_at, created_at, updated_at
         FROM schedules WHERE id=$1 AND space_id=$2 AND user_id=$3",
    )
    .bind(id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .fetch_optional(state.pool())
    .await
    .map_err(|error| error.to_string())
}

pub async fn create(
    state: &AppState,
    actor: &Actor,
    bot_id: &str,
    input: CreateSchedule,
) -> Result<ScheduleRow, String> {
    validate_thread(state, actor, bot_id, input.thread_id.as_deref()).await?;
    let name = clean_name(&input.name)?;
    let instructions = clean_instructions(&input.instructions)?;
    let cron = validate_cron(&input.cron)?;
    let timezone = validate_timezone(&input.timezone)?;
    let next = if input.enabled {
        Some(next_fire(&cron, &timezone, Utc::now())?)
    } else {
        None
    };
    let id = Uuid::new_v4().to_string();
    sqlx::query_as(
        "INSERT INTO schedules
            (id, space_id, user_id, bot_id, thread_id, name, cron, timezone, instructions, enabled, next_run_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
         RETURNING id, bot_id, thread_id, name, cron, timezone, instructions, enabled,
                   last_run_at, next_run_at, created_at, updated_at",
    )
    .bind(&id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .bind(bot_id)
    .bind(input.thread_id.as_deref())
    .bind(name)
    .bind(&cron)
    .bind(&timezone)
    .bind(instructions)
    .bind(input.enabled)
    .bind(next)
    .fetch_one(state.pool())
    .await
    .map_err(|error| error.to_string())
}

async fn update(
    state: &AppState,
    actor: &Actor,
    id: &str,
    input: UpdateSchedule,
) -> Result<Option<ScheduleRow>, String> {
    let existing = match get_row(state, actor, id).await? {
        Some(row) => row,
        None => return Ok(None),
    };
    let name = match input.name {
        Some(value) => clean_name(&value)?,
        None => existing.name.clone(),
    };
    let instructions = match input.instructions {
        Some(value) => clean_instructions(&value)?,
        None => existing.instructions.clone(),
    };
    let cron = match input.cron {
        Some(value) => validate_cron(&value)?,
        None => existing.cron.clone(),
    };
    let timezone = match input.timezone {
        Some(value) => validate_timezone(&value)?,
        None => existing.timezone.clone(),
    };
    let enabled = input.enabled.unwrap_or(existing.enabled);
    let thread_id = input.thread_id.or(existing.thread_id);
    validate_thread(state, actor, &existing.bot_id, thread_id.as_deref()).await?;
    let next = if enabled {
        Some(next_fire(&cron, &timezone, Utc::now())?)
    } else {
        None
    };
    sqlx::query_as(
        "UPDATE schedules
         SET name=$1, cron=$2, timezone=$3, instructions=$4, enabled=$5, thread_id=$6,
             next_run_at=$7, updated_at=now()
         WHERE id=$8 AND space_id=$9 AND user_id=$10
         RETURNING id, bot_id, thread_id, name, cron, timezone, instructions, enabled,
                   last_run_at, next_run_at, created_at, updated_at",
    )
    .bind(name)
    .bind(cron)
    .bind(timezone)
    .bind(instructions)
    .bind(enabled)
    .bind(thread_id)
    .bind(next)
    .bind(id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .fetch_optional(state.pool())
    .await
    .map_err(|error| error.to_string())
}

pub async fn tick_loop(state: AppState) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
        if let Err(error) = fire_due(&state).await {
            tracing::warn!("schedule tick failed: {error}");
        }
    }
}

async fn fire_due(state: &AppState) -> Result<(), String> {
    let due: Vec<ScheduleRow> = sqlx::query_as(
        "SELECT id, bot_id, thread_id, name, cron, timezone, instructions, enabled,
                last_run_at, next_run_at, created_at, updated_at
         FROM schedules
         WHERE enabled AND next_run_at IS NOT NULL AND next_run_at <= now()
         ORDER BY next_run_at ASC
         LIMIT 8",
    )
    .fetch_all(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    for row in due {
        if let Err(error) = enqueue(state, &row, true).await {
            tracing::warn!("schedule {} failed to enqueue: {error}", row.id);
        }
    }
    Ok(())
}

async fn enqueue(state: &AppState, row: &ScheduleRow, from_tick: bool) -> Result<String, String> {
    let mut tx = state.pool().begin().await.map_err(|e| e.to_string())?;
    let locked: Option<ScheduleRow> = if from_tick {
        sqlx::query_as("SELECT id,bot_id,thread_id,name,cron,timezone,instructions,enabled,last_run_at,next_run_at,created_at,updated_at FROM schedules WHERE id=$1 AND enabled AND next_run_at<=now() FOR UPDATE SKIP LOCKED")
            .bind(&row.id).fetch_optional(&mut *tx).await.map_err(|e| e.to_string())?
    } else {
        Some(row.clone())
    };
    let Some(row) = locked.as_ref() else {
        return Ok(String::new());
    };

    let bot = sqlx::query_as::<_, (String, String, String)>(
        "SELECT space_id, user_id, id FROM bots WHERE id=$1",
    )
    .bind(&row.bot_id)
    .fetch_optional(state.pool())
    .await
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "bot not found".to_string())?;
    let thread_id = match row.thread_id.as_deref() {
        Some(id) => id.to_string(),
        None => latest_thread(state, &row.bot_id).await?,
    };
    validate_thread(
        state,
        &Actor {
            space_id: bot.0.clone(),
            user_id: bot.1.clone(),
        },
        &row.bot_id,
        Some(&thread_id),
    )
    .await?;
    let prompt = format!("Scheduled task 「{}」:\n{}", row.name, row.instructions);
    let seq: i32 = sqlx::query_scalar(
        "UPDATE threads SET next_message_seq=next_message_seq+1, updated_at=now()
         WHERE id=$1 RETURNING next_message_seq-1",
    )
    .bind(&thread_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|error| error.to_string())?;
    let run_id = Uuid::new_v4().to_string();
    let message_id = Uuid::new_v4().to_string();
    let body = if from_tick {
        format!("[排程] {}", row.name)
    } else {
        format!("[排程試跑] {}", row.name)
    };
    sqlx::query(
        "INSERT INTO messages (id,thread_id,seq,role,body,blocks,run_id)
         VALUES ($1,$2,$3,'user',$4,$5,$6)",
    )
    .bind(&message_id)
    .bind(&thread_id)
    .bind(seq)
    .bind(&body)
    .bind(json!([{"kind":"scheduleRun","scheduleId":row.id,"name":row.name,"human":describe_cron(&row.cron)}]))
    .bind(&run_id)
    .execute(&mut *tx)
    .await
    .map_err(|error| error.to_string())?;
    sqlx::query(
        "INSERT INTO runs (id,space_id,bot_id,thread_id,user_id,status,prompt,checkpoint)
         VALUES ($1,$2,$3,$4,$5,'queued',$6,$7)",
    )
    .bind(&run_id)
    .bind(&bot.0)
    .bind(&row.bot_id)
    .bind(&thread_id)
    .bind(&bot.1)
    .bind(&prompt)
    .bind(json!({"messageSeq": seq, "scheduleId": row.id}))
    .execute(&mut *tx)
    .await
    .map_err(|error| error.to_string())?;
    if from_tick {
        let next = next_after_due(
            &row.cron,
            &row.timezone,
            row.next_run_at.unwrap_or_else(Utc::now),
            Utc::now(),
        )?;
        sqlx::query(
            "UPDATE schedules SET last_run_at=now(),next_run_at=$2,updated_at=now() WHERE id=$1",
        )
        .bind(&row.id)
        .bind(next)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }
    tx.commit().await.map_err(|error| error.to_string())?;
    Ok(run_id)
}

async fn latest_thread(state: &AppState, bot_id: &str) -> Result<String, String> {
    sqlx::query_scalar(
        "SELECT id FROM threads
         WHERE bot_id=$1 AND status='active' AND room_id IS NULL
         ORDER BY updated_at DESC LIMIT 1",
    )
    .bind(bot_id)
    .fetch_optional(state.pool())
    .await
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "bot has no conversation to run this schedule in".into())
}

fn clean_name(name: &str) -> Result<String, String> {
    let value = name.trim();
    if value.is_empty() || value.chars().count() > 80 {
        return Err("name must be 1–80 characters".into());
    }
    Ok(value.to_string())
}

fn clean_instructions(text: &str) -> Result<String, String> {
    let value = text.trim();
    if value.is_empty() || value.chars().count() > 8_000 {
        return Err("instructions must be 1–8000 characters".into());
    }
    Ok(value.to_string())
}

pub fn validate_cron(expr: &str) -> Result<String, String> {
    let trimmed = expr.trim();
    if interval_seconds(trimmed)?.is_none() {
        parse_schedule(trimmed)?;
    }
    Ok(trimmed.to_string())
}

fn validate_timezone(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok("Asia/Taipei".into());
    }
    Tz::from_str(trimmed).map_err(|_| "invalid IANA timezone".to_string())?;
    Ok(trimmed.to_string())
}

fn interval_seconds(expr: &str) -> Result<Option<i64>, String> {
    let Some(raw) = expr.strip_prefix("@every ") else {
        return Ok(None);
    };
    if !raw.is_ascii() || raw.len() < 2 {
        return Err("invalid fixed interval".into());
    }
    let (number, unit) = raw.split_at(raw.len() - 1);
    let n: i64 = number.parse().map_err(|_| "invalid interval")?;
    if !(1..=365).contains(&n) {
        return Err("interval must be 1–365".into());
    }
    let scale = match unit {
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => return Err("interval unit must be m, h, or d".into()),
    };
    Ok(Some(n * scale))
}

fn parse_schedule(expr: &str) -> Result<Schedule, String> {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    if fields.len() != 5 {
        return Err("cron must have exactly 5 fields".into());
    }
    if fields[2] != "*" && fields[4] != "*" {
        return Err("use either day-of-month or day-of-week, not both".into());
    }
    let weekday = standard_weekdays(fields[4])?;
    let six = format!(
        "0 {} {} {} {} {}",
        fields[0], fields[1], fields[2], fields[3], weekday
    );
    Schedule::from_str(&six).map_err(|error| format!("invalid cron: {error}"))
}

// UI/API use Unix weekdays (0/7=Sun, 1=Mon); cron crate uses 1=Sun.
fn standard_weekdays(field: &str) -> Result<String, String> {
    if field == "*" {
        return Ok("*".into());
    }
    fn value(raw: &str) -> Result<u32, String> {
        match raw.to_ascii_uppercase().as_str() {
            "SUN" => Ok(0),
            "MON" => Ok(1),
            "TUE" => Ok(2),
            "WED" => Ok(3),
            "THU" => Ok(4),
            "FRI" => Ok(5),
            "SAT" => Ok(6),
            _ => raw
                .parse::<u32>()
                .ok()
                .filter(|n| *n <= 7)
                .ok_or_else(|| "invalid weekday".into()),
        }
    }
    let mut days = std::collections::BTreeSet::new();
    for part in field.split(',') {
        let (base, step) = if let Some((base, n)) = part.split_once('/') {
            (
                base,
                n.parse::<u32>()
                    .ok()
                    .filter(|n| (1..=7).contains(n))
                    .ok_or("invalid weekday step")?,
            )
        } else {
            (part, 1)
        };
        let (start, end) = if base == "*" {
            (0, 6)
        } else if let Some((a, b)) = base.split_once('-') {
            (value(a)?, value(b)?)
        } else {
            let n = value(base)?;
            (n, n)
        };
        if start > end {
            return Err("weekday range must be ascending".into());
        }
        for n in (start..=end).step_by(step as usize) {
            days.insert(n % 7 + 1);
        }
    }
    Ok(days
        .into_iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(","))
}

pub fn next_fire(expr: &str, timezone: &str, from: DateTime<Utc>) -> Result<DateTime<Utc>, String> {
    if let Some(seconds) = interval_seconds(expr)? {
        return Ok(from + chrono::TimeDelta::seconds(seconds));
    }
    let schedule = parse_schedule(expr)?;
    let tz: Tz = Tz::from_str(timezone).map_err(|_| "invalid IANA timezone")?;
    let local = from.with_timezone(&tz);
    schedule
        .after(&local)
        .next()
        .map(|when| when.with_timezone(&Utc))
        .ok_or_else(|| "cron has no future run".into())
}

pub fn describe_cron(expr: &str) -> String {
    if let Ok(Some(seconds)) = interval_seconds(expr) {
        return format!("每隔 {} 分鐘（固定間隔）", seconds / 60);
    }
    let parts: Vec<&str> = expr.trim().split_whitespace().collect();
    if parts.len() != 5 {
        return expr.to_string();
    }
    let (min, hour, dom, month, dow) = (parts[0], parts[1], parts[2], parts[3], parts[4]);
    if expr.trim() == "* * * * *" {
        return "每分鐘".into();
    }
    if let Some(rest) = min.strip_prefix("*/") {
        if hour == "*" && dom == "*" && month == "*" && dow == "*" {
            return if rest
                .parse::<u32>()
                .ok()
                .is_some_and(|n| n > 0 && 60 % n == 0)
            {
                format!("每 {rest} 分鐘")
            } else {
                format!("日曆排程：{expr}")
            };
        }
    }
    if min == "0" && hour == "*" && dom == "*" && month == "*" && dow == "*" {
        return "每小時".into();
    }
    if min == "0" {
        if let Some(rest) = hour.strip_prefix("*/") {
            if dom == "*" && month == "*" && dow == "*" {
                return if rest
                    .parse::<u32>()
                    .ok()
                    .is_some_and(|n| n > 0 && 24 % n == 0)
                {
                    format!("每 {rest} 小時")
                } else {
                    format!("日曆排程：{expr}")
                };
            }
        }
    }
    if min.parse::<u32>().is_ok() && hour.parse::<u32>().is_ok() && month == "*" {
        let at = format!("{hour:0>2}:{min:0>2}");
        if dom == "*" && dow == "*" {
            return format!("每天 {at}");
        }
        if dom == "*" && dow == "1-5" {
            return format!("工作日 {at}");
        }
        if dom == "*" && dow == "1" {
            return format!("每週一 {at}");
        }
        if dom == "1" && dow == "*" {
            return format!("每月 1 日 {at}");
        }
    }
    expr.to_string()
}

#[cfg(test)]
mod tests {
    use super::{describe_cron, next_fire, validate_cron};
    use chrono::TimeZone;

    #[test]
    fn weekday_nine_is_valid() {
        assert_eq!(validate_cron("0 9 * * 1-5").unwrap(), "0 9 * * 1-5");
        assert!(validate_cron("not cron").is_err());
    }

    #[test]
    fn describes_common_patterns() {
        assert_eq!(describe_cron("0 9 * * *"), "每天 09:00");
        assert_eq!(describe_cron("0 9 * * 1-5"), "工作日 09:00");
        assert_eq!(describe_cron("*/15 * * * *"), "每 15 分鐘");
    }

    #[test]
    fn next_fire_is_in_the_future() {
        let from = chrono::Utc.with_ymd_and_hms(2026, 9, 5, 0, 0, 0).unwrap();
        let next = next_fire("0 9 * * *", "Asia/Taipei", from).unwrap();
        assert!(next > from);
    }
}

async fn validate_thread(
    state: &AppState,
    actor: &Actor,
    bot: &str,
    thread: Option<&str>,
) -> Result<(), String> {
    let owns: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM bots WHERE id=$1 AND space_id=$2 AND user_id=$3)",
    )
    .bind(bot)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .fetch_one(state.pool())
    .await
    .map_err(|e| e.to_string())?;
    if !owns {
        return Err("bot not found".into());
    }
    if let Some(id) = thread {
        let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM threads WHERE id=$1 AND bot_id=$2 AND status='active' AND room_id IS NULL)")
            .bind(id).bind(bot).fetch_one(state.pool()).await.map_err(|e| e.to_string())?;
        if !valid {
            return Err("schedule conversation must belong to this bot and be active".into());
        }
    }
    Ok(())
}

fn next_after_due(
    expr: &str,
    timezone: &str,
    due: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>, String> {
    if let Some(seconds) = interval_seconds(expr)? {
        let elapsed = (now - due).num_seconds().max(0);
        return Ok(due + chrono::TimeDelta::seconds((elapsed / seconds + 1) * seconds));
    }
    next_fire(expr, timezone, now)
}

#[cfg(test)]
mod regression_tests {
    use super::*;
    use chrono::{Datelike, TimeZone};
    #[test]
    fn intervals_cross_month_and_dst_without_reset() {
        let from = Utc.with_ymd_and_hms(2026, 1, 31, 9, 0, 0).unwrap();
        assert_eq!(
            (next_fire("@every 3d", "America/New_York", from).unwrap() - from).num_hours(),
            72
        );
        let dst = Utc.with_ymd_and_hms(2026, 3, 7, 9, 0, 0).unwrap();
        assert_eq!(
            (next_fire("@every 2d", "America/New_York", dst).unwrap() - dst).num_hours(),
            48
        );
    }
    #[test]
    fn weekday_means_monday_through_friday() {
        let friday = Utc.with_ymd_and_hms(2026, 9, 4, 2, 0, 0).unwrap();
        let next = next_fire("0 9 * * 1-5", "Asia/Taipei", friday).unwrap();
        assert_eq!(next.weekday(), chrono::Weekday::Mon);
    }
    #[test]
    fn invalid_formats_and_timezones_fail_closed() {
        for expr in [
            "0 0 9 * * 1",
            "99 25 * * *",
            "@every 0d",
            "@every 366h",
            "@every 5z",
        ] {
            assert!(validate_cron(expr).is_err(), "{expr}");
        }
        assert!(validate_timezone("Asia/Typo").is_err());
    }
    #[test]
    fn missed_intervals_preserve_phase() {
        let due = Utc.with_ymd_and_hms(2026, 9, 5, 0, 0, 0).unwrap();
        assert_eq!(
            next_after_due(
                "@every 3m",
                "UTC",
                due,
                due + chrono::TimeDelta::seconds(400)
            )
            .unwrap(),
            due + chrono::TimeDelta::seconds(540)
        );
    }
}
