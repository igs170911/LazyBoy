use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::controller::ControlError;
use crate::screen::normalize_display;
use crate::{ActionDecision, ActionVerdict};

pub const PRIMARY_SOCKET: &str = "/tmp/lazyboy/cua.sock";

#[derive(Debug, Clone)]
pub struct CuaClient {
    bin: PathBuf,
    motion_sessions: Arc<tokio::sync::Mutex<HashMap<(PathBuf, String), SystemTime>>>,
    /// Displays whose driver session saw a timed-out mutation. The socket state
    /// of such a session is unknown, so the next mutation gets a fresh one.
    suspect: Arc<tokio::sync::Mutex<HashSet<String>>>,
}

impl Default for CuaClient {
    fn default() -> Self {
        Self {
            bin: PathBuf::from("cua-driver"),
            motion_sessions: Arc::default(),
            suspect: Arc::default(),
        }
    }
}

impl CuaClient {
    pub fn socket_for_display(display: &str) -> PathBuf {
        let number = normalize_display(display)
            .trim_start_matches(':')
            .to_string();
        if number == "1" {
            PathBuf::from(PRIMARY_SOCKET)
        } else {
            PathBuf::from(format!("/tmp/lazyboy/cua-{number}.sock"))
        }
    }

    /// Every call runs in its own CLI process, so the driver would otherwise
    /// give each one an implicit session that dies with the process. Trajectory
    /// recording, snapshots, and browser binds only line up under one label.
    pub fn session_for_display(display: &str) -> String {
        let number = normalize_display(display)
            .trim_start_matches(':')
            .to_string();
        if let Ok(name) =
            std::fs::read_to_string(format!("/tmp/lazyboy/screen-{number}.agent-name"))
        {
            let name = public_agent_name(&name);
            if !name.is_empty() {
                return name;
            }
        }
        format!("lazyboy-{number}")
    }

    pub fn dbus_file(display: &str) -> PathBuf {
        let number = normalize_display(display)
            .trim_start_matches(':')
            .to_string();
        PathBuf::from(format!("/tmp/lazyboy/screen-{number}.dbus"))
    }

    pub async fn version(&self) -> Result<String, ControlError> {
        let mut command = Command::new(&self.bin);
        command.arg("--version");
        let output = bounded_output(&mut command, Duration::from_secs(10)).await?;
        if !output.status.success() {
            return Err(ControlError::DriverUnavailable);
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    pub async fn status(&self, display: &str) -> Result<String, ControlError> {
        let socket = Self::socket_for_display(display);
        if !socket.exists() {
            return Err(ControlError::DriverUnavailable);
        }
        let mut command = Command::new(&self.bin);
        command.args(["status", "--socket", &socket.to_string_lossy()]);
        let output = bounded_output(&mut command, Duration::from_secs(10)).await?;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if !output.status.success() || text.to_ascii_lowercase().contains("not running") {
            return Err(ControlError::DriverUnhealthy);
        }
        Ok(text)
    }

    /// `cua-driver manifest`: the driver's own description of its CLI surface.
    /// `None` when the binary predates the verb or answers with anything but a
    /// JSON object, so an unexpected driver stays as opaque as it was.
    pub async fn manifest(&self) -> Option<Value> {
        let mut command = Command::new(&self.bin);
        command.arg("manifest");
        let output = bounded_output(&mut command, Duration::from_secs(10))
            .await
            .ok()?;
        if !output.status.success() {
            return None;
        }
        // A telemetry banner can precede the payload, so take the line that
        // actually parses instead of trusting stdout to be one object.
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(|line| serde_json::from_str(line.trim()).ok())
    }

    /// `cua-driver list-tools`: every tool this driver build can dispatch.
    /// Read-only and daemon-free, which makes it safe on the health path.
    pub async fn tool_names(&self) -> Option<Vec<String>> {
        let mut command = Command::new(&self.bin);
        command.arg("list-tools");
        let output = bounded_output(&mut command, Duration::from_secs(10))
            .await
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let names = parse_tool_names(&text);
        (!names.is_empty()).then_some(names)
    }

    // Apply Cua's supported motion settings once per named session/daemon.
    // Short, tight glides avoid the driver's 750 ms default flight.
    async fn configure_cursor_motion(&self, screen: &str, body: &Value, force: bool) {
        let Some(session) = body.get("session").and_then(Value::as_str) else {
            return;
        };
        let socket = Self::socket_for_display(screen);
        let Ok(stamp) = std::fs::metadata(&socket).and_then(|meta| meta.modified()) else {
            return;
        };
        let key = (socket, session.to_owned());
        {
            let configured = self.motion_sessions.lock().await;
            if !force && configured.get(&key) == Some(&stamp) {
                return;
            }
        }
        let _ = tokio::time::timeout(
            Duration::from_millis(500),
            self.attempt(
                screen,
                "set_agent_cursor_motion",
                &json!({
                    "session":session,"glide_duration_ms":120,"turn_radius":8,
                    "spring":1,"dwell_after_click_ms":40,
                }),
                &[],
            ),
        )
        .await;
        let mut configured = self.motion_sessions.lock().await;
        if configured.len() >= 128 {
            configured.clear();
        }
        // Remember the attempt even when the tool refuses, so a missing or
        // slow overlay cannot add 500 ms to every later click.
        configured.insert(key, stamp);
    }

    pub async fn call(
        &self,
        screen: &str,
        tool: &str,
        payload: &Value,
        extra: &[&str],
    ) -> Result<Value, ControlError> {
        let mut body = with_session_label(screen, payload);
        if !read_after_session_restart(tool) {
            self.repair_suspect_session(screen, &body).await?;
        }
        if needs_cursor_motion(tool) {
            self.configure_cursor_motion(screen, &body, false).await;
        }
        let mut escalated = false;
        let mut revived = false;
        loop {
            let outcome = match self.attempt(screen, tool, &body, extra).await {
                Ok(outcome) => outcome,
                Err(ControlError::Timeout) => {
                    // A mutation that ran out of clock may still have landed, so
                    // it is never replayed: mark the session unusable and report
                    // the unknown outcome instead of trying again.
                    if tool != "start_session" {
                        self.suspect
                            .lock()
                            .await
                            .insert(normalize_display(screen).to_string());
                    }
                    return Err(ControlError::Timeout);
                }
                Err(error) => return Err(error),
            };
            if outcome.session_ended && !revived && tool != "start_session" {
                let session = json!({"session":body["session"]});
                let started = self.attempt(screen, "start_session", &session, &[]).await?;
                if let Some(error) = started.error {
                    return Err(error);
                }
                revived = true;
                self.configure_cursor_motion(screen, &body, true).await;
                if !read_after_session_restart(tool) {
                    return Err(ControlError::StaleReference);
                }
                continue;
            }
            if !escalated
                && outcome.error.is_some()
                && let Some(mode) = recommended_delivery(&outcome.value)
                && let Some(map) = body.as_object_mut()
            {
                map.insert("delivery_mode".to_string(), json!(mode));
                escalated = true;
                continue;
            }
            return match outcome.error {
                Some(error) => Err(error),
                None => Ok(outcome.value),
            };
        }
    }

    /// One timed-out call is enough to distrust a session: the request may
    /// still be queued, half-written, or already applied. The next mutation
    /// therefore opens a fresh named session instead of inheriting that state.
    /// Reads skip the extra round trip — a stale read cannot do damage, and a
    /// replayed mutation can.
    async fn repair_suspect_session(&self, screen: &str, body: &Value) -> Result<(), ControlError> {
        let key = normalize_display(screen).to_string();
        if !self.suspect.lock().await.remove(&key) {
            return Ok(());
        }
        let session = json!({"session": body.get("session")});
        match self.attempt(screen, "start_session", &session, &[]).await {
            Ok(outcome) => {
                // A driver that answers, even to refuse, proved the transport is
                // alive; the mutation's own error is the better signal then.
                if outcome.error.is_none() {
                    // A new session does not inherit the cursor glide tuning.
                    self.configure_cursor_motion(screen, body, true).await;
                }
                Ok(())
            }
            Err(error @ ControlError::Timeout) => {
                // Still unknown: keep the marker and do not push a mutation into
                // the same state that just timed out.
                self.suspect.lock().await.insert(key);
                Err(error)
            }
            Err(error) => Err(error),
        }
    }

    async fn attempt(
        &self,
        screen: &str,
        tool: &str,
        payload: &Value,
        extra: &[&str],
    ) -> Result<Outcome, ControlError> {
        let socket = Self::socket_for_display(screen);
        if !socket.exists() {
            return Err(ControlError::DriverUnavailable);
        }
        let mut command = Command::new(&self.bin);
        command
            .env("DISPLAY", normalize_display(screen))
            .env(
                "CUA_DRIVER_RS_HOME",
                format!(
                    "/tmp/lazyboy/cua-home-{}",
                    normalize_display(screen).trim_start_matches(':')
                ),
            )
            .args(["call", "--socket", &socket.to_string_lossy()]);
        command.args(extra);
        command.arg(tool);

        apply_desktop_bus(&mut command, screen);
        let started = Instant::now();
        let output = bounded_input_output(&mut command, payload).await?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let (value, error) = decode_stdout(
            &stdout,
            &stderr,
            output.status.success(),
            classify_cua_failure,
        );
        tracing::info!(
            backend = "cua",
            tool,
            screen,
            duration_ms = started.elapsed().as_millis() as u64,
            success = error.is_none()
        );
        let session_ended = !output.status.success()
            && [&*stdout, &*stderr].iter().any(|text| {
                text.trim_start().starts_with("session '") && text.contains("has ended; tool call")
            });
        Ok(Outcome {
            value,
            error,
            session_ended,
        })
    }
}

fn public_agent_name(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|ch| !ch.is_control())
        .take(80)
        .collect()
}

/// `list-tools` prints `tool_name: one-line description`. The CLI also prints
/// banners and usage text whose leading token is lowercase-shaped, so these
/// words are dropped before they can pass for a capability.
const TOOL_LIST_NOISE: &[&str] = &[
    "usage",
    "error",
    "warning",
    "warn",
    "note",
    "help",
    "subcommands",
];

fn parse_tool_names(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let (name, _) = line.split_once(':')?;
            let name = name.trim();
            let shaped = name.len() > 2
                && !TOOL_LIST_NOISE.contains(&name)
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
            shaped.then(|| name.to_string())
        })
        .collect()
}

fn needs_cursor_motion(tool: &str) -> bool {
    matches!(
        tool,
        "click"
            | "drag"
            | "move_cursor"
            | "scroll"
            | "type_text"
            | "press_key"
            | "hotkey"
            | "mouse_button_down"
            | "mouse_button_up"
            | "mouse_drag"
            | "browser_click"
            | "browser_type"
            | "browser_navigate"
            | "set_value"
    )
}

fn read_after_session_restart(tool: &str) -> bool {
    matches!(
        tool,
        "get_desktop_state"
            | "list_windows"
            | "get_window_state"
            | "get_accessibility_tree"
            | "get_browser_state"
            | "health_report"
            | "get_cursor_position"
    )
}

struct Outcome {
    session_ended: bool,
    value: Value,
    error: Option<ControlError>,
}

/// Refusals and prose diagnostics can arrive with exit code 0, so only a JSON
/// object counts as proof that the driver ran the tool.
fn decode_stdout(
    stdout: &str,
    stderr: &str,
    success: bool,
    classify: impl Fn(&str) -> ControlError,
) -> (Value, Option<ControlError>) {
    let combined = format!("{stdout}\n{stderr}");
    let trimmed = stdout.trim();
    let empty = Value::Null;
    let decoded = (!trimmed.is_empty())
        .then(|| parse_jsonish(trimmed))
        .flatten()
        .filter(Value::is_object);
    let error = if !success || trimmed.starts_with('\u{274c}') || decoded.is_none() {
        Some(classify(&combined))
    } else {
        response_error(decoded.as_ref().unwrap_or(&empty))
    };
    (decoded.unwrap_or(empty), error)
}

/// The documented contract for `background_unavailable` is one retry with the
/// delivery mode named in the structured escalation.
fn recommended_delivery(value: &Value) -> Option<String> {
    ["/escalation/recommended", "/error/escalation/recommended"]
        .into_iter()
        .filter_map(|pointer| value.pointer(pointer).and_then(Value::as_str))
        .find(|mode| matches!(*mode, "foreground" | "background"))
        .map(str::to_string)
}

/// What the driver says about the effect of its own reply. Every field is
/// additive in the driver contract, so an older build yields `None`: a reply
/// with no semantic evidence is not proof that anything happened, and claiming
/// `done` would be the difference between one look and one more blind click.
pub fn action_verdict(value: &Value) -> Option<ActionVerdict> {
    let effect = ["effect", "status"]
        .into_iter()
        .filter_map(|key| value.get(key).and_then(Value::as_str))
        // `status: ok` is the driver's "no error", the same reading
        // `response_error` takes, so it carries no evidence either.
        .find(|text| !text.is_empty() && *text != "ok")
        .map(str::to_string);
    let verified = value.get("verified").and_then(Value::as_bool);
    let degraded = value.get("degraded").and_then(Value::as_bool) == Some(true);
    let escalation = recommended_delivery(value);
    let code = ["code", "reason_code"]
        .into_iter()
        .filter_map(|key| value.get(key).and_then(Value::as_str))
        .find(|code| !code.is_empty() && *code != "ok");

    let decision = if effect.as_deref() == Some("confirmed") || verified == Some(true) {
        ActionDecision::Done
    } else if effect.as_deref() == Some("suspected_noop") || code.is_some() {
        ActionDecision::Escalate
    } else if effect.is_some() || verified == Some(false) || degraded || escalation.is_some() {
        ActionDecision::VerifyFreshState
    } else {
        return None;
    };
    Some(ActionVerdict {
        decision,
        effect,
        verified,
        escalation,
    })
}

fn with_session_label(display: &str, payload: &Value) -> Value {
    let mut body = payload.clone();
    if let Some(map) = body.as_object_mut() {
        map.entry("session")
            .or_insert_with(|| json!(CuaClient::session_for_display(display)));
    }
    body
}

fn response_error(value: &Value) -> Option<ControlError> {
    if value
        .get("code")
        .and_then(Value::as_str)
        .is_some_and(|code| code != "ok")
        || value.get("ok").and_then(Value::as_bool) == Some(false)
        || value.get("isError").and_then(Value::as_bool) == Some(true)
        || ["status", "effect"].iter().any(|key| {
            matches!(
                value.get(*key).and_then(Value::as_str),
                Some("refused" | "error")
            )
        })
    {
        Some(classify_cua_failure(&value.to_string()))
    } else {
        None
    }
}

async fn bounded_input_output(
    command: &mut Command,
    payload: &Value,
) -> Result<std::process::Output, ControlError> {
    command
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    tokio::time::timeout(Duration::from_secs(120), async {
        let mut child = command
            .spawn()
            .map_err(|_| ControlError::DriverUnavailable)?;
        let mut input = child.stdin.take().ok_or(ControlError::DriverUnhealthy)?;
        let bytes = payload.to_string();
        // Drain stdout/stderr while writing so large inputs cannot deadlock.
        let write = async {
            input.write_all(bytes.as_bytes()).await?;
            input.shutdown().await?;
            drop(input);
            Ok::<(), std::io::Error>(())
        };
        let (written, output) = tokio::join!(write, child.wait_with_output());
        // A driver that exits before reading stdin (unknown tool, rejected
        // arguments) closes the pipe, so a broken pipe is expected and the exit
        // status plus stderr hold the real reason. Losing them here would
        // downgrade every fast refusal to a generic driver failure.
        if let Err(error) = written
            && error.kind() != std::io::ErrorKind::BrokenPipe
        {
            return Err(ControlError::DriverUnhealthy);
        }
        output.map_err(|_| ControlError::DriverUnhealthy)
    })
    .await
    .map_err(|_| ControlError::Timeout)?
}

async fn bounded_output(
    command: &mut Command,
    timeout: Duration,
) -> Result<std::process::Output, ControlError> {
    command.kill_on_drop(true);
    tokio::time::timeout(timeout, command.output())
        .await
        .map_err(|_| ControlError::Timeout)?
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => ControlError::DriverUnavailable,
            std::io::ErrorKind::PermissionDenied => ControlError::PermissionDenied,
            _ => ControlError::DriverUnhealthy,
        })
}

fn apply_desktop_bus(command: &mut Command, display: &str) {
    let dbus = CuaClient::dbus_file(display);
    if let Ok(address) = std::fs::read_to_string(&dbus) {
        let address = address.trim();
        if !address.is_empty() {
            command.env("DBUS_SESSION_BUS_ADDRESS", address);
        }
    }
    let number = normalize_display(display)
        .trim_start_matches(':')
        .to_string();
    let runtime = PathBuf::from(format!("/tmp/lazyboy/screen-{number}.runtime"));
    if let Ok(value) = std::fs::read_to_string(runtime) {
        let value = value.trim();
        if !value.is_empty() {
            command.env("XDG_RUNTIME_DIR", value);
        }
    }
}

pub fn parse_jsonish(text: &str) -> Option<Value> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Ok(value) = serde_json::from_str::<Value>(text) {
        return Some(value);
    }
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end > start {
        serde_json::from_str(&text[start..=end]).ok()
    } else {
        None
    }
}

pub fn walk<'a>(value: &'a Value, out: &mut Vec<&'a Value>) {
    out.push(value);
    match value {
        Value::Object(map) => {
            for item in map.values() {
                walk(item, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                walk(item, out);
            }
        }
        _ => {}
    }
}

pub fn first_array_of_objects<'a>(value: &'a Value, required: &str) -> Vec<&'a Value> {
    let mut nodes = Vec::new();
    walk(value, &mut nodes);
    for node in nodes {
        if let Some(items) = node.as_array()
            && items.iter().any(|item| item.get(required).is_some())
        {
            return items.iter().collect();
        }
        if let Some(items) = node.get(required).and_then(Value::as_array)
            && items.iter().any(Value::is_object)
        {
            return items.iter().collect();
        }
    }
    Vec::new()
}

fn classify_cua_failure(text: &str) -> ControlError {
    let lower = text.to_ascii_lowercase();
    if lower.contains("stale")
        || lower.contains("not a live binding in this session")
        || (lower.contains("session") && lower.contains("ended"))
    {
        ControlError::StaleReference
    } else if lower.contains("not_found") || lower.contains("not found") {
        ControlError::TargetNotFound
    } else if lower.contains("timeout") {
        ControlError::Timeout
    } else if lower.contains("permission") || lower.contains("consent") {
        ControlError::PermissionDenied
    } else if lower.contains("invalid_action_target") {
        ControlError::InvalidAction("Cua rejected the action target".into())
    } else if lower.contains("unsupported") || lower.contains("route_unavailable") {
        ControlError::Unsupported
    } else {
        // Driver diagnostics can echo typed text or credentials. Keep raw
        // output out of Agent-visible errors and downstream logs.
        ControlError::DriverUnhealthy
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn agent_name_preserves_unicode_without_control_characters() {
        assert_eq!(
            super::public_agent_name("  小幫手\n Alice\u{0007}  "),
            "小幫手 Alice"
        );
        assert_eq!(
            super::public_agent_name(&"小".repeat(100)).chars().count(),
            80
        );
    }

    #[test]
    fn session_label_is_the_agent_name_even_when_a_color_file_exists() {
        use std::fs::{self, File};
        use std::io::Write;

        let display = ":1903";
        let number = super::normalize_display(display)
            .trim_start_matches(':')
            .to_string();
        let name_path = format!("/tmp/lazyboy/screen-{number}.agent-name");
        let color_path = format!("/tmp/lazyboy/screen-{number}.agent-color");
        let old_name = fs::read_to_string(&name_path).ok();
        let old_color = fs::read_to_string(&color_path).ok();

        fs::create_dir_all("/tmp/lazyboy").unwrap();
        {
            let mut file = File::create(&color_path).unwrap();
            file.write_all(b"#8b5cf6").unwrap();
        }
        {
            let mut file = File::create(&name_path).unwrap();
            file.write_all("\n小幫手 Alice\n".as_bytes()).unwrap();
        }
        assert_eq!(
            super::CuaClient::session_for_display(display),
            "小幫手 Alice"
        );

        fs::remove_file(&name_path).unwrap();
        assert_eq!(
            super::CuaClient::session_for_display(display),
            "lazyboy-1903"
        );

        if let Some(value) = old_name {
            fs::write(&name_path, value).unwrap();
        } else {
            let _ = fs::remove_file(&name_path);
        }
        if let Some(value) = old_color {
            fs::write(&color_path, value).unwrap();
        } else {
            let _ = fs::remove_file(&color_path);
        }
    }

    #[test]
    fn cursor_motion_is_only_configured_before_visible_input() {
        assert!(super::needs_cursor_motion("click"));
        assert!(super::needs_cursor_motion("browser_click"));
        assert!(super::needs_cursor_motion("type_text"));
        assert!(!super::needs_cursor_motion("get_desktop_state"));
        assert!(!super::needs_cursor_motion("list_windows"));
        assert!(!super::needs_cursor_motion("health_report"));
        assert!(!super::needs_cursor_motion("set_agent_cursor_motion"));
    }

    use super::*;

    #[test]
    fn expired_sessions_only_retry_observations() {
        assert_eq!(
            classify_cua_failure(
                "confirmation provider failed: target bt-old is not a live binding in this session — re-run get_browser_state with pid + window_id"
            ),
            ControlError::StaleReference
        );
        assert!(read_after_session_restart("get_desktop_state"));
        assert!(read_after_session_restart("get_browser_state"));
        for tool in ["click", "type_text", "browser_type", "hotkey", "launch_app"] {
            assert!(!read_after_session_restart(tool));
        }
    }

    #[test]
    fn structured_refusals_are_errors_but_page_text_is_not() {
        assert!(response_error(&serde_json::json!({"code": "invalid_action_target"})).is_some());
        assert!(matches!(
            response_error(
                &json!({"effect":"refused","escalation":{"reason":"route_unavailable"}})
            ),
            Some(ControlError::Unsupported)
        ));
        assert!(response_error(&serde_json::json!({"outline": "❌ payment declined"})).is_none());
    }

    #[test]
    fn only_a_json_object_proves_the_tool_ran() {
        let classify = |text: &str| classify_cua_failure(text);
        let (value, error) = decode_stdout(r#"{"width":1280}"#, "", true, classify);
        assert!(error.is_none());
        assert_eq!(value["width"], 1280);

        for malformed in ["null", "true", "42", "[]", r#""ok""#] {
            let (_, error) = decode_stdout(malformed, "", true, classify);
            assert!(error.is_some(), "non-object response accepted: {malformed}");
        }

        // Prose with a zero exit code used to be reported as success.
        let (value, error) = decode_stdout("no window matched", "", true, classify);
        assert!(matches!(error, Some(ControlError::DriverUnhealthy)));
        assert!(value.is_null());

        let (_, error) = decode_stdout("\u{274c} unsupported tool", "", true, classify);
        assert!(matches!(error, Some(ControlError::Unsupported)));

        let (_, error) = decode_stdout(r#"{"code":"background_unavailable"}"#, "", true, classify);
        assert!(error.is_some());

        // A crash with empty stdout must never look like an empty success.
        let (_, error) = decode_stdout("", "signal: 11", false, classify);
        assert!(error.is_some());
    }

    #[test]
    fn escalation_is_read_from_the_documented_pointers() {
        assert_eq!(
            recommended_delivery(&json!({"escalation": {"recommended": "foreground"}})).as_deref(),
            Some("foreground")
        );
        assert_eq!(
            recommended_delivery(&json!({"error": {"escalation": {"recommended": "background"}}}))
                .as_deref(),
            Some("background")
        );
        assert!(recommended_delivery(&json!({"escalation": {"recommended": "reboot"}})).is_none());
        assert!(recommended_delivery(&json!({"ok": true})).is_none());
    }

    #[tokio::test]
    async fn piped_json_reaches_eof_without_argv_exposure() {
        let mut command = Command::new("sh");
        command.args(["-c", "cat"]);
        let payload = serde_json::json!({"text": "秘密🙂"});
        let output = tokio::time::timeout(
            Duration::from_secs(2),
            bounded_input_output(&mut command, &payload),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap(),
            payload
        );
    }

    #[tokio::test]
    async fn hung_driver_is_bounded() {
        let mut command = Command::new("sh");
        command.args(["-c", "exec sleep 30"]);
        assert!(matches!(
            bounded_output(&mut command, Duration::from_millis(20)).await,
            Err(ControlError::Timeout)
        ));
    }

    #[tokio::test]
    async fn driver_that_never_reads_stdin_still_reports_its_exit() {
        // Bigger than the pipe buffer: the child never reads, so the write can
        // only fail with EPIPE and must not swallow the driver's own error.
        let payload = json!({ "blob": "x".repeat(1 << 20) });
        let mut command = Command::new("sh");
        command.args(["-c", "echo invalid_action_target >&2; exit 1"]);
        let output = tokio::time::timeout(
            Duration::from_secs(10),
            bounded_input_output(&mut command, &payload),
        )
        .await
        .expect("bounded")
        .expect("exit status survives a broken stdin pipe");
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("invalid_action_target"),
            "driver stderr must reach the classifier"
        );
    }

    #[test]
    fn unknown_driver_failure_does_not_echo_secret() {
        let error = classify_cua_failure("failed typing secret-password");
        assert_eq!(error, ControlError::DriverUnhealthy);
    }

    #[test]
    fn primary_display_uses_well_known_socket() {
        assert_eq!(
            CuaClient::socket_for_display(":1"),
            PathBuf::from(PRIMARY_SOCKET)
        );
        assert_eq!(
            CuaClient::socket_for_display(":2"),
            PathBuf::from("/tmp/lazyboy/cua-2.sock")
        );
    }

    #[test]
    fn extracts_json_object_from_noisy_stdout() {
        let parsed = parse_jsonish("✅ ok\n{\"status\":\"ok\",\"x\":1}\n").unwrap();
        assert_eq!(parsed["status"], "ok");
    }

    #[test]
    fn a_confirmed_effect_outranks_an_advisory_escalation() {
        let verdict = action_verdict(&json!({
            "effect": "confirmed",
            "verified": true,
            "escalation": {"recommended": "foreground"}
        }))
        .expect("verdict");
        assert_eq!(verdict.decision, ActionDecision::Done);
        assert_eq!(verdict.escalation.as_deref(), Some("foreground"));
    }

    #[test]
    fn unverifiable_effect_becomes_verify_fresh_state() {
        assert_eq!(
            action_verdict(&json!({"effect": "unverifiable"}))
                .expect("verdict")
                .decision,
            ActionDecision::VerifyFreshState
        );
        assert_eq!(
            action_verdict(&json!({"verified": false}))
                .expect("verdict")
                .decision,
            ActionDecision::VerifyFreshState
        );
    }

    #[test]
    fn suspected_noop_and_refusal_codes_escalate() {
        assert_eq!(
            action_verdict(&json!({"effect": "suspected_noop"}))
                .expect("verdict")
                .decision,
            ActionDecision::Escalate
        );
        assert_eq!(
            action_verdict(&json!({"reason_code": "target_gone"}))
                .expect("verdict")
                .decision,
            ActionDecision::Escalate
        );
    }

    #[test]
    fn a_reply_without_semantic_evidence_claims_nothing() {
        // `status: ok` only means the driver did not error, so it proves no
        // effect and must not be reported as one.
        assert!(action_verdict(&json!({"status": "ok", "windows": []})).is_none());
        assert_eq!(
            action_verdict(&json!({"degraded": true}))
                .expect("verdict")
                .decision,
            ActionDecision::VerifyFreshState
        );
    }

    #[test]
    fn the_tool_list_keeps_names_and_drops_banner_noise() {
        let text = "Cua Driver sends content-free product telemetry by default.\n\
                    usage: cua-driver [SUBCOMMAND]\n\
                    click: Click against a target pid\n\
                    start_session: Open a named session\n";
        assert_eq!(
            parse_tool_names(text),
            vec!["click".to_string(), "start_session".to_string()]
        );
    }
}
