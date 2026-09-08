//! Round policy: how long a run may keep going, and when it must stop.
//!
//! Earlier every run got a fixed number of turns. A fixed quota punishes real
//! work - a build, a 200-row sheet, a site that reloads slowly - and it teaches
//! the agent to schedule its honesty around turn 39. Here turns are not the
//! budget; evidence is. A run keeps going until the work is verified, the human
//! says stop, or the guard sees the agent going in circles instead of moving.
//! Only then, and with the evidence in hand, does the run park.
//!
//! Three layers, cheapest first:
//! 1. Coaching (`Reflect`): the loop injects one pointed instruction and the run
//!    continues. Triggers are the same action again, the same failure again, no
//!    new success for a long time, or a soft checkpoint.
//! 2. Derailment halt (`LoopDetected`): the same action keeps being repeated, or
//!    nothing has worked for a very long time. The run parks in `waiting_input`
//!    with the concrete evidence so the human can unblock it.
//! 3. Circuit breaker (`BudgetExhausted`): a large turn and wall-clock ceiling
//!    that exists only so a bug cannot burn an API key overnight. It is not a
//!    task budget; reaching it is a bug report, not a result.

use std::collections::HashMap;
use std::time::Duration;

use serde_json::Value;

use crate::execution::StopReason;

/// Turns before the first "where are you?" self-check. Generous on purpose:
/// the first stretch of a hard task is normal, not suspicious.
pub const SOFT_CHECKPOINT_TURNS: u32 = 60;
/// Turns between later self-checks.
pub const SOFT_CHECKPOINT_EVERY: u32 = 120;
/// Circuit breaker. No honest task is 1000 model turns long; if it is, it
/// belongs in a skill or a schedule, not in one run.
pub const HARD_CAP_TURNS: u32 = 1_000;
/// Wall clock before a run is asked to justify itself out loud.
pub const SOFT_WALL_MINUTES: u64 = 75;
/// Wall clock circuit breaker.
pub const HARD_WALL_MINUTES: u64 = 240;

/// Same mutating action, in a row, before the run is coached.
pub const REPEAT_WARN: u32 = 3;
/// Same mutating action, in a row, before the run parks.
pub const REPEAT_HALT: u32 = 6;
/// Same mutating action, counted over the whole run.
pub const REPEAT_HALT_TOTAL: u32 = 12;
/// Same action failing, in a row.
pub const FAILURE_WARN: u32 = 3;
pub const FAILURE_HALT: u32 = 8;
/// Anything failing, in a row, whatever the tool.
pub const FAILURE_HALT_ANY: u32 = 14;
/// Turns without a new success before a self-check.
pub const STALE_REFLECT: u32 = 40;
/// Turns without a new success before parking the run.
pub const STALE_HALT: u32 = 150;
/// Coaching has to stay rare enough that the model actually reads it.
pub const MAX_REFLECTIONS: u32 = 8;

/// A run that repeats itself is not a run that waits or polls, so these tools
/// reset the no-progress clock even when their arguments repeat.
const PROGRESS_WHEN_REPEATED: [&str; 2] = ["shell", "wait"];

fn parse_u64(value: Option<String>) -> Option<u64> {
    value?.trim().parse::<u64>().ok().filter(|value| *value > 0)
}

fn parse_u32(value: Option<String>) -> Option<u32> {
    parse_u64(value).and_then(|value| u32::try_from(value).ok())
}

/// The knobs a deployment may want to move. Every one of them is a breaker,
/// not a quota: the defaults sit far outside honest work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunPolicy {
    pub soft_checkpoint_turns: u32,
    pub soft_checkpoint_every: u32,
    pub hard_cap_turns: u32,
    pub soft_wall: Duration,
    pub hard_wall: Duration,
}

impl Default for RunPolicy {
    fn default() -> Self {
        Self {
            soft_checkpoint_turns: SOFT_CHECKPOINT_TURNS,
            soft_checkpoint_every: SOFT_CHECKPOINT_EVERY,
            hard_cap_turns: HARD_CAP_TURNS,
            soft_wall: Duration::from_secs(SOFT_WALL_MINUTES * 60),
            hard_wall: Duration::from_secs(HARD_WALL_MINUTES * 60),
        }
    }
}

impl RunPolicy {
    /// `LAZYBOY_RUN_SOFT_TURNS`, `LAZYBOY_RUN_SOFT_EVERY`,
    /// `LAZYBOY_RUN_CAP_TURNS`, `LAZYBOY_RUN_SOFT_MINUTES`,
    /// `LAZYBOY_RUN_HARD_MINUTES`. Unset or unparsable keeps the default.
    pub fn from_env() -> Self {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let mut policy = Self::default();
        if let Some(value) = parse_u32(lookup("LAZYBOY_RUN_SOFT_TURNS")) {
            policy.soft_checkpoint_turns = value;
        }
        if let Some(value) = parse_u32(lookup("LAZYBOY_RUN_SOFT_EVERY")) {
            policy.soft_checkpoint_every = value;
        }
        if let Some(value) = parse_u32(lookup("LAZYBOY_RUN_CAP_TURNS")) {
            policy.hard_cap_turns = value;
        }
        if let Some(value) = parse_u64(lookup("LAZYBOY_RUN_SOFT_MINUTES")) {
            policy.soft_wall = Duration::from_secs(value * 60);
        }
        if let Some(value) = parse_u64(lookup("LAZYBOY_RUN_HARD_MINUTES")) {
            policy.hard_wall = Duration::from_secs(value * 60);
        }
        policy
    }
}

/// What the loop should do with a verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Continue,
    /// Inject one instruction into the next model turn and keep running.
    Reflect(String),
    /// Park the run and hand the reason, plus the evidence, to the human.
    Halt {
        reason: StopReason,
        note: String,
    },
}

impl Verdict {
    pub fn is_halt(&self) -> bool {
        matches!(self, Self::Halt { .. })
    }
}

/// One finished tool call, as far as the policy is concerned.
#[derive(Debug, Clone, Copy)]
pub struct ActionObserved<'a> {
    pub name: &'a str,
    pub args: &'a Value,
    /// Human-readable rendering, reused verbatim in the pause message.
    pub label: &'a str,
    pub ok: bool,
    pub turn: u32,
    /// False for reads and observations: repeating those is how an agent looks
    /// at a page again, not how it gets stuck clicking.
    pub changes_state: bool,
}

/// Key that identifies "the same action". Object keys are sorted so `{a,b}` and
/// `{b,a}` match, while values are kept because `click #12` and `click #13` are
/// genuinely different actions.
fn signature(name: &str, args: &Value) -> String {
    let mut out = String::with_capacity(name.len() + 32);
    out.push_str(name);
    out.push('(');
    write_canonical(args, &mut out);
    out.push(')');
    const MAX_KEY_CHARS: usize = 400;
    let chars: Vec<char> = out.chars().collect();
    if chars.len() > MAX_KEY_CHARS {
        out = chars.iter().take(MAX_KEY_CHARS).collect();
        out.push('…');
    }
    out
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(key);
                out.push(':');
                write_canonical(&map[*key], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        other => out.push_str(other.to_string().as_str()),
    }
}

/// Per-run derailment detector. A resumed run starts with a clean slate, because
/// the human has just told it to continue.
pub struct LoopGuard {
    policy: RunPolicy,
    counts: HashMap<String, u32>,
    failures: HashMap<String, u32>,
    last_key: Option<String>,
    repeat_streak: u32,
    any_failure_streak: u32,
    warned_repeat: bool,
    warned_failure: bool,
    watched_since: u32,
    /// Set by the first successful tool call. Until a run has actually done
    /// something, "no progress" is meaningless: turns spent talking or thinking
    /// are handled by the nudge limit, not by this guard.
    acted: bool,
    last_progress_turn: u32,
    last_stale_reflect_turn: u32,
    checkpoint_turn: u32,
    soft_wall_spoken: bool,
    reflections: u32,
}

impl LoopGuard {
    pub fn new(policy: RunPolicy) -> Self {
        Self::with_watch(policy, 0)
    }

    /// `watched_from` is the turn number the guard starts counting at, so a
    /// resumed run is not judged for work it already finished.
    pub fn with_watch(policy: RunPolicy, watched_from: u32) -> Self {
        Self {
            policy,
            counts: HashMap::new(),
            failures: HashMap::new(),
            last_key: None,
            repeat_streak: 0,
            any_failure_streak: 0,
            warned_repeat: false,
            warned_failure: false,
            watched_since: watched_from,
            acted: false,
            last_progress_turn: watched_from,
            last_stale_reflect_turn: watched_from,
            checkpoint_turn: 0,
            soft_wall_spoken: false,
            reflections: 0,
        }
    }

    pub fn policy(&self) -> RunPolicy {
        self.policy
    }

    /// Called once per model turn, before the model is asked for anything.
    pub fn on_turn(&mut self, turns: u32, elapsed: Duration) -> Verdict {
        if turns >= self.policy.hard_cap_turns {
            return Verdict::Halt {
                reason: StopReason::BudgetExhausted,
                note: format!(
                    "已達到保險上限 {cap} 輪（這是迴圈失控的保護，不是任務做完）",
                    cap = self.policy.hard_cap_turns
                ),
            };
        }
        let minutes = elapsed.as_secs() / 60;
        if elapsed >= self.policy.hard_wall {
            return Verdict::Halt {
                reason: StopReason::BudgetExhausted,
                note: format!(
                    "已執行 {minutes} 分鐘，超過 {hard} 分鐘的最後保護上限",
                    hard = self.policy.hard_wall.as_secs() / 60
                ),
            };
        }
        if let Some(halt) = self.stale_check(turns) {
            return halt;
        }
        if self.is_checkpoint(turns) {
            self.checkpoint_turn = turns;
            return self.reflect(checkpoint_text(turns, minutes));
        }
        if !self.soft_wall_spoken && elapsed >= self.policy.soft_wall {
            self.soft_wall_spoken = true;
            return self.reflect(wall_text(minutes, self.policy.hard_wall.as_secs() / 60));
        }
        Verdict::Continue
    }

    /// Called after every tool result.
    pub fn on_action(&mut self, action: &ActionObserved<'_>) -> Verdict {
        let key = signature(action.name, action.args);
        if self.last_key.as_deref() == Some(key.as_str()) {
            self.repeat_streak = self.repeat_streak.saturating_add(1);
        } else {
            self.repeat_streak = 1;
            self.warned_repeat = false;
        }
        let seen = self
            .counts
            .get(&key)
            .copied()
            .unwrap_or(0)
            .saturating_add(1);
        self.counts.insert(key.clone(), seen);
        self.last_key = Some(key.clone());

        if !action.ok {
            self.any_failure_streak = self.any_failure_streak.saturating_add(1);
            let failures = self
                .failures
                .get(&key)
                .copied()
                .unwrap_or(0)
                .saturating_add(1);
            self.failures.insert(key.clone(), failures);
            if failures >= FAILURE_HALT {
                return Verdict::Halt {
                    reason: StopReason::LoopDetected,
                    note: format!(
                        "同一個動作「{label}」連續失敗 {failures} 次",
                        label = action.label
                    ),
                };
            }
            if self.any_failure_streak >= FAILURE_HALT_ANY {
                return Verdict::Halt {
                    reason: StopReason::LoopDetected,
                    note: format!(
                        "連續 {count} 個動作都沒有成功（最後一個：{label}）",
                        count = self.any_failure_streak,
                        label = action.label,
                    ),
                };
            }
            if failures >= FAILURE_WARN && !self.warned_failure {
                self.warned_failure = true;
                return self.reflect(failure_text(action.label, failures));
            }
            return Verdict::Continue;
        }

        self.any_failure_streak = 0;
        self.failures.remove(&key);
        self.warned_failure = false;
        self.acted = true;
        if seen == 1 || PROGRESS_WHEN_REPEATED.contains(&action.name) {
            self.last_progress_turn = action.turn;
        }

        if action.changes_state {
            if self.repeat_streak >= REPEAT_HALT {
                return Verdict::Halt {
                    reason: StopReason::LoopDetected,
                    note: format!(
                        "同一個動作「{label}」連續做了 {streak} 次，沒有新的進展",
                        label = action.label,
                        streak = self.repeat_streak,
                    ),
                };
            }
            if seen >= REPEAT_HALT_TOTAL {
                return Verdict::Halt {
                    reason: StopReason::LoopDetected,
                    note: format!(
                        "同一個動作「{label}」在這輪任務裡已經做了 {seen} 次",
                        label = action.label,
                    ),
                };
            }
            if self.repeat_streak >= REPEAT_WARN && !self.warned_repeat {
                self.warned_repeat = true;
                return self.reflect(repeat_text(action.label, self.repeat_streak));
            }
        }
        Verdict::Continue
    }

    /// Nothing new has succeeded for a long stretch: either the agent is
    /// circling the same wall, or it is honestly waiting and should say so.
    fn stale_check(&mut self, turns: u32) -> Option<Verdict> {
        if !self.acted {
            return None;
        }
        let since = turns.saturating_sub(self.last_progress_turn);
        if since < STALE_REFLECT {
            return None;
        }
        if since >= STALE_HALT {
            return Some(Verdict::Halt {
                reason: StopReason::LoopDetected,
                note: format!(
                    "從第 {watched} 輪起，連續 {since} 輪沒有任何新的成功動作，看起來在鬼打牆",
                    watched = self.watched_since
                ),
            });
        }
        if turns.saturating_sub(self.last_stale_reflect_turn) >= STALE_REFLECT {
            self.last_stale_reflect_turn = turns;
            return Some(self.reflect(stale_text(turns, since)));
        }
        None
    }

    fn is_checkpoint(&self, turns: u32) -> bool {
        if turns < self.policy.soft_checkpoint_turns || turns <= self.checkpoint_turn {
            return false;
        }
        let every = self.policy.soft_checkpoint_every.max(1);
        turns == self.policy.soft_checkpoint_turns
            || turns
                .saturating_sub(self.policy.soft_checkpoint_turns)
                .is_multiple_of(every)
    }

    /// Coaching is capped so a confused model is never nagged forever; the
    /// breakers above still work after the cap is reached.
    fn reflect(&mut self, text: String) -> Verdict {
        if self.reflections >= MAX_REFLECTIONS {
            return Verdict::Continue;
        }
        self.reflections = self.reflections.saturating_add(1);
        Verdict::Reflect(text)
    }
}

fn checkpoint_text(turns: u32, minutes: u64) -> String {
    format!(
        "Checkpoint: {turns} turns, {minutes} minutes in. Write exactly three short lines \
         (已完成 / 還缺 / 下一個動作) and then make that next action with a tool call in the \
         same reply. Do not repeat an action that already produced the same result; if the \
         current plan cannot work, replace it with a different route."
    )
}

fn wall_text(minutes: u64, hard_minutes: u64) -> String {
    format!(
        "Time check: {minutes} minutes on this task. If the work is genuinely long (a build, \
         a download, a queue), say so in one line and continue with a tool. If you are stuck, \
         stop guessing: state what you have tried and what you need, then end with a \
         standalone [NEEDS_INPUT] line. Nothing stops at {hard_minutes} minutes except you."
    )
}

fn repeat_text(label: &str, streak: u32) -> String {
    format!(
        "You have sent the same action {streak} times in a row: {label}. The result you already \
         have is the result you will get again. Do not send it again unchanged: read the last \
         tool result and change something - observe the screen again, scroll, target a selector \
         instead of an id, reload, or use the shell. If only the human can unblock this, say \
         what you need and end with a standalone [NEEDS_INPUT] line."
    )
}

fn failure_text(label: &str, failures: u32) -> String {
    format!(
        "{label} has failed {failures} times in a row. Use the error text: different arguments, \
         a different tool, or a fresh observation of the current state before you retry. If the \
         blocker is outside your reach, say exactly what is missing and end with a standalone \
         [NEEDS_INPUT] line."
    )
}

fn stale_text(turns: u32, since: u32) -> String {
    format!(
        "{turns} turns in, nothing new has succeeded for {since} turns. Step back before the \
         next call: state what you have actually verified, what is still missing, then take one \
         action you have not tried yet. If the task cannot move without the human, summarise the \
         state and end with a standalone [NEEDS_INPUT] line."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn act<'a>(
        name: &'a str,
        args: &'a Value,
        turn: u32,
        ok: bool,
        changes_state: bool,
    ) -> ActionObserved<'a> {
        ActionObserved {
            name,
            args,
            label: "browser: click #12",
            ok,
            turn,
            changes_state,
        }
    }

    #[test]
    fn an_ordinary_long_run_is_never_stopped() {
        let mut guard = LoopGuard::new(RunPolicy::default());
        for turn in 1..=39 {
            assert_eq!(
                guard.on_turn(turn, Duration::from_secs(60 * turn as u64)),
                Verdict::Continue,
                "turn {turn}"
            );
            let args = json!({"command": format!("step {turn}")});
            assert_eq!(
                guard.on_action(&act("shell", &args, turn, true, true)),
                Verdict::Continue,
                "action {turn}"
            );
        }
    }

    #[test]
    fn nothing_in_the_default_policy_stops_a_run_near_turn_forty() {
        // The complaint this replaces was "40 輪真的太少了".
        let policy = RunPolicy::default();
        assert!(policy.hard_cap_turns > 200);
        assert!(policy.hard_wall > Duration::from_secs(120 * 60));
        let mut guard = LoopGuard::new(policy);
        assert_eq!(
            guard.on_turn(40, Duration::from_secs(600)),
            Verdict::Continue
        );
    }

    #[test]
    fn looking_at_the_screen_again_is_not_a_loop() {
        let mut guard = LoopGuard::new(RunPolicy::default());
        let args = json!({});
        for turn in 1..=20 {
            assert_eq!(
                guard.on_action(&act("computer_observe", &args, turn, true, false)),
                Verdict::Continue,
                "turn {turn}"
            );
        }
    }

    #[test]
    fn clicking_the_same_control_learns_to_stop() {
        let mut guard = LoopGuard::new(RunPolicy::default());
        let args = json!({"action": "click", "element": 12});
        for turn in 1..REPEAT_WARN {
            assert_eq!(
                guard.on_action(&act("browser", &args, turn, true, true)),
                Verdict::Continue
            );
        }
        assert!(matches!(
            guard.on_action(&act("browser", &args, REPEAT_WARN, true, true)),
            Verdict::Reflect(_)
        ));
        for turn in REPEAT_WARN + 1..REPEAT_HALT {
            assert_eq!(
                guard.on_action(&act("browser", &args, turn, true, true)),
                Verdict::Continue
            );
        }
        assert!(matches!(
            guard.on_action(&act("browser", &args, REPEAT_HALT, true, true)),
            Verdict::Halt {
                reason: StopReason::LoopDetected,
                ..
            }
        ));
    }

    #[test]
    fn argument_order_does_not_make_the_same_action_look_new() {
        assert_eq!(
            signature("browser", &json!({"element": 12, "action": "click"})),
            signature("browser", &json!({"action": "click", "element": 12}))
        );
        assert_ne!(
            signature("browser", &json!({"action": "click", "element": 12})),
            signature("browser", &json!({"action": "click", "element": 13}))
        );
    }

    #[test]
    fn a_repeated_failure_is_coached_then_parked() {
        let mut guard = LoopGuard::new(RunPolicy::default());
        let args = json!({"command": "npm test"});
        for turn in 1..FAILURE_WARN {
            assert_eq!(
                guard.on_action(&act("shell", &args, turn, false, true)),
                Verdict::Continue
            );
        }
        assert!(matches!(
            guard.on_action(&act("shell", &args, FAILURE_WARN, false, true)),
            Verdict::Reflect(_)
        ));
        for turn in FAILURE_WARN + 1..FAILURE_HALT {
            guard.on_action(&act("shell", &args, turn, false, true));
        }
        assert!(
            guard
                .on_action(&act("shell", &args, FAILURE_HALT, false, true))
                .is_halt()
        );
    }

    #[test]
    fn a_whole_batch_of_different_failures_also_stops() {
        let mut guard = LoopGuard::new(RunPolicy::default());
        for turn in 1..FAILURE_HALT_ANY {
            let args = json!({"command": format!("try {turn}")});
            let verdict = guard.on_action(&act("shell", &args, turn, false, true));
            assert!(!verdict.is_halt(), "halted too early at {turn}");
        }
        let args = json!({"command": "last try"});
        assert!(
            guard
                .on_action(&act("shell", &args, FAILURE_HALT_ANY, false, true))
                .is_halt()
        );
    }

    #[test]
    fn polling_a_build_or_a_queue_is_not_treated_as_spinning() {
        let mut guard = LoopGuard::new(RunPolicy::default());
        let args = json!({"ms": 30000});
        for turn in 1..=160 {
            assert_eq!(
                guard.on_action(&act("wait", &args, turn, true, false)),
                Verdict::Continue,
                "turn {turn}"
            );
            // A checkpoint question is fine; what a polling run must never get
            // is a halt.
            assert!(
                !guard.on_turn(turn, Duration::from_secs(30)).is_halt(),
                "a polling run was halted at turn {turn}"
            );
        }
    }

    #[test]
    fn re_observing_the_same_screen_eventually_stops_asking() {
        let mut guard = LoopGuard::new(RunPolicy::default());
        let first = json!({"command": "ls"});
        guard.on_action(&act("shell", &first, 1, true, true));
        let eye = json!({});
        let mut reflected = 0;
        for turn in 2..=150 {
            guard.on_action(&act("computer_observe", &eye, turn, true, false));
            if matches!(
                guard.on_turn(turn, Duration::from_secs(10)),
                Verdict::Reflect(_)
            ) {
                reflected += 1;
            }
        }
        assert!(reflected >= 1, "the run should be asked to step back");
        assert!(guard.on_turn(152, Duration::from_secs(10)).is_halt());
    }

    #[test]
    fn checkpoints_are_asks_not_stops() {
        let mut guard = LoopGuard::new(RunPolicy::default());
        assert!(matches!(
            guard.on_turn(SOFT_CHECKPOINT_TURNS, Duration::from_secs(600)),
            Verdict::Reflect(_)
        ));
        assert_eq!(
            guard.on_turn(SOFT_CHECKPOINT_TURNS + 1, Duration::from_secs(601)),
            Verdict::Continue
        );
        let next = SOFT_CHECKPOINT_TURNS + SOFT_CHECKPOINT_EVERY;
        assert!(matches!(
            guard.on_turn(next, Duration::from_secs(700)),
            Verdict::Reflect(_)
        ));
    }

    #[test]
    fn coaching_is_rare_and_the_breaker_still_works_after_it() {
        let policy = RunPolicy {
            soft_checkpoint_turns: 10,
            soft_checkpoint_every: 10,
            ..RunPolicy::default()
        };
        let mut guard = LoopGuard::new(policy);
        let mut asks = 0;
        for turn in 1..=120 {
            if matches!(
                guard.on_turn(turn, Duration::from_secs(2)),
                Verdict::Reflect(_)
            ) {
                asks += 1;
            }
            let args = json!({"command": format!("step {turn}")});
            guard.on_action(&act("shell", &args, turn, true, true));
        }
        assert_eq!(asks, MAX_REFLECTIONS as usize, "coaching must stop nagging");
        assert_eq!(
            guard.on_turn(HARD_CAP_TURNS, Duration::from_secs(60)),
            Verdict::Halt {
                reason: StopReason::BudgetExhausted,
                note: format!(
                    "已達到保險上限 {HARD_CAP_TURNS} 輪（這是迴圈失控的保護，不是任務做完）"
                ),
            }
        );
    }

    #[test]
    fn wall_clock_breaks_last_of_all() {
        let mut guard = LoopGuard::new(RunPolicy::default());
        assert!(matches!(
            guard.on_turn(5, Duration::from_secs(SOFT_WALL_MINUTES * 60)),
            Verdict::Reflect(_)
        ));
        assert!(matches!(
            guard.on_turn(6, Duration::from_secs(HARD_WALL_MINUTES * 60 + 1)),
            Verdict::Halt {
                reason: StopReason::BudgetExhausted,
                ..
            }
        ));
    }

    #[test]
    fn a_resumed_run_is_not_judged_for_the_turns_it_already_has() {
        let mut guard = LoopGuard::with_watch(RunPolicy::default(), 120);
        for turn in 120..150 {
            assert_eq!(
                guard.on_turn(turn, Duration::from_secs(60)),
                Verdict::Continue,
                "turn {turn}"
            );
        }
    }

    #[test]
    fn overrides_are_opt_in_and_tolerant() {
        assert_eq!(RunPolicy::from_lookup(|_| None), RunPolicy::default());
        assert_eq!(
            RunPolicy::from_lookup(
                |name| (name == "LAZYBOY_RUN_CAP_TURNS").then(|| "not-a-number".to_string())
            )
            .hard_cap_turns,
            HARD_CAP_TURNS
        );
        assert_eq!(
            RunPolicy::from_lookup(
                |name| (name == "LAZYBOY_RUN_CAP_TURNS").then(|| "25".to_string())
            )
            .hard_cap_turns,
            25
        );
        assert_eq!(
            RunPolicy::from_lookup(
                |name| (name == "LAZYBOY_RUN_HARD_MINUTES").then(|| "30".to_string())
            )
            .hard_wall,
            Duration::from_secs(30 * 60)
        );
    }
}
