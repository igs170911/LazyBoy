use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::Value;
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

    pub fn dbus_file(display: &str) -> PathBuf {
        let number = normalize_display(display)
            .trim_start_matches(':')
            .to_string();
        PathBuf::from(format!("/tmp/lazyboy/screen-{number}.dbus"))
    }

    pub async fn version(&self) -> Result<String, ControlError> {
        let output = Command::new(&self.bin)
            .arg("--version")
            .output()
            .await
            .map_err(|_| ControlError::DriverUnavailable)?;
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
        let output = Command::new(&self.bin)
            .args(["status", "--socket", &socket.to_string_lossy()])
            .output()
            .await
            .map_err(|_| ControlError::DriverUnavailable)?;
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
        let socket = Self::socket_for_display(screen);
        if !socket.exists() {
            return Err(ControlError::DriverUnavailable);
        }
        let mut command = Command::new(&self.bin);
        command
            .env("DISPLAY", normalize_display(screen))
            .env(
                "CUA_DRIVER_RS_HOME",
                std::env::var("CUA_DRIVER_RS_HOME")
                    .unwrap_or_else(|_| "/tmp/lazyboy/cua-home".into()),
            )
            .args(["call", "--socket", &socket.to_string_lossy()]);
        command.args(extra);
        command.arg(tool);
        command.arg(payload.to_string());
        apply_desktop_bus(&mut command, screen);
        let started = Instant::now();
        let output = command
            .output()
            .await
            .map_err(|error| ControlError::internal(error.to_string()))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{stdout}\n{stderr}");
        tracing::info!(
            backend = "cua",
            tool,
            screen,
            duration_ms = started.elapsed().as_millis() as u64,
            success = output.status.success() && !combined.contains('❌')
        );
        if !output.status.success() || combined.contains('❌') {
            return Err(classify_cua_failure(&combined));
        }
        Ok(parse_jsonish(&stdout).unwrap_or(Value::String(stdout.trim().to_string())))
    }
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
    if lower.contains("stale") {
        ControlError::StaleReference
    } else if lower.contains("not_found") || lower.contains("not found") {
        ControlError::TargetNotFound
    } else if lower.contains("timeout") {
        ControlError::Timeout
    } else if lower.contains("permission") || lower.contains("consent") {
        ControlError::PermissionDenied
    } else if lower.contains("unsupported") {
        ControlError::Unsupported
    } else if Path::new(PRIMARY_SOCKET)
        .parent()
        .is_some_and(|dir| !dir.exists())
    {
        ControlError::DriverUnavailable
    } else {
        ControlError::internal(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
