use chrono::{DateTime, Utc};
use lazyboy_contracts::RunStatus;

/// Humans may take the pointer during a live run. The worker pauses at the
/// next turn boundary (or aborts the in-flight model/tool call) instead of
/// competing for the mouse.
pub fn execution_blocks_user_takeover(
    _has_lease: bool,
    _lease_expires_at: Option<DateTime<Utc>>,
    _run_status: Option<RunStatus>,
    _now: DateTime<Utc>,
) -> bool {
    false
}

pub fn user_holds_control(
    holder: lazyboy_contracts::ControlHolder,
    control_bot_id: Option<&str>,
    bot_id: &str,
    lease_expires_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> bool {
    holder == lazyboy_contracts::ControlHolder::User
        && control_bot_id == Some(bot_id)
        && lease_expires_at.is_some_and(|expires| expires > now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-03T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn live_run_does_not_block_takeover() {
        assert!(!execution_blocks_user_takeover(
            true,
            Some(now() + Duration::minutes(4)),
            Some(RunStatus::Running),
            now(),
        ));
    }

    #[test]
    fn waiting_takeover_allows_the_human() {
        assert!(!execution_blocks_user_takeover(
            true,
            Some(now() + Duration::minutes(4)),
            Some(RunStatus::WaitingTakeover),
            now(),
        ));
    }

    #[test]
    fn expired_lease_without_active_run_does_not_block() {
        assert!(!execution_blocks_user_takeover(
            true,
            Some(now() - Duration::minutes(1)),
            Some(RunStatus::Completed),
            now(),
        ));
    }

    #[test]
    fn no_lease_does_not_block() {
        assert!(!execution_blocks_user_takeover(false, None, None, now()));
    }
}
