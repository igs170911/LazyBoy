//! Bounded maintenance of expendable diagnostics, never user-authored content.
use crate::state::AppState;
use sqlx::PgPool;
use std::time::Duration;

fn days(name: &str, default: i32) -> i32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<i32>().ok())
        .filter(|v| (1..=3650).contains(v))
        .unwrap_or(default)
}

pub async fn retention_loop(state: AppState) {
    let recording_days = days("LAZYBOY_RECORDING_RETENTION_DAYS", 30);
    let rules = [
        (
            "events",
            include_str!("retention/events.sql"),
            days("LAZYBOY_EVENT_RETENTION_DAYS", 30),
        ),
        (
            "checkpoints",
            include_str!("retention/checkpoints.sql"),
            days("LAZYBOY_CHECKPOINT_RETENTION_DAYS", 7),
        ),
        (
            "runs",
            include_str!("retention/runs.sql"),
            days("LAZYBOY_RUN_RETENTION_DAYS", 90),
        ),
        (
            "run_activity",
            include_str!("retention/run_activity.sql"),
            days("LAZYBOY_RUN_ACTIVITY_RETENTION_DAYS", 7),
        ),
        (
            "recordings",
            include_str!("retention/recordings.sql"),
            recording_days,
        ),
        (
            "revisions",
            include_str!("retention/revisions.sql"),
            days("LAZYBOY_MEMORY_HISTORY_RETENTION_DAYS", 90),
        ),
        (
            "deleted_memories",
            include_str!("retention/deleted_memories.sql"),
            days("LAZYBOY_MEMORY_HISTORY_RETENTION_DAYS", 90),
        ),
        ("leases", include_str!("retention/leases.sql"), 7),
        (
            "profile_locks",
            include_str!("retention/profile_locks.sql"),
            7,
        ),
    ];
    loop {
        for (name, query, age) in rules {
            let mut removed = 0;
            // Limit both transaction size and work per hour; defer excess backlog.
            for _ in 0..20 {
                match batch(state.pool(), query, age).await {
                    Ok(count) => {
                        removed += count;
                        if count < 1000 {
                            break;
                        }
                    }
                    Err(error) => {
                        tracing::warn!(name, %error, "retention batch failed; retry next hour");
                        break;
                    }
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            if removed > 0 {
                tracing::info!(
                    name,
                    rows = removed,
                    "retention cleaned expired diagnostics"
                );
            }
        }
        if let Err(error) = clean_frames(&state, recording_days).await {
            tracing::warn!(%error, "recording file retention failed; retry next hour");
        }
        if let Ok(bytes) =
            sqlx::query_scalar::<_, i64>("SELECT pg_database_size(current_database())")
                .fetch_one(state.pool())
                .await
        {
            let warn_mb = std::env::var("LAZYBOY_DB_WARN_MB")
                .ok()
                .and_then(|v| v.parse::<i64>().ok())
                .filter(|v| *v > 0 && *v < 1_000_000)
                .unwrap_or(1024);
            tracing::info!(bytes, "database size after retention");
            if bytes > warn_mb * 1024 * 1024 {
                tracing::warn!(
                    bytes,
                    warn_mb,
                    "database exceeds configured size warning; review retained conversations and memories"
                );
            }
        }
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}

async fn batch(pool: &PgPool, query: &str, age: i32) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    // One maintenance writer, even when multiple API processes start together.
    let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(72189431)")
        .fetch_one(&mut *tx)
        .await?;
    if !acquired {
        return Ok(0);
    }
    sqlx::query("SET LOCAL statement_timeout = '10s'")
        .execute(&mut *tx)
        .await?;
    sqlx::query("SET LOCAL lock_timeout = '1s'")
        .execute(&mut *tx)
        .await?;
    let result = sqlx::query(query)
        .bind(age)
        .bind(1000_i64)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(result.rows_affected())
}

async fn clean_frames(
    state: &AppState,
    age: i32,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let root = std::path::Path::new(&state.data_dir).join("teach");
    let mut dirs = match tokio::fs::read_dir(&root).await {
        Ok(dirs) => dirs,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let mut cleaned = 0;
    while let Some(entry) = dirs.next_entry().await? {
        if cleaned >= 1000 {
            break;
        }
        // Ignore symlinks and unexpected names; never traverse a user's home.
        if !entry.file_type().await?.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        if uuid::Uuid::parse_str(&id).is_err() {
            continue;
        }
        let eligible: Option<bool> = sqlx::query_scalar(
            "SELECT status IN ('saved','failed','draft') AND updated_at < now() - make_interval(days => $2) FROM taught_skills WHERE id=$1"
        ).bind(&id).bind(age).fetch_optional(state.pool()).await?;
        let old_orphan = eligible.is_none()
            && entry
                .metadata()
                .await?
                .modified()?
                .elapsed()
                .unwrap_or(Duration::ZERO)
                > Duration::from_secs(age as u64 * 86400);
        if eligible == Some(true) || old_orphan {
            tokio::fs::remove_dir_all(entry.path()).await?;
            cleaned += 1;
        }
    }
    if cleaned > 0 {
        tracing::info!(cleaned, "removed expired teaching frame directories");
    }
    Ok(())
}
