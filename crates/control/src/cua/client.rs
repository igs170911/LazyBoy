use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::controller::ControlError;
use crate::screen::normalize_display;

pub const PRIMARY_SOCKET: &str = "/tmp/lazyboy/cua.sock";

#[derive(Debug, Clone)]
pub struct CuaClient {
    bin: PathBuf,
}

impl Default for CuaClient {
    fn default() -> Self {
        Self {
            bin: PathBuf::from("cua-driver"),
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
        format!(
            "lazyboy-{}",
            normalize_display(display).trim_start_matches(':')
        )
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

    pub async fn call(
        &self,
        screen: &str,
        tool: &str,
        payload: &Value,
        extra: &[&str],
    ) -> Result<Value, ControlError> {
        let mut body = with_session_label(screen, payload);
        let mut escalated = false;
        loop {
            let outcome = self.attempt(screen, tool, &body, extra).await?;
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
        Ok(Outcome { value, error })
    }
}

struct Outcome {
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
        || matches!(
            value.get("status").and_then(Value::as_str),
            Some("refused" | "error")
        )
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
    if lower.contains("stale") || (lower.contains("session") && lower.contains("ended")) {
        ControlError::StaleReference
    } else if lower.contains("not_found") || lower.contains("not found") {
        ControlError::TargetNotFound
    } else if lower.contains("timeout") {
        ControlError::Timeout
    } else if lower.contains("permission") || lower.contains("consent") {
        ControlError::PermissionDenied
    } else if lower.contains("invalid_action_target") {
        ControlError::InvalidAction("Cua rejected the action target".into())
    } else if lower.contains("unsupported") {
        ControlError::Unsupported
    } else {
        // Driver diagnostics can echo typed text or credentials. Keep raw
        // output out of Agent-visible errors and downstream logs.
        ControlError::DriverUnhealthy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_refusals_are_errors_but_page_text_is_not() {
        assert!(response_error(&serde_json::json!({"code": "invalid_action_target"})).is_some());
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
}
