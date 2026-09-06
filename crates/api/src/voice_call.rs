use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message as AxumMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use futures_util::{SinkExt, StreamExt};
use lazyboy_contracts::SessionAttachment;
use lazyboy_harness::{VoiceConnectRequest, VoiceEvent, VoiceSocket, create_voice};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::db::Actor;
use crate::state::AppState;
use crate::voice::{resolve_space_voice, voice_instructions};

struct PreparedCall {
    call_id: String,
    bot_id: String,
    bot_name: String,
    provider: lazyboy_contracts::VoiceProvider,
    connect: VoiceConnectRequest,
}

pub async fn call_ws(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    ws: WebSocketUpgrade,
) -> axum::response::Response {
    let actor = match state.bootstrap().await {
        Ok(actor) => actor,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"message":"internal error"})),
            )
                .into_response();
        }
    };
    match prepare_call(&state, &actor, &session_id).await {
        Ok(prep) => {
            if !state.calls.try_begin(&prep.bot_id, &prep.call_id).await {
                return (
                    StatusCode::CONFLICT,
                    Json(json!({"message":"already on a call"})),
                )
                    .into_response();
            }
            let provider = create_voice(prep.provider);
            let socket = match provider.connect(prep.connect.clone()).await {
                Ok(socket) => socket,
                Err(error) => {
                    state.calls.end(&prep.bot_id, &prep.call_id).await;
                    return (
                        StatusCode::BAD_GATEWAY,
                        Json(json!({"message": error.to_string()})),
                    )
                        .into_response();
                }
            };
            ws.on_upgrade(move |client| run_call(state, actor, session_id, prep, client, socket))
                .into_response()
        }
        Err((status, message)) => (status, Json(json!({"message": message}))).into_response(),
    }
}

async fn prepare_call(
    state: &AppState,
    actor: &Actor,
    session_id: &str,
) -> Result<PreparedCall, (StatusCode, String)> {
    let row: Option<(String, Option<String>, String, String)> = sqlx::query_as(
        "SELECT t.bot_id, t.room_id, b.name, b.instructions
         FROM threads t JOIN bots b ON b.id=t.bot_id
         WHERE t.id=$1 AND t.space_id=$2 AND t.user_id=$3 AND t.status='active'",
    )
    .bind(session_id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .fetch_optional(state.pool())
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let Some((bot_id, room_id, bot_name, bot_instructions)) = row else {
        return Err((StatusCode::NOT_FOUND, "session not found".into()));
    };
    if room_id.is_some() {
        return Err((
            StatusCode::BAD_REQUEST,
            "group calls are not supported".into(),
        ));
    }
    let space = state
        .db
        .get_space(actor)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "workspace not found".into()))?;
    let (provider, resolved) =
        resolve_space_voice(&space).map_err(|message| (StatusCode::CONFLICT, message))?;
    let history = recent_text_history(state, session_id).await.unwrap_or_default();
    let mut connect = VoiceConnectRequest::from_resolved(
        &resolved,
        voice_instructions(&bot_name, &bot_instructions),
    );
    connect.history = history;
    Ok(PreparedCall {
        call_id: Uuid::new_v4().to_string(),
        bot_id,
        bot_name,
        provider,
        connect,
    })
}

async fn recent_text_history(
    state: &AppState,
    session_id: &str,
) -> Result<Vec<(String, String)>, sqlx::Error> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT role, body FROM messages
         WHERE thread_id=$1 AND role IN ('user','assistant') AND body <> ''
         ORDER BY seq DESC LIMIT 10",
    )
    .bind(session_id)
    .fetch_all(state.pool())
    .await?;
    Ok(rows.into_iter().rev().collect())
}

async fn run_call(
    state: AppState,
    actor: Actor,
    session_id: String,
    prep: PreparedCall,
    client: WebSocket,
    mut provider: Box<dyn VoiceSocket>,
) {
    let call_id = prep.call_id.clone();
    let bot_id = prep.bot_id.clone();
    let (client_write, mut client_read) = client.split();
    let client_write = Arc::new(Mutex::new(client_write));
    let _ = send_json(
        &client_write,
        json!({
            "type": "ready",
            "callId": call_id,
            "botId": bot_id,
            "botName": prep.bot_name,
            "provider": prep.provider.as_str(),
            "voice": prep.connect.voice_id,
        }),
    )
    .await;

    let mut user_partial = String::new();
    let mut assistant_partial = String::new();
    let mut last_progress = String::new();
    let mut last_spoken_at = std::time::Instant::now()
        .checked_sub(Duration::from_secs(30))
        .unwrap_or_else(std::time::Instant::now);

    loop {
        tokio::select! {
            client_msg = client_read.next() => {
                match client_msg {
                    None | Some(Ok(AxumMessage::Close(_))) => break,
                    Some(Err(_)) => break,
                    Some(Ok(AxumMessage::Ping(payload))) => {
                        let _ = client_write.lock().await.send(AxumMessage::Pong(payload)).await;
                    }
                    Some(Ok(AxumMessage::Pong(_))) => {}
                    Some(Ok(AxumMessage::Binary(bytes))) => {
                        if provider.send(VoiceEvent::AudioPcm(bytes.to_vec())).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(AxumMessage::Text(_))) => {}
                }
            }
            provider_msg = provider.recv() => {
                match provider_msg {
                    Ok(None) | Err(_) => break,
                    Ok(Some(event)) => {
                        if handle_provider_event(
                            &state,
                            &actor,
                            &session_id,
                            &prep,
                            &mut provider,
                            &client_write,
                            event,
                            &mut user_partial,
                            &mut assistant_partial,
                        )
                        .await
                        .is_err()
                        {
                            break;
                        }
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(800)) => {
                let _ = watch_computer(
                    &state,
                    &actor,
                    &session_id,
                    &bot_id,
                    prep.provider,
                    &mut provider,
                    &client_write,
                    &mut last_progress,
                    &mut last_spoken_at,
                )
                .await;
            }
        }
    }

    state.calls.end(&bot_id, &call_id).await;
}

async fn handle_provider_event(
    state: &AppState,
    actor: &Actor,
    session_id: &str,
    prep: &PreparedCall,
    provider: &mut Box<dyn VoiceSocket>,
    client_write: &Arc<Mutex<futures_util::stream::SplitSink<WebSocket, AxumMessage>>>,
    event: VoiceEvent,
    user_partial: &mut String,
    assistant_partial: &mut String,
) -> Result<(), ()> {
    match event {
        VoiceEvent::AudioPcm(bytes) => {
            client_write
                .lock()
                .await
                .send(AxumMessage::Binary(bytes.into()))
                .await
                .map_err(|_| ())?;
        }
        VoiceEvent::SpeechStarted => {
            send_json(client_write, json!({"type":"speech","state":"started"})).await?;
        }
        VoiceEvent::SpeechStopped => {
            send_json(client_write, json!({"type":"speech","state":"stopped"})).await?;
        }
        VoiceEvent::InputTranscript { text, final_ } => {
            if final_ {
                let body = if text.trim().is_empty() {
                    user_partial.trim().to_string()
                } else {
                    text.trim().to_string()
                };
                user_partial.clear();
                if !body.is_empty() {
                    let _ = persist_transcript(state, session_id, "user", &body, &prep.call_id).await;
                    send_json(
                        client_write,
                        json!({"type":"transcript","role":"user","text":body,"final":true}),
                    )
                    .await?;
                }
            } else {
                user_partial.push_str(&text);
                send_json(
                    client_write,
                    json!({"type":"transcript","role":"user","text":user_partial,"final":false}),
                )
                .await?;
            }
        }
        VoiceEvent::OutputTranscript { text, final_ } => {
            if final_ {
                let body = if text.trim().is_empty() {
                    assistant_partial.trim().to_string()
                } else {
                    text.trim().to_string()
                };
                assistant_partial.clear();
                if !body.is_empty() {
                    let _ =
                        persist_transcript(state, session_id, "assistant", &body, &prep.call_id)
                            .await;
                    send_json(
                        client_write,
                        json!({"type":"transcript","role":"assistant","text":body,"final":true}),
                    )
                    .await?;
                }
            } else {
                assistant_partial.push_str(&text);
                send_json(
                    client_write,
                    json!({"type":"transcript","role":"assistant","text":assistant_partial,"final":false}),
                )
                .await?;
            }
        }
        VoiceEvent::FunctionCall {
            call_id,
            name,
            arguments,
        } => {
            let output = dispatch_voice_tool(state, actor, &prep.bot_id, session_id, &prep.call_id, &name, &arguments)
                .await;
            let _ = provider
                .send(VoiceEvent::FunctionCallOutput {
                    call_id,
                    output: output.to_string(),
                })
                .await;
            tokio::time::sleep(Duration::from_millis(250)).await;
            let _ = provider.send(VoiceEvent::ResponseCreate).await;
            if let Some(status) = output.get("status").and_then(Value::as_str) {
                send_json(
                    client_write,
                    json!({
                        "type":"computer",
                        "status": status,
                        "step": output.get("step"),
                        "takeover": output.get("takeover").and_then(Value::as_bool).unwrap_or(false)
                    }),
                )
                .await?;
            }
        }
        VoiceEvent::Error { message } => {
            send_json(client_write, json!({"type":"error","message":message})).await?;
        }
        _ => {}
    }
    Ok(())
}

async fn dispatch_voice_tool(
    state: &AppState,
    actor: &Actor,
    bot_id: &str,
    session_id: &str,
    call_id: &str,
    name: &str,
    arguments: &str,
) -> Value {
    match name {
        "computer_status" => computer_tool_status(state, actor, bot_id, session_id).await,
        "stop_computer_task" => match crate::sessions::cancel_session_runs(state, session_id).await
        {
            Ok(ids) => json!({"status":"cancelled","runIds": ids}),
            Err(error) => json!({"status":"error","reason": error}),
        },
        "start_computer_task" | "follow_up_computer_task" => {
            let prompt = serde_json::from_str::<Value>(arguments)
                .ok()
                .and_then(|value| {
                    value
                        .get("prompt")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_default();
            let prompt = prompt.trim().to_string();
            if prompt.is_empty() {
                return json!({"status":"error","reason":"missing prompt"});
            }
            start_or_follow(state, actor, bot_id, session_id, call_id, &prompt).await
        }
        other => json!({"status":"error","reason": format!("unknown tool {other}")}),
    }
}

async fn start_or_follow(
    state: &AppState,
    actor: &Actor,
    bot_id: &str,
    session_id: &str,
    call_id: &str,
    prompt: &str,
) -> Value {
    if crate::skills::recording_skill(state.pool(), bot_id)
        .await
        .is_some()
    {
        return json!({"status":"blocked","reason":"teaching"});
    }
    let status = computer_tool_status(state, actor, bot_id, session_id).await;
    if status
        .get("takeover")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || status.get("status").and_then(Value::as_str) == Some("takeover")
    {
        return json!({"status":"blocked","reason":"takeover","takeover":true});
    }
    if status.get("status").and_then(Value::as_str) == Some("user_control") {
        return json!({"status":"blocked","reason":"user_control"});
    }
    let blocks = vec![json!({"kind":"voice","callId": call_id})];
    match crate::runs::send(
        state,
        actor,
        bot_id,
        session_id,
        prompt,
        Some(&Uuid::new_v4().to_string()),
        &blocks,
        &[] as &[SessionAttachment],
    )
    .await
    {
        Ok(result) => json!({
            "status": if result.get("queuedBehindActive").and_then(Value::as_bool).unwrap_or(false) {
                "followed_up"
            } else {
                "queued"
            },
            "runId": result.get("runId"),
        }),
        Err(error) => json!({"status":"error","reason": error}),
    }
}

async fn computer_tool_status(
    state: &AppState,
    actor: &Actor,
    bot_id: &str,
    session_id: &str,
) -> Value {
    let holder: Option<String> = sqlx::query_scalar(
        "SELECT c.control_holder FROM computers c
         JOIN bots b ON b.computer_id=c.id
         WHERE b.id=$1 AND b.space_id=$2 AND b.user_id=$3",
    )
    .bind(bot_id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .fetch_optional(state.pool())
    .await
    .ok()
    .flatten();
    if holder.as_deref() == Some("user") {
        return json!({"status":"user_control","takeover":false});
    }
    let row: Option<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT id, status, checkpoint->>'step'
         FROM runs
         WHERE bot_id=$1 AND thread_id=$2
           AND status IN ('queued','leased','running','waiting_input','waiting_takeover')
         ORDER BY CASE status
                    WHEN 'running' THEN 0 WHEN 'leased' THEN 1
                    WHEN 'waiting_takeover' THEN 2 WHEN 'waiting_input' THEN 3
                    ELSE 4 END,
                  created_at ASC
         LIMIT 1",
    )
    .bind(bot_id)
    .bind(session_id)
    .fetch_optional(state.pool())
    .await
    .ok()
    .flatten();
    match row {
        Some((run_id, status, step)) if status == "waiting_takeover" => json!({
            "status":"takeover",
            "runId": run_id,
            "step": step,
            "takeover": true
        }),
        Some((run_id, status, step)) => json!({
            "status": status,
            "runId": run_id,
            "step": step,
            "takeover": false
        }),
        None => json!({"status":"idle","takeover":false}),
    }
}

async fn watch_computer(
    state: &AppState,
    actor: &Actor,
    session_id: &str,
    bot_id: &str,
    voice_provider: lazyboy_contracts::VoiceProvider,
    provider: &mut Box<dyn VoiceSocket>,
    client_write: &Arc<Mutex<futures_util::stream::SplitSink<WebSocket, AxumMessage>>>,
    last_progress: &mut String,
    last_spoken_at: &mut std::time::Instant,
) -> Result<(), ()> {
    let snapshot = computer_tool_status(state, actor, bot_id, session_id).await;
    let key = snapshot.to_string();
    if key == *last_progress {
        return Ok(());
    }
    let previous = last_progress.clone();
    *last_progress = key;
    send_json(
        client_write,
        json!({
            "type":"computer",
            "status": snapshot.get("status"),
            "step": snapshot.get("step"),
            "takeover": snapshot.get("takeover")
        }),
    )
    .await?;
    if let Some(line) = speakable_progress(&previous, &snapshot) {
        let urgent = snapshot.get("status").and_then(Value::as_str)
            == Some("takeover")
            || snapshot.get("status").and_then(Value::as_str) == Some("failed");
        if urgent || last_spoken_at.elapsed() > Duration::from_secs(6) {
            let _ = provider.send(VoiceEvent::SpeakNow { text: line }).await;
            if voice_provider == lazyboy_contracts::VoiceProvider::Openai {
                let _ = provider.send(VoiceEvent::ResponseCreate).await;
            }
            *last_spoken_at = std::time::Instant::now();
        }
    }
    Ok(())
}

pub fn speakable_progress(previous: &str, snapshot: &Value) -> Option<String> {
    let status = snapshot.get("status")?.as_str()?;
    if previous.contains(&format!("\"status\":\"{status}\"")) {
        return None;
    }
    match status {
        "takeover" => Some("需要你接手畫面登入或驗證。".into()),
        "user_control" => Some("畫面現在在你手上，完成後再叫我繼續。".into()),
        "failed" => Some("這次電腦操作沒做成。".into()),
        "idle" if previous.contains("running") || previous.contains("queued") => {
            Some("電腦上的工作做完了。".into())
        }
        _ => None,
    }
}

async fn persist_transcript(
    state: &AppState,
    session_id: &str,
    role: &str,
    body: &str,
    call_id: &str,
) -> Result<(), String> {
    let mut tx = state
        .pool()
        .begin()
        .await
        .map_err(|error| error.to_string())?;
    let seq: i32 = sqlx::query_scalar(
        "UPDATE threads SET next_message_seq=next_message_seq+1, updated_at=now()
         WHERE id=$1 RETURNING next_message_seq-1",
    )
    .bind(session_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|error| error.to_string())?;
    let message_id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO messages (id,thread_id,seq,role,body,blocks)
         VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(&message_id)
    .bind(session_id)
    .bind(seq)
    .bind(role)
    .bind(body)
    .bind(json!([{"kind":"voice","callId": call_id}]))
    .execute(&mut *tx)
    .await
    .map_err(|error| error.to_string())?;
    tx.commit().await.map_err(|error| error.to_string())?;
    let _ = crate::sessions::append_event(
        state,
        session_id,
        "message.created",
        json!({"id":message_id,"seq":seq,"role":role,"body":body,"voice":true}),
    )
    .await;
    Ok(())
}

async fn send_json(
    client_write: &Arc<Mutex<futures_util::stream::SplitSink<WebSocket, AxumMessage>>>,
    value: Value,
) -> Result<(), ()> {
    client_write
        .lock()
        .await
        .send(AxumMessage::Text(value.to_string().into()))
        .await
        .map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::speakable_progress;
    use serde_json::json;

    #[test]
    fn takeover_and_idle_after_work_are_spoken_once() {
        let takeover = json!({"status":"takeover","takeover":true});
        assert_eq!(
            speakable_progress("{\"status\":\"running\"}", &takeover).as_deref(),
            Some("需要你接手畫面登入或驗證。")
        );
        assert!(speakable_progress(&takeover.to_string(), &takeover).is_none());
        let idle = json!({"status":"idle"});
        assert!(speakable_progress("{\"status\":\"running\"}", &idle).unwrap().contains("做完"));
    }
}
