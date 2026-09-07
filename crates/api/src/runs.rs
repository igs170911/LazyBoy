use lazyboy_harness::execution::{
    ExecutionMode, GOAL_CONTINUE, GOAL_INSTRUCTIONS, GoalOutcome, MAX_NUDGES_GOAL,
    MAX_NUDGES_PLAIN, NEEDS_INPUT_MARKER, StopReason, VERIFY_BEFORE_DONE, asks_for_input,
    goal_outcome, goal_request, stop_reason,
};
use lazyboy_harness::policy::{ActionObserved, LoopGuard, RunPolicy, Verdict};
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
use crate::tools::{ToolCtx, ToolOutcome, dispatch, tool_definitions};

const SCREENSHOT_CAPTION: &str = "Desktop screenshot (1280x800) with yellow numbered marks. Click by those element ids. The live VNC view has no marks.";

const TAKEOVER_RESUME_PROMPT: &str = "The user finished collaborating and released control. Continue the original task from the CURRENT screen. Do not restart from scratch. Element ids and page refs from before the handoff are invalid; use only the fresh observation below.";

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

The shell is one real terminal that stays open between calls: same directory, same exports, same background jobs. cd where the work is and stay there. A long-running job (server, build, download) comes back as status=running and keeps going — read it with log_lines, stop it with keys \"C-c\", and never type a second command into a terminal that is still busy.

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

Explain your next concrete action briefly before tool calls, in the user's language. When a tool fails, explain what failed and how you will recover in your next update. Recover from temporary errors by observing current state and choosing a different action; never replay an uncertain mutation blindly. Before any necessary stop, state what is completed, what remains, the specific blocker, and the next action needed. Never silently stop or claim a failed tool succeeded.

Waiting is a tool call, never a reply. Ending your turn with \"waiting for X\" stops the whole run; nobody resumes it. If something must finish first, call wait (or click, which waits) and continue.

Multi-step tasks and taught skills: you are done only when the playbook's check passes (for example the course shows completed, the form shows a confirmation). Do not stop with a status sentence in the middle; keep calling tools until the check passes or you are truly blocked, then say exactly why. Never repeat an earlier reply word for word; describe the current screen.

When you genuinely must stop and need the human — a decision only they can make, a credential you do not have, a file that is missing, or a result they must approve — first say what you already did and what the next step would be, then end that reply with a standalone [NEEDS_INPUT] line. The run pauses for their answer and resumes exactly where you stopped. Never use it when the task is simply finished.

Never ask for passwords, codes, or tokens in chat. At a login wall: call list_accounts, then use_saved_login {accountId} when a saved account matches. For a simple Cloudflare connection-check checkbox, first take computer_observe and use connection_check once with coordinates from that screenshot. Check the returned page content before continuing; disappearance of the checkbox alone is not success. Never reload repeatedly or restart the browser to retry. For other CAPTCHA, 2FA, an unsuccessful connection check, or no matching saved login, call request_takeover with site and why so the human signs in on YOUR screen. Recurring work uses create_schedule (five-field cron, Asia/Taipei unless told otherwise).

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
    // A message that continues an existing task belongs to that task: reuse its
    // run id so the loop resumes from the saved harness state instead of
    // starting over and fighting for the same desktop. That covers /goal
    // steering and every run paused while waiting for an answer. A human who
    // still holds the mouse is never interrupted by an incoming message.
    let merged_run: Option<String> = if room_id.is_none() {
        sqlx::query_scalar(
            "SELECT r.id FROM runs r
             WHERE r.bot_id=$1 AND r.thread_id=$2
               AND (
                 (r.status IN ('queued','leased','running','waiting_input','waiting_takeover')
                  AND btrim(r.prompt) ~ '^/goal($|[[:space:]])')
                 OR r.status='waiting_input'
                 OR (r.status='waiting_takeover' AND NOT EXISTS (
                       SELECT 1 FROM computers c JOIN bots b ON b.computer_id=c.id
                       WHERE b.id=r.bot_id AND c.control_holder='user'))
               )
             ORDER BY r.created_at ASC LIMIT 1",
        )
        .bind(bot_id)
        .bind(thread_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|error| error.to_string())?
    } else {
        None
    };
    let merged = merged_run.is_some();
    let run_id = merged_run.unwrap_or_else(|| Uuid::new_v4().to_string());
    if merged {
        // The answer itself arrives through the steering read; waking the run
        // only makes it claimable again and retires the pending question.
        sqlx::query(
            "UPDATE runs SET status='queued', retry_count=0,
                    checkpoint=checkpoint-'awaitResume', updated_at=now()
             WHERE id=$1 AND status IN ('waiting_input','waiting_takeover')",
        )
        .bind(&run_id)
        .execute(&mut *tx)
        .await
        .map_err(|error| error.to_string())?;
    }
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
        if merged && index == 0 {
            continue;
        }
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
        .bind(thread_id)
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
    let queued_behind_active = if merged {
        true
    } else {
        sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM runs WHERE bot_id=$1 AND id<>$2
             AND status IN ('queued','leased','running','waiting_input','waiting_takeover'))",
        )
        .bind(bot_id)
        .bind(&run_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|error| error.to_string())?
    };
    if let Some(room_id) = room_id.as_deref() {
        sqlx::query("UPDATE rooms SET updated_at=now() WHERE id=$1")
            .bind(room_id)
            .execute(&mut *tx)
            .await
            .map_err(|error| error.to_string())?;
    }
    tx.commit().await.map_err(|error| error.to_string())?;
    // The user message and its event were written in the transaction above
    // rather than through `sessions::append_event`, so this is where the open
    // chat windows get told to read it: every other tab sees the message without
    // waiting for its next poll.
    state.wakes.wake(thread_id);
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

/// `worker_loop` claim projection: run id, bot id, thread id, prompt, user id, space id.
type RetryCandidateRow = (String, String, String, String, String, String);

pub async fn worker_loop(state: AppState) {
    let inflight = Arc::new(tokio::sync::Semaphore::new(16));
    let lease_owner = format!("api-{}", Uuid::new_v4());
    // Without a knock the loop sits on its 200 ms timer before it can see a run
    // that was queued a moment ago; going straight to the claim query is what
    // makes the thinking indicator follow the message instead of the timer.
    let mut wakes = state.wakes.subscribe();
    loop {
        tokio::select! {
            _ = wakes.wait_any() => {}
            _ = tokio::time::sleep(Duration::from_millis(200)) => {}
        }
        let interrupted: Vec<(String, String, String)> = sqlx::query_as(
            "WITH doomed AS (
                 UPDATE runs SET status='failed',error=$1,completed_at=now(),
                     lease_owner=NULL, lease_expires_at=NULL
                 WHERE status IN ('leased','running') AND lease_expires_at<now()
                   AND COALESCE((checkpoint->>'toolsStarted')::boolean,false)
                 RETURNING id, thread_id, bot_id
             )
             SELECT id, thread_id, bot_id FROM doomed LIMIT 50",
        )
        .bind(INTERRUPTED_ERROR)
        .fetch_all(state.pool())
        .await
        .unwrap_or_default();
        for (orphan_run, orphan_thread, orphan_bot) in interrupted {
            report_run_failure(
                &state,
                &orphan_run,
                &orphan_thread,
                &orphan_bot,
                INTERRUPTED_ERROR,
            )
            .await;
        }
        let Ok(permit) = inflight.clone().try_acquire_owned() else {
            continue;
        };
        let queued: Result<Option<RetryCandidateRow>, _> = sqlx::query_as(
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
                    report_run_failure(&state, &run_id, &thread_id, &bot_id, &error).await;
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
    crate::monitor::record(
        state,
        run_id,
        "run",
        json!({"event": "started", "task": crate::monitor::snippet(prompt, 160)}),
    )
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
        connection_check_attempted: std::sync::Mutex::new(false),
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
    // The human answered a task that had paused to ask. Their message is an
    // answer, not a new request, so the loop feeds it in before the next model
    // turn instead of letting the run restart from scratch.
    let await_resume = checkpoint.get("awaitResume").is_some();
    if await_resume {
        let _ = sqlx::query("UPDATE runs SET checkpoint = checkpoint - 'awaitResume' WHERE id=$1")
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
    let goal_mode = goal_request(prompt).is_some();
    let goal_text = prompt
        .trim()
        .strip_prefix("/goal")
        .unwrap_or("")
        .trim()
        .to_string();
    let file_skill = if !resume_after_takeover && !goal_mode {
        crate::file_skills::slash(prompt, &state.data_dir)
    } else {
        None
    };
    let initial_prompt = if goal_mode {
        format!(
            "Execute this goal until it is verified complete:\n{}",
            goal_text
        )
    } else if let Some((skill, args)) = &file_skill {
        format!(
            "Run the /{} skill with these arguments: {}",
            skill.name, args
        )
    } else {
        prompt.to_string()
    };
    let mut first = if resume_after_takeover {
        vec![UserContent::text(TAKEOVER_RESUME_PROMPT)]
    } else {
        vec![UserContent::text(initial_prompt)]
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
        if let Some(skill) = if goal_mode || file_skill.is_some() {
            None
        } else {
            crate::skills::skill_for_prompt(state.pool(), bot_id, prompt).await
        } {
            first.push(UserContent::text(crate::skills::format_playbook_for_run(
                &skill,
            )));
            skill_check = Some(crate::skills::skill_check_hint(&skill));
        }
        if let Some((skill, args)) = &file_skill {
            first.push(UserContent::text(format!(
                "File skill /{} (read-only source, follow its instructions):\n{}\nArguments: {}",
                skill.name, skill.instructions, args
            )));
            skill_check = Some(format!("完成 /{} 的指令，並用工具驗證結果。", skill.name));
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
    let chat_only = !resume_after_takeover
        && !goal_mode
        && file_skill.is_none()
        && skill_check.is_none()
        && !workspace_file
        && is_plain_chat(prompt);
    if chat_only {
        // Memory is recalled separately and injected into the preamble below.
        // Do not expose even memory tools here: a plain greeting must be one
        // model call with no chance of accidentally invoking any capability.
        defs.clear();
        tracing::info!(run_id, "chat-only turn: all tools withheld");
        crate::monitor::record(
            state,
            run_id,
            "notice",
            json!({"text": "這一輪是對白，工具先收起來（不會動到電腦）。"}),
        )
        .await;
    }
    // Computer work keeps going until it verifies or explains a blocker.
    // Plain chat stays short so a confused model cannot burn budget.
    let goal_mode = goal_mode || !chat_only;
    let execution_mode = if chat_only {
        ExecutionMode::Bounded(4)
    } else {
        ExecutionMode::Goal
    };
    let turn_limit = match execution_mode {
        ExecutionMode::Goal => None,
        ExecutionMode::Bounded(limit) => Some(limit as i64),
    };
    let mut nudges: u32 = 0;
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
    if !resume_after_takeover
        && let (Some(saved_history), Some(saved_pending)) = (
            checkpoint.get("harnessHistory"),
            checkpoint.get("harnessPending"),
        )
        && let (Ok(restored), Ok(mut next)) = (
            serde_json::from_value::<Vec<Message>>(saved_history.clone()),
            serde_json::from_value::<Message>(saved_pending.clone()),
        )
    {
        history = restored;
        if let Message::User { content } = &mut next {
            content.push(UserContent::text("Resumed after a completed tool batch. Do not repeat completed actions. Observe current browser/desktop before any new mutation; prior element references may be stale."));
        }
        pending = next;
    }

    let mut final_text = String::new();
    let mut turns: u32 = checkpoint
        .get("harnessTurns")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(u32::MAX as u64) as u32;
    // Messages sent to this same thread while a /goal run is working are
    // steering input. Keep the run alive and deliver each new message once
    // before the next model turn instead of waiting for a second run to win
    // the bot lease.
    let mut steering_seq = checkpoint
        .get("steeringSeq")
        .and_then(Value::as_i64)
        .map(|seq| seq as i32)
        .unwrap_or(current_seq);
    let mut used_gui = false;
    let mut did_work = false;
    // One verification demand per run: enough to catch "I'm done" that isn't,
    // without trapping the model in an endless self-audit.
    let mut verified = false;
    // Round policy. A run gets no quota; it gets a watcher. `LoopGuard` looks
    // for the shape of going in circles - the same action, the same failure, a
    // long stretch with nothing new succeeding - and only the last-resort
    // breakers use turns or wall clock. Wall clock is measured per attempt: a
    // run that parked for three days waiting for a human starts fresh.
    let run_policy = RunPolicy::from_env();
    let mut guard = LoopGuard::with_watch(run_policy, turns);
    let attempt_clock = std::time::Instant::now();
    let memory = if ctx.memory_enabled {
        match state
            .memory
            .recall(state.pool(), actor, bot_id, prompt, None)
            .await
        {
            Ok(items) => state.memory.durable_block(&items),
            Err(error) => {
                crate::monitor::record(
                    state,
                    run_id,
                    "notice",
                    json!({"text": format!("記憶讀取失敗，這輪不用記憶：{error}")}),
                )
                .await;
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
    if goal_mode {
        preamble.push_str("\n\n");
        preamble.push_str(GOAL_INSTRUCTIONS);
    }
    if !chat_only {
        if let Some(index) = crate::skills::skills_preamble(&skills) {
            preamble.push_str("\n\n");
            preamble.push_str(&index);
        }
        let file_skill_index = crate::file_skills::index(&state.data_dir);
        if !file_skill_index.is_empty() {
            preamble.push_str("\n\n");
            preamble.push_str(&file_skill_index);
        }
        if let Ok(accounts) = crate::vault::list_on(state.pool(), actor, bot_id).await
            && !accounts.is_empty()
        {
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

    while execution_mode.allows_turn(turns) {
        turns = turns.saturating_add(1);
        if goal_mode || (await_resume && !resume_after_takeover) {
            let steering: Vec<(i32, String)> = sqlx::query_as(
                "SELECT seq, body FROM messages
                 WHERE thread_id=$1 AND role='user' AND seq>$2
                 ORDER BY seq ASC LIMIT 12",
            )
            .bind(thread_id)
            .bind(steering_seq)
            .fetch_all(state.pool())
            .await
            .map_err(|error| error.to_string())?;
            if let Some((last_seq, _)) = steering.last() {
                steering_seq = *last_seq;
                let guidance = steering
                    .iter()
                    .map(|(_, body)| body.trim())
                    .filter(|body| !body.is_empty())
                    .collect::<Vec<_>>();
                if !guidance.is_empty() {
                    let text = if goal_mode {
                        format!(
                            "The user added this guidance in the same goal thread. Incorporate it into the current goal and continue verifying the result:\n{}",
                            guidance.join("\n")
                        )
                    } else {
                        format!(
                            "The human answered your question about the paused task. Continue the original work from where it stopped with tools: do not repeat finished steps and do not start over.\n{}",
                            guidance.join("\n")
                        )
                    };
                    crate::monitor::record(
                        state,
                        run_id,
                        "notice",
                        json!({
                            "turn": turns,
                            "text": format!("收到新指示：{}", crate::monitor::snippet(&guidance.join(" / "), 160)),
                        }),
                    )
                    .await;
                    if let Message::User { content } = &mut pending {
                        content.push(UserContent::text(text));
                    }
                }
            }
        }
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
        // Watcher first: a checkpoint question belongs to the model turn that
        // is about to happen, and a derailment or breaker parks the run before
        // another model call is paid for.
        match guard.on_turn(turns, attempt_clock.elapsed()) {
            Verdict::Continue => {}
            Verdict::Reflect(text) => {
                tracing::info!(run_id, turn = turns, "round policy: coaching the run");
                crate::monitor::record(
                    state,
                    run_id,
                    "notice",
                    json!({
                        "turn": turns,
                        "text": format!("系統檢查點：{}", crate::monitor::snippet(&text, 160)),
                    }),
                )
                .await;
                if let Message::User { content } = &mut pending {
                    content.push(UserContent::text(text));
                }
            }
            Verdict::Halt { reason, note } => {
                let limit = match execution_mode {
                    ExecutionMode::Bounded(limit) => limit,
                    ExecutionMode::Goal => 0,
                };
                return pause_for_answer(
                    state,
                    bot_id,
                    thread_id,
                    run_id,
                    lease_owner,
                    &ctx,
                    PauseRequest {
                        reason,
                        draft: &final_text,
                        history: &history,
                        turns,
                        steering_seq,
                        limit,
                        note: Some(note),
                        screenshots,
                        screenshot_bytes,
                        used_gui,
                    },
                )
                .await;
            }
        }
        drop_history_screenshots(&mut history, &pending);
        set_run_progress(state, run_id, MODEL_STEP, turns, turn_limit).await;
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
            result = complete_with_retry(
                &model,
                pending.clone(),
                &preamble,
                &history,
                &defs,
                Trace { state, run_id, turn: turns },
            ) => result
        }?;
        let assistant = Message::Assistant {
            id: None,
            content: content.clone(),
        };
        history.push(pending.clone());
        history.push(assistant);

        let mut calls = Vec::new();
        let mut turn_text = String::new();
        for item in &content {
            match item {
                AssistantContent::Text(text) => {
                    final_text.push_str(&text.text);
                    turn_text.push_str(&text.text);
                }
                AssistantContent::ToolCall(call) => calls.push(call.clone()),
                _ => {}
            }
        }
        let model_elapsed = model_started.elapsed().as_millis() as u64;
        tracing::info!(
            run_id,
            turn = turns,
            elapsed_ms = model_elapsed,
            tool_calls = calls.len(),
            "model turn"
        );
        crate::monitor::record(
            state,
            run_id,
            "model",
            json!({
                "turn": turns,
                "elapsedMs": model_elapsed,
                "toolCalls": calls.len(),
                "text": crate::monitor::snippet(&turn_text, 200),
            }),
        )
        .await;
        if calls.is_empty() {
            // A model that quits a playbook early, or parrots an earlier reply
            // instead of describing the current screen, gets pushed back to
            // the tools a few times before the text is accepted.
            let parroted = earlier_replies
                .iter()
                .any(|earlier| earlier == final_text.trim());
            let declared_done = goal_mode
                && matches!(
                    goal_outcome(&final_text),
                    GoalOutcome::Complete | GoalOutcome::NeedsInput
                );
            let nudge_limit = if goal_mode {
                MAX_NUDGES_GOAL
            } else {
                MAX_NUDGES_PLAIN
            };
            let mut verify_chosen = false;
            let nudge = if goal_mode {
                match goal_outcome(&final_text) {
                    GoalOutcome::Continue => Some(GOAL_CONTINUE.to_string()),
                    GoalOutcome::Complete | GoalOutcome::NeedsInput => None,
                }
            } else if parroted {
                Some(
                    "Your reply repeats an earlier message word for word, so it cannot describe the current screen. Below is what the screen shows RIGHT NOW. Act on it with a tool call. Waiting is done by calling wait or by clicking the control (the click waits for it to enable), never by replying. Reply in text only once the task is finished or you are truly blocked (say why).".to_string(),
                )
            } else if let Some(check) = &skill_check {
                Some(format!(
                    "The run is not finished; your text reply ended nothing but your own turn. Check: {check}\nBelow is the current screen. If the next control is [disabled], click it anyway — the click waits up to 45s for it to enable — or call wait. Ids marked [below viewport] scroll automatically. Only reply in text when the check passes or you are truly blocked, and then say exactly what blocks you."
                ))
            } else if did_work && !verified && !asks_for_input(&final_text) {
                // First stop attempt of a run that already moved something: make
                // it prove the work is finished, or ask the human properly.
                verify_chosen = true;
                Some(VERIFY_BEFORE_DONE.to_string())
            } else {
                None
            };
            let may_nudge =
                nudges < nudge_limit && execution_mode.allows_turn(turns.saturating_add(2));
            if let Some(text) = may_nudge.then_some(nudge).flatten() {
                nudges = nudges.saturating_add(1);
                verified |= verify_chosen;
                tracing::info!(
                    run_id,
                    turn = turns,
                    parroted,
                    verify = verify_chosen,
                    "nudging model back to tools"
                );
                crate::monitor::record(
                    state,
                    run_id,
                    "notice",
                    json!({
                        "turn": turns,
                        "text": if verify_chosen {
                            format!("要求模型先證明做完了才准結束（第 {nudges} 次攔下）。")
                        } else if parroted {
                            format!("模型重複了舊的回答，叫它看現在畫面繼續做（第 {nudges} 次）。")
                        } else {
                            format!("模型想用一句話收尾，叫它用工具繼續（第 {nudges} 次）。")
                        },
                    }),
                )
                .await;
                earlier_replies.push(final_text.trim().to_string());
                final_text.clear();
                let mut content = vec![UserContent::text(text)];
                if used_gui || skill_check.is_some() {
                    prepare_run_computer(state, actor, bot_id, run_id, &ctx, true).await?;
                }
                if (used_gui || skill_check.is_some()) && ctx.gui_block.lock().unwrap().is_none() {
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
            // Nothing left to retry: report where the work stands and let the
            // human decide instead of ending the task in silence.
            let stalled = nudges >= nudge_limit && !declared_done;
            if let Some(reason) = stop_reason(execution_mode, turns, &final_text, did_work, stalled)
            {
                let limit = match execution_mode {
                    ExecutionMode::Bounded(limit) => limit,
                    ExecutionMode::Goal => 0,
                };
                return pause_for_answer(
                    state,
                    bot_id,
                    thread_id,
                    run_id,
                    lease_owner,
                    &ctx,
                    PauseRequest {
                        reason,
                        draft: &final_text,
                        history: &history,
                        turns,
                        steering_seq,
                        limit,
                        note: None,
                        screenshots,
                        screenshot_bytes,
                        used_gui,
                    },
                )
                .await;
            }
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
                    | "connection_check"
                    | "wait"
                    | "use_saved_login"
                    | "request_takeover"
            );
            did_work = true;
            let step = describe_step(&name, &call.function.arguments);
            set_run_progress(state, run_id, &step, turns, turn_limit).await;
            let fence=sqlx::query("UPDATE runs SET checkpoint=COALESCE(checkpoint,'{}'::jsonb)||jsonb_build_object('toolsStarted',true) WHERE id=$1 AND lease_owner=$2 AND status='running'")
                .bind(run_id).bind(lease_owner).execute(state.pool()).await.map_err(|e| e.to_string())?;
            if fence.rows_affected() != 1 {
                return Err("run lease lost before tool dispatch".into());
            }
            let tool_started = std::time::Instant::now();
            let mut tool_timed_out = false;
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
                    // A slow tool is a normal turn, not a dead run: say the
                    // effects are unknown and let the model re-observe.
                    Err(_) => {
                        tool_timed_out = true;
                        ToolOutcome {
                        text: format!("tool {name} timed out after 150 seconds. Its effects are unknown: observe the current screen or files before anything else, and never repeat a step that already worked."),
                        image: None,
                        pause: false,
                        blocks: Vec::new(),
                        }
                    }
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
            let status = tool_status(tool_timed_out, outcome.pause, &outcome.text);
            crate::monitor::record(
                state,
                run_id,
                "tool",
                json!({
                    "turn": turns,
                    "name": name.clone(),
                    "step": step.clone(),
                    "status": status,
                    "elapsedMs": tool_started.elapsed().as_millis() as u64,
                    "snippet": crate::monitor::snippet(&outcome.text, 200),
                }),
            )
            .await;
            match guard.on_action(&ActionObserved {
                name: call.function.name.as_str(),
                args: &call.function.arguments,
                label: &step,
                ok: status == "ok",
                turn: turns,
                changes_state: action_changes_state(&name, &call.function.arguments),
            }) {
                Verdict::Continue => {}
                Verdict::Reflect(text) => {
                    tracing::info!(run_id, turn = turns, step = %step, "round policy: coaching mid-turn");
                    crate::monitor::record(
                        state,
                        run_id,
                        "notice",
                        json!({
                            "turn": turns,
                            "text": format!("系統提示：{}", crate::monitor::snippet(&text, 160)),
                        }),
                    )
                    .await;
                    results.push(UserContent::text(text));
                }
                Verdict::Halt { reason, note } => {
                    let limit = match execution_mode {
                        ExecutionMode::Bounded(limit) => limit,
                        ExecutionMode::Goal => 0,
                    };
                    return pause_for_answer(
                        state,
                        bot_id,
                        thread_id,
                        run_id,
                        lease_owner,
                        &ctx,
                        PauseRequest {
                            reason,
                            draft: &final_text,
                            history: &history,
                            turns,
                            steering_seq,
                            limit,
                            note: Some(note),
                            screenshots,
                            screenshot_bytes,
                            used_gui,
                        },
                    )
                    .await;
                }
            }
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
        save_harness_checkpoint(
            state,
            run_id,
            lease_owner,
            &history,
            &pending,
            turns,
            steering_seq,
        )
        .await?;
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
    // The turn budget ran out straight after a tool batch, so the model never
    // got a turn to explain itself. Park the run with its state instead of
    // delivering an empty answer that looks like a finished task.
    if let Some(reason) = stop_reason(execution_mode, turns, "", did_work, false) {
        let limit = match execution_mode {
            ExecutionMode::Bounded(limit) => limit,
            ExecutionMode::Goal => 0,
        };
        return pause_for_answer(
            state,
            bot_id,
            thread_id,
            run_id,
            lease_owner,
            &ctx,
            PauseRequest {
                reason,
                draft: &final_text,
                history: &history,
                turns,
                steering_seq,
                limit,
                note: None,
                screenshots,
                screenshot_bytes,
                used_gui,
            },
        )
        .await;
    }
    let needs_input = goal_mode && goal_outcome(&final_text) == GoalOutcome::NeedsInput;
    if needs_input {
        let next = Message::User {
            content: vec![UserContent::text(
                "The goal was paused for required user input. Read the user's new information and continue from completed work.",
            )],
        };
        save_harness_checkpoint(
            state,
            run_id,
            lease_owner,
            &history,
            &next,
            turns,
            steering_seq,
        )
        .await?;
    }
    let final_text = final_text
        .replace("[GOAL_COMPLETE]", "")
        .replace("[GOAL_BLOCKED]", "")
        .trim()
        .to_string();
    append_bot_message(state, thread_id, run_id, bot_id, &final_text).await?;
    let completed = sqlx::query(
        "UPDATE runs
         SET status=$3, completed_at=CASE WHEN $3='completed' THEN now() ELSE NULL END, updated_at=now(),
             lease_owner=NULL, lease_expires_at=NULL
         WHERE id=$1 AND lease_owner=$2 AND status='running'",
    )
    .bind(run_id)
    .bind(lease_owner)
    .bind(if needs_input { "waiting_input" } else { "completed" })
    .execute(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    if completed.rows_affected() != 1 {
        return Err("run lease was lost before completion".into());
    }
    crate::monitor::record(
        state,
        run_id,
        "run",
        json!({
            "event": if needs_input { "waiting_input" } else { "completed" },
            "reason": if needs_input { Some(final_text.as_str()) } else { None },
            "turns": turns,
        }),
    )
    .await;
    let click_misses = *ctx.click_misses.lock().unwrap();
    let takeover = *ctx.takeover_requested.lock().unwrap();
    record_run_metrics(
        state,
        thread_id,
        run_id,
        if needs_input {
            "run.paused"
        } else {
            "run.completed"
        },
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

/// Where a helper that is not the run loop itself writes its diagnostics.
#[derive(Clone, Copy)]
struct Trace<'a> {
    state: &'a AppState,
    run_id: &'a str,
    turn: u32,
}

async fn complete_with_retry(
    model: &DynModel,
    pending: Message,
    preamble: &str,
    history: &[Message],
    defs: &[ToolDefinition],
    trace: Trace<'_>,
) -> Result<Vec<AssistantContent>, String> {
    let mut last = String::new();
    for attempt in 0..3 {
        if attempt > 0 {
            let failure = crate::monitor::classify_run_error(&last);
            set_run_step(
                trace.state,
                trace.run_id,
                &format!(
                    "{} 正在自動重試模型（第 {}/3 次）；保留已完成的操作。",
                    failure.headline,
                    attempt + 1
                ),
            )
            .await;
        }
        let started = std::time::Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(165),
            complete_once(model, pending.clone(), preamble, history, defs),
        )
        .await;
        match result {
            Ok(Ok(content)) => return Ok(content),
            Ok(Err(error)) => {
                crate::monitor::record(
                    trace.state,
                    trace.run_id,
                    "retry",
                    json!({
                        "turn": trace.turn,
                        "attempt": attempt + 1,
                        "error": error,
                        "gaveUp": attempt == 2 || !retryable_run_error(&error),
                    }),
                )
                .await;
                if !retryable_run_error(&error) {
                    return Err(error);
                }
                tracing::warn!(
                    attempt = attempt + 1,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "model attempt failed: {error}"
                );
                last = error;
            }
            Err(_) => {
                let error = "model request timed out after 165 seconds";
                crate::monitor::record(
                    trace.state,
                    trace.run_id,
                    "retry",
                    json!({"turn": trace.turn, "attempt": attempt + 1, "error": error, "gaveUp": attempt == 2}),
                )
                .await;
                tracing::warn!(
                    attempt = attempt + 1,
                    "model attempt exceeded its 165 second budget"
                );
                last = error.into();
            }
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
    // One budget per attempt, kept just below the caller's, so the model's own
    // timeout is what gets reported instead of a generic outer cancellation.
    let response = tokio::time::timeout(Duration::from_secs(150), model.completion(request))
        .await
        .map_err(|_| "AI 回應逾時（150 秒）".to_string())?
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

/// A checkpoint that cannot be written freezes the run: progress stops and every
/// later hiccup turns fatal. Drop the oldest turns and cap long tool dumps until
/// the row fits again.
fn shrink_checkpoint(history: &mut Vec<Message>, pending: &mut Message) {
    const LIMIT: usize = 768 * 1024;
    const MAX_PART: usize = 48 * 1024;
    let cap_parts = |message: &mut Message| {
        let Message::User { content } = message else {
            return;
        };
        for part in content.iter_mut() {
            let UserContent::Text(text) = part else {
                continue;
            };
            if text.text.len() > MAX_PART {
                let mut cut = MAX_PART;
                while cut > 0 && !text.text.is_char_boundary(cut) {
                    cut -= 1;
                }
                text.text.truncate(cut);
                text.text.push_str("\n…(truncated)");
            }
        }
    };
    cap_parts(pending);
    history.iter_mut().for_each(cap_parts);
    let fits = |turns: &[Message]| {
        json!({"harnessHistory": turns, "harnessPending": pending})
            .to_string()
            .len()
            <= LIMIT
    };
    while !fits(history) && history.len() > 1 {
        history.remove(0);
    }
}

async fn save_harness_checkpoint(
    state: &AppState,
    run_id: &str,
    owner: &str,
    history: &[Message],
    pending: &Message,
    turns: u32,
    steering_seq: i32,
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
    shrink_checkpoint(&mut history, &mut pending);
    let value = json!({"harnessHistory":history,"harnessPending":pending,"toolsStarted":false,"harnessTurns":turns,"steeringSeq":steering_seq});
    // Only a checkpoint that still cannot fit after shrinking fails closed, and
    // loudly: the run keeps its uncertain-effects flag instead of freezing.
    if value.to_string().len() > 1024 * 1024 {
        tracing::error!(
            run_id,
            "harness checkpoint still exceeds 1 MB after shrinking"
        );
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
    // A stop with nothing to say is a bug, not a message. An empty assistant
    // bubble is exactly what made "finished" and "died mid-task" look alike.
    let has_blocks = blocks.as_array().is_some_and(|items| !items.is_empty());
    if body.trim().is_empty() && !has_blocks {
        return Ok(());
    }
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
        tokio::time::sleep(Duration::from_millis(750)).await;
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
            crate::monitor::record(
                state,
                run_id,
                "run",
                json!({"event": "paused", "reason": "takeover", "turns": turns}),
            )
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

/// Work that stopped before it was verified as finished. The run parks in
/// `waiting_input` with a one-click question and keeps its harness state, so the
/// human's next message resumes exactly where the tools left off.
struct PauseRequest<'a> {
    reason: StopReason,
    draft: &'a str,
    history: &'a [Message],
    turns: u32,
    steering_seq: i32,
    limit: u32,
    /// Concrete evidence from the round policy, shown instead of a generic
    /// category when the run parked on a loop.
    note: Option<String>,
    screenshots: u32,
    screenshot_bytes: u64,
    used_gui: bool,
}

const PAUSED_PENDING: &str = "This run is paused for the human's answer. When their reply arrives, continue the work that has already started: observe the current screen before any new mutation and never repeat finished steps.";

async fn pause_for_answer(
    state: &AppState,
    bot_id: &str,
    thread_id: &str,
    run_id: &str,
    lease_owner: &str,
    ctx: &ToolCtx,
    req: PauseRequest<'_>,
) -> Result<(), String> {
    let draft = req.draft.replace(NEEDS_INPUT_MARKER, "").trim().to_string();
    // The round policy arrives with concrete evidence ("the same click six
    // times in a row"), which always beats a category name for the human.
    let stall = req.note.clone().unwrap_or_else(|| match req.reason {
        StopReason::BudgetExhausted => "已達本輪執行上限".to_string(),
        StopReason::MidTaskText => {
            "模型多次未能提供可執行的下一步，系統已要求它重新確認並繼續，但仍無法推進".to_string()
        }
        StopReason::LoopDetected => "重複同一個動作，任務沒有新的進展".to_string(),
    });
    let draft = if draft.is_empty() {
        match req.reason {
            StopReason::BudgetExhausted => format!(
                "我在這個任務上用了 {} 輪，還沒有做到可以幫你確認完成的地步，先停在目前的畫面。要我繼續嗎？\n\n任務尚未確認完成。停止原因：{}。已保存操作進度；回覆下一步指示或接管確認現況後可繼續。",
                req.turns, stall
            ),
            StopReason::MidTaskText => format!(
                "模型多次未提供可執行的下一步，系統無法確認任務已完成。請補充下一步指示或接管確認現況後繼續。\n\n任務尚未確認完成。停止原因：{}。已保存操作進度；回覆下一步指示或接管確認現況後可繼續。",
                stall
            ),
            StopReason::LoopDetected => format!(
                "我在這個任務上用了 {} 輪，一直在同一個動作上打轉，先停在目前的畫面。要我換個做法繼續嗎？\n\n任務尚未確認完成。停止原因：{}。已保存操作進度；告訴我該怎麼做，或直接接管電腦。",
                req.turns, stall
            ),
        }
    } else if !asks_for_input(req.draft) {
        // A model's optimistic draft must not hide a harness-detected stall.
        format!(
            "{draft}\n\n任務尚未確認完成。停止原因：{stall}。已保存操作進度；回覆下一步指示或接管確認現況後可繼續。"
        )
    } else {
        draft
    };
    let next = Message::User {
        content: vec![UserContent::text(PAUSED_PENDING)],
    };
    save_harness_checkpoint(
        state,
        run_id,
        lease_owner,
        req.history,
        &next,
        req.turns,
        req.steering_seq,
    )
    .await?;
    let paused = sqlx::query(
        "UPDATE runs
            SET status='waiting_input', lease_owner=NULL, lease_expires_at=NULL, updated_at=now(),
                checkpoint=COALESCE(checkpoint,'{}'::jsonb)||jsonb_build_object('awaitResume',
                    jsonb_build_object('reason',$3,'turns',$4,'limit',$5))
         WHERE id=$1 AND lease_owner=$2 AND status='running'",
    )
    .bind(run_id)
    .bind(lease_owner)
    .bind(req.reason.as_str())
    .bind(req.turns as i64)
    .bind(req.limit as i64)
    .execute(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    if paused.rows_affected() != 1 {
        return Err("run lease was lost before pausing".into());
    }
    crate::monitor::record(
        state,
        run_id,
        "run",
        json!({
            "event": "paused",
            "reason": draft,
            "turns": req.turns,
            "limit": req.limit,
        }),
    )
    .await;
    append_bot_message_with(
        state,
        thread_id,
        run_id,
        bot_id,
        &draft,
        json!([{
            "kind": "resume",
            "reason": req.reason.as_str(),
            "turns": req.turns,
            "limit": req.limit,
        }]),
    )
    .await?;
    let click_misses = *ctx.click_misses.lock().unwrap();
    record_run_metrics(
        state,
        thread_id,
        run_id,
        "run.paused",
        req.turns,
        req.screenshots,
        req.screenshot_bytes,
        click_misses,
        req.used_gui,
        false,
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
    tracing::info!(
        run_id,
        reason = req.reason.as_str(),
        turns = req.turns,
        "run paused for an answer"
    );
    Ok(())
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

/// Plain doing-words that never show up in a greeting. `prompt_needs_desktop`
/// cannot name every app, site, or file, so a request is also a task when it
/// asks for an action to be performed on something.
fn has_work_verb(normalized: &str) -> bool {
    const VERBS: &[&str] = &[
        "整理",
        "彙整",
        "彙總",
        "存到",
        "存進",
        "存入",
        "儲存",
        "存檔",
        "建立",
        "新增",
        "產生",
        "產出",
        "改名",
        "重命名",
        "移動",
        "複製",
        "刪除",
        "刪掉",
        "翻譯",
        "摘要",
        "總結",
        "歸納",
        "比對",
        "比較",
        "填入",
        "填寫",
        "提交",
        "送出",
        "歸檔",
        "轉檔",
        "轉換",
        "壓縮",
        "解凍",
        "分割",
        "合併",
        "報名",
        "預訂",
        "預約",
        "訂閱",
        "退訂",
        "追蹤",
        "回覆",
        "寄送",
        "領取",
        "打卡",
        "簽到",
        "紀錄",
        "記錄",
        "檢查",
        "測試",
        "執行",
        "下載",
        "上傳",
        "安裝",
        "更新",
        "設定",
        "organize",
        "rename",
        "move ",
        "copy",
        "delete",
        "save",
        "create",
        "generate",
        "download",
        "upload",
        "translate",
        "summarize",
        "submit",
        "book",
        "reserve",
        "archive",
        "convert",
        "compare",
        "send",
        "reply",
        "schedule",
        "install",
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
    if EXACT.contains(&normalized) {
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
    // Withholding every tool is the most damaging mistake this function can
    // make: the task silently degrades into a paragraph and looks like the run
    // gave up. Only an obvious pleasantry counts as chat; anything with a work
    // verb keeps its tools.
    if chars <= 12 && !has_task_verb(&normalized) && !has_work_verb(&normalized) {
        return true;
    }
    is_chat_intent(&normalized) && chars <= 24 && !has_work_verb(&normalized)
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
            | "connection_check"
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
            | "connection_check"
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
        "connection_check" => "嘗試連線驗證並確認結果".to_string(),
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
        "shell" => {
            // The terminal does four different things; the feed says which.
            let session = get("session").filter(|name| !name.trim().is_empty() && *name != "main");
            let suffix = session.map(|name| format!(" ·{name}")).unwrap_or_default();
            if args.get("reset").and_then(Value::as_bool).unwrap_or(false) {
                format!("重開終端機{suffix}")
            } else if let Some(keys) = get("keys") {
                format!("輸入 {}{suffix}", short(Some(keys), 20))
            } else {
                match get("command")
                    .or(get("cmd"))
                    .filter(|line| !line.trim().is_empty())
                {
                    Some(command) => format!("{}{suffix}", short(Some(command), 60)),
                    None => format!("讀終端機{suffix}"),
                }
            }
        }
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

/// The error the claim sweep writes when a worker died holding a tool open.
const INTERRUPTED_ERROR: &str =
    "Worker interrupted after tool execution; inspect current state before continuing.";

/// Step plus turn counter: what the bubble header reads out of the checkpoint.
pub(crate) async fn set_run_progress(
    state: &AppState,
    run_id: &str,
    step: &str,
    turn: u32,
    limit: Option<i64>,
) {
    let result = sqlx::query(
        "UPDATE runs SET checkpoint = COALESCE(checkpoint, '{}'::jsonb)
                || jsonb_build_object('step', $2::text, 'stepAt', now(),
                                      'turn', $3::bigint, 'turnLimit', $4::bigint),
                updated_at = now()
         WHERE id = $1",
    )
    .bind(run_id)
    .bind(step)
    .bind(turn as i64)
    .bind(limit)
    .execute(state.pool())
    .await;
    if let Err(error) = result {
        tracing::warn!(run_id, "failed to record run progress: {error}");
    }
}

/// Tell the human a run died, in their language, with the one next action. The
/// raw error stays available through the run activity endpoint.
async fn report_run_failure(
    state: &AppState,
    run_id: &str,
    thread_id: &str,
    bot_id: &str,
    error: &str,
) {
    let progress: Option<(Option<String>, Option<i64>)> = sqlx::query_as(
        "SELECT checkpoint->>'step', (checkpoint->>'turn')::bigint FROM runs WHERE id=$1",
    )
    .bind(run_id)
    .fetch_optional(state.pool())
    .await
    .unwrap_or(None);
    let (last_step, turn) = progress.unwrap_or((None, None));
    let failure = crate::monitor::classify_run_error(error);
    let body = crate::monitor::failure_message(&failure, last_step.as_deref(), turn);
    let _ = append_bot_message_with(
        state,
        thread_id,
        run_id,
        bot_id,
        &body,
        json!([{
            "kind": "error",
            "code": failure.code,
            "retryable": failure.retryable,
            "runId": run_id,
            "turn": turn,
            "step": last_step,
        }]),
    )
    .await;
    crate::monitor::record(
        state,
        run_id,
        "run",
        json!({"event": "failed", "error": error}),
    )
    .await;
}

/// Display-only reading of a tool result. Tools report failure in prose, so the
/// trail marks the shapes it recognises rather than pretending to know more.
fn tool_status(timed_out: bool, pause: bool, text: &str) -> &'static str {
    if timed_out {
        return "timed_out";
    }
    if pause {
        return "paused";
    }
    const FAILURES: [&str; 14] = [
        "error",
        "failed",
        "failure",
        "unable to",
        "cannot ",
        "can't ",
        "timed out",
        "denied",
        "no such",
        "exception",
        "not available",
        "失敗",
        "錯誤",
        "無法",
    ];
    let head: String = text.to_lowercase().chars().take(160).collect();
    if FAILURES.iter().any(|needle| head.contains(needle)) {
        "error"
    } else {
        "ok"
    }
}

/// Does this tool call change the world? The round policy only counts
/// mutating actions, so polling the screen, waiting, or reading a file can
/// never look like a derailment on its own.
fn action_changes_state(name: &str, args: &Value) -> bool {
    match name {
        "computer_act" | "shell" | "write_file" | "launch_app" | "open_path"
        | "create_schedule" | "cancel_schedule" | "remember" | "forget_memory"
        | "use_saved_login" | "request_takeover" => true,
        "browser" => matches!(
            args.get("action").and_then(Value::as_str),
            Some("click") | Some("type") | Some("navigate") | Some("press")
        ),
        // Observations, waits, reads, and connected-service calls (mcp_*) are
        // allowed to repeat: they are how a run learns what changed.
        _ => false,
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
        RunHalt, SCREENSHOT_CAPTION, TAKEOVER_RESUME_PROMPT, describe_step,
        drop_history_screenshots, halt_from_status, history_window_start, is_plain_chat,
        retryable_run_error, screenshot_parts, tool_needs_gui, tool_needs_sandbox, tool_status,
    };
    use rig_core::completion::message::{Message, UserContent};
    use serde_json::json;

    #[test]
    fn takeover_resume_invalidates_pre_handoff_refs() {
        assert!(
            TAKEOVER_RESUME_PROMPT
                .contains("Element ids and page refs from before the handoff are invalid")
        );
        assert!(TAKEOVER_RESUME_PROMPT.contains("fresh observation"));
        assert!(tool_needs_gui("browser"));
        assert!(tool_needs_gui("computer_observe"));
    }

    #[test]
    fn the_trail_reads_a_tool_result_as_ok_error_timeout_or_pause() {
        assert_eq!(tool_status(false, false, "clicked element #12"), "ok");
        assert_eq!(
            tool_status(false, false, "Error: selector not found"),
            "error"
        );
        assert_eq!(tool_status(false, false, "操作失敗：視窗關閉"), "error");
        assert_eq!(tool_status(true, false, "tool timed out"), "timed_out");
        assert_eq!(tool_status(false, true, "需要人先登入"), "paused");
    }

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
    fn a_work_verb_outranks_a_short_prompt() {
        // The old length rule silently stripped every tool from a short
        // imperative; that is the "it stopped halfway" bug in its purest form.
        for prompt in [
            "把這張圖壓縮到 800px",
            "把報價整理成表格",
            "翻譯這段",
            "幫我把檔案改名",
            "幫我把这份報告存成 pdf",
            "rename the screenshots",
            "submit the form",
        ] {
            assert!(!is_plain_chat(prompt), "{prompt} must keep its tools");
        }
        for prompt in ["你好", "YouTube 是什麼？", "今天天氣如何", "寫一首詩"] {
            assert!(is_plain_chat(prompt), "{prompt} should stay in chat");
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
        assert_eq!(describe_step("shell", &json!({})), "shell: 讀終端機");
        assert_eq!(
            describe_step("shell", &json!({"keys":"C-c","session":"build"})),
            "shell: 輸入 C-c ·build"
        );
        assert_eq!(
            describe_step("shell", &json!({"command":"make","session":"main"})),
            "shell: make"
        );
        assert_eq!(
            describe_step("shell", &json!({"reset":true,"session":"build"})),
            "shell: 重開終端機 ·build"
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
