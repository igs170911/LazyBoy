use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;

use crate::cua::CuaController;
use crate::legacy::LegacyController;
use crate::{
    ActionRequest, ActionResult, BrowserRequest, CdpPage, RecordingRequest, RecordingResult,
    RecordingSession,
};
use lazyboy_contracts::ComputerObservation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlContext {
    pub display: String,
    pub profile_path: Option<String>,
}

impl ControlContext {
    pub fn new(display: impl Into<String>, profile_path: Option<String>) -> Self {
        Self {
            display: display.into(),
            profile_path,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerHealth {
    pub backend: String,
    pub version: Option<String>,
    pub healthy: bool,
    pub degraded: bool,
    pub details: Vec<String>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ControlError {
    #[error("computer driver is not installed")]
    DriverUnavailable,
    #[error("computer driver is unhealthy")]
    DriverUnhealthy,
    #[error("display is unavailable")]
    DisplayUnavailable,
    #[error("accessibility is unavailable")]
    AccessibilityUnavailable,
    #[error("browser is unavailable")]
    BrowserUnavailable,
    #[error("target not found")]
    TargetNotFound,
    #[error("stale UI reference; take a fresh observation")]
    StaleReference,
    #[error("permission denied")]
    PermissionDenied,
    #[error("computer action timed out")]
    Timeout,
    #[error("unsupported computer action")]
    Unsupported,
    #[error("computer is busy")]
    Busy,
    #[error("{0}")]
    InvalidAction(String),
    #[error("{0}")]
    Internal(String),
}

impl ControlError {
    pub fn internal(text: impl Into<String>) -> Self {
        let text = text.into();
        Self::Internal(truncate_error(&text))
    }

    pub fn is_client_error(&self) -> bool {
        matches!(
            self,
            Self::TargetNotFound
                | Self::StaleReference
                | Self::Unsupported
                | Self::InvalidAction(_)
                | Self::PermissionDenied
        )
    }
}

fn truncate_error(text: &str) -> String {
    const LIMIT: usize = 800;
    let trimmed = text.trim();
    if trimmed.len() <= LIMIT {
        trimmed.to_string()
    } else {
        format!("{}…", &trimmed[..trimmed.floor_char_boundary(LIMIT)])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputerDriver {
    Legacy,
    Cua,
}

impl ComputerDriver {
    pub const ENV: &'static str = "LAZYBOY_COMPUTER_DRIVER";

    pub fn from_env() -> Self {
        match std::env::var(Self::ENV) {
            Ok(value) if value.trim().is_empty() => Self::Legacy,
            Ok(value) => match value.parse() {
                Ok(driver) => driver,
                Err(error) => {
                    tracing::error!("{error}; using legacy");
                    Self::Legacy
                }
            },
            Err(_) => Self::Legacy,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::Cua => "cua",
        }
    }

    pub fn controller(self) -> Arc<dyn ComputerController> {
        match self {
            Self::Legacy => Arc::new(LegacyController),
            Self::Cua => Arc::new(CuaController::default()),
        }
    }
}

impl FromStr for ComputerDriver {
    type Err = UnknownComputerDriver;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "legacy" => Ok(Self::Legacy),
            "cua" => Ok(Self::Cua),
            other => Err(UnknownComputerDriver(other.to_string())),
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("unknown computer driver {0:?}; expected legacy or cua")]
pub struct UnknownComputerDriver(pub String);

#[async_trait]
pub trait ComputerController: Send + Sync {
    fn backend(&self) -> ComputerDriver;

    async fn health(&self, ctx: &ControlContext) -> Result<ControllerHealth, ControlError>;

    async fn observe(&self, ctx: &ControlContext) -> Result<ComputerObservation, ControlError>;

    async fn act(
        &self,
        request: &ActionRequest,
        ctx: &ControlContext,
    ) -> Result<ActionResult, ControlError>;

    async fn browser(
        &self,
        request: &BrowserRequest,
        ctx: &ControlContext,
    ) -> Result<CdpPage, ControlError>;

    async fn start_recording(
        &self,
        request: &RecordingRequest,
        ctx: &ControlContext,
    ) -> Result<RecordingSession, ControlError>;

    async fn stop_recording(
        &self,
        request: &RecordingRequest,
        ctx: &ControlContext,
    ) -> Result<(), ControlError>;

    async fn collect_recording(
        &self,
        request: &RecordingRequest,
        ctx: &ControlContext,
    ) -> Result<RecordingResult, ControlError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_driver_names() {
        assert_eq!(
            "legacy".parse::<ComputerDriver>().unwrap(),
            ComputerDriver::Legacy
        );
        assert_eq!(
            "CUA".parse::<ComputerDriver>().unwrap(),
            ComputerDriver::Cua
        );
        assert!("xdotool".parse::<ComputerDriver>().is_err());
        assert_eq!(ComputerDriver::Cua.as_str(), "cua");
    }

    #[test]
    fn long_unicode_errors_do_not_panic() {
        let text = "錯".repeat(300);
        let error = ControlError::internal(&text).to_string();
        assert!(error.ends_with('…'));
        assert!(error.len() <= 803);
        assert!(text.starts_with(error.trim_end_matches('…')));
    }

    #[test]
    fn invalid_browser_action_is_a_client_error() {
        let error = ControlError::InvalidAction(
            "browser navigate only accepts http, https, or about URLs".into(),
        );
        assert!(error.is_client_error());
    }
}
