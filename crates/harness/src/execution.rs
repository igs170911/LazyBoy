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
    let reply = reply.trim();
    // A bare marker provides neither verification nor a reason to the human.
    if reply.lines().count() < 2 {
        return GoalOutcome::Continue;
    }
    match reply.lines().last().map(str::trim) {
        Some("[GOAL_COMPLETE]") => GoalOutcome::Complete,
        Some("[GOAL_BLOCKED]") => GoalOutcome::NeedsInput,
        _ => GoalOutcome::Continue,
    }
}

pub const GOAL_INSTRUCTIONS: &str = "Persistent goal execution: plan the requested work, execute it, and verify each requested outcome. Intermediate progress replies do not finish the run. Preserve completed work and incorporate user steering. End your final reply with a standalone [GOAL_COMPLETE] line only when all outcomes are verified; explain the verification. When required information or human action is missing, explain exactly what is needed and end with a standalone [GOAL_BLOCKED] line. For a simple Cloudflare connection-check checkbox, observe the current screen and try connection_check once, then verify the requested content. For other CAPTCHA, failed verification, login or 2FA use request_takeover. Never claim completion merely because you planned the work.";
pub const GOAL_CONTINUE: &str = "The goal remains active. Continue the plan with tools and verify the outcome. Finish only with a standalone [GOAL_COMPLETE] line after verification, or [GOAL_BLOCKED] when required human input is missing.";

/// A run that stops in the middle must say so instead of going quiet. The
/// model marks the moment; the loop turns the marker into a paused run the
/// human can continue with one click.
pub const NEEDS_INPUT_MARKER: &str = "[NEEDS_INPUT]";

/// Turn budgets for self-correction. A goal run is expected to fight through
/// obstacles, so it gets more attempts than a plain task.
pub const MAX_NUDGES_PLAIN: u32 = 6;
pub const MAX_NUDGES_GOAL: u32 = 8;

/// Sent once per stop attempt: the cheapest way to tell "the work is done" from
/// "the model just ran out of sentences".
pub const VERIFY_BEFORE_DONE: &str = "Before you finish, verify the result against the CURRENT screen or file contents: say what you checked and what you still owe. If any requested outcome is missing, act on it now with a tool call. If the human must decide, supply something, or do a step you cannot do, say what you did so far and what the next step is, then end with a standalone [NEEDS_INPUT] line. Never end a half-finished task with a plain status sentence.";

/// Why the loop is handing the turn back to the human.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// The model stopped mid-task and asked the human to take it from here.
    MidTaskText,
    /// The bounded turn budget is spent with work still outstanding.
    BudgetExhausted,
}

impl StopReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MidTaskText => "mid_task_text",
            Self::BudgetExhausted => "budget_exhausted",
        }
    }
}

/// Only a standalone marker line asks for input, never a quoted mention.
pub fn asks_for_input(reply: &str) -> bool {
    reply
        .trim()
        .lines()
        .last()
        .is_some_and(|line| line.trim() == NEEDS_INPUT_MARKER)
}

/// A run that never touched a tool answered in prose, which is a complete
/// reply. Once a run has done work, ending is a decision the human gets to
/// make: report where the work stands, then ask before stopping.
pub fn stop_reason(
    mode: ExecutionMode,
    turns: u32,
    reply: &str,
    did_work: bool,
    stalled: bool,
) -> Option<StopReason> {
    if !did_work {
        return None;
    }
    if !mode.allows_turn(turns.saturating_add(1)) {
        return Some(StopReason::BudgetExhausted);
    }
    (stalled || asks_for_input(reply)).then_some(StopReason::MidTaskText)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_prose_answer_is_not_a_stop() {
        assert_eq!(stop_reason(ExecutionMode::Bounded(40), 3, "done", false, false), None);
    }

    #[test]
    fn only_a_standalone_marker_asks_for_input() {
        assert!(asks_for_input("我做到一半。\n[NEEDS_INPUT]"));
        assert!(!asks_for_input("我不会用 [NEEDS_INPUT] 这种标记"));
        assert!(!asks_for_input("任务完成"));
    }

    #[test]
    fn a_stalled_or_asking_run_hands_the_turn_to_the_human() {
        let mode = ExecutionMode::Bounded(40);
        assert_eq!(
            stop_reason(mode, 3, "需要密码\n[NEEDS_INPUT]", true, false),
            Some(StopReason::MidTaskText)
        );
        assert_eq!(stop_reason(mode, 3, "我先跳到下一步", true, true), Some(StopReason::MidTaskText));
        assert_eq!(stop_reason(mode, 3, "看起来好了", true, false), None);
    }

    #[test]
    fn a_spent_budget_reports_before_a_mid_task_stop() {
        assert_eq!(
            stop_reason(ExecutionMode::Bounded(40), 40, "", true, false),
            Some(StopReason::BudgetExhausted)
        );
        assert_eq!(
            stop_reason(ExecutionMode::Bounded(40), 40, "需要密码\n[NEEDS_INPUT]", true, false),
            Some(StopReason::BudgetExhausted)
        );
        // A goal run that left the loop on its own terms reported its outcome;
        // the after-loop guard must not invent a second stop for it.
        assert_eq!(stop_reason(ExecutionMode::Goal, 4000, "", true, false), None);
        assert_eq!(
            stop_reason(ExecutionMode::Goal, 4000, "我先跳过这一步", true, true),
            Some(StopReason::MidTaskText)
        );
    }

    #[test]
    fn goal_runs_get_more_self_correction_than_plain_runs() {
        assert_eq!(MAX_NUDGES_GOAL, 8);
        assert_eq!(MAX_NUDGES_PLAIN, 6);
    }

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
        assert_eq!(goal_outcome("[GOAL_COMPLETE]"), GoalOutcome::Continue);
        assert_eq!(goal_outcome("\n[GOAL_BLOCKED]"), GoalOutcome::Continue);
        assert_eq!(goal_outcome("Next I will use [GOAL_COMPLETE]."), GoalOutcome::Continue);
        assert_eq!(goal_outcome("Verified output.\n[GOAL_COMPLETE]"), GoalOutcome::Complete);
        assert_eq!(goal_outcome("Please supply the date.\n[GOAL_BLOCKED]"), GoalOutcome::NeedsInput);
    }
}
