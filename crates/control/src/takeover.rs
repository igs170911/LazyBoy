use chrono::{DateTime, Utc};
use lazyboy_contracts::RunStatus;

/// Mirrors computer.takeover: an execution lease blocks user control unless
/// the run is waiting for a human.
pub fn execution_blocks_user_takeover(
    has_lease: bool,
    lease_expires_at: Option<DateTime<Utc>>,
    run_status: Option<RunStatus>,
    now: DateTime<Utc>,
) -> bool {
    if !has_lease {
        return false;
    }
    if run_status == Some(RunStatus::WaitingTakeover) {
        return false;
    }
    let lease_active = lease_expires_at.is_some_and(|expires| expires > now);
    let run_active = run_status.is_some_and(RunStatus::is_active);
    lease_active || run_active
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
    fn live_run_blocks_takeover() {
        assert!(execution_blocks_user_takeover(
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
