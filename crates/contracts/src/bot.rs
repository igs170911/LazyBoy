use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{ComputerMode, ModelProvider};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBotInput {
    pub name: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub instructions: String,
    #[serde(default = "default_team")]
    pub computer_mode: ComputerMode,
    pub model_provider: Option<ModelProvider>,
    pub model_id: Option<String>,
    #[serde(default = "default_true")]
    pub memory_enabled: bool,
}

fn default_team() -> ComputerMode {
    ComputerMode::Team
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bot {
    pub id: String,
    pub space_id: String,
    pub name: String,
    pub title: String,
    pub description: String,
    pub avatar_color: String,
    pub avatar_shape: String,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub hidden: bool,
    pub group_name: Option<String>,
    pub unread_count: i64,
    pub last_message_at: Option<DateTime<Utc>>,
    pub instructions: String,
    pub thread_id: String,
    pub computer_id: String,
    pub computer_mode: ComputerMode,
    pub model_provider: Option<ModelProvider>,
    pub model_id: Option<String>,
    pub memory_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBotInput {
    pub name: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub avatar_color: String,
    pub avatar_shape: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub memory_enabled: Option<bool>,
}
