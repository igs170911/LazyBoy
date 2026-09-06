//! Execution policy belongs to the harness, independently of task playbooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    Bounded(u32),
    Goal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalOutcome {
    Continue,
    Complete,
    NeedsInput,
}

pub fn goal_request(prompt: &str) -> Option<&str> {
    let rest = prompt.trim().strip_prefix("/goal")?;
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| rest.trim())
}

impl ExecutionMode {
    pub fn allows_turn(self, turns: u32) -> bool {
        match self {
            Self::Goal => true,
            Self::Bounded(limit) => turns < limit,
        }
    }
}

/// Only a standalone terminal marker is an outcome, not a quoted mention.
pub fn goal_outcome(reply: &str) -> GoalOutcome {
    match reply.trim().lines().last().map(str::trim) {
        Some("[GOAL_COMPLETE]") => GoalOutcome::Complete,
        Some("[GOAL_BLOCKED]") => GoalOutcome::NeedsInput,
        _ => GoalOutcome::Continue,
    }
}

pub const GOAL_INSTRUCTIONS: &str = "Persistent goal execution: plan the requested work, execute it, and verify each requested outcome. Intermediate progress replies do not finish the run. Preserve completed work and incorporate user steering. End your final reply with a standalone [GOAL_COMPLETE] line only when all outcomes are verified; explain the verification. When required information or human action is missing, explain exactly what is needed and end with a standalone [GOAL_BLOCKED] line. For login, CAPTCHA or 2FA use request_takeover. Never claim completion merely because you planned the work.";
pub const GOAL_CONTINUE: &str = "The goal remains active. Continue the plan with tools and verify the outcome. Finish only with a standalone [GOAL_COMPLETE] line after verification, or [GOAL_BLOCKED] when required human input is missing.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goals_are_unbounded_while_normal_runs_remain_bounded() {
        assert!(ExecutionMode::Goal.allows_turn(40));
        assert!(ExecutionMode::Goal.allows_turn(u32::MAX));
        assert!(!ExecutionMode::Bounded(40).allows_turn(40));
    }

    #[test]
    fn goal_command_requires_a_token_boundary() {
        assert_eq!(goal_request(" /goal\nfinish this"), Some("finish this"));
        assert_eq!(goal_request("/goalkeeper"), None);
    }

    #[test]
    fn progress_and_quoted_markers_do_not_complete_a_goal() {
        assert_eq!(goal_outcome("Next I will use [GOAL_COMPLETE]."), GoalOutcome::Continue);
        assert_eq!(goal_outcome("Verified output.\n[GOAL_COMPLETE]"), GoalOutcome::Complete);
        assert_eq!(goal_outcome("Please supply the date.\n[GOAL_BLOCKED]"), GoalOutcome::NeedsInput);
    }
}
