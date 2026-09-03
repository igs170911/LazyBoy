use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use axum::{Json, Router};
use futures_util::stream;
use lazyboy_contracts::{
    CreateSessionInput, SendSessionMessageInput, Session, SessionMessage, UpdateSessionInput,
};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::db::Actor;
use crate::state::AppState;

type ApiError = (StatusCode, Json<Value>);

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/bots/{id}/sessions", get(list_sessions).post(create_session))
        .route(
            "/api/sessions/{id}",
            get(get_session).patch(update_session).delete(delete_session),
        )
        .route(
            "/api/sessions/{id}/messages",
            get(list_messages).post(send_message),
        )
        .route("/api/sessions/{id}/events", get(events))
}

async fn actor(state: &AppState) -> Result<Actor, ApiError> {
    state
        .bootstrap()
        .await
        .map_err(|error| internal(error.to_string()))
}

fn internal(message: String) -> ApiError {
    tracing::error!("sessions: {message}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"message":"internal error"})),
    )
}

fn session_from_row(
    row: (
        String,
        String,
        String,
        String,
        chrono::DateTime<chrono::Utc>,
        chrono::DateTime<chrono::Utc>,
        i32,
        String,
        i32,
    ),
) -> Session {
    Session {
        id: row.0,
        bot_id: row.1,
        title: row.2,
        status: row.3,
        created_at: row.4,
        updated_at: row.5,
        next_message_seq: row.6,
        history_summary: row.7,
        history_summary_seq: row.8,
    }
}

async fn list_sessions(
    State(state): State<AppState>,
    Path(bot_id): Path<String>,
) -> Result<Json<Vec<Session>>, ApiError> {
    let actor = actor(&state).await?;
    let exists: Option<i32> = sqlx::query_scalar(
        "SELECT 1 FROM bots WHERE id=$1 AND space_id=$2 AND user_id=$3",
    )
    .bind(&bot_id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .fetch_optional(state.pool())
    .await
    .map_err(|error| internal(error.to_string()))?;
    if exists.is_none() {
        return Err((StatusCode::NOT_FOUND, Json(json!({"message":"bot not found"}))));
    }
    let rows = sqlx::query_as(
        "SELECT id, bot_id, title, status, created_at, updated_at, next_message_seq,
                history_summary, history_summary_seq
         FROM threads
         WHERE bot_id=$1 AND space_id=$2 AND user_id=$3
         ORDER BY updated_at DESC, created_at DESC",
    )
    .bind(bot_id)
    .bind(actor.space_id)
    .bind(actor.user_id)
    .fetch_all(state.pool())
    .await
    .map_err(|error| internal(error.to_string()))?;
    Ok(Json(rows.into_iter().map(session_from_row).collect()))
}

async fn create_session(
    State(state): State<AppState>,
    Path(bot_id): Path<String>,
    Json(input): Json<CreateSessionInput>,
) -> Result<(StatusCode, Json<Session>), ApiError> {
    let actor = actor(&state).await?;
    let exists: Option<i32> = sqlx::query_scalar(
        "SELECT 1 FROM bots WHERE id=$1 AND space_id=$2 AND user_id=$3",
    )
    .bind(&bot_id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .fetch_optional(state.pool())
    .await
    .map_err(|error| internal(error.to_string()))?;
    if exists.is_none() {
        return Err((StatusCode::NOT_FOUND, Json(json!({"message":"bot not found"}))));
    }
    let title = normalized_title(&input.title);
    let row = sqlx::query_as(
        "INSERT INTO threads (id, space_id, bot_id, user_id, title)
         VALUES ($1,$2,$3,$4,$5)
         RETURNING id, bot_id, title, status, created_at, updated_at, next_message_seq,
                   history_summary, history_summary_seq",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(actor.space_id)
    .bind(bot_id)
    .bind(actor.user_id)
    .bind(title)
    .fetch_one(state.pool())
    .await
    .map_err(|error| internal(error.to_string()))?;
    Ok((StatusCode::CREATED, Json(session_from_row(row))))
}

async fn get_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Session>, ApiError> {
    let actor = actor(&state).await?;
    let row = scoped_session_row(&state, &actor, &id)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"message":"session not found"}))))?;
    Ok(Json(session_from_row(row)))
}

async fn update_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateSessionInput>,
) -> Result<Json<Session>, ApiError> {
    let actor = actor(&state).await?;
    if input
        .status
        .as_deref()
        .is_some_and(|status| !matches!(status, "active" | "archived"))
    {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"message":"invalid status"}))));
    }
    let title = input.title.as_deref().map(normalized_title);
    let row = sqlx::query_as(
        "UPDATE threads
         SET title=COALESCE($1,title), status=COALESCE($2,status), updated_at=now()
         WHERE id=$3 AND space_id=$4 AND user_id=$5
         RETURNING id, bot_id, title, status, created_at, updated_at, next_message_seq,
                   history_summary, history_summary_seq",
    )
    .bind(title)
    .bind(input.status)
    .bind(id)
    .bind(actor.space_id)
    .bind(actor.user_id)
    .fetch_optional(state.pool())
    .await
    .map_err(|error| internal(error.to_string()))?
    .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"message":"session not found"}))))?;
    Ok(Json(session_from_row(row)))
}

async fn delete_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let actor = actor(&state).await?;
    let mut tx = state.pool().begin().await.map_err(|error| internal(error.to_string()))?;
    let bot_id: Option<String> = sqlx::query_scalar(
        "DELETE FROM threads WHERE id=$1 AND space_id=$2 AND user_id=$3 RETURNING bot_id",
    )
    .bind(&id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| internal(error.to_string()))?;
    let Some(bot_id) = bot_id else {
        return Err((StatusCode::NOT_FOUND, Json(json!({"message":"session not found"}))));
    };
    let remaining: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM threads WHERE bot_id=$1)")
            .bind(&bot_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|error| internal(error.to_string()))?;
    if !remaining {
        sqlx::query(
            "INSERT INTO threads (id,space_id,bot_id,user_id,title) VALUES ($1,$2,$3,$4,'New session')",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(&actor.space_id)
        .bind(bot_id)
        .bind(&actor.user_id)
        .execute(&mut *tx)
        .await
        .map_err(|error| internal(error.to_string()))?;
    }
    tx.commit().await.map_err(|error| internal(error.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_messages(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<SessionMessage>>, ApiError> {
    let actor = actor(&state).await?;
    Ok(Json(messages_for_session(&state, &actor, &id).await?))
}

async fn send_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<SendSessionMessageInput>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let actor = actor(&state).await?;
    if input.text.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"message":"empty message"}))));
    }
    let session = scoped_session_row(&state, &actor, &id)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"message":"session not found"}))))?;
    let result = crate::runs::send(
        &state,
        &actor,
        &session.1,
        &id,
        input.text.trim(),
        input.client_nonce.as_deref(),
        &input.blocks,
    )
    .await
    .map_err(|message| (StatusCode::BAD_REQUEST, Json(json!({"message":message}))))?;
    Ok((StatusCode::ACCEPTED, Json(result)))
}

async fn events(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let actor = actor(&state).await?;
    if scoped_session_row(&state, &actor, &id).await?.is_none() {
        return Err((StatusCode::NOT_FOUND, Json(json!({"message":"session not found"}))));
    }
    let after = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(0);
    let stream_state = (
        state,
        id,
        actor,
        after,
        Vec::<(i32, String, Value)>::new(),
    );
    let output = stream::unfold(stream_state, |(state, id, actor, mut after, mut pending)| async move {
        loop {
            if let Some((seq, kind, payload)) = pending.pop() {
                after = seq;
                let event = Event::default()
                    .id(seq.to_string())
                    .event(kind)
                    .json_data(payload)
                    .unwrap_or_else(|_| Event::default().event("error").data("{}"));
                return Some((Ok(event), (state, id, actor, after, pending)));
            }
            match sqlx::query_as::<_, (i32, String, Value)>(
                "SELECT e.seq,e.type,e.payload FROM events e
                 JOIN threads t ON t.id=e.thread_id
                 WHERE e.thread_id=$1 AND e.seq>$2 AND t.space_id=$3 AND t.user_id=$4
                 ORDER BY e.seq ASC LIMIT 100",
            )
            .bind(&id)
            .bind(after)
            .bind(&actor.space_id)
            .bind(&actor.user_id)
            .fetch_all(state.pool())
            .await
            {
                Ok(mut rows) if !rows.is_empty() => {
                    rows.reverse();
                    pending = rows;
                }
                Ok(_) | Err(_) => tokio::time::sleep(Duration::from_millis(750)).await,
            }
        }
    });
    Ok(Sse::new(output).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    ))
}

pub async fn default_session_for_bot(
    state: &AppState,
    actor: &Actor,
    bot_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT id FROM threads
         WHERE bot_id=$1 AND space_id=$2 AND user_id=$3 AND status='active'
         ORDER BY updated_at DESC, created_at ASC LIMIT 1",
    )
    .bind(bot_id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .fetch_optional(state.pool())
    .await
}

pub async fn messages_for_session(
    state: &AppState,
    actor: &Actor,
    id: &str,
) -> Result<Vec<SessionMessage>, ApiError> {
    let rows: Vec<(
        String,
        String,
        i32,
        String,
        String,
        Value,
        Option<String>,
        Option<String>,
        chrono::DateTime<chrono::Utc>,
    )> = sqlx::query_as(
        "SELECT m.id, m.thread_id, m.seq, m.role, m.body, m.blocks, m.run_id,
                m.client_nonce, m.created_at
         FROM messages m JOIN threads t ON t.id=m.thread_id
         WHERE t.id=$1 AND t.space_id=$2 AND t.user_id=$3
         ORDER BY m.seq ASC",
    )
    .bind(id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .fetch_all(state.pool())
    .await
    .map_err(|error| internal(error.to_string()))?;
    Ok(rows
        .into_iter()
        .map(|row| SessionMessage {
            id: row.0,
            session_id: row.1,
            seq: row.2,
            role: row.3,
            body: row.4,
            blocks: row.5,
            run_id: row.6,
            client_nonce: row.7,
            created_at: row.8,
        })
        .collect())
}

async fn scoped_session_row(
    state: &AppState,
    actor: &Actor,
    id: &str,
) -> Result<
    Option<(
        String,
        String,
        String,
        String,
        chrono::DateTime<chrono::Utc>,
        chrono::DateTime<chrono::Utc>,
        i32,
        String,
        i32,
    )>,
    ApiError,
> {
    sqlx::query_as(
        "SELECT id, bot_id, title, status, created_at, updated_at, next_message_seq,
                history_summary, history_summary_seq
         FROM threads WHERE id=$1 AND space_id=$2 AND user_id=$3",
    )
    .bind(id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .fetch_optional(state.pool())
    .await
    .map_err(|error| internal(error.to_string()))
}

pub async fn append_event(
    state: &AppState,
    thread_id: &str,
    kind: &str,
    payload: Value,
) -> Result<i32, sqlx::Error> {
    let mut tx = state.pool().begin().await?;
    let seq: i32 = sqlx::query_scalar(
        "UPDATE threads SET next_event_seq=next_event_seq+1, updated_at=now()
         WHERE id=$1 RETURNING next_event_seq",
    )
    .bind(thread_id)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO events (id,thread_id,seq,type,payload) VALUES ($1,$2,$3,$4,$5)")
        .bind(Uuid::new_v4().to_string())
        .bind(thread_id)
        .bind(seq)
        .bind(kind)
        .bind(payload)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(seq)
}

fn normalized_title(title: &str) -> String {
    let value: String = title.trim().chars().take(120).collect();
    if value.is_empty() {
        "New session".to_string()
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::normalized_title;

    #[test]
    fn session_titles_are_bounded_and_have_a_default() {
        assert_eq!(normalized_title("   "), "New session");
        assert_eq!(normalized_title("  Research  "), "Research");
        assert_eq!(normalized_title(&"x".repeat(150)).chars().count(), 120);
    }
}
