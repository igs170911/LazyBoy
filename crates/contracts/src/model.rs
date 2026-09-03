use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelProvider {
    Xai,
    Openai,
    Anthropic,
    Openrouter,
}

impl ModelProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Xai => "xai",
            Self::Openai => "openai",
            Self::Anthropic => "anthropic",
            Self::Openrouter => "openrouter",
        }
    }

    pub fn env_key_name(self) -> &'static str {
        match self {
            Self::Xai => "XAI_API_KEY",
            Self::Openai => "OPENAI_API_KEY",
            Self::Anthropic => "ANTHROPIC_API_KEY",
            Self::Openrouter => "OPENROUTER_API_KEY",
        }
    }

    /// First-party OpenAI-compatible base URL when we own the client mapping.
    pub fn default_base_url(self) -> Option<&'static str> {
        match self {
            Self::Xai => Some("https://api.x.ai/v1"),
            Self::Openai => Some("https://api.openai.com/v1"),
            Self::Anthropic => None,
            Self::Openrouter => Some("https://openrouter.ai/api/v1"),
        }
    }
}

impl std::str::FromStr for ModelProvider {
    type Err = UnknownModelProvider;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "xai" => Ok(Self::Xai),
            "openai" => Ok(Self::Openai),
            "anthropic" => Ok(Self::Anthropic),
            "openrouter" => Ok(Self::Openrouter),
            other => Err(UnknownModelProvider(other.to_string())),
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("unknown model provider: {0}")]
pub struct UnknownModelProvider(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCapabilities {
    pub vision: bool,
}

pub const DEFAULT_XAI_MODEL: &str = "grok-4.6";

pub fn default_model_id(provider: ModelProvider) -> Option<&'static str> {
    match provider {
        ModelProvider::Xai => Some(DEFAULT_XAI_MODEL),
        ModelProvider::Openai | ModelProvider::Anthropic | ModelProvider::Openrouter => None,
    }
}

/// Best-effort catalog. Unknown ids for a wired provider default to "no vision"
/// so we never shove screenshots at a model that may not accept them.
pub fn model_capabilities(provider: ModelProvider, model_id: &str) -> ModelCapabilities {
    let vision = match provider {
        ModelProvider::Xai => {
            let id = model_id.to_ascii_lowercase();
            id.starts_with("grok-4")
                || id.contains("vision")
                || id.starts_with("grok-2")
                || id.starts_with("grok-3")
        }
        ModelProvider::Openai => {
            let id = model_id.to_ascii_lowercase();
            id.contains("gpt-4o") || id.contains("gpt-5") || id.contains("vision")
        }
        ModelProvider::Anthropic => model_id.to_ascii_lowercase().contains("claude"),
        ModelProvider::Openrouter => {
            let id = model_id.to_ascii_lowercase();
            id.contains("vision") || id.contains("gpt-4o") || id.contains("claude") || id.contains("grok")
        }
    };
    ModelCapabilities { vision }
}
