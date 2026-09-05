use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use lazyboy_contracts::{ModelProvider, SessionAttachment};
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

const SYSTEM_CHAT: &str = "You are this bot's assistant. This message is conversation — a greeting, small talk, a question you can answer from knowledge, planning, or explaining.

Reply in text only. Do not try to use the desktop, browser, files, or shell, and do not narrate that you are checking a screen. You have a Linux desktop for later if the user asks you to operate it; this turn does not need it.

Be brief and friendly. If they ask who you are, say you can chat and also do work on a computer when they want that.";

const SYSTEM: &str = "You are this bot's assistant. You have a Linux desktop you can use, but most conversation does not need it.

Reply in text — no tools — for greetings, small talk, questions you can answer from knowledge, planning, or explaining. Do not call computer_observe, computer_act, browser, launch_app, open_path, wait, list_files, or shell just to check the screen or because a desktop exists. A hello does not need a screenshot or a file listing.

Use tools only when the user wants something done on the computer: open a site, click through a UI, run a command, read/write workspace files, or follow a taught skill. Route directly by task:
1) Website, video, search, email, or anything in Chromium: use browser first. Navigate directly, then snapshot/click/type/press by element id. Do not use shell/curl or pixel clicks to inspect a web page.
2) Workspace files or commands: use list_files, read_file, write_file, or shell.
3) Connected services: use an MCP tool when it directly matches the task.
4) Opening a local file or non-browser app: use open_path or launch_app.
5) Native GUI with no DOM (dialogs, file manager, XFCE): use computer_act by element id. Those ids are AT-SPI controls, not window boxes.

When you ARE using the desktop: the human watches the same live screen. Only the latest screenshot you received is current; they may have interacted since. Call computer_observe before coordinate clicks, after navigation, when the outcome is uncertain, and before describing what is on screen. Never guess the screen state from files, history or memory. Never kill or restart the browser, display, or desktop processes; if the browser tool reports it is unavailable, use computer_observe / computer_act on the existing window instead.

When you use the browser tool:
- snapshot first; click {\"action\":\"click\",\"element\":N}; type {\"action\":\"type\",\"element\":N,\"text\":\"...\"}; open a URL with navigate.
- Yellow numbered marks on the screenshot match the element list. Click the number, not guessed pixels.
- Elements tagged [below viewport ...] / [above viewport ...] are outside the visible area but still clickable by id; the click scrolls to them. Do not scroll manually just to reach them.
- A control is disabled only when its entry says [disabled]. Never claim a button is disabled, counting down or loading unless the element list or the screenshot shows that.
- Clicking a [disabled] control is fine: the click waits up to 45s for it to become enabled (stay timers, short videos) and then clicks. So when Next is disabled, just click Next. Only if the click reports it is still disabled, call wait with a longer time and click again.
- Element ids are renumbered whenever the page changes; after navigation use the ids from the newest result, not older ones.
- If the control is not in the element list at all, take a fresh snapshot; it may not be rendered yet.

When a click changes nothing: do not repeat it blindly. Take a fresh snapshot, read the [disabled] tags and the page text, then act. Pages that need patience (training videos, quizzes, slow forms) are normal: keep working through them step by step and report progress in one short sentence when done.

Waiting is a tool call, never a reply. Ending your turn with \"waiting for X\" stops the whole run; nobody resumes it. If something must finish first, call wait (or click, which waits) and continue.

Multi-step tasks and taught skills: you are done only when the playbook's check passes (for example the course shows completed, the form shows a confirmation). Do not stop with a status sentence in the middle; keep calling tools until the check passes or you are truly blocked, then say exactly why. Never repeat an earlier reply word for word; describe the current screen.

Never ask for passwords, codes, or tokens in chat. At a login wall: call list_accounts, then use_saved_login {accountId} when a saved account matches. If none matches, or 2FA/CAPTCHA appears, call request_takeover with site and why so the human signs in on YOUR screen. Recurring work uses create_schedule (five-field cron, Asia/Taipei unless told otherwise).

computer_act examples (native windows only):
- {\"kind\":\"click\",\"element\":1}
- {\"kind\":\"type\",\"element\":1,\"text\":\"filename.pdf\"}
- {\"kind\":\"click\",\"x\":N,\"y\":N} (canvas / no numbered control)
- {\"kind\":\"type\",\"text\":\"...\"}
- {\"kind\":\"key\",\"key\":\"Return\"}
- {\"kind\":\"focus\",\"title\":\"Open File\"}
- wait: {\"seconds\":30,\"reason\":\"video playing\"}

On a Team Computer, relative files live in your bot folder; use shared/ for shared work. User-attached files appear in inbox/ for two hours only — chat history does not keep the bytes. Open them with open_path when you need the original file. Finish the user's task.";

pub async fn send(
    state: &AppState,
    actor: &Actor,
    bot_id: &str,
    thread_id: &str,
    text: &str,
    client_nonce: Option<&str>,
    blocks: &[Value],
    attachments: &[SessionAttachment],
) -> Result<Value, String> {
    if crate::skills::recording_skill(state.pool(), bot_id)
        .await
        .is_some()
    {
        return Err("示範進行中：先按「完成示範」或「取消」，再送訊息。".into());
    }
    let decoded = crate::attachments::decode_incoming(attachments)?;
    if text.trim().is_empty() && decoded.is_empty() {
        return Err("empty message".into());
    }
    let stored_body = crate::attachments::caption_for_title(text, &decoded);
    let mut stored_blocks = blocks.to_vec();
    stored_blocks.extend(crate::attachments::stored_blocks(&decoded));
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
    .bind(&stored_body)
    .bind(json!(stored_blocks))
    .bind(&run_id)
    .bind(client_nonce)
    .execute(&mut *tx)
    .await
    .map_err(|error| error.to_string())?;
    if crate::sessions::is_default_session_title(&current_title) {
        sqlx::query("UPDATE threads SET title=$2 WHERE id=$1")
            .bind(thread_id)
            .bind(crate::sessions::title_from_first_message(&stored_body))
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
        .bind(&stored_body)
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
    .bind(json!({"id":message_id,"seq":seq,"role":"user","body":stored_body,"runId":run_id}))
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
    if let Err(error) =
        crate::attachments::stage_for_bots(state, actor, &member_ids, &decoded).await
    {
        tracing::warn!("stage attachments for {message_id}: {error}");
    }
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
        let _ = sqlx::query("UPDATE runs SET status='failed',error='Worker interrupted after tool execution; inspect current state before continuing.',completed_at=now(),lease_owner=NULL,lease_expires_at=NULL WHERE status IN ('leased','running') AND lease_expires_at<now() AND COALESCE((checkpoint->>'toolsStarted')::boolean,false)")
            .execute(state.pool()).await;
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
                let retryable = retryable_run_error(&error);
                let next_status: Option<String> = sqlx::query_scalar(
                    "UPDATE runs
                     SET status=CASE WHEN $4 AND retry_count < max_retries AND NOT COALESCE((checkpoint->>'toolsStarted')::boolean,false) THEN 'queued' ELSE 'failed' END,
                         error=$2, completed_at=CASE WHEN $4 AND retry_count < max_retries AND NOT COALESCE((checkpoint->>'toolsStarted')::boolean,false) THEN NULL ELSE now() END,
                         lease_owner=NULL, lease_expires_at=NULL, updated_at=now()
                     WHERE id=$1 AND lease_owner=$3 AND status IN ('leased','running') RETURNING status",
                )
                    .bind(&run_id)
                    .bind(&error)
                    .bind(&owner)
                    .bind(retryable)
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

    let (model, vision) = bot_model(state, actor, &bot).await?;
    let skills = crate::skills::saved_skills(state.pool(), bot_id).await;

    let ctx = Arc::new(ToolCtx {
        sandbox: state.sandbox.clone(),
        computer: std::sync::Mutex::new(None),
        context: std::sync::Mutex::new(adapter_context_for(
            actor,
            bot_id,
            "run",
            None,
            Some(run_id),
        )),
        mode: parse_mode(&computer.scope),
        bot_id: bot_id.to_string(),
        vision,
        gui_block: std::sync::Mutex::new(None),
        previous_frame: std::sync::Mutex::new(None),
        elements: std::sync::Mutex::new(Vec::new()),
        miss_streak: std::sync::Mutex::new(0),
        last_click_key: std::sync::Mutex::new(None),
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
    let blocks: Vec<Value> = if resume_after_takeover {
        Vec::new()
    } else {
        let raw: Value = sqlx::query_scalar(
            "SELECT blocks FROM messages WHERE thread_id=$1 AND seq=$2 AND role='user'",
        )
        .bind(thread_id)
        .bind(current_seq)
        .fetch_optional(state.pool())
        .await
        .ok()
        .flatten()
        .unwrap_or(json!([]));
        raw.as_array().cloned().unwrap_or_default()
    };
    if !resume_after_takeover {
        first.extend(crate::attachments::llm_parts(state, actor, bot_id, &blocks, vision).await);
    }
    let mut skill_check: Option<String> = None;
    if !resume_after_takeover {
        // The user named a taught skill: hand the model the full playbook up
        // front so it does not have to guess or call use_skill first.
        if let Some(skill) = crate::skills::skill_for_prompt(state.pool(), bot_id, prompt).await {
            first.push(UserContent::text(crate::skills::format_playbook_for_run(
                &skill,
            )));
            skill_check = Some(crate::skills::skill_check_hint(&skill));
        }
    }
    let mut earlier_replies: Vec<String> = assistant_texts(&history);
    if skill_check.is_some() {
        // A skill run is self-contained. Old chat turns about the same site
        // ("Next is still counting down" x6) otherwise anchor the model into
        // repeating its past conclusions instead of reading the screen.
        history.clear();
        first.push(UserContent::text(
            "Earlier chat history is intentionally omitted for this skill run. Work only from the playbook above and the current screen.",
        ));
    }
    let workspace_file = blocks
        .iter()
        .any(|block| block.get("kind").and_then(Value::as_str) == Some("file"));
    // Greetings and small talk must not even *see* desktop tools: models
    // otherwise "check the screen" or `ls` the home on "hi" and boot Docker.
    let chat_only =
        !resume_after_takeover && skill_check.is_none() && !workspace_file && is_plain_chat(prompt);
    if chat_only {
        // Memory is recalled separately and injected into the preamble below.
        // Do not expose even memory tools here: a plain greeting must be one
        // model call with no chance of accidentally invoking any capability.
        defs.clear();
        tracing::info!(run_id, "chat-only turn: all tools withheld");
    }
    // Taught skills run long (a 24-page course is 24 clicks); plain chats stay
    // bounded tighter so a confused model cannot burn budget for as long.
    let max_turns: u32 = if skill_check.is_some() {
        80
    } else if chat_only {
        4
    } else {
        40
    };
    let mut nudges: u8 = 0;
    let mut screenshots: u32 = 0;
    let mut screenshot_bytes: u64 = 0;
    if resume_after_takeover {
        set_run_step(state, run_id, computer::STEP_HANDOFF).await;
        prepare_run_computer(state, actor, bot_id, run_id, &ctx, true).await?;
    }
    if resume_after_takeover && ctx.gui_block.lock().unwrap().is_none() {
        let outcome = dispatch(&ctx, "computer_observe", &json!({})).await;
        first.push(UserContent::text(outcome.text));
        if let Some(image) = outcome.image {
            screenshot_bytes += image.len() as u64;
            screenshots += 1;
            first.extend(screenshot_parts(image));
        }
    }
    let mut pending = Message::User { content: first };
    if !resume_after_takeover {
        if let (Some(saved_history), Some(saved_pending)) = (
            checkpoint.get("harnessHistory"),
            checkpoint.get("harnessPending"),
        ) {
            if let (Ok(restored), Ok(mut next)) = (
                serde_json::from_value::<Vec<Message>>(saved_history.clone()),
                serde_json::from_value::<Message>(saved_pending.clone()),
            ) {
                history = restored;
                if let Message::User { content } = &mut next {
                    content.push(UserContent::text("Resumed after a completed tool batch. Do not repeat completed actions. Observe current browser/desktop before any new mutation; prior element references may be stale."));
                }
                pending = next;
            }
        }
    }

    let mut final_text = String::new();
    let mut turns: u32 = checkpoint
        .get("harnessTurns")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(u32::MAX as u64) as u32;
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
    let system = if chat_only { SYSTEM_CHAT } else { SYSTEM };
    let mut preamble = if bot.instructions.trim().is_empty() {
        system.to_string()
    } else {
        format!(
            "{system}\n\nBot-specific instructions:\n{}",
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
    if !chat_only {
        if let Some(index) = crate::skills::skills_preamble(&skills) {
            preamble.push_str("\n\n");
            preamble.push_str(&index);
        }
        if let Ok(accounts) = crate::vault::list_on(state.pool(), actor, bot_id).await {
            if !accounts.is_empty() {
                let names = accounts
                    .iter()
                    .map(|item| format!("{} ({})", item.site, item.username))
                    .collect::<Vec<_>>()
                    .join(", ");
                preamble.push_str(&format!(
                    "\n\nSaved logins (passwords are not shown): {names}. At a login wall call use_saved_login with the matching accountId from list_accounts."
                ));
            }
        }
    }

    for _ in turns..max_turns {
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
        drop_history_screenshots(&mut history, &pending);
        set_run_step(state, run_id, MODEL_STEP).await;
        let model_started = std::time::Instant::now();
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
            result = complete_with_retry(&model, pending.clone(), &preamble, &history, &defs) => result
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
        tracing::info!(
            run_id,
            turn = turns,
            elapsed_ms = model_started.elapsed().as_millis() as u64,
            tool_calls = calls.len(),
            "model turn"
        );
        if calls.is_empty() {
            // A model that quits a playbook early, or parrots an earlier reply
            // instead of describing the current screen, gets pushed back to
            // the tools a couple of times before we accept the text.
            let parroted = earlier_replies
                .iter()
                .any(|earlier| earlier == final_text.trim());
            let nudge = if parroted {
                Some(
                    "Your reply repeats an earlier message word for word, so it cannot describe the current screen. Below is what the screen shows RIGHT NOW. Act on it with a tool call. Waiting is done by calling wait or by clicking the control (the click waits for it to enable), never by replying. Reply in text only once the task is finished or you are truly blocked (say why).".to_string(),
                )
            } else {
                skill_check.as_ref().map(|check| {
                    format!(
                        "The run is not finished; your text reply ended nothing but your own turn. Check: {check}\nBelow is the current screen. If the next control is [disabled], click it anyway — the click waits up to 45s for it to enable — or call wait. Ids marked [below viewport] scroll automatically. Only reply in text when the check passes or you are truly blocked, and then say exactly what blocks you."
                    )
                })
            };
            match nudge {
                Some(text) if nudges < 6 && turns + 2 < max_turns => {
                    nudges += 1;
                    tracing::info!(
                        run_id,
                        turn = turns,
                        parroted,
                        "nudging model back to tools"
                    );
                    earlier_replies.push(final_text.trim().to_string());
                    final_text.clear();
                    let mut content = vec![UserContent::text(text)];
                    if used_gui || skill_check.is_some() {
                        prepare_run_computer(state, actor, bot_id, run_id, &ctx, true).await?;
                    }
                    if (used_gui || skill_check.is_some())
                        && ctx.gui_block.lock().unwrap().is_none()
                    {
                        set_run_step(state, run_id, "computer_observe: 重新確認畫面").await;
                        let outcome = dispatch(&ctx, "computer_observe", &json!({})).await;
                        content.push(UserContent::text(outcome.text));
                        if let Some(image) = outcome.image {
                            screenshot_bytes += image.len() as u64;
                            screenshots += 1;
                            content.extend(screenshot_parts(image));
                        }
                    }
                    pending = Message::User { content };
                    continue;
                }
                _ => break,
            }
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
            if !defs.iter().any(|tool| tool.name == name) {
                results.push(UserContent::tool_result_for(
                    call.id.clone(),
                    call.provider.clone(),
                    name,
                    vec![ToolResultContent::text(
                        "This turn is conversation only; the desktop is not available. Reply in text.",
                    )],
                ));
                continue;
            }
            if tool_needs_sandbox(&name) {
                prepare_run_computer(state, actor, bot_id, run_id, &ctx, tool_needs_gui(&name))
                    .await?;
            }
            used_gui |= matches!(
                name.as_str(),
                "computer_observe"
                    | "computer_act"
                    | "open_path"
                    | "launch_app"
                    | "browser"
                    | "wait"
                    | "use_saved_login"
                    | "request_takeover"
            );
            let step = describe_step(&name, &call.function.arguments);
            set_run_step(state, run_id, &step).await;
            let fence=sqlx::query("UPDATE runs SET checkpoint=COALESCE(checkpoint,'{}'::jsonb)||jsonb_build_object('toolsStarted',true) WHERE id=$1 AND lease_owner=$2 AND status='running'")
                .bind(run_id).bind(lease_owner).execute(state.pool()).await.map_err(|e| e.to_string())?;
            if fence.rows_affected() != 1 {
                return Err("run lease lost before tool dispatch".into());
            }
            let tool_started = std::time::Instant::now();
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
                    Duration::from_secs(150),
                    dispatch(&ctx, &name, &call.function.arguments),
                ) => match outcome {
                    Ok(outcome) => outcome,
                    Err(_) => return Err(format!("tool {name} timed out; its effects are unknown. Inspect the current state before continuing")),
                }
            };
            tracing::info!(
                run_id,
                turn = turns,
                step = %step,
                elapsed_ms = tool_started.elapsed().as_millis() as u64,
                result_chars = outcome.text.chars().count(),
                screenshot = outcome.image.is_some(),
                pause = outcome.pause,
                "tool call"
            );
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
            if !outcome.blocks.is_empty() && !outcome.pause {
                let _ = append_bot_message_with(
                    state,
                    thread_id,
                    run_id,
                    bot_id,
                    &outcome.text,
                    json!(outcome.blocks.clone()),
                )
                .await;
            }
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
                let _ = computer::takeover(state, actor, bot_id).await;
                append_bot_message_with(
                    state,
                    thread_id,
                    run_id,
                    bot_id,
                    &outcome.text,
                    json!(outcome.blocks),
                )
                .await?;
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
        save_harness_checkpoint(state, run_id, lease_owner, &history, &pending, turns).await?;
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

/// Resolve the model a bot runs on (bot override → workspace default → env
/// credentials). Returns the connected model and whether it accepts images.
pub(crate) async fn bot_model(
    state: &AppState,
    actor: &Actor,
    bot: &crate::db::BotRow,
) -> Result<(DynModel, bool), String> {
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
    Ok((model, backend.capabilities.vision))
}

pub(crate) async fn complete_once(
    model: &DynModel,
    pending: Message,
    preamble: &str,
    history: &[Message],
    defs: &[ToolDefinition],
) -> Result<Vec<AssistantContent>, String> {
    match model {
        DynModel::Xai(model) => complete_with(model, pending, preamble, history, defs).await,
        DynModel::OpenAi(model) => complete_with(model, pending, preamble, history, defs).await,
        DynModel::OpenAiResponses(model) => {
            complete_with(model, pending, preamble, history, defs).await
        }
    }
}

fn retryable_run_error(error: &str) -> bool {
    let Some((_, suffix)) = error.split_once("status ") else {
        return true;
    };
    let Some(code) = suffix.get(..3).and_then(|value| value.parse::<u16>().ok()) else {
        return true;
    };
    !matches!(code, 400..=499 if !matches!(code, 408 | 409 | 425 | 429))
}

async fn complete_with_retry(
    model: &DynModel,
    pending: Message,
    preamble: &str,
    history: &[Message],
    defs: &[ToolDefinition],
) -> Result<Vec<AssistantContent>, String> {
    let mut last = String::new();
    for attempt in 0..3 {
        let result = tokio::time::timeout(
            Duration::from_secs(60),
            complete_once(model, pending.clone(), preamble, history, defs),
        )
        .await;
        match result {
            Ok(Ok(content)) => return Ok(content),
            Ok(Err(error)) => {
                if !retryable_run_error(&error) {
                    return Err(error);
                }
                last = error;
            }
            Err(_) => last = "model request timed out after 60 seconds".into(),
        }
        if attempt < 2 {
            tokio::time::sleep(Duration::from_millis(500 * (1 << attempt))).await;
        }
    }
    Err(last)
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
    // `Low` makes OpenAI-compatible backends downscale the 1280x800 frame to
    // ~512px before the model sees it: small text vanishes and coordinate
    // clicks land 2-3x off. Coordinates only work at native resolution.
    vec![
        UserContent::text(SCREENSHOT_CAPTION),
        UserContent::image_base64(encoded, Some(media), Some(ImageDetail::High)),
    ]
}

fn has_screenshot(message: &Message) -> bool {
    match message {
        Message::User { content } => content
            .iter()
            .any(|part| matches!(part, UserContent::Image(_))),
        _ => false,
    }
}

/// Keep exactly one screenshot in the model's context: the one in `pending`
/// if it carries a fresh frame, otherwise the most recent one already in
/// history. Without the fallback a "(screen unchanged)" turn would leave the
/// model with no picture of the desktop at all.
/// Plain-text bodies of the assistant turns in a history, used to catch a model
/// that answers by repeating an earlier reply instead of reading the screen.
fn assistant_texts(history: &[Message]) -> Vec<String> {
    history
        .iter()
        .filter_map(|message| match message {
            Message::Assistant { content, .. } => Some(
                content
                    .iter()
                    .filter_map(|item| match item {
                        AssistantContent::Text(text) => Some(text.text.trim().to_string()),
                        _ => None,
                    })
                    .collect::<String>(),
            ),
            _ => None,
        })
        .filter(|text| !text.is_empty())
        .collect()
}

async fn save_harness_checkpoint(
    state: &AppState,
    run_id: &str,
    owner: &str,
    history: &[Message],
    pending: &Message,
    turns: u32,
) -> Result<(), String> {
    let mut history = history.to_vec();
    let mut pending = pending.clone();
    for message in history.iter_mut().chain(std::iter::once(&mut pending)) {
        if let Message::User { content } = message {
            content.retain(|part| {
                !matches!(part, UserContent::Image(_))
                    && !matches!(part,UserContent::Text(text) if text.text==SCREENSHOT_CAPTION)
            });
        }
    }
    let value = json!({"harnessHistory":history,"harnessPending":pending,"toolsStarted":false,"harnessTurns":turns});
    // Large/unsupported checkpoints fail closed: keep the uncertain-effects flag.
    if value.to_string().len() > 1024 * 1024 {
        return Ok(());
    }
    let result=sqlx::query("UPDATE runs SET checkpoint=COALESCE(checkpoint,'{}'::jsonb)||$3,updated_at=now() WHERE id=$1 AND lease_owner=$2 AND status='running'")
        .bind(run_id).bind(owner).bind(value).execute(state.pool()).await.map_err(|e|e.to_string())?;
    if result.rows_affected() != 1 {
        return Err("run lease lost while checkpointing".into());
    }
    Ok(())
}

fn drop_history_screenshots(history: &mut [Message], pending: &Message) {
    let keep = if has_screenshot(pending) {
        None
    } else {
        history.iter().rposition(has_screenshot)
    };
    for (index, message) in history.iter_mut().enumerate() {
        if keep == Some(index) {
            continue;
        }
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

/// Cancel every unfinished run of a bot and free the screen/execution leases
/// it held, so the desktop is available to a human immediately.
pub(crate) async fn cancel_active_runs(
    state: &AppState,
    bot_id: &str,
) -> Result<Vec<String>, String> {
    let run_ids: Vec<String> = sqlx::query_scalar(
        "UPDATE runs SET status = 'cancelled', completed_at = now(), updated_at = now()
         WHERE bot_id = $1 AND status IN ('queued','leased','running','waiting_input','waiting_takeover')
         RETURNING id",
    )
    .bind(bot_id)
    .fetch_all(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    for run_id in &run_ids {
        computer::release_screen_execution(state, run_id).await?;
    }
    sqlx::query(
        "UPDATE computers SET execution_bot_id = NULL, execution_run_id = NULL,
                execution_lease_expires_at = NULL, updated_at = now()
         WHERE execution_bot_id = $1",
    )
    .bind(bot_id)
    .execute(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    Ok(run_ids)
}

pub(crate) async fn append_bot_message(
    state: &AppState,
    thread_id: &str,
    run_id: &str,
    bot_id: &str,
    body: &str,
) -> Result<(), String> {
    append_bot_message_with(state, thread_id, run_id, bot_id, body, json!([])).await
}

pub(crate) async fn append_bot_message_with(
    state: &AppState,
    thread_id: &str,
    run_id: &str,
    bot_id: &str,
    body: &str,
    blocks: Value,
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
        "INSERT INTO messages (id,thread_id,seq,role,body,blocks,run_id,speaker_bot_id)
         VALUES ($1,$2,$3,'assistant',$4,$5,$6,$7)",
    )
    .bind(&message_id)
    .bind(thread_id)
    .bind(seq)
    .bind(body)
    .bind(&blocks)
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

fn is_punct(c: char) -> bool {
    matches!(
        c,
        '!' | '?'
            | '.'
            | ','
            | ';'
            | ':'
            | '~'
            | '"'
            | '\''
            | '('
            | ')'
            | '！'
            | '？'
            | '。'
            | '，'
            | '、'
            | '；'
            | '：'
            | '～'
            | '…'
            | '・'
            | '·'
            | '「'
            | '」'
            | '『'
            | '』'
            | '（'
            | '）'
            | '【'
            | '】'
            | '《'
            | '》'
    )
}

fn normalize_prompt(prompt: &str) -> String {
    let lowered = prompt.trim().to_lowercase();
    let mut out = String::new();
    let mut pending_space = false;
    for c in lowered.chars() {
        if c.is_whitespace() || is_punct(c) {
            if !out.is_empty() {
                pending_space = true;
            }
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(c);
    }
    out
}

fn first_token(text: &str) -> &str {
    text.split_whitespace().next().unwrap_or("")
}

fn looks_like_url(text: &str) -> bool {
    let lower = text.to_lowercase();
    if lower.contains("://") || lower.contains("www.") {
        return true;
    }
    lower.split_whitespace().any(|token| {
        let host = token.split('/').next().unwrap_or(token);
        let host = host.split('?').next().unwrap_or(host);
        let Some((_, tld)) = host.rsplit_once('.') else {
            return false;
        };
        matches!(
            tld,
            "com"
                | "org"
                | "net"
                | "io"
                | "ai"
                | "app"
                | "dev"
                | "co"
                | "edu"
                | "gov"
                | "tv"
                | "me"
                | "cc"
                | "info"
                | "xyz"
                | "tw"
                | "cn"
                | "hk"
                | "jp"
        ) && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    })
}

fn is_text_only_request(normalized: &str) -> bool {
    const PHRASES: &[&str] = &[
        "what is ",
        "what are ",
        "who is ",
        "why ",
        "how does ",
        "how do ",
        "tell me about ",
        "explain ",
        "can you explain ",
        "could you explain ",
        "please explain ",
        "是什麼",
        "是什么",
        "什麼是",
        "什么是",
        "為什麼",
        "为什么",
        "請解釋",
        "请解释",
        "幫我解釋",
        "帮我解释",
        "請說明",
        "请说明",
    ];
    PHRASES.iter().any(|phrase| normalized.contains(phrase))
}

fn prompt_needs_memory(prompt: &str) -> bool {
    let normalized = normalize_prompt(prompt);
    const PHRASES: &[&str] = &[
        "remember ",
        "remember that",
        "do you remember",
        "forget ",
        "記住",
        "记住",
        "記得",
        "记得",
        "忘記",
        "忘记",
    ];
    PHRASES.iter().any(|phrase| normalized.contains(phrase))
}

fn prompt_needs_desktop(prompt: &str) -> bool {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return false;
    }
    if looks_like_url(trimmed) {
        return true;
    }
    let lower = trimmed.to_lowercase();
    let normalized = normalize_prompt(trimmed);
    // Mentioning a site or app in an informational question is not a request
    // to operate it ("YouTube 是什麼？", "how do I open Chrome?").
    if is_text_only_request(&normalized) {
        return false;
    }
    const MARKERS: &[&str] = &[
        "open ",
        "open the",
        "launch",
        "click",
        "browser",
        "chrome",
        "chromium",
        "firefox",
        "desktop",
        "screenshot",
        "terminal",
        "xterm",
        "install ",
        "download",
        "upload",
        "type into",
        "navigate",
        "visit ",
        "go to ",
        "google ",
        "youtube",
        "video",
        "watch ",
        "email",
        "gmail",
        "on the computer",
        "on my computer",
        "use the computer",
        "use the desktop",
        "file manager",
        "run this",
        "run the ",
        "打開",
        "开启",
        "開啟",
        "启动",
        "啟動",
        "點擊",
        "点击",
        "點一下",
        "点一下",
        "瀏覽器",
        "浏览器",
        "桌面",
        "電腦",
        "电脑",
        "螢幕",
        "屏幕",
        "截圖",
        "截图",
        "終端",
        "终端",
        "安裝",
        "安装",
        "下載",
        "下载",
        "上傳",
        "上传",
        "檔案",
        "档案",
        "資料夾",
        "文件夹",
        "網頁",
        "网页",
        "網站",
        "网站",
        "上網",
        "上网",
        "搜尋",
        "搜索",
        "登入",
        "登录",
        "進電腦",
        "进电脑",
        "操作電腦",
        "操作电脑",
        "用電腦",
        "用电脑",
        "看畫面",
        "看画面",
        "看一下影片",
        "看一下視頻",
        "看一下视频",
        "影片",
        "視頻",
        "视频",
        "幫我開",
        "帮我开",
        "幫我點",
        "帮我点",
        "幫我搜",
        "帮我搜",
        "執行指令",
        "执行指令",
        "在電腦",
        "在电脑",
        "到網站",
        "到网站",
        "去網站",
        "去网站",
        "執行「",
        "执行「",
        "執行\"",
        "执行\"",
        "排程",
        "以後每天",
        "以后每天",
        "每個工作日",
        "每个工作日",
        "every day",
        "weekdays",
        "every monday",
        "schedule",
    ];
    if MARKERS.iter().any(|marker| lower.contains(marker)) {
        return true;
    }
    if trimmed.contains('?') || trimmed.contains('？') {
        return false;
    }
    const SHELL: &[&str] = &[
        "ls", "pwd", "cd", "cat", "chmod", "chown", "rm", "mv", "cp", "mkdir", "touch", "htop",
        "top", "ps", "df", "du", "whoami", "uname", "curl", "wget", "git", "npm", "npx", "pip",
        "pip3", "python", "python3", "node", "cargo", "make", "docker", "apt", "apt-get", "sudo",
        "bash", "sh", "zsh",
    ];
    SHELL.contains(&first_token(&normalized))
}

fn is_follow_up_task(normalized: &str) -> bool {
    const FOLLOW: &[&str] = &[
        "繼續",
        "继续",
        "接著",
        "接着",
        "接著做",
        "接着做",
        "再來",
        "再来",
        "然後",
        "然后",
        "continue",
        "keep going",
        "go on",
        "go ahead",
        "resume",
        "keep at it",
        "從畫面",
        "从画面",
    ];
    FOLLOW
        .iter()
        .any(|item| normalized == *item || normalized.starts_with(&format!("{item} ")))
}

fn has_task_verb(normalized: &str) -> bool {
    const VERBS: &[&str] = &[
        "幫我",
        "帮我",
        "幫忙",
        "帮忙",
        "請你",
        "请你",
        "請幫",
        "请帮",
        "please",
        "can you",
        "could you",
        "would you",
        "will you",
    ];
    VERBS.iter().any(|verb| normalized.contains(verb))
}

fn is_greeting(normalized: &str) -> bool {
    const EXACT: &[&str] = &[
        "hi",
        "hi hi",
        "hii",
        "hiii",
        "hey",
        "hey there",
        "hey hey",
        "hello",
        "hello there",
        "hello hi",
        "hi hello",
        "yo",
        "sup",
        "howdy",
        "hiya",
        "good morning",
        "good afternoon",
        "good evening",
        "good night",
        "morning",
        "evening",
        "how are you",
        "how are you doing",
        "how's it going",
        "hows it going",
        "whats up",
        "what's up",
        "what up",
        "who are you",
        "what can you do",
        "what do you do",
        "what are you",
        "thanks",
        "thank you",
        "thx",
        "ty",
        "ok",
        "okay",
        "cool",
        "nice",
        "got it",
        "understood",
        "嗨",
        "嗨嗨",
        "你好",
        "您好",
        "哈囉",
        "哈罗",
        "嗨你好",
        "你好啊",
        "你好呀",
        "你好嗨",
        "早安",
        "午安",
        "晚安",
        "在嗎",
        "在嘛",
        "在不在",
        "在吗",
        "你好嗎",
        "你好吗",
        "你是誰",
        "你是谁",
        "你會什麼",
        "你会什么",
        "你能做什麼",
        "你能做什么",
        "你可以做什麼",
        "你可以做什么",
        "謝謝",
        "谢谢",
        "感謝",
        "感谢",
        "好",
        "嗯",
        "喔",
        "哦",
        "哈哈",
        "呵",
        "聊聊",
        "聊天",
        "陪我聊天",
        "說說話",
        "说说话",
        "👋",
        "🙋",
        "😊",
        "🙂",
        "😀",
    ];
    if EXACT.iter().any(|item| normalized == *item) {
        return true;
    }
    const PREFIXES: &[&str] = &["hi ", "hey ", "hello ", "嗨", "你好"];
    PREFIXES.iter().any(|prefix| {
        normalized.starts_with(prefix)
            && normalized.chars().count() <= 16
            && !has_task_verb(normalized)
    })
}

fn is_chat_intent(normalized: &str) -> bool {
    const PHRASES: &[&str] = &[
        "陪我聊天",
        "跟我聊天",
        "來聊天",
        "来聊天",
        "只是聊天",
        "隨便聊聊",
        "随便聊聊",
        "just chatting",
        "just saying hi",
        "let's chat",
        "lets chat",
        "wanna chat",
        "want to chat",
    ];
    PHRASES.iter().any(|phrase| normalized.contains(phrase))
}

/// True when this user message should stay in text chat: no desktop tools,
/// no container boot, no screenshot.
fn is_plain_chat(prompt: &str) -> bool {
    if prompt_needs_desktop(prompt) || prompt_needs_memory(prompt) {
        return false;
    }
    let normalized = normalize_prompt(prompt);
    if normalized.is_empty() {
        return true;
    }
    if is_text_only_request(&normalized) {
        return true;
    }
    if is_follow_up_task(&normalized) {
        return false;
    }
    if is_greeting(&normalized) {
        return true;
    }
    let chars = normalized.chars().count();
    if chars <= 24 && !has_task_verb(&normalized) {
        return true;
    }
    is_chat_intent(&normalized) && chars <= 48
}

fn tool_needs_sandbox(name: &str) -> bool {
    matches!(
        name,
        "shell"
            | "list_files"
            | "read_file"
            | "write_file"
            | "computer_observe"
            | "computer_act"
            | "browser"
            | "open_path"
            | "launch_app"
            | "wait"
            | "use_saved_login"
            | "request_takeover"
    )
}

fn tool_needs_gui(name: &str) -> bool {
    matches!(
        name,
        "computer_observe"
            | "computer_act"
            | "browser"
            | "open_path"
            | "launch_app"
            | "wait"
            | "use_saved_login"
            | "request_takeover"
    )
}

async fn prepare_run_computer(
    state: &AppState,
    actor: &Actor,
    bot_id: &str,
    run_id: &str,
    ctx: &ToolCtx,
    need_gui: bool,
) -> Result<(), String> {
    if ctx.computer.lock().unwrap().is_none() {
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
        if computer.state != "running" {
            let step = if computer.state == "suspended" {
                computer::STEP_WAKING
            } else {
                computer::STEP_BOOTING
            };
            set_run_step(state, run_id, step).await;
        }
        computer::boot(state, actor, bot_id).await?;
        let computer = state
            .db
            .get_computer(bot.computer_id.as_deref().unwrap_or(""))
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "computer not found".to_string())?;
        let computer_ref = computer::computer_ref(&computer)
            .ok_or_else(|| "computer is not running".to_string())?;
        *ctx.computer.lock().unwrap() = Some(computer_ref);
    }
    if !need_gui || ctx.adapter().display.is_some() {
        return Ok(());
    }
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
    let bound = computer::ensure_bot_screen(state, actor, bot_id, &computer, Some(run_id)).await?;
    let mut gui_block = bound.gui_block;
    let screen = if let Some(row) = bound.row {
        let held_by_other = row
            .execution_run_id
            .as_deref()
            .is_some_and(|held| held != run_id);
        let user_holding = row.control_holder == "user";
        if held_by_other || user_holding {
            set_run_step(state, run_id, computer::STEP_HANDOFF).await;
        }
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
    *ctx.gui_block.lock().unwrap() = gui_block;
    *ctx.context.lock().unwrap() =
        adapter_context_for(actor, bot_id, "run", screen.as_ref(), Some(run_id));
    Ok(())
}

fn history_window_start(summary_seq: i32, current_seq: i32) -> i32 {
    summary_seq.min(current_seq.saturating_sub(1)).max(0)
}

const MODEL_STEP: &str = "思考中";

/// Human-readable label for what the run is doing right now. Surfaced through
/// `computer.status` so the chat can show "working: browser click" instead of a
/// bare spinner while a tool runs.
fn describe_step(name: &str, args: &Value) -> String {
    fn short(value: Option<&str>, max: usize) -> String {
        let text = value.unwrap_or("").replace('\n', " ");
        if text.chars().count() > max {
            format!("{}…", text.chars().take(max).collect::<String>())
        } else {
            text
        }
    }
    let get = |key: &str| args.get(key).and_then(Value::as_str);
    let detail = match name {
        "computer_observe" => "看畫面".to_string(),
        "computer_act" => args
            .get("actions")
            .and_then(Value::as_array)
            .map(|actions| {
                actions
                    .iter()
                    .take(4)
                    .map(|action| {
                        let field = |key: &str| action.get(key).and_then(Value::as_str);
                        let kind = field("kind").or(field("type")).unwrap_or("?");
                        let target = if let Some(text) = field("text").or(field("keys")) {
                            short(Some(text), 24)
                        } else if let Some(id) = lazyboy_control::element_id(action.get("element"))
                        {
                            format!("#{id}")
                        } else if let (Some(x), Some(y)) = (
                            action.get("x").and_then(Value::as_i64),
                            action.get("y").and_then(Value::as_i64),
                        ) {
                            format!("({x},{y})")
                        } else {
                            String::new()
                        };
                        format!("{kind} {target}").trim().to_string()
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default(),
        "browser" => {
            let target = get("url")
                .or(get("text"))
                .or(get("selector"))
                .map(|s| short(Some(s), 40));
            let target = target.or_else(|| {
                lazyboy_control::element_id(args.get("element")).map(|id| format!("#{id}"))
            });
            format!(
                "{} {}",
                get("action").unwrap_or("snapshot"),
                target.unwrap_or_default()
            )
            .trim()
            .to_string()
        }
        "shell" => short(get("command").or(get("cmd")), 60),
        "wait" => format!(
            "{}s {}",
            args.get("seconds")
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                .round(),
            short(get("reason"), 30)
        )
        .trim()
        .to_string(),
        "launch_app" | "open_path" => short(get("app").or(get("path")), 40),
        "read_file" | "write_file" | "list_dir" => short(get("path"), 40),
        "use_skill" => format!("讀取技能 {}", short(get("name"), 30)),
        "use_saved_login" => "填入已存帳號".into(),
        "list_accounts" => "列出已存帳號".into(),
        "create_schedule" => format!("排程 {}", short(get("name"), 30)),
        "list_schedules" => "列出排程".into(),
        "cancel_schedule" => "取消排程".into(),
        "request_takeover" => short(get("site").or(get("reason")), 40),
        _ => String::new(),
    };
    if detail.is_empty() {
        name.to_string()
    } else {
        format!("{name}: {detail}")
    }
}

pub(crate) async fn set_run_step(state: &AppState, run_id: &str, step: &str) {
    let result = sqlx::query(
        "UPDATE runs SET checkpoint = COALESCE(checkpoint, '{}'::jsonb)
                || jsonb_build_object('step', $2::text, 'stepAt', now()),
                updated_at = now()
         WHERE id = $1",
    )
    .bind(run_id)
    .bind(step)
    .execute(state.pool())
    .await;
    if let Err(error) = result {
        tracing::warn!(run_id, "failed to record run step: {error}");
    }
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
    use super::{
        RunHalt, SCREENSHOT_CAPTION, describe_step, drop_history_screenshots, halt_from_status,
        history_window_start, is_plain_chat, retryable_run_error, screenshot_parts, tool_needs_gui,
        tool_needs_sandbox,
    };
    use rig_core::completion::message::{Message, UserContent};
    use serde_json::json;

    #[test]
    fn chat_tools_do_not_need_the_desktop() {
        assert!(!tool_needs_sandbox("remember"));
        assert!(!tool_needs_sandbox("recall_memory"));
        assert!(!tool_needs_sandbox("use_skill"));
        assert!(tool_needs_sandbox("shell"));
        assert!(tool_needs_gui("computer_observe"));
        assert!(tool_needs_gui("browser"));
        assert!(!tool_needs_gui("shell"));
        assert!(!tool_needs_gui("list_files"));
    }

    #[test]
    fn greetings_stay_in_chat() {
        for prompt in [
            "hi",
            "Hi!",
            "hello",
            "hey there",
            "你好",
            "嗨",
            "哈囉！",
            "在嗎",
            "你是誰",
            "thanks",
            "謝謝",
            "聊聊",
            "👋",
            "今天心情不好",
            "寫一首詩",
            "量子力學是什麼",
            "hello, can you explain Rust ownership?",
            "YouTube 是什麼？",
            "how do I open Chrome?",
            "請解釋量子力學",
        ] {
            assert!(is_plain_chat(prompt), "{prompt} should stay in chat");
        }
    }

    #[test]
    fn computer_tasks_are_not_plain_chat() {
        for prompt in [
            "打開 youtube",
            "open chrome",
            "幫我開 gmail",
            "go to https://example.com",
            "ls",
            "git status",
            "繼續",
            "接著做",
            "執行「完成 STAR 訓練」",
            "看畫面現在怎樣",
            "download this file",
            "幫我點 Next",
            "幫我看一下 YouTube 上的 Rust 教學",
            "幫我看一下這個影片",
            "watch this YouTube video",
            "hi check my email",
            "remember that I prefer dark mode",
            "記住我喜歡繁體中文",
        ] {
            assert!(!is_plain_chat(prompt), "{prompt} should keep desktop tools");
        }
    }

    #[test]
    fn step_labels_summarize_tool_arguments() {
        assert_eq!(
            describe_step("computer_observe", &json!({})),
            "computer_observe: 看畫面"
        );
        assert_eq!(
            describe_step(
                "computer_act",
                &json!({"actions":[{"kind":"click","x":10,"y":20},{"kind":"type","text":"hello world"}]})
            ),
            "computer_act: click (10,20), type hello world"
        );
        assert_eq!(
            describe_step("browser", &json!({"action":"click","element":12})),
            "browser: click #12"
        );
        assert_eq!(
            describe_step("shell", &json!({"command":"ls\n-la"})),
            "shell: ls -la"
        );
        assert_eq!(
            describe_step("mcp_search", &json!({"query":"x"})),
            "mcp_search"
        );
    }

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

    fn user_with_shot(label: &str, byte: u8) -> Message {
        let mut content = vec![UserContent::text(label)];
        content.extend(screenshot_parts(vec![0xFF, 0xD8, 0xFF, byte]));
        Message::User { content }
    }

    fn image_count(history: &[Message]) -> usize {
        history
            .iter()
            .filter_map(|message| match message {
                Message::User { content } => Some(content),
                _ => None,
            })
            .flatten()
            .filter(|part| matches!(part, UserContent::Image(_)))
            .count()
    }

    #[test]
    fn history_keeps_latest_screenshot_when_new_turn_has_none() {
        let mut history = vec![
            user_with_shot("a", 1),
            Message::Assistant {
                id: None,
                content: vec![],
            },
            user_with_shot("b", 2),
        ];
        let pending = Message::User {
            content: vec![UserContent::text("(screen unchanged)")],
        };
        drop_history_screenshots(&mut history, &pending);
        assert_eq!(image_count(&history), 1);
        let Message::User { content } = &history[2] else {
            panic!("expected user message");
        };
        assert!(
            content
                .iter()
                .any(|part| matches!(part, UserContent::Image(_)))
        );
    }

    #[test]
    fn history_drops_every_screenshot_when_new_turn_has_one() {
        let mut history = vec![user_with_shot("a", 1), user_with_shot("b", 2)];
        let pending = user_with_shot("c", 3);
        drop_history_screenshots(&mut history, &pending);
        assert_eq!(image_count(&history), 0);
        assert!(history.iter().all(|message| match message {
            Message::User { content } => !content.iter().any(|part| matches!(
                part,
                UserContent::Text(text) if text.text == SCREENSHOT_CAPTION
            )),
            _ => true,
        }));
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

    #[test]
    fn permanent_provider_errors_are_not_retried() {
        assert!(!retryable_run_error(
            "ProviderResponseError: status 403 Forbidden"
        ));
        assert!(!retryable_run_error(
            "ProviderResponseError: status 422 Unprocessable"
        ));
        assert!(retryable_run_error(
            "ProviderResponseError: status 429 Too Many Requests"
        ));
        assert!(retryable_run_error("connection reset"));
    }
}
