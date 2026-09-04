use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use lazyboy_contracts::ModelProvider;
use lazyboy_harness::{
    CredentialChain, DynModel, ResolveModelRequest, connect_model, resolve_backend,
};
use rig_core::completion::message::{
    AssistantContent, ImageDetail, ImageMediaType, Message, ToolResultContent, UserContent,
};
use rig_core::completion::{CompletionModel, ToolDefinition};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::computer::{self, adapter_context_for};
use crate::db::{Actor, parse_mode};
use crate::state::AppState;
use crate::tools::{ToolCtx, dispatch, tool_definitions};

const SCREENSHOT_CAPTION: &str = "Desktop screenshot (1280x800) with yellow numbered marks. Click by those element ids. The live VNC view has no marks.";

const SYSTEM: &str = "You operate this bot's Linux desktop. The human always sees the live screen. You do not need a screenshot for every step.

Prefer the fast path, in this order:
1) shell, list_files, read_file, write_file
2) MCP tools when they match the task
3) browser for anything in Chromium: snapshot (page text + numbered elements), click/type/press by element id, navigate by URL. Do not pixel-click the Chromium window.
4) launch_app / open_path to open a site or file
5) computer_act only for native GUI that has no DOM (dialogs, canvas, XFCE)

When you use the browser tool:
- snapshot first; click {\"action\":\"click\",\"element\":N}; type {\"action\":\"type\",\"element\":N,\"text\":\"...\"}; open a URL with navigate.
- Yellow numbered marks on the screenshot match the element list. Click the number, not guessed pixels.
- If the control is not in the element list, login/2FA/CAPTCHA, or clicks do nothing, call request_takeover and stop. Do not guess-click.

computer_act examples (native windows only):
- {\"kind\":\"click\",\"element\":1}
- {\"kind\":\"click\",\"x\":N,\"y\":N}
- {\"kind\":\"type\",\"text\":\"...\"}
- {\"kind\":\"key\",\"key\":\"Return\"}
- {\"kind\":\"focus\",\"title\":\"Open File\"}

On a Team Computer, relative files live in your bot folder; use shared/ for shared work. Finish the user's task.";

pub async fn send(
    state: &AppState,
    actor: &Actor,
    bot_id: &str,
    thread_id: &str,
    text: &str,
    client_nonce: Option<&str>,
    blocks: &[Value],
) -> Result<Value, String> {
    let mut tx = state
        .pool()
        .begin()
        .await
        .map_err(|error| error.to_string())?;
    let scoped: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT t.title, t.room_id FROM threads t JOIN bots b ON b.id=t.bot_id
         WHERE t.id=$1 AND t.bot_id=$2 AND t.space_id=$3 AND t.user_id=$4
           AND b.space_id=$3 AND b.user_id=$4 AND t.status='active'
         FOR UPDATE OF t",
    )
    .bind(thread_id)
    .bind(bot_id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| error.to_string())?;
    let Some((current_title, room_id)) = scoped else {
        return Err("session not found".into());
    };
    if let Some(nonce) = client_nonce {
        let existing: Option<(String, Option<String>)> = sqlx::query_as(
            "SELECT id, run_id FROM messages WHERE thread_id=$1 AND client_nonce=$2",
        )
        .bind(thread_id)
        .bind(nonce)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|error| error.to_string())?;
        if let Some((message_id, run_id)) = existing {
            tx.rollback().await.map_err(|error| error.to_string())?;
            return Ok(json!({
                "messageId": message_id,
                "runId": run_id,
                "duplicate": true,
                "queued": true
            }));
        }
    }
    let run_id = Uuid::new_v4().to_string();
    let message_id = Uuid::new_v4().to_string();
    let seq: i32 = sqlx::query_scalar(
        "UPDATE threads
         SET next_message_seq=next_message_seq+1, updated_at=now()
         WHERE id=$1 RETURNING next_message_seq-1",
    )
    .bind(thread_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|error| error.to_string())?;
    sqlx::query(
        "INSERT INTO messages (id,thread_id,seq,role,body,blocks,run_id,client_nonce)
         VALUES ($1,$2,$3,'user',$4,$5,$6,$7)",
    )
    .bind(&message_id)
    .bind(thread_id)
    .bind(seq)
    .bind(text)
    .bind(json!(blocks))
    .bind(&run_id)
    .bind(client_nonce)
    .execute(&mut *tx)
    .await
    .map_err(|error| error.to_string())?;
    if crate::sessions::is_default_session_title(&current_title) {
        sqlx::query("UPDATE threads SET title=$2 WHERE id=$1")
            .bind(thread_id)
            .bind(crate::sessions::title_from_first_message(text))
            .execute(&mut *tx)
            .await
            .map_err(|error| error.to_string())?;
    }
    let mut member_ids: Vec<String> = if let Some(room_id) = room_id.as_deref() {
        sqlx::query_scalar("SELECT bot_id FROM room_members WHERE room_id=$1 ORDER BY created_at")
            .bind(room_id)
            .fetch_all(&mut *tx)
            .await
            .map_err(|error| error.to_string())?
    } else {
        vec![bot_id.to_string()]
    };
    if member_ids.is_empty() {
        member_ids.push(bot_id.to_string());
    }
    for (index, member_id) in member_ids.iter().enumerate() {
        let member_run = if index == 0 {
            run_id.clone()
        } else {
            Uuid::new_v4().to_string()
        };
        sqlx::query(
            "INSERT INTO runs (id,space_id,bot_id,thread_id,user_id,status,prompt,checkpoint)
             VALUES ($1,$2,$3,$4,$5,'queued',$6,$7)",
        )
        .bind(&member_run)
        .bind(&actor.space_id)
        .bind(member_id)
        .bind(&thread_id)
        .bind(&actor.user_id)
        .bind(text)
        .bind(json!({"messageSeq":seq}))
        .execute(&mut *tx)
        .await
        .map_err(|error| error.to_string())?;
    }
    let event_seq: i32 = sqlx::query_scalar(
        "UPDATE threads SET next_event_seq=next_event_seq+1 WHERE id=$1 RETURNING next_event_seq",
    )
    .bind(thread_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|error| error.to_string())?;
    sqlx::query(
        "INSERT INTO events (id,thread_id,seq,type,payload) VALUES ($1,$2,$3,'message.created',$4)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(thread_id)
    .bind(event_seq)
    .bind(json!({"id":message_id,"seq":seq,"role":"user","body":text,"runId":run_id}))
    .execute(&mut *tx)
    .await
    .map_err(|error| error.to_string())?;
    let queued_behind_active: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM runs WHERE bot_id=$1 AND id<>$2
         AND status IN ('queued','leased','running','waiting_input','waiting_takeover'))",
    )
    .bind(bot_id)
    .bind(&run_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|error| error.to_string())?;
    if let Some(room_id) = room_id.as_deref() {
        sqlx::query("UPDATE rooms SET updated_at=now() WHERE id=$1")
            .bind(room_id)
            .execute(&mut *tx)
            .await
            .map_err(|error| error.to_string())?;
    }
    tx.commit().await.map_err(|error| error.to_string())?;
    Ok(json!({
        "messageId": message_id,
        "runId": run_id,
        "duplicate": false,
        "queued": true,
        "queuedBehindActive": queued_behind_active
    }))
}

pub async fn worker_loop(state: AppState) {
    let inflight = Arc::new(tokio::sync::Semaphore::new(16));
    let lease_owner = format!("api-{}", Uuid::new_v4());
    loop {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let Ok(permit) = inflight.clone().try_acquire_owned() else {
            continue;
        };
        let queued: Result<Option<(String, String, String, String, String, String)>, _> =
            sqlx::query_as(
                "WITH candidate AS (
               SELECT r.id
               FROM runs r
               WHERE r.retry_count < r.max_retries
                 AND (
                   r.status='queued'
                   OR (
                     r.status IN ('leased','running')
                     AND (r.lease_expires_at IS NULL OR r.lease_expires_at < now())
                   )
                 )
                 AND NOT EXISTS (
                   SELECT 1 FROM runs a
                   WHERE a.bot_id=r.bot_id AND a.id<>r.id
                     AND a.status IN ('leased','running','waiting_input','waiting_takeover')
                     AND (a.lease_expires_at IS NULL OR a.lease_expires_at >= now())
                 )
               ORDER BY CASE WHEN r.status='queued' THEN 1 ELSE 0 END, r.created_at
               FOR UPDATE SKIP LOCKED
               LIMIT 1
             )
             UPDATE runs r
             SET status='leased', lease_owner=$1,
                 lease_expires_at=now()+interval '5 minutes',
                 lease_fence=lease_fence+1, retry_count=retry_count+1, updated_at=now()
             FROM candidate c WHERE r.id=c.id
             RETURNING r.id,r.bot_id,r.thread_id,r.prompt,r.user_id,r.space_id",
            )
            .bind(&lease_owner)
            .fetch_optional(state.pool())
            .await;
        let Ok(Some((run_id, bot_id, thread_id, prompt, user_id, space_id))) = queued else {
            drop(permit);
            continue;
        };
        let state = state.clone();
        let owner = lease_owner.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let actor = Actor { user_id, space_id };
            if let Err(error) = execute_run(
                &state, &actor, &owner, &run_id, &bot_id, &thread_id, &prompt,
            )
            .await
            {
                tracing::error!("run {run_id} failed: {error}");
                let next_status: Option<String> = sqlx::query_scalar(
                    "UPDATE runs
                     SET status=CASE WHEN retry_count < max_retries THEN 'queued' ELSE 'failed' END,
                         error=$2, completed_at=CASE WHEN retry_count < max_retries THEN NULL ELSE now() END,
                         lease_owner=NULL, lease_expires_at=NULL, updated_at=now()
                     WHERE id=$1 AND lease_owner=$3 AND status IN ('leased','running') RETURNING status",
                )
                    .bind(&run_id)
                    .bind(&error)
                    .bind(&owner)
                    .fetch_optional(state.pool())
                    .await
                    .ok()
                    .flatten();
                if next_status.as_deref() == Some("failed") {
                    let _ = append_bot_message(
                        &state,
                        &thread_id,
                        &run_id,
                        &bot_id,
                        &format!("Run failed after retries: {error}"),
                    )
                    .await;
                    let _ = crate::sessions::append_event(
                        &state,
                        &thread_id,
                        "run.failed",
                        json!({"runId":run_id,"error":error}),
                    )
                    .await;
                }
                let _ = computer::release_screen_execution(&state, &run_id).await;
                let _ = sqlx::query(
                    "UPDATE computers SET execution_bot_id = NULL, execution_run_id = NULL, execution_lease_expires_at = NULL, updated_at = now()
                     WHERE execution_run_id = $1",
                )
                .bind(&run_id)
                .execute(state.pool())
                .await;
            }
        });
    }
}

async fn execute_run(
    state: &AppState,
    actor: &Actor,
    lease_owner: &str,
    run_id: &str,
    bot_id: &str,
    thread_id: &str,
    prompt: &str,
) -> Result<(), String> {
    let started = sqlx::query(
        "UPDATE runs SET status='running', started_at=COALESCE(started_at,now()), updated_at=now()
         WHERE id=$1 AND lease_owner=$2 AND lease_expires_at>now() AND status='leased'",
    )
    .bind(run_id)
    .bind(lease_owner)
    .execute(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    if started.rows_affected() != 1 {
        let status: Option<String> = sqlx::query_scalar("SELECT status FROM runs WHERE id=$1")
            .bind(run_id)
            .fetch_optional(state.pool())
            .await
            .ok()
            .flatten();
        if halt_from_status(status.as_deref()).is_some() {
            return Ok(());
        }
        return Err("run lease was lost before execution".into());
    }
    let _ = crate::sessions::append_event(state, thread_id, "run.started", json!({"runId":run_id}))
        .await;

    computer::boot(state, actor, bot_id).await?;
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
    let bound = computer::ensure_bot_screen(state, actor, bot_id, &computer, Some(run_id)).await?;
    let mut gui_block = bound.gui_block;
    let screen = if let Some(row) = bound.row {
        let row = computer::take_screen_execution(state, &row, run_id).await?;
        if gui_block.is_none() {
            gui_block =
                computer::take_profile_lock(state, &computer, bot_id, &bot.name, run_id, &row)
                    .await?;
        }
        Some(row)
    } else {
        None
    };

    let space = state
        .db
        .get_space(actor)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "workspace not found".to_string())?;
    let provider = bot
        .model_provider
        .as_deref()
        .or(Some(space.default_model_provider.as_str()))
        .unwrap_or("xai")
        .parse::<ModelProvider>()
        .map_err(|error| error.to_string())?;
    let model_id = bot
        .model_id
        .clone()
        .filter(|value| !value.is_empty())
        .or_else(|| Some(space.default_model_id.clone()).filter(|value| !value.is_empty()));
    let backend = resolve_backend(ResolveModelRequest {
        provider,
        model_id,
        base_url: space.default_model_base_url.clone(),
        credentials: CredentialChain {
            bot: None,
            space: space.default_model_api_key.clone(),
            env: lazyboy_harness::credential_from_env(provider),
        },
    })
    .map_err(|error| error.to_string())?;
    let model = connect_model(&backend).map_err(|error| error.to_string())?;

    let ctx = Arc::new(ToolCtx {
        sandbox: state.sandbox.clone(),
        computer: computer_ref,
        context: adapter_context_for(actor, bot_id, "run", screen.as_ref(), Some(run_id)),
        mode: parse_mode(&computer.scope),
        bot_id: bot_id.to_string(),
        vision: backend.capabilities.vision,
        gui_block,
        previous_frame: std::sync::Mutex::new(None),
        elements: std::sync::Mutex::new(Vec::new()),
        miss_streak: std::sync::Mutex::new(0),
        click_misses: std::sync::Mutex::new(0),
        takeover_requested: std::sync::Mutex::new(false),
        pool: state.pool().clone(),
        memory: state.memory.clone(),
        actor: actor.clone(),
        session_id: thread_id.to_string(),
        run_id: run_id.to_string(),
        memory_enabled: bot.memory_enabled && state.memory.globally_enabled(),
        mcp: state.mcp.clone(),
    });

    let mut defs = tool_definitions(ctx.memory_enabled);
    let mcp_defs = state.mcp.definitions().await;
    if !mcp_defs.is_empty() {
        defs.extend(mcp_defs);
    }
    let (summary, summary_seq): (String, i32) = sqlx::query_as(
        "SELECT history_summary, history_summary_seq FROM threads
         WHERE id=$1 AND space_id=$2 AND user_id=$3",
    )
    .bind(thread_id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .fetch_one(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    let checkpoint: Value = sqlx::query_scalar("SELECT checkpoint FROM runs WHERE id=$1")
        .bind(run_id)
        .fetch_one(state.pool())
        .await
        .map_err(|error| error.to_string())?;
    let current_seq: i32 = checkpoint
        .get("messageSeq")
        .and_then(Value::as_i64)
        .map(|seq| seq as i32)
        .unwrap_or(i32::MAX);
    let resume_after_takeover = checkpoint
        .get("resumeAfterTakeover")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if resume_after_takeover {
        let _ = sqlx::query(
            "UPDATE runs SET checkpoint = checkpoint - 'resumeAfterTakeover' WHERE id=$1",
        )
        .bind(run_id)
        .execute(state.pool())
        .await;
    }
    let history_end = if resume_after_takeover {
        i32::MAX
    } else {
        current_seq
    };
    let recent: Vec<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT role, body, speaker_bot_id, speaker_name FROM (
           SELECT m.role, m.body, m.seq, m.speaker_bot_id, b.name AS speaker_name
           FROM messages m
           LEFT JOIN bots b ON b.id=m.speaker_bot_id
           WHERE m.thread_id=$1 AND m.seq>$2 AND m.seq<$3
           ORDER BY m.seq DESC LIMIT 30
         ) history ORDER BY seq ASC",
    )
    .bind(thread_id)
    .bind(history_window_start(summary_seq, current_seq))
    .bind(history_end)
    .fetch_all(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    let mut history: Vec<Message> = Vec::new();
    if !summary.trim().is_empty() {
        history.push(Message::User {
            content: vec![UserContent::text(format!(
                "Conversation summary through message {summary_seq}:\n{summary}"
            ))],
        });
    }
    for (role, body, speaker_id, speaker_name) in recent {
        if role == "user" {
            history.push(Message::User {
                content: vec![UserContent::text(body)],
            });
        } else if speaker_id.as_deref() == Some(bot_id) || speaker_id.is_none() {
            history.push(Message::Assistant {
                id: None,
                content: vec![AssistantContent::text(body)],
            });
        } else {
            let name = speaker_name.unwrap_or_else(|| "agent".into());
            history.push(Message::User {
                content: vec![UserContent::text(format!("[{name}]: {body}"))],
            });
        }
    }
    let mut first = if resume_after_takeover {
        vec![UserContent::text(
            "The user finished collaborating and released control. Continue the original task from the CURRENT screen. Do not restart from scratch.",
        )]
    } else {
        vec![UserContent::text(prompt)]
    };
    let mut screenshots: u32 = 0;
    let mut screenshot_bytes: u64 = 0;
    if resume_after_takeover && ctx.gui_block.is_none() {
        let outcome = dispatch(&ctx, "computer_observe", &json!({})).await;
        first.push(UserContent::text(outcome.text));
        if let Some(image) = outcome.image {
            screenshot_bytes += image.len() as u64;
            screenshots += 1;
            first.extend(screenshot_parts(image));
        }
    }
    let mut pending = Message::User { content: first };
    let mut final_text = String::new();
    let mut turns: u32 = 0;
    let mut used_gui = false;
    let memory = if ctx.memory_enabled {
        match state
            .memory
            .recall(state.pool(), actor, bot_id, prompt, None)
            .await
        {
            Ok(items) => state.memory.durable_block(&items),
            Err(error) => {
                tracing::warn!("memory retrieval failed for run {run_id}: {error}");
                String::new()
            }
        }
    } else {
        String::new()
    };
    let mut preamble = if bot.instructions.trim().is_empty() {
        SYSTEM.to_string()
    } else {
        format!(
            "{SYSTEM}\n\nBot-specific instructions:\n{}",
            bot.instructions.trim()
        )
    };
    let room_mates: Vec<String> = sqlx::query_scalar(
        "SELECT b.name FROM threads t
         JOIN room_members m ON m.room_id=t.room_id
         JOIN bots b ON b.id=m.bot_id
         WHERE t.id=$1 AND b.id<>$2
         ORDER BY b.name",
    )
    .bind(thread_id)
    .bind(bot_id)
    .fetch_all(state.pool())
    .await
    .unwrap_or_default();
    if !room_mates.is_empty() {
        preamble.push_str(&format!(
            "\n\nYou are {} in a group chat with: {}. Reply as yourself only. Other agents' lines are prefixed with [Name]. Do not speak for them.",
            bot.name,
            room_mates.join("、")
        ));
    }
    let mcp_names: Vec<String> = defs
        .iter()
        .filter(|tool| tool.name.starts_with("mcp_"))
        .map(|tool| tool.name.clone())
        .collect();
    if !mcp_names.is_empty() {
        preamble.push_str(&format!(
            "\n\nMCP tools available: {}. Use them when they help complete the user's request.",
            mcp_names.join(", ")
        ));
    }
    if !memory.is_empty() {
        preamble.push_str("\n\n");
        preamble.push_str(&memory);
    }

    for _ in 0..24 {
        turns += 1;
        if let Some(halt) = renew_or_halt(state, run_id, lease_owner).await? {
            return finish_halt(
                state,
                thread_id,
                run_id,
                halt,
                turns,
                screenshots,
                screenshot_bytes,
                &ctx,
                used_gui,
            )
            .await;
        }
        drop_history_screenshots(&mut history);
        let content = tokio::select! {
            halt = wait_for_halt(state, run_id) => {
                return finish_halt(
                    state,
                    thread_id,
                    run_id,
                    halt,
                    turns,
                    screenshots,
                    screenshot_bytes,
                    &ctx,
                    used_gui,
                )
                .await;
            }
            result = complete_once(&model, pending.clone(), &preamble, &history, &defs) => result
        }?;
        let assistant = Message::Assistant {
            id: None,
            content: content.clone(),
        };
        history.push(pending.clone());
        history.push(assistant);

        let mut calls = Vec::new();
        for item in &content {
            match item {
                AssistantContent::Text(text) => final_text.push_str(&text.text),
                AssistantContent::ToolCall(call) => calls.push(call.clone()),
                _ => {}
            }
        }
        if calls.is_empty() {
            break;
        }
        final_text.clear();
        let mut results = Vec::new();
        let mut screen: Option<Vec<u8>> = None;
        for call in calls {
            if let Some(halt) = renew_or_halt(state, run_id, lease_owner).await? {
                return finish_halt(
                    state,
                    thread_id,
                    run_id,
                    halt,
                    turns,
                    screenshots,
                    screenshot_bytes,
                    &ctx,
                    used_gui,
                )
                .await;
            }
            let name = call.function.name.clone();
            used_gui |= matches!(
                name.as_str(),
                "computer_observe" | "computer_act" | "open_path" | "launch_app" | "browser"
            );
            let outcome = tokio::select! {
                halt = wait_for_halt(state, run_id) => {
                    return finish_halt(
                        state,
                        thread_id,
                        run_id,
                        halt,
                        turns,
                        screenshots,
                        screenshot_bytes,
                        &ctx,
                        used_gui,
                    )
                    .await;
                }
                outcome = tokio::time::timeout(
                    Duration::from_secs(90),
                    dispatch(&ctx, &name, &call.function.arguments),
                ) => match outcome {
                    Ok(outcome) => outcome,
                    Err(_) => crate::tools::ToolOutcome {
                        text: format!("工具 {name} 執行逾時（90 秒），請稍後重試。"),
                        image: None,
                        pause: false,
                    },
                }
            };
            // xAI rejects images inside tool results. Attach a changed
            // screenshot as a following user image instead.
            if let Some(image) = outcome.image {
                screen = Some(image);
            }
            results.push(UserContent::tool_result_for(
                call.id.clone(),
                call.provider.clone(),
                name,
                vec![ToolResultContent::text(&outcome.text)],
            ));
            if outcome.pause {
                sqlx::query(
                    "UPDATE runs SET status = 'waiting_takeover',
                            checkpoint = COALESCE(checkpoint, '{}'::jsonb)
                                || jsonb_build_object('resumeAfterTakeover', true),
                            updated_at = now()
                     WHERE id = $1",
                )
                .bind(run_id)
                .execute(state.pool())
                .await
                .map_err(|error| error.to_string())?;
                append_bot_message(state, thread_id, run_id, bot_id, &outcome.text).await?;
                let click_misses = *ctx.click_misses.lock().unwrap();
                record_run_metrics(
                    state,
                    thread_id,
                    run_id,
                    "run.paused",
                    turns,
                    screenshots,
                    screenshot_bytes,
                    click_misses,
                    used_gui,
                    true,
                )
                .await;
                return Ok(());
            }
        }
        if let Some(png) = screen {
            screenshot_bytes += png.len() as u64;
            screenshots += 1;
            results.extend(screenshot_parts(png));
        }
        pending = Message::User { content: results };
    }

    let status: Option<String> = sqlx::query_scalar("SELECT status FROM runs WHERE id = $1")
        .bind(run_id)
        .fetch_optional(state.pool())
        .await
        .map_err(|error| error.to_string())?;
    if let Some(halt) = halt_from_status(status.as_deref()) {
        return finish_halt(
            state,
            thread_id,
            run_id,
            halt,
            turns,
            screenshots,
            screenshot_bytes,
            &ctx,
            used_gui,
        )
        .await;
    }
    append_bot_message(state, thread_id, run_id, bot_id, &final_text).await?;
    let completed = sqlx::query(
        "UPDATE runs
         SET status='completed', completed_at=now(), updated_at=now(),
             lease_owner=NULL, lease_expires_at=NULL
         WHERE id=$1 AND lease_owner=$2 AND status='running'",
    )
    .bind(run_id)
    .bind(lease_owner)
    .execute(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    if completed.rows_affected() != 1 {
        return Err("run lease was lost before completion".into());
    }
    let click_misses = *ctx.click_misses.lock().unwrap();
    let takeover = *ctx.takeover_requested.lock().unwrap();
    record_run_metrics(
        state,
        thread_id,
        run_id,
        "run.completed",
        turns,
        screenshots,
        screenshot_bytes,
        click_misses,
        used_gui,
        takeover,
    )
    .await;
    computer::release_screen_execution(state, run_id).await?;
    sqlx::query(
        "UPDATE computers SET execution_bot_id = NULL, execution_run_id = NULL, execution_lease_expires_at = NULL, updated_at = now()
         WHERE execution_run_id = $1",
    )
    .bind(run_id)
    .execute(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    Ok(())
}

async fn complete_once(
    model: &DynModel,
    pending: Message,
    preamble: &str,
    history: &[Message],
    defs: &[ToolDefinition],
) -> Result<Vec<AssistantContent>, String> {
    match model {
        DynModel::Xai(model) => complete_with(model, pending, preamble, history, defs).await,
        DynModel::OpenAi(model) => complete_with(model, pending, preamble, history, defs).await,
    }
}

async fn complete_with<M>(
    model: &M,
    pending: Message,
    preamble: &str,
    history: &[Message],
    defs: &[ToolDefinition],
) -> Result<Vec<AssistantContent>, String>
where
    M: CompletionModel + Clone,
{
    let request = model
        .completion_request(pending)
        .preamble(preamble.to_string())
        .messages(history.to_vec())
        .tools(defs.to_vec())
        .build();
    let response = tokio::time::timeout(Duration::from_secs(120), model.completion(request))
        .await
        .map_err(|_| "AI 回應逾時（120 秒）".to_string())?
        .map_err(|error| error.to_string())?;
    Ok(response.choice.into_iter().collect())
}

fn screenshot_parts(image: Vec<u8>) -> Vec<UserContent> {
    let media = if image.starts_with(&[0xFF, 0xD8, 0xFF]) {
        ImageMediaType::JPEG
    } else {
        ImageMediaType::PNG
    };
    let encoded = base64::engine::general_purpose::STANDARD.encode(image);
    vec![
        UserContent::text(SCREENSHOT_CAPTION),
        UserContent::image_base64(encoded, Some(media), Some(ImageDetail::Low)),
    ]
}

fn drop_history_screenshots(history: &mut [Message]) {
    for message in history.iter_mut() {
        let Message::User { content } = message else {
            continue;
        };
        content.retain(|part| match part {
            UserContent::Image(_) => false,
            UserContent::Text(text) if text.text == SCREENSHOT_CAPTION => false,
            _ => true,
        });
    }
}

pub(crate) async fn append_bot_message(
    state: &AppState,
    thread_id: &str,
    run_id: &str,
    bot_id: &str,
    body: &str,
) -> Result<(), String> {
    let mut tx = state
        .pool()
        .begin()
        .await
        .map_err(|error| error.to_string())?;
    let seq: i32 = sqlx::query_scalar(
        "UPDATE threads SET next_message_seq=next_message_seq+1,updated_at=now()
         WHERE id=$1 RETURNING next_message_seq-1",
    )
    .bind(thread_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|error| error.to_string())?;
    let message_id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO messages (id,thread_id,seq,role,body,run_id,speaker_bot_id)
         VALUES ($1,$2,$3,'assistant',$4,$5,$6)",
    )
    .bind(&message_id)
    .bind(thread_id)
    .bind(seq)
    .bind(body)
    .bind(run_id)
    .bind(bot_id)
    .execute(&mut *tx)
    .await
    .map_err(|error| error.to_string())?;
    tx.commit().await.map_err(|error| error.to_string())?;
    let _ = crate::sessions::append_event(
        state,
        thread_id,
        "message.created",
        json!({"id":message_id,"seq":seq,"role":"assistant","body":body,"runId":run_id}),
    )
    .await;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunHalt {
    Cancelled,
    Takeover,
}

fn halt_from_status(status: Option<&str>) -> Option<RunHalt> {
    match status {
        Some("cancelled") => Some(RunHalt::Cancelled),
        Some("waiting_takeover") => Some(RunHalt::Takeover),
        _ => None,
    }
}

async fn run_status_halt(state: &AppState, run_id: &str) -> Result<Option<RunHalt>, String> {
    let status: Option<String> = sqlx::query_scalar("SELECT status FROM runs WHERE id = $1")
        .bind(run_id)
        .fetch_optional(state.pool())
        .await
        .map_err(|error| error.to_string())?;
    Ok(halt_from_status(status.as_deref()))
}

async fn wait_for_halt(state: &AppState, run_id: &str) -> RunHalt {
    loop {
        if let Ok(Some(halt)) = run_status_halt(state, run_id).await {
            return halt;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn renew_or_halt(
    state: &AppState,
    run_id: &str,
    lease_owner: &str,
) -> Result<Option<RunHalt>, String> {
    if let Some(halt) = run_status_halt(state, run_id).await? {
        return Ok(Some(halt));
    }
    match renew_lease(state, run_id, lease_owner).await {
        Ok(()) => run_status_halt(state, run_id).await,
        Err(error) => {
            if let Some(halt) = run_status_halt(state, run_id).await? {
                Ok(Some(halt))
            } else {
                Err(error)
            }
        }
    }
}

async fn finish_halt(
    state: &AppState,
    thread_id: &str,
    run_id: &str,
    halt: RunHalt,
    turns: u32,
    screenshots: u32,
    screenshot_bytes: u64,
    ctx: &crate::tools::ToolCtx,
    used_gui: bool,
) -> Result<(), String> {
    match halt {
        RunHalt::Cancelled => Ok(()),
        RunHalt::Takeover => {
            let _ = sqlx::query(
                "UPDATE runs SET lease_owner=NULL, lease_expires_at=NULL, updated_at=now()
                 WHERE id=$1 AND status='waiting_takeover'",
            )
            .bind(run_id)
            .execute(state.pool())
            .await;
            let click_misses = *ctx.click_misses.lock().unwrap();
            record_run_metrics(
                state,
                thread_id,
                run_id,
                "run.paused",
                turns,
                screenshots,
                screenshot_bytes,
                click_misses,
                used_gui,
                true,
            )
            .await;
            Ok(())
        }
    }
}

async fn renew_lease(state: &AppState, run_id: &str, lease_owner: &str) -> Result<(), String> {
    let renewed = sqlx::query(
        "UPDATE runs SET lease_expires_at=now()+interval '5 minutes',updated_at=now()
         WHERE id=$1 AND lease_owner=$2 AND status IN ('leased','running')",
    )
    .bind(run_id)
    .bind(lease_owner)
    .execute(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    if renewed.rows_affected() == 1 {
        Ok(())
    } else {
        Err("run lease was lost".into())
    }
}

fn history_window_start(summary_seq: i32, current_seq: i32) -> i32 {
    summary_seq.min(current_seq.saturating_sub(1)).max(0)
}

async fn record_run_metrics(
    state: &AppState,
    thread_id: &str,
    run_id: &str,
    event: &str,
    turns: u32,
    screenshots: u32,
    screenshot_bytes: u64,
    click_misses: u32,
    used_gui: bool,
    takeover: bool,
) {
    let payload = json!({
        "runId": run_id,
        "turns": turns,
        "screenshotsToModel": screenshots,
        "screenshotBytes": screenshot_bytes,
        "clickMisses": click_misses,
        "usedGui": used_gui,
        "takeover": takeover,
    });
    tracing::info!(
        run_id,
        turns,
        screenshots,
        screenshot_bytes,
        click_misses,
        used_gui,
        takeover,
        "run metrics"
    );
    let _ = crate::sessions::append_event(state, thread_id, event, payload).await;
}

#[cfg(test)]
mod tests {
    use super::{RunHalt, halt_from_status, history_window_start, screenshot_parts};
    use rig_core::completion::message::UserContent;

    #[test]
    fn history_never_reads_past_the_current_prompt() {
        assert_eq!(history_window_start(10, 5), 4);
        assert_eq!(history_window_start(3, 20), 3);
        assert_eq!(history_window_start(-1, 1), 0);
    }

    #[test]
    fn screenshot_parts_keeps_caption_and_image() {
        let parts = screenshot_parts(vec![0xFF, 0xD8, 0xFF, 0x00]);
        assert_eq!(parts.len(), 2);
        assert!(matches!(parts[0], UserContent::Text(_)));
        assert!(matches!(parts[1], UserContent::Image(_)));
    }

    #[test]
    fn halt_maps_paused_and_cancelled_runs() {
        assert_eq!(
            halt_from_status(Some("cancelled")),
            Some(RunHalt::Cancelled)
        );
        assert_eq!(
            halt_from_status(Some("waiting_takeover")),
            Some(RunHalt::Takeover)
        );
        assert_eq!(halt_from_status(Some("running")), None);
        assert_eq!(halt_from_status(None), None);
    }
}
