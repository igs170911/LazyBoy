use lazyboy_contracts::UiElement;
use serde::{Deserialize, Serialize};

pub fn sanitize_skill_id(skill_id: &str) -> String {
    let cleaned: String = skill_id
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .take(80)
        .collect();
    if cleaned.is_empty() {
        "unknown".into()
    } else {
        cleaned
    }
}

pub fn teach_trajectory_dir(skill_id: &str) -> String {
    format!("/tmp/lazyboy/teach-{}", sanitize_skill_id(skill_id))
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserPage {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub restarted: bool,
    /// Seconds the click waited for a disabled control to become enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waited_seconds: Option<f64>,
    #[serde(default)]
    pub elements: Vec<UiElement>,
}
