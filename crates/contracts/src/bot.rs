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
}

fn default_team() -> ComputerMode {
    ComputerMode::Team
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bot {
    pub id: String,
    pub space_id: String,
    pub name: String,
    pub title: String,
    pub description: String,
    pub instructions: String,
    pub thread_id: String,
    pub computer_id: String,
    pub computer_mode: ComputerMode,
    pub model_provider: Option<ModelProvider>,
    pub model_id: Option<String>,
}
