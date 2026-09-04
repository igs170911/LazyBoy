use std::collections::HashSet;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use lazyboy_contracts::{CreateRoomInput, CreateSessionInput, Room, RoomMember, RoomStatus, Session};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::db::Actor;
use crate::sessions::{normalized_title, session_from_row};
use crate::state::AppState;

type ApiError = (StatusCode, Json<Value>);

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/rooms", get(list_rooms).post(create_room))
        .route("/api/rooms/{id}", get(get_room).delete(delete_room))
        .route("/api/rooms/{id}/sessions", get(list_sessions).post(create_session))
        .route("/api/rooms/{id}/status", get(room_status))
}

async fn actor(state: &AppState) -> Result<Actor, ApiError> {
    state
        .bootstrap()
        .await
        .map_err(|error| internal(error.to_string()))
}

fn internal(message: String) -> ApiError {
    tracing::error!("rooms: {message}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"message":"internal error"})),
    )
}

fn member_from_row(row: (String, String, String, String)) -> RoomMember {
    RoomMember {
        id: row.0,
        name: row.1,
        avatar_color: row.2,
        avatar_shape: row.3,
    }
}

async fn members_for(state: &AppState, room_id: &str) -> Result<Vec<RoomMember>, sqlx::Error> {
    let rows: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT b.id, b.name, b.avatar_color, b.avatar_shape
         FROM room_members m JOIN bots b ON b.id=m.bot_id
         WHERE m.room_id=$1
         ORDER BY m.created_at, b.name",
    )
    .bind(room_id)
    .fetch_all(state.pool())
    .await?;
    Ok(rows.into_iter().map(member_from_row).collect())
}

async fn room_from_id(state: &AppState, actor: &Actor, id: &str) -> Result<Option<Room>, sqlx::Error> {
    let row: Option<(String, String, Option<chrono::DateTime<chrono::Utc>>, Option<String>, i64)> =
        sqlx::query_as(
            "SELECT r.id, r.name,
                    (SELECT MAX(m.created_at) FROM messages m JOIN threads t ON t.id=m.thread_id WHERE t.room_id=r.id),
                    (SELECT m.body FROM messages m JOIN threads t ON t.id=m.thread_id
                     WHERE t.room_id=r.id ORDER BY m.created_at DESC, m.seq DESC LIMIT 1),
                    (SELECT COUNT(*) FROM messages m JOIN threads t ON t.id=m.thread_id
                     WHERE t.room_id=r.id AND m.role='assistant' AND m.created_at > COALESCE(
                        (SELECT MIN(b.last_read_at) FROM room_members rm JOIN bots b ON b.id=rm.bot_id WHERE rm.room_id=r.id),
                        now()
                     ))
             FROM rooms r
             WHERE r.id=$1 AND r.space_id=$2 AND r.user_id=$3",
        )
        .bind(id)
        .bind(&actor.space_id)
        .bind(&actor.user_id)
        .fetch_optional(state.pool())
        .await?;
    let Some((id, name, last_message_at, last_preview, unread_count)) = row else {
        return Ok(None);
    };
    Ok(Some(Room {
        members: members_for(state, &id).await?,
        id,
        name,
        last_message_at,
        last_preview,
        unread_count,
    }))
}

async fn list_rooms(State(state): State<AppState>) -> Result<Json<Vec<Room>>, ApiError> {
    let actor = actor(&state).await?;
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM rooms WHERE space_id=$1 AND user_id=$2 ORDER BY updated_at DESC, created_at DESC",
    )
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .fetch_all(state.pool())
    .await
    .map_err(|error| internal(error.to_string()))?;
    let mut rooms = Vec::new();
    for id in ids {
        if let Some(room) = room_from_id(&state, &actor, &id)
            .await
            .map_err(|error| internal(error.to_string()))?
        {
            rooms.push(room);
        }
    }
    Ok(Json(rooms))
}

async fn create_room(
    State(state): State<AppState>,
    Json(input): Json<CreateRoomInput>,
) -> Result<(StatusCode, Json<Room>), ApiError> {
    let actor = actor(&state).await?;
    let name = input.name.trim();
    let mut seen = HashSet::new();
    let member_ids: Vec<String> = input
        .member_ids
        .into_iter()
        .filter(|id| seen.insert(id.clone()))
        .collect();
    if name.is_empty() || member_ids.len() < 2 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"message":"群組需要名稱，並至少兩位機器人"})),
        ));
    }
    let mut members = Vec::new();
    for bot_id in &member_ids {
        let exists: Option<(String, String, String, String)> = sqlx::query_as(
            "SELECT id, name, avatar_color, avatar_shape FROM bots
             WHERE id=$1 AND space_id=$2 AND user_id=$3",
        )
        .bind(bot_id)
        .bind(&actor.space_id)
        .bind(&actor.user_id)
        .fetch_optional(state.pool())
        .await
        .map_err(|error| internal(error.to_string()))?;
        let Some(row) = exists else {
            return Err((StatusCode::BAD_REQUEST, Json(json!({"message":"找不到機器人"}))));
        };
        members.push(member_from_row(row));
    }
    let room_id = Uuid::new_v4().to_string();
    let host_id = members[0].id.clone();
    let mut tx = state.pool().begin().await.map_err(|error| internal(error.to_string()))?;
    sqlx::query("INSERT INTO rooms (id,space_id,user_id,name) VALUES ($1,$2,$3,$4)")
        .bind(&room_id)
        .bind(&actor.space_id)
        .bind(&actor.user_id)
        .bind(name)
        .execute(&mut *tx)
        .await
        .map_err(|error| internal(error.to_string()))?;
    for member in &members {
        sqlx::query("INSERT INTO room_members (room_id,bot_id) VALUES ($1,$2)")
            .bind(&room_id)
            .bind(&member.id)
            .execute(&mut *tx)
            .await
            .map_err(|error| internal(error.to_string()))?;
    }
    sqlx::query(
        "INSERT INTO threads (id,space_id,bot_id,user_id,title,room_id)
         VALUES ($1,$2,$3,$4,'新對話',$5)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(&actor.space_id)
    .bind(&host_id)
    .bind(&actor.user_id)
    .bind(&room_id)
    .execute(&mut *tx)
    .await
    .map_err(|error| internal(error.to_string()))?;
    tx.commit().await.map_err(|error| internal(error.to_string()))?;
    Ok((
        StatusCode::CREATED,
        Json(Room {
            id: room_id,
            name: name.into(),
            members,
            last_message_at: None,
            last_preview: None,
            unread_count: 0,
        }),
    ))
}

async fn get_room(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Room>, ApiError> {
    let actor = actor(&state).await?;
    room_from_id(&state, &actor, &id)
        .await
        .map_err(|error| internal(error.to_string()))?
        .map(Json)
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"message":"group not found"}))))
}

async fn delete_room(State(state): State<AppState>, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    let actor = actor(&state).await?;
    let deleted = sqlx::query("DELETE FROM rooms WHERE id=$1 AND space_id=$2 AND user_id=$3")
        .bind(&id)
        .bind(&actor.space_id)
        .bind(&actor.user_id)
        .execute(state.pool())
        .await
        .map_err(|error| internal(error.to_string()))?;
    if deleted.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, Json(json!({"message":"group not found"}))));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn list_sessions(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<Session>>, ApiError> {
    let actor = actor(&state).await?;
    if room_from_id(&state, &actor, &id)
        .await
        .map_err(|error| internal(error.to_string()))?
        .is_none()
    {
        return Err((StatusCode::NOT_FOUND, Json(json!({"message":"group not found"}))));
    }
    let rows = sqlx::query_as(
        "SELECT id, bot_id, title, status, created_at, updated_at, next_message_seq,
                history_summary, history_summary_seq
         FROM threads
         WHERE room_id=$1 AND space_id=$2 AND user_id=$3 AND status='active'
         ORDER BY updated_at DESC, created_at DESC",
    )
    .bind(id)
    .bind(actor.space_id)
    .bind(actor.user_id)
    .fetch_all(state.pool())
    .await
    .map_err(|error| internal(error.to_string()))?;
    Ok(Json(rows.into_iter().map(session_from_row).collect()))
}

async fn create_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CreateSessionInput>,
) -> Result<(StatusCode, Json<Session>), ApiError> {
    let actor = actor(&state).await?;
    let Some(room) = room_from_id(&state, &actor, &id)
        .await
        .map_err(|error| internal(error.to_string()))?
    else {
        return Err((StatusCode::NOT_FOUND, Json(json!({"message":"group not found"}))));
    };
    let host = room
        .members
        .first()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, Json(json!({"message":"群組沒有成員"}))))?;
    let title = normalized_title(&input.title);
    let row = sqlx::query_as(
        "INSERT INTO threads (id, space_id, bot_id, user_id, title, room_id)
         VALUES ($1,$2,$3,$4,$5,$6)
         RETURNING id, bot_id, title, status, created_at, updated_at, next_message_seq,
                   history_summary, history_summary_seq",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(&actor.space_id)
    .bind(&host.id)
    .bind(&actor.user_id)
    .bind(title)
    .bind(&id)
    .fetch_one(state.pool())
    .await
    .map_err(|error| internal(error.to_string()))?;
    Ok((StatusCode::CREATED, Json(session_from_row(row))))
}

async fn room_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<RoomStatus>, ApiError> {
    let actor = actor(&state).await?;
    if room_from_id(&state, &actor, &id)
        .await
        .map_err(|error| internal(error.to_string()))?
        .is_none()
    {
        return Err((StatusCode::NOT_FOUND, Json(json!({"message":"group not found"}))));
    }
    let rows: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT DISTINCT b.id, b.name, b.avatar_color, b.avatar_shape
         FROM runs r
         JOIN bots b ON b.id=r.bot_id
         JOIN threads t ON t.id=r.thread_id
         WHERE t.room_id=$1 AND t.space_id=$2 AND t.user_id=$3
           AND r.status IN ('queued','leased','running','waiting_input','waiting_takeover')
         ORDER BY b.name",
    )
    .bind(&id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .fetch_all(state.pool())
    .await
    .map_err(|error| internal(error.to_string()))?;
    Ok(Json(RoomStatus {
        busy: rows.into_iter().map(member_from_row).collect(),
    }))
}
