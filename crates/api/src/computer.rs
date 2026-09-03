use std::path::PathBuf;
use std::time::Duration;

use chrono::{TimeDelta, Utc};
use lazyboy_contracts::{
    BrowserProfileMode, ComputerCapabilities, ComputerMode, ComputerState, ComputerStatus, ControlHolder,
    DEFAULT_SCREEN_HEIGHT, DEFAULT_SCREEN_WIDTH,
};
use lazyboy_control::{
    admit_gui, admit_new_screen, browser_profile_path, execution_blocks_user_takeover, profile_lock_key,
    screen_layout, team_bot_workspace_directory, user_holds_control, AdapterContext, CommandRequest,
    EnsureScreenRequest, ProvisionRequest,
};
use uuid::Uuid;

use crate::db::{
    parse_holder, parse_kind, parse_mode, parse_profile_mode, parse_run_status, parse_state, Actor, ComputerRow,
    ScreenRow,
};
use crate::state::AppState;

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
            sqlx::query("UPDATE computer_screens SET profile_path = $2, updated_at = now() WHERE id = $1")
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
            Err(error) if error.to_string().contains("computer_screens_computer_id_slot") => {
                return Ok(BoundScreen {
                    row: None,
                    gui_block: Some(admit_new_screen(&caps, &used, None).err().map(|block| block.message()).unwrap_or_else(|| {
                        lazyboy_contracts::TEAM_SCREENS_FULL.to_string()
                    })),
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
        match state.sandbox.ensure_screen(&computer_ref, request.clone(), &ctx).await {
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

async fn guest_has_screens(state: &AppState, actor: &Actor, bot_id: &str, computer: &ComputerRow) -> bool {
    let Some(computer_ref) = computer_ref(computer) else {
        return false;
    };
    let result = state
        .sandbox
        .execute(
            &computer_ref,
            CommandRequest {
                argv: vec!["test".into(), "-x".into(), "/usr/local/bin/lazyboy-screen".into()],
                cwd: None,
                timeout_ms: Some(5_000),
            },
            &adapter_context(actor, bot_id, "probe"),
        )
        .await;
    matches!(result, Ok(output) if output.code == 0)
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
        .unwrap_or_else(|| format!("Another bot is using this shared browser profile. Currently in use by {other}.")),
    ))
}

pub async fn boot(state: &AppState, actor: &Actor, bot_id: &str) -> Result<ComputerStatus, String> {
    let bot = state
        .db
        .get_bot(actor, bot_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "bot not found".to_string())?;
    let computer_id = bot.computer_id.clone().ok_or_else(|| "bot has no computer".to_string())?;
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
    let claimed = sqlx::query(
        "UPDATE computers SET state = 'booting', updated_at = now()
         WHERE id = $1 AND state IN ('stopped','suspended','error')",
    )
    .bind(&computer_id)
    .execute(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    if claimed.rows_affected() != 1 && computer.state != "booting" {
        return Err("Computer is busy".into());
    }
    let ctx = adapter_context(actor, bot_id, "boot");
    let home = home_path(&state.data_dir, &computer.home_key);
    tokio::fs::create_dir_all(&home)
        .await
        .map_err(|error| error.to_string())?;
    let provisioned = state
        .sandbox
        .provision(
            ProvisionRequest {
                home_key: computer.home_key.clone(),
                home_path: home.to_string_lossy().into_owned(),
                provider_ref: computer.provider_ref.clone(),
            },
            &ctx,
        )
        .await
        .map_err(|error| error.to_string())?;
    if parse_mode(&computer.scope) == ComputerMode::Team {
        let folder = team_bot_workspace_directory(bot_id).map_err(|error| error.to_string())?;
        let _ = state
            .sandbox
            .execute(
                &provisioned,
                CommandRequest {
                    argv: vec!["mkdir".into(), "-p".into(), "shared".into(), folder],
                    cwd: None,
                    timeout_ms: Some(10_000),
                },
                &ctx,
            )
            .await;
    }
    sqlx::query(
        "UPDATE computers SET state = 'running', provider_ref = $2, kind = $3, updated_at = now()
         WHERE id = $1 AND state = 'booting'",
    )
    .bind(&computer_id)
    .bind(&provisioned.provider_ref)
    .bind(provisioned.kind.as_str())
    .execute(state.pool())
    .await
    .map_err(|error| error.to_string())?;
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

pub async fn stop(state: &AppState, actor: &Actor, bot_id: &str) -> Result<ComputerStatus, String> {
    let bot = state
        .db
        .get_bot(actor, bot_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "bot not found".to_string())?;
    let computer_id = bot.computer_id.clone().ok_or_else(|| "bot has no computer".to_string())?;
    let computer = state
        .db
        .get_computer(&computer_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "computer not found".to_string())?;
    if let Some(provider_ref) = &computer.provider_ref {
        let ctx = adapter_context(actor, bot_id, "stop");
        let _ = state
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
            .await;
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
    let computer = state.db.get_computer(&computer_id).await.map_err(|e| e.to_string())?.unwrap();
    Ok(status_from(bot_id, &computer, None, None))
}

pub async fn restart(state: &AppState, actor: &Actor, bot_id: &str) -> Result<ComputerStatus, String> {
    let bot = state
        .db
        .get_bot(actor, bot_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "bot not found".to_string())?;
    let computer_id = bot.computer_id.clone().ok_or_else(|| "bot has no computer".to_string())?;
    let computer = state
        .db
        .get_computer(&computer_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "computer not found".to_string())?;
    if let Some(computer_ref) = computer_ref(&computer) {
        let ctx = adapter_context(actor, bot_id, "restart");
        let _ = state.sandbox.stop(&computer_ref, &ctx).await;
        let _ = state.sandbox.destroy(&computer_ref, &ctx).await;
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

pub async fn takeover(state: &AppState, actor: &Actor, bot_id: &str) -> Result<(String, String), String> {
    let bot = state
        .db
        .get_bot(actor, bot_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "bot not found".to_string())?;
    let computer_id = bot.computer_id.clone().ok_or_else(|| "bot has no computer".to_string())?;
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
    let screen = bound.row.ok_or_else(|| bound.gui_block.unwrap_or_else(|| "screen unavailable".into()))?;
    let active = state.db.active_run(bot_id).await.map_err(|error| error.to_string())?;
    let run_status = active
        .as_ref()
        .and_then(|(_, status)| parse_run_status(status));
    if execution_blocks_user_takeover(
        screen.execution_run_id.is_some(),
        screen.execution_lease_expires_at,
        run_status,
        Utc::now(),
    ) {
        return Err("Stop the bot first".into());
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

pub fn user_has_screen_control(computer: &ComputerRow, screen: Option<&ScreenRow>, bot_id: &str) -> bool {
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
        let Ok(rows) = rows else { continue };
        for computer in rows {
            let active: Result<Option<(i64,)>, _> = sqlx::query_as(
                "SELECT 1 FROM runs WHERE status IN ('queued','leased','running','waiting_input','waiting_takeover')
                 AND bot_id IN (SELECT id FROM bots WHERE computer_id = $1) LIMIT 1",
            )
            .bind(&computer.id)
            .fetch_optional(state.pool())
            .await;
            if matches!(active, Ok(Some(_))) {
                continue;
            }
            if let Some(provider_ref) = &computer.provider_ref {
                let ctx = AdapterContext {
                    operation_id: "idle".into(),
                    space_id: computer.space_id.clone(),
                    user_id: computer.user_id.clone(),
                    ..Default::default()
                };
                let _ = state
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
                    .await;
            }
            let _ = sqlx::query("UPDATE computers SET state = 'stopped', updated_at = now() WHERE id = $1")
                .bind(&computer.id)
                .execute(state.pool())
                .await;
        }
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

pub async fn current_status(state: &AppState, actor: &Actor, bot_id: &str) -> Result<ComputerStatus, String> {
    let bot = state
        .db
        .get_bot(actor, bot_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "bot not found".to_string())?;
    let computer_id = bot.computer_id.ok_or_else(|| "bot has no computer".to_string())?;
    let computer = state
        .db
        .get_computer(&computer_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "computer not found".to_string())?;
    let screen = state
        .db
        .get_screen(&computer.id, bot_id)
        .await
        .map_err(|error| error.to_string())?;
    let mut status = status_from(bot_id, &computer, screen.as_ref(), None);
    status.takeover_requested = state
        .db
        .active_run(bot_id)
        .await
        .ok()
        .flatten()
        .and_then(|(_, run_status)| parse_run_status(&run_status))
        == Some(lazyboy_contracts::RunStatus::WaitingTakeover);
    Ok(status)
}

pub fn _keep_state(state: ComputerState, holder: ControlHolder, mode: BrowserProfileMode) {
    let _ = (state, holder, mode);
}
