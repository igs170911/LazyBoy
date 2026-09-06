use std::path::PathBuf;
use std::time::Duration;

use chrono::{TimeDelta, Utc};
use lazyboy_contracts::{
    BrowserProfileMode, ComputerCapabilities, ComputerMode, ComputerState, ComputerStatus,
    ControlHolder, DEFAULT_SCREEN_HEIGHT, DEFAULT_SCREEN_WIDTH,
};
use lazyboy_control::{
    AdapterContext, CommandRequest, EnsureScreenRequest, ProvisionRequest, admit_gui,
    admit_new_screen, browser_profile_path, execution_blocks_user_takeover, profile_lock_key,
    screen_layout, team_bot_workspace_directory, user_holds_control,
};
use uuid::Uuid;

use crate::db::{
    Actor, ComputerRow, ScreenRow, parse_holder, parse_kind, parse_mode, parse_profile_mode,
    parse_run_status, parse_state,
};
use crate::state::AppState;

pub const STEP_BOOTING: &str = "電腦啟動中";
pub const STEP_WAKING: &str = "喚醒中";
pub const STEP_HANDOFF: &str = "換手中";

pub fn status_from(
    bot_id: &str,
    computer: &ComputerRow,
    screen: Option<&ScreenRow>,
    busy_bot_name: Option<String>,
) -> ComputerStatus {
    let control_holder = screen
        .map(|row| parse_holder(&row.control_holder))
        .unwrap_or_else(|| parse_holder(&computer.control_holder));
    let control_bot_id = screen
        .and_then(|row| {
            if parse_holder(&row.control_holder) == ControlHolder::User {
                Some(row.bot_id.clone())
            } else {
                None
            }
        })
        .or_else(|| computer.control_bot_id.clone());
    ComputerStatus {
        bot_id: bot_id.to_string(),
        mode: parse_mode(&computer.scope),
        kind: parse_kind(&computer.kind),
        state: parse_state(&computer.state),
        control_holder,
        control_bot_id,
        takeover_requested: false,
        screen_available: computer.provider_ref.is_some()
            && computer.state == "running"
            && screen.is_some(),
        screen_width: DEFAULT_SCREEN_WIDTH,
        screen_height: DEFAULT_SCREEN_HEIGHT,
        home_revision: Some(computer.home_revision.clone()),
        busy_bot_name,
        busy_session_id: None,
        busy_run_id: None,
        busy_step: None,
        using_computer: false,
        waiting_run_id: None,
        waiting_session_id: None,
        queued_runs: 0,
        multi_screen: true,
        screen_id: screen.map(|row| row.id.clone()),
        display: screen.map(|row| row.display.clone()),
        profile_mode: screen
            .map(|row| parse_profile_mode(&row.profile_mode))
            .unwrap_or_else(|| parse_profile_mode(&computer.browser_profile_mode)),
    }
}

pub fn home_path(data_dir: &str, home_key: &str) -> PathBuf {
    PathBuf::from(data_dir).join("homes").join(home_key)
}

pub fn adapter_context(actor: &Actor, bot_id: &str, operation: &str) -> AdapterContext {
    AdapterContext {
        operation_id: operation.into(),
        space_id: actor.space_id.clone(),
        user_id: actor.user_id.clone(),
        bot_id: Some(bot_id.into()),
        ..Default::default()
    }
}

pub fn adapter_context_for(
    actor: &Actor,
    bot_id: &str,
    operation: &str,
    screen: Option<&ScreenRow>,
    run_id: Option<&str>,
) -> AdapterContext {
    let mut ctx = adapter_context(actor, bot_id, operation);
    ctx.run_id = run_id.map(str::to_string);
    if let Some(screen) = screen {
        ctx.screen_id = Some(screen.id.clone());
        ctx.screen_slot = Some(screen.slot as u32);
        ctx.display = Some(screen.display.clone());
        ctx.profile_path = Some(screen.profile_path.clone());
        ctx.screen_lease_id = screen.execution_run_id.clone();
    }
    ctx
}

pub struct BoundScreen {
    pub row: Option<ScreenRow>,
    pub gui_block: Option<String>,
}

pub async fn ensure_bot_screen(
    state: &AppState,
    actor: &Actor,
    bot_id: &str,
    computer: &ComputerRow,
    run_id: Option<&str>,
) -> Result<BoundScreen, String> {
    let Some(computer_ref) = computer_ref(computer) else {
        return Err("computer is not running".into());
    };
    let caps = state
        .sandbox
        .capabilities(&computer_ref, &adapter_context(actor, bot_id, "caps"))
        .await
        .unwrap_or(ComputerCapabilities { multi_screen: true });
    let existing = state
        .db
        .get_screen(&computer.id, bot_id)
        .await
        .map_err(|error| error.to_string())?;
    let used: Vec<u32> = state
        .db
        .list_screen_slots(&computer.id)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|slot| slot as u32)
        .collect();
    let slot = match admit_new_screen(&caps, &used, existing.as_ref().map(|row| row.slot as u32)) {
        Ok(slot) => slot,
        Err(block) => {
            return Ok(BoundScreen {
                row: existing,
                gui_block: Some(block.message()),
            });
        }
    };
    let layout = screen_layout(slot).map_err(|error| error.to_string())?;
    let profile_mode = parse_profile_mode(&computer.browser_profile_mode);
    let profile_path = browser_profile_path(profile_mode, bot_id, run_id);
    let row = if let Some(existing) = existing {
        if existing.profile_path != profile_path && profile_mode == BrowserProfileMode::PerTask {
            sqlx::query(
                "UPDATE computer_screens SET profile_path = $2, updated_at = now() WHERE id = $1",
            )
            .bind(&existing.id)
            .bind(&profile_path)
            .execute(state.pool())
            .await
            .map_err(|error| error.to_string())?;
        }
        state
            .db
            .get_screen(&computer.id, bot_id)
            .await
            .map_err(|error| error.to_string())?
            .unwrap_or(existing)
    } else {
        let id = Uuid::new_v4().to_string();
        let inserted = sqlx::query_as::<_, ScreenRow>(
            "INSERT INTO computer_screens (
                id, computer_id, bot_id, slot, display, view_port, profile_mode, profile_path
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
             ON CONFLICT (computer_id, bot_id) DO UPDATE SET updated_at = now()
             RETURNING id, computer_id, bot_id, slot, display, view_port, profile_mode, profile_path,
                       control_holder, control_lease_id, control_lease_expires_at, execution_run_id,
                       execution_lease_expires_at, execution_fence",
        )
        .bind(&id)
        .bind(&computer.id)
        .bind(bot_id)
        .bind(slot as i32)
        .bind(&layout.display)
        .bind(layout.view_port as i32)
        .bind(profile_mode.as_str())
        .bind(&profile_path)
        .fetch_one(state.pool())
        .await;
        match inserted {
            Ok(row) => row,
            Err(error)
                if error
                    .to_string()
                    .contains("computer_screens_computer_id_slot") =>
            {
                return Ok(BoundScreen {
                    row: None,
                    gui_block: Some(
                        admit_new_screen(&caps, &used, None)
                            .err()
                            .map(|block| block.message())
                            .unwrap_or_else(|| lazyboy_contracts::TEAM_SCREENS_FULL.to_string()),
                    ),
                });
            }
            Err(error) => return Err(error.to_string()),
        }
    };
    let ctx = adapter_context_for(actor, bot_id, "screen", Some(&row), run_id);
    let request = EnsureScreenRequest {
        slot: row.slot as u32,
        profile_path: row.profile_path.clone(),
        bot_id: bot_id.to_string(),
    };
    let mut last_error = None;
    for attempt in 0..8 {
        match state
            .sandbox
            .ensure_screen(&computer_ref, request.clone(), &ctx)
            .await
        {
            Ok(_) => {
                last_error = None;
                break;
            }
            Err(error) => {
                let busy = error.to_string().contains("busy starting");
                last_error = Some(error);
                if !busy || attempt == 7 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(400)).await;
            }
        }
    }
    if let Some(error) = last_error {
        tracing::warn!("ensure screen {}: {error}", row.display);
        return Err(format!("ensure screen {}: {error}", row.display));
    }
    Ok(BoundScreen {
        row: Some(row),
        gui_block: None,
    })
}

async fn restore_computer_screens(
    state: &AppState,
    actor: &Actor,
    computer: &ComputerRow,
    skip_bot_id: &str,
) {
    let Ok(screens) = state.db.list_screens(&computer.id).await else {
        return;
    };
    let Some(computer_ref) = computer_ref(computer) else {
        return;
    };
    for screen in screens {
        if screen.bot_id == skip_bot_id {
            continue;
        }
        let ctx = adapter_context_for(actor, &screen.bot_id, "screen", Some(&screen), None);
        let _ = state
            .sandbox
            .ensure_screen(
                &computer_ref,
                EnsureScreenRequest {
                    slot: screen.slot as u32,
                    profile_path: screen.profile_path.clone(),
                    bot_id: screen.bot_id.clone(),
                },
                &ctx,
            )
            .await;
    }
}

async fn guest_has_screens(
    state: &AppState,
    actor: &Actor,
    bot_id: &str,
    computer: &ComputerRow,
) -> bool {
    matches!(
        probe_computer_container(state, actor, bot_id, computer).await,
        ContainerProbe::Alive
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ContainerProbe {
    Alive,
    Missing,
    Unknown,
}

fn container_missing_error(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.contains("is not running")
        || error.contains("no such container")
        || error.contains("404 not found")
        || error.contains("status code 409")
}

async fn probe_computer_container(
    state: &AppState,
    actor: &Actor,
    bot_id: &str,
    computer: &ComputerRow,
) -> ContainerProbe {
    let Some(computer_ref) = computer_ref(computer) else {
        return ContainerProbe::Missing;
    };
    match state
        .sandbox
        .execute(
            &computer_ref,
            CommandRequest {
                argv: vec![
                    "test".into(),
                    "-x".into(),
                    "/usr/local/bin/lazyboy-screen".into(),
                ],
                cwd: None,
                timeout_ms: Some(5_000),
                stdin: None,
            },
            &adapter_context(actor, bot_id, "probe"),
        )
        .await
    {
        Ok(output) if output.code == 0 => ContainerProbe::Alive,
        Ok(_) => ContainerProbe::Missing,
        Err(error) if container_missing_error(&error.to_string()) => ContainerProbe::Missing,
        Err(error) => {
            tracing::warn!("computer {} probe failed: {error}", computer.id);
            ContainerProbe::Unknown
        }
    }
}

async fn mark_computer_missing(state: &AppState, computer: &mut ComputerRow) {
    if computer.state != "running" {
        return;
    }
    tracing::warn!(
        "computer {} is marked running but the container is gone",
        computer.id
    );
    if let Err(error) = sqlx::query(
        "UPDATE computers SET state = 'error', updated_at = now() WHERE id = $1 AND state = 'running'",
    )
    .bind(&computer.id)
    .execute(state.pool())
    .await
    {
        tracing::error!("failed to mark computer {} missing: {error}", computer.id);
        return;
    }
    computer.state = "error".into();
}

pub async fn take_screen_execution(
    state: &AppState,
    screen: &ScreenRow,
    run_id: &str,
) -> Result<ScreenRow, String> {
    sqlx::query(
        "UPDATE computer_screens
         SET execution_run_id = $2, execution_lease_expires_at = $3,
             execution_fence = execution_fence + 1,
             control_holder = 'none', control_lease_id = NULL, control_lease_expires_at = NULL,
             updated_at = now()
         WHERE id = $1",
    )
    .bind(&screen.id)
    .bind(run_id)
    .bind(Utc::now() + TimeDelta::minutes(5))
    .execute(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    state
        .db
        .get_screen(&screen.computer_id, &screen.bot_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "screen missing after lease".to_string())
}

pub async fn release_screen_execution(state: &AppState, run_id: &str) -> Result<(), String> {
    sqlx::query(
        "UPDATE computer_screens
         SET execution_run_id = NULL, execution_lease_expires_at = NULL, updated_at = now()
         WHERE execution_run_id = $1",
    )
    .bind(run_id)
    .execute(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    sqlx::query("DELETE FROM computer_profile_locks WHERE run_id = $1")
        .bind(run_id)
        .execute(state.pool())
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub async fn take_profile_lock(
    state: &AppState,
    computer: &ComputerRow,
    bot_id: &str,
    bot_name: &str,
    run_id: &str,
    screen: &ScreenRow,
) -> Result<Option<String>, String> {
    let mode = parse_profile_mode(&screen.profile_mode);
    let key = profile_lock_key(mode, bot_id, Some(run_id));
    let expires = Utc::now() + TimeDelta::minutes(5);
    let taken: Option<String> = sqlx::query_scalar(
        "INSERT INTO computer_profile_locks (computer_id, profile_key, bot_id, run_id, expires_at)
         VALUES ($1,$2,$3,$4,$5)
         ON CONFLICT (computer_id, profile_key) DO UPDATE
           SET bot_id = EXCLUDED.bot_id, run_id = EXCLUDED.run_id, expires_at = EXCLUDED.expires_at
           WHERE computer_profile_locks.bot_id = EXCLUDED.bot_id
              OR computer_profile_locks.expires_at < now()
         RETURNING bot_id",
    )
    .bind(&computer.id)
    .bind(&key)
    .bind(bot_id)
    .bind(run_id)
    .bind(expires)
    .fetch_optional(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    if taken.as_deref() == Some(bot_id) {
        return Ok(None);
    }
    let holder: Option<String> = sqlx::query_scalar(
        "SELECT b.name FROM computer_profile_locks l JOIN bots b ON b.id = l.bot_id
         WHERE l.computer_id = $1 AND l.profile_key = $2 AND l.expires_at > now()",
    )
    .bind(&computer.id)
    .bind(&key)
    .fetch_optional(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    let other = holder.unwrap_or_else(|| bot_name.to_string());
    Ok(Some(
        admit_gui(
            &ComputerCapabilities { multi_screen: true },
            bot_id,
            true,
            None,
            Some("other"),
            Some(&other),
        )
        .err()
        .map(|block| block.message())
        .unwrap_or_else(|| {
            format!(
                "Another bot is using this shared browser profile. Currently in use by {other}."
            )
        }),
    ))
}

pub async fn boot(state: &AppState, actor: &Actor, bot_id: &str) -> Result<ComputerStatus, String> {
    let bot = state
        .db
        .get_bot(actor, bot_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "bot not found".to_string())?;
    let computer_id = bot
        .computer_id
        .clone()
        .ok_or_else(|| "bot has no computer".to_string())?;
    let computer = state
        .db
        .get_computer(&computer_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "computer not found".to_string())?;
    if computer.state == "running" && computer.provider_ref.is_some() {
        if guest_has_screens(state, actor, bot_id, &computer).await {
            let screen = ensure_bot_screen(state, actor, bot_id, &computer, None)
                .await
                .ok()
                .and_then(|bound| bound.row);
            restore_computer_screens(state, actor, &computer, bot_id).await;
            return Ok(status_from(bot_id, &computer, screen.as_ref(), None));
        }
        if let Some(computer_ref) = computer_ref(&computer) {
            let ctx = adapter_context(actor, bot_id, "reprovision");
            let _ = state.sandbox.stop(&computer_ref, &ctx).await;
            let _ = state.sandbox.destroy(&computer_ref, &ctx).await;
        }
        sqlx::query(
            "UPDATE computers SET state = 'stopped', provider_ref = NULL, updated_at = now() WHERE id = $1",
        )
        .bind(&computer_id)
        .execute(state.pool())
        .await
        .map_err(|error| error.to_string())?;
    }
    if computer.state == "suspended" && computer.provider_ref.is_some() {
        match resume_paused(state, actor, bot_id, &computer).await {
            Ok(status) => return Ok(status),
            Err(error) => {
                tracing::warn!("computer {computer_id} resume failed: {error}");
                let _ = sqlx::query(
                    "UPDATE computers SET state = 'stopped', updated_at = now()
                     WHERE id = $1 AND state = 'suspended'",
                )
                .bind(&computer_id)
                .execute(state.pool())
                .await;
            }
        }
    }
    let claimed = sqlx::query(
        "UPDATE computers SET state = 'booting', updated_at = now()
         WHERE id = $1 AND state IN ('stopped','suspended','error')",
    )
    .bind(&computer_id)
    .execute(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    // Only the request that atomically transitions the row to booting may
    // provision it.  Allowing every caller that observes `booting` through
    // here starts duplicate containers and can leave the row inconsistent.
    if claimed.rows_affected() != 1 {
        return Err("Computer is busy".into());
    }
    let ctx = adapter_context(actor, bot_id, "boot");
    let home = home_path(&state.data_dir, &computer.home_key);
    if let Err(error) = tokio::fs::create_dir_all(&home).await {
        let message = error.to_string();
        mark_boot_error(state, &computer_id, None).await;
        return Err(message);
    }
    let provisioned = match tokio::time::timeout(
        Duration::from_secs(120),
        state.sandbox.provision(
            ProvisionRequest {
                home_key: computer.home_key.clone(),
                home_path: home.to_string_lossy().into_owned(),
                provider_ref: computer.provider_ref.clone(),
            },
            &ctx,
        ),
    )
    .await
    {
        Ok(Ok(provisioned)) => provisioned,
        Ok(Err(error)) => {
            let message = error.to_string();
            mark_boot_error(state, &computer_id, None).await;
            return Err(message);
        }
        Err(_) => {
            let message = "computer provision timed out after 120 seconds".to_string();
            mark_boot_error(state, &computer_id, None).await;
            return Err(message);
        }
    };
    if parse_mode(&computer.scope) == ComputerMode::Team {
        let folder = match team_bot_workspace_directory(bot_id) {
            Ok(folder) => folder,
            Err(error) => {
                let message = error.to_string();
                mark_boot_error(state, &computer_id, None).await;
                return Err(message);
            }
        };
        let setup = tokio::time::timeout(
            Duration::from_secs(15),
            state.sandbox.execute(
                &provisioned,
                CommandRequest {
                    argv: vec!["mkdir".into(), "-p".into(), "shared".into(), folder],
                    cwd: None,
                    timeout_ms: Some(10_000),
                    stdin: None,
                },
                &ctx,
            ),
        )
        .await;
        if let Err(error) = match setup {
            Ok(result) => result.map_err(|error| error.to_string()),
            Err(_) => Err("computer workspace setup timed out after 15 seconds".into()),
        } {
            // Workspace setup is additive; a failed mkdir should not make an
            // otherwise usable desktop unavailable. The next run can retry it.
            tracing::warn!("computer workspace setup failed for {bot_id}: {error}");
        }
    }
    let running = sqlx::query(
        "UPDATE computers SET state = 'running', provider_ref = $2, kind = $3, updated_at = now()
         WHERE id = $1 AND state = 'booting'",
    )
    .bind(&computer_id)
    .bind(&provisioned.provider_ref)
    .bind(provisioned.kind.as_str())
    .execute(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    if running.rows_affected() != 1 {
        return Err("computer boot was superseded".into());
    }
    let computer = state
        .db
        .get_computer(&computer_id)
        .await
        .map_err(|error| error.to_string())?
        .unwrap();
    let screen = ensure_bot_screen(state, actor, bot_id, &computer, None)
        .await
        .ok()
        .and_then(|bound| bound.row);
    restore_computer_screens(state, actor, &computer, bot_id).await;
    Ok(status_from(bot_id, &computer, screen.as_ref(), None))
}

async fn resume_paused(
    state: &AppState,
    actor: &Actor,
    bot_id: &str,
    computer: &ComputerRow,
) -> Result<ComputerStatus, String> {
    let ctx = adapter_context(actor, bot_id, "resume");
    let home = home_path(&state.data_dir, &computer.home_key);
    let resumed = match tokio::time::timeout(
        Duration::from_secs(20),
        state.sandbox.resume(
            ProvisionRequest {
                home_key: computer.home_key.clone(),
                home_path: home.to_string_lossy().into_owned(),
                provider_ref: computer.provider_ref.clone(),
            },
            &ctx,
        ),
    )
    .await
    {
        Ok(Ok(resumed)) => resumed,
        Ok(Err(error)) => return Err(error.to_string()),
        Err(_) => return Err("computer resume timed out after 20 seconds".into()),
    };
    let running = sqlx::query(
        "UPDATE computers SET state = 'running', provider_ref = $2, kind = $3, updated_at = now()
         WHERE id = $1 AND state = 'suspended'",
    )
    .bind(&computer.id)
    .bind(&resumed.provider_ref)
    .bind(resumed.kind.as_str())
    .execute(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    if running.rows_affected() != 1 {
        let current = state
            .db
            .get_computer(&computer.id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "computer not found".to_string())?;
        if current.state == "running" {
            let screen = ensure_bot_screen(state, actor, bot_id, &current, None)
                .await
                .ok()
                .and_then(|bound| bound.row);
            return Ok(status_from(bot_id, &current, screen.as_ref(), None));
        }
        return Err("computer resume was superseded".into());
    }
    let computer = state
        .db
        .get_computer(&computer.id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "computer not found".to_string())?;
    let screen = ensure_bot_screen(state, actor, bot_id, &computer, None)
        .await
        .ok()
        .and_then(|bound| bound.row);
    restore_computer_screens(state, actor, &computer, bot_id).await;
    Ok(status_from(bot_id, &computer, screen.as_ref(), None))
}

async fn mark_boot_error(state: &AppState, computer_id: &str, provider_ref: Option<&str>) {
    if let Err(error) = sqlx::query(
        "UPDATE computers SET state = 'error', provider_ref = COALESCE($2, provider_ref), updated_at = now()
         WHERE id = $1 AND state = 'booting'",
    )
    .bind(computer_id)
    .bind(provider_ref)
    .execute(state.pool())
    .await
    {
        tracing::error!("failed to mark computer {computer_id} boot error: {error}");
    }
}

pub async fn stop(state: &AppState, actor: &Actor, bot_id: &str) -> Result<ComputerStatus, String> {
    let bot = state
        .db
        .get_bot(actor, bot_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "bot not found".to_string())?;
    let computer_id = bot
        .computer_id
        .clone()
        .ok_or_else(|| "bot has no computer".to_string())?;
    let computer = state
        .db
        .get_computer(&computer_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "computer not found".to_string())?;
    if computer_has_active_work(state, &computer_id).await {
        return Err("電腦仍有工作或示範進行中，請先停止工作再關閉電腦。".into());
    }
    if let Some(provider_ref) = &computer.provider_ref {
        let ctx = adapter_context(actor, bot_id, "stop");
        state
            .sandbox
            .stop(
                &lazyboy_control::ComputerRef {
                    id: provider_ref.clone(),
                    home_key: computer.home_key.clone(),
                    kind: parse_kind(&computer.kind),
                    provider_ref: provider_ref.clone(),
                    fresh: false,
                },
                &ctx,
            )
            .await
            .map_err(|error| format!("無法關閉電腦：{error}"))?;
    }
    sqlx::query(
        "UPDATE computers SET state = 'stopped', control_holder = 'none', control_lease_id = NULL,
                control_lease_expires_at = NULL, control_bot_id = NULL, control_run_id = NULL, updated_at = now()
         WHERE id = $1",
    )
    .bind(&computer_id)
    .execute(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    let computer = state
        .db
        .get_computer(&computer_id)
        .await
        .map_err(|e| e.to_string())?
        .unwrap();
    Ok(status_from(bot_id, &computer, None, None))
}

pub async fn restart(
    state: &AppState,
    actor: &Actor,
    bot_id: &str,
) -> Result<ComputerStatus, String> {
    let bot = state
        .db
        .get_bot(actor, bot_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "bot not found".to_string())?;
    let computer_id = bot
        .computer_id
        .clone()
        .ok_or_else(|| "bot has no computer".to_string())?;
    let computer = state
        .db
        .get_computer(&computer_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "computer not found".to_string())?;
    if let Some(computer_ref) = computer_ref(&computer) {
        let ctx = adapter_context(actor, bot_id, "restart");
        if tokio::time::timeout(
            Duration::from_secs(20),
            state.sandbox.stop(&computer_ref, &ctx),
        )
        .await
        .is_err()
        {
            tracing::warn!(
                "computer stop timed out during restart: {}",
                computer_ref.id
            );
        }
        if tokio::time::timeout(
            Duration::from_secs(20),
            state.sandbox.destroy(&computer_ref, &ctx),
        )
        .await
        .is_err()
        {
            tracing::warn!(
                "computer destroy timed out during restart: {}",
                computer_ref.id
            );
        }
    }
    sqlx::query(
        "UPDATE computers SET state = 'stopped', provider_ref = NULL, control_holder = 'none',
                control_lease_id = NULL, control_lease_expires_at = NULL, control_bot_id = NULL,
                control_run_id = NULL, execution_bot_id = NULL, execution_run_id = NULL,
                execution_lease_expires_at = NULL, updated_at = now()
         WHERE id = $1",
    )
    .bind(&computer_id)
    .execute(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    boot(state, actor, bot_id).await
}

pub async fn takeover(
    state: &AppState,
    actor: &Actor,
    bot_id: &str,
) -> Result<(String, String), String> {
    let bot = state
        .db
        .get_bot(actor, bot_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "bot not found".to_string())?;
    let computer_id = bot
        .computer_id
        .clone()
        .ok_or_else(|| "bot has no computer".to_string())?;
    let computer = state
        .db
        .get_computer(&computer_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "computer not found".to_string())?;
    if computer.state != "running" || computer.provider_ref.is_none() {
        return Err("computer must be running".into());
    }
    let bound = ensure_bot_screen(state, actor, bot_id, &computer, None).await?;
    let screen = bound.row.ok_or_else(|| {
        bound
            .gui_block
            .unwrap_or_else(|| "screen unavailable".into())
    })?;
    let active = state
        .db
        .active_run(bot_id)
        .await
        .map_err(|error| error.to_string())?;
    let run_status = active
        .as_ref()
        .and_then(|(_, status, _)| parse_run_status(status));
    if execution_blocks_user_takeover(
        screen.execution_run_id.is_some(),
        screen.execution_lease_expires_at,
        run_status,
        Utc::now(),
    ) {
        return Err("Stop the task first".into());
    }
    let lease_id = Uuid::new_v4().to_string();
    let expires = Utc::now() + TimeDelta::minutes(15);
    sqlx::query(
        "UPDATE computer_screens SET control_holder = 'user', control_lease_id = $2, control_lease_expires_at = $3,
                updated_at = now() WHERE id = $1",
    )
    .bind(&screen.id)
    .bind(&lease_id)
    .bind(expires)
    .execute(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    sqlx::query(
        "UPDATE computers SET control_holder = 'user', control_lease_id = $2, control_lease_expires_at = $3,
                control_bot_id = $4, updated_at = now() WHERE id = $1",
    )
    .bind(&computer_id)
    .bind(&lease_id)
    .bind(expires)
    .bind(bot_id)
    .execute(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    let paused: Vec<(String, String)> = sqlx::query_as(
        "UPDATE runs SET status = 'waiting_takeover',
                checkpoint = COALESCE(checkpoint, '{}'::jsonb)
                    || jsonb_build_object('resumeAfterTakeover', true),
                updated_at = now()
         WHERE bot_id = $1 AND status IN ('queued','leased','running')
         RETURNING id, thread_id",
    )
    .bind(bot_id)
    .fetch_all(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    for (run_id, thread_id) in paused {
        crate::runs::set_run_step(state, &run_id, STEP_HANDOFF).await;
        let _ = crate::runs::append_bot_message(
            state,
            &thread_id,
            &run_id,
            bot_id,
            "你已接手操作。完成後釋放控制權，我會從目前畫面繼續。",
        )
        .await;
    }
    Ok((lease_id, expires.to_rfc3339()))
}

pub async fn release(state: &AppState, actor: &Actor, bot_id: &str) -> Result<(), String> {
    let bot = state
        .db
        .get_bot(actor, bot_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "bot not found".to_string())?;
    let Some(computer_id) = bot.computer_id else {
        return Ok(());
    };
    sqlx::query(
        "UPDATE computer_screens SET control_holder = 'none', control_lease_id = NULL,
                control_lease_expires_at = NULL, updated_at = now()
         WHERE computer_id = $1 AND bot_id = $2",
    )
    .bind(&computer_id)
    .bind(bot_id)
    .execute(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    sqlx::query(
        "UPDATE computers SET control_holder = 'none', control_lease_id = NULL, control_lease_expires_at = NULL,
                control_bot_id = NULL, control_run_id = NULL, updated_at = now()
         WHERE id = $1 AND control_bot_id = $2",
    )
    .bind(computer_id)
    .bind(bot_id)
    .execute(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    // Releasing human control resumes work that was paused by either the bot or a forced takeover.
    sqlx::query(
        "UPDATE runs SET status = 'queued', updated_at = now()
         WHERE bot_id = $1 AND status = 'waiting_takeover'",
    )
    .bind(bot_id)
    .execute(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub async fn heartbeat(state: &AppState, actor: &Actor, bot_id: &str) -> Result<(), String> {
    let bot = state
        .db
        .get_bot(actor, bot_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "bot not found".to_string())?;
    let Some(computer_id) = bot.computer_id else {
        return Ok(());
    };
    let expires = Utc::now() + TimeDelta::minutes(15);
    sqlx::query("UPDATE computers SET updated_at = now() WHERE id = $1")
        .bind(&computer_id)
        .execute(state.pool())
        .await
        .map_err(|error| error.to_string())?;
    sqlx::query(
        "UPDATE computer_screens SET control_lease_expires_at = $2, updated_at = now()
         WHERE computer_id = $1 AND bot_id = $3 AND control_holder = 'user'",
    )
    .bind(&computer_id)
    .bind(expires)
    .bind(bot_id)
    .execute(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    sqlx::query(
        "UPDATE computers SET control_lease_expires_at = $2, updated_at = now()
         WHERE id = $1 AND control_holder = 'user' AND control_bot_id = $3",
    )
    .bind(computer_id)
    .bind(expires)
    .bind(bot_id)
    .execute(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    Ok(())
}

#[allow(dead_code)]
pub fn user_has_control(computer: &ComputerRow, bot_id: &str) -> bool {
    user_has_screen_control(computer, None, bot_id)
}

pub fn user_has_screen_control(
    computer: &ComputerRow,
    screen: Option<&ScreenRow>,
    bot_id: &str,
) -> bool {
    if let Some(screen) = screen {
        return user_holds_control(
            parse_holder(&screen.control_holder),
            Some(screen.bot_id.as_str()),
            bot_id,
            screen.control_lease_expires_at,
            Utc::now(),
        );
    }
    user_holds_control(
        parse_holder(&computer.control_holder),
        computer.control_bot_id.as_deref(),
        bot_id,
        computer.control_lease_expires_at,
        Utc::now(),
    )
}

pub async fn idle_loop(state: AppState) {
    loop {
        tokio::time::sleep(Duration::from_secs(60)).await;
        crate::attachments::sweep_all_inboxes(&state.data_dir).await;
        pause_idle_computers(&state).await;
        stop_parked_computers(&state).await;
    }
}

async fn computer_has_active_work(state: &AppState, computer_id: &str) -> bool {
    let active: Result<Option<(i64,)>, _> = sqlx::query_as(
        "SELECT 1 FROM runs WHERE status IN ('queued','leased','running','waiting_input','waiting_takeover')
         AND bot_id IN (SELECT id FROM bots WHERE computer_id = $1)
         UNION ALL
         SELECT 1 FROM taught_skills WHERE status IN ('recording','drafting')
         AND bot_id IN (SELECT id FROM bots WHERE computer_id = $1)
         LIMIT 1",
    )
    .bind(computer_id)
    .fetch_optional(state.pool())
    .await;
    matches!(active, Ok(Some(_)))
}

fn idle_adapter(computer: &ComputerRow, operation: &str) -> AdapterContext {
    AdapterContext {
        operation_id: operation.into(),
        space_id: computer.space_id.clone(),
        user_id: computer.user_id.clone(),
        ..Default::default()
    }
}

async fn pause_idle_computers(state: &AppState) {
    let cutoff = Utc::now() - TimeDelta::minutes(10);
    let rows = sqlx::query_as::<_, ComputerRow>(
        "SELECT id, space_id, user_id, scope, scope_key, home_key, home_revision, kind, provider_ref, state,
                control_holder, control_lease_id, control_lease_expires_at, control_bot_id, control_run_id,
                execution_run_id, execution_bot_id, execution_lease_expires_at, execution_fence,
                browser_profile_mode
         FROM computers WHERE state = 'running' AND updated_at < $1",
    )
    .bind(cutoff)
    .fetch_all(state.pool())
    .await;
    let Ok(rows) = rows else { return };
    for computer in rows {
        if computer_has_active_work(state, &computer.id).await {
            continue;
        }
        if let Some(computer_ref) = computer_ref(&computer) {
            if state
                .sandbox
                .suspend(&computer_ref, &idle_adapter(&computer, "idle"))
                .await
                .is_err()
            {
                continue;
            }
        }
        let _ = sqlx::query(
            "UPDATE computers SET state = 'suspended', updated_at = now()
             WHERE id = $1 AND state = 'running'",
        )
        .bind(&computer.id)
        .execute(state.pool())
        .await;
    }
}

async fn stop_parked_computers(state: &AppState) {
    let cutoff = Utc::now() - TimeDelta::hours(6);
    let rows = sqlx::query_as::<_, ComputerRow>(
        "SELECT id, space_id, user_id, scope, scope_key, home_key, home_revision, kind, provider_ref, state,
                control_holder, control_lease_id, control_lease_expires_at, control_bot_id, control_run_id,
                execution_run_id, execution_bot_id, execution_lease_expires_at, execution_fence,
                browser_profile_mode
         FROM computers WHERE state = 'suspended' AND updated_at < $1",
    )
    .bind(cutoff)
    .fetch_all(state.pool())
    .await;
    let Ok(rows) = rows else { return };
    for computer in rows {
        if computer_has_active_work(state, &computer.id).await {
            continue;
        }
        if let Some(computer_ref) = computer_ref(&computer) {
            let _ = state
                .sandbox
                .stop(&computer_ref, &idle_adapter(&computer, "parked"))
                .await;
        }
        let _ = sqlx::query(
            "UPDATE computers SET state = 'stopped', updated_at = now()
             WHERE id = $1 AND state = 'suspended'",
        )
        .bind(&computer.id)
        .execute(state.pool())
        .await;
    }
}

pub fn computer_ref(computer: &ComputerRow) -> Option<lazyboy_control::ComputerRef> {
    let provider_ref = computer.provider_ref.clone()?;
    Some(lazyboy_control::ComputerRef {
        id: provider_ref.clone(),
        home_key: computer.home_key.clone(),
        kind: parse_kind(&computer.kind),
        provider_ref,
        fresh: false,
    })
}

pub async fn current_status(
    state: &AppState,
    actor: &Actor,
    bot_id: &str,
) -> Result<ComputerStatus, String> {
    let bot = state
        .db
        .get_bot(actor, bot_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "bot not found".to_string())?;
    let computer_id = bot
        .computer_id
        .ok_or_else(|| "bot has no computer".to_string())?;
    let mut computer = state
        .db
        .get_computer(&computer_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "computer not found".to_string())?;
    if computer.state == "running"
        && probe_computer_container(state, actor, bot_id, &computer).await
            == ContainerProbe::Missing
    {
        mark_computer_missing(state, &mut computer).await;
    }
    let screen = state
        .db
        .get_screen(&computer.id, bot_id)
        .await
        .map_err(|error| error.to_string())?;
    // Newest-first is wrong here: a message queued behind a paused run must not
    // hide the pause. Rank by what the user needs to know about.
    let active: Vec<ActiveRunRow> = sqlx::query_as(
        "SELECT id, status, thread_id, checkpoint->>'step' AS step FROM runs
         WHERE bot_id = $1
           AND status IN ('queued','leased','running','waiting_input','waiting_takeover')
         ORDER BY CASE status
                    WHEN 'running' THEN 0 WHEN 'leased' THEN 1
                    WHEN 'waiting_takeover' THEN 2 WHEN 'waiting_input' THEN 3
                    ELSE 4 END,
                  created_at ASC",
    )
    .bind(bot_id)
    .fetch_all(state.pool())
    .await
    .unwrap_or_default();
    let waiting = active.iter().find(|run| run.status == "waiting_takeover");
    let busy = active.iter().find(|run| {
        parse_run_status(&run.status).is_some_and(|status| {
            status.is_active() && status != lazyboy_contracts::RunStatus::WaitingTakeover
        })
    });
    // While the bot is paused for the human, later messages just queue up; the
    // spinner would lie, so report them as queued instead of busy.
    let busy = if waiting.is_some() {
        busy.filter(|run| run.status != "queued")
    } else {
        busy
    };
    let busy_bot_name = busy.map(|_| bot.name.clone());
    let mut status = status_from(bot_id, &computer, screen.as_ref(), busy_bot_name);
    status.takeover_requested = waiting.is_some();
    if let Some(run) = busy {
        status.busy_run_id = Some(run.id.clone());
        status.busy_session_id = Some(run.thread_id.clone());
        status.busy_step = run.step.clone();
        status.using_computer = screen
            .as_ref()
            .and_then(|row| row.execution_run_id.as_deref())
            == Some(run.id.as_str())
            || computer.execution_run_id.as_deref() == Some(run.id.as_str());
    }
    if let Some(run) = waiting {
        status.waiting_run_id = Some(run.id.clone());
        status.waiting_session_id = Some(run.thread_id.clone());
    }
    status.queued_runs = active
        .iter()
        .filter(|run| {
            run.status == "queued" && Some(run.id.as_str()) != busy.map(|b| b.id.as_str())
        })
        .count() as u32;
    Ok(status)
}

#[derive(sqlx::FromRow)]
struct ActiveRunRow {
    id: String,
    status: String,
    thread_id: String,
    step: Option<String>,
}

pub fn _keep_state(state: ComputerState, holder: ControlHolder, mode: BrowserProfileMode) {
    let _ = (state, holder, mode);
}
