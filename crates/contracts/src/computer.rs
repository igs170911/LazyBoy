use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComputerMode {
    Team,
    Dedicated,
}

impl ComputerMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Team => "team",
            Self::Dedicated => "dedicated",
        }
    }
}

impl std::str::FromStr for ComputerMode {
    type Err = UnknownComputerMode;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "team" => Ok(Self::Team),
            "dedicated" => Ok(Self::Dedicated),
            other => Err(UnknownComputerMode(other.to_string())),
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("unknown computer mode: {0}")]
pub struct UnknownComputerMode(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComputerState {
    Stopped,
    Booting,
    Running,
    Suspended,
    Error,
}

impl ComputerState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Booting => "booting",
            Self::Running => "running",
            Self::Suspended => "suspended",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ControlHolder {
    None,
    Bot,
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SandboxKind {
    Docker,
}

impl SandboxKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Docker => "docker",
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("dedicated computers require a bot id")]
pub struct DedicatedRequiresBotId;

/// Stable identity for upserting a computer row.
pub fn computer_scope_key(
    mode: ComputerMode,
    space_id: &str,
    bot_id: Option<&str>,
) -> Result<String, DedicatedRequiresBotId> {
    match mode {
        ComputerMode::Team => Ok(format!("team:{space_id}")),
        ComputerMode::Dedicated => {
            let bot_id = bot_id.ok_or(DedicatedRequiresBotId)?;
            Ok(format!("bot:{bot_id}"))
        }
    }
}

/// Durable workspace key. Team computers share one home; dedicated homes follow the bot.
pub fn computer_home_key(
    mode: ComputerMode,
    space_id: &str,
    bot_id: Option<&str>,
) -> Result<String, DedicatedRequiresBotId> {
    match mode {
        ComputerMode::Team => Ok(format!("team-{space_id}")),
        ComputerMode::Dedicated => {
            let bot_id = bot_id.ok_or(DedicatedRequiresBotId)?;
            Ok(bot_id.to_string())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BrowserProfileMode {
    Shared,
    PerBot,
    PerTask,
}

impl BrowserProfileMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::PerBot => "per-bot",
            Self::PerTask => "per-task",
        }
    }
}

impl Default for BrowserProfileMode {
    fn default() -> Self {
        Self::PerBot
    }
}

impl std::str::FromStr for BrowserProfileMode {
    type Err = UnknownProfileMode;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "shared" => Ok(Self::Shared),
            "per-bot" | "per_bot" | "perbot" => Ok(Self::PerBot),
            "per-task" | "per_task" | "pertask" => Ok(Self::PerTask),
            other => Err(UnknownProfileMode(other.to_string())),
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("unknown browser profile mode: {0}")]
pub struct UnknownProfileMode(pub String);

/// Frozen ComputerProvider capability bits.
/// lifecycle: provision / reconnect / destroy / suspend / resume
/// desktop(screenId): observe / act
/// exec: shell; files: list/read/write
/// hydrate/export: workspace truth is the home bind, not vendor disk
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerCapabilities {
    pub multi_screen: bool,
}

impl Default for ComputerCapabilities {
    fn default() -> Self {
        Self { multi_screen: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerStatus {
    pub bot_id: String,
    pub mode: ComputerMode,
    pub kind: SandboxKind,
    pub state: ComputerState,
    pub control_holder: ControlHolder,
    pub control_bot_id: Option<String>,
    pub takeover_requested: bool,
    pub screen_available: bool,
    pub screen_width: u32,
    pub screen_height: u32,
    pub home_revision: Option<String>,
    pub busy_bot_name: Option<String>,
    pub busy_session_id: Option<String>,
    pub busy_run_id: Option<String>,
    pub multi_screen: bool,
    pub screen_id: Option<String>,
    pub display: Option<String>,
    pub profile_mode: BrowserProfileMode,
}

pub const DEFAULT_SCREEN_WIDTH: u32 = 1280;
pub const DEFAULT_SCREEN_HEIGHT: u32 = 800;
pub const TEAM_SCREEN_LIMIT: u32 = 8;

pub const MULTI_SCREEN_UNAVAILABLE: &str = "This computer does not support multiple screens. Desktop tools are already in use on the shared display. File and shell tools still work.";
pub const TEAM_SCREENS_FULL: &str =
    "This Team computer has no free screens left. File and shell tools still work.";
pub const PROFILE_LOCKED: &str =
    "Another bot is using this shared browser profile. File and shell tools still work.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn team_and_dedicated_keys_differ() {
        assert_eq!(
            computer_scope_key(ComputerMode::Team, "space-1", None).unwrap(),
            "team:space-1"
        );
        assert_eq!(
            computer_home_key(ComputerMode::Team, "space-1", None).unwrap(),
            "team-space-1"
        );
        assert_eq!(
            computer_scope_key(ComputerMode::Dedicated, "space-1", Some("bot-9")).unwrap(),
            "bot:bot-9"
        );
        assert_eq!(
            computer_home_key(ComputerMode::Dedicated, "space-1", Some("bot-9")).unwrap(),
            "bot-9"
        );
        assert!(computer_home_key(ComputerMode::Dedicated, "space-1", None).is_err());
    }

    #[test]
    fn default_profile_mode_is_per_bot() {
        assert_eq!(BrowserProfileMode::default(), BrowserProfileMode::PerBot);
        assert_eq!(
            "per-bot".parse::<BrowserProfileMode>().unwrap(),
            BrowserProfileMode::PerBot
        );
    }
}
