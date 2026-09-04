use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelProvider {
    Xai,
    OpencodeGo,
    OpenaiCompatible,
    Openai,
    Anthropic,
    Openrouter,
}

impl ModelProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Xai => "xai",
            Self::OpencodeGo => "opencode-go",
            Self::OpenaiCompatible => "openai-compatible",
            Self::Openai => "openai",
            Self::Anthropic => "anthropic",
            Self::Openrouter => "openrouter",
        }
    }

    pub fn env_key_name(self) -> &'static str {
        match self {
            Self::Xai => "XAI_API_KEY",
            Self::OpencodeGo => "OPENCODE_GO_API_KEY",
            Self::OpenaiCompatible | Self::Openai => "OPENAI_API_KEY",
            Self::Anthropic => "ANTHROPIC_API_KEY",
            Self::Openrouter => "OPENROUTER_API_KEY",
        }
    }

    pub fn requires_api_key(self) -> bool {
        !matches!(self, Self::OpenaiCompatible)
    }

    pub fn requires_base_url(self) -> bool {
        matches!(self, Self::OpenaiCompatible)
    }

    /// First-party OpenAI-compatible base URL when we own the client mapping.
    pub fn default_base_url(self) -> Option<&'static str> {
        match self {
            Self::Xai => Some("https://api.x.ai/v1"),
            Self::OpencodeGo => Some("https://opencode.ai/zen/go/v1"),
            Self::OpenaiCompatible => None,
            Self::Openai => Some("https://api.openai.com/v1"),
            Self::Anthropic => None,
            Self::Openrouter => Some("https://openrouter.ai/api/v1"),
        }
    }

    pub fn selectable() -> &'static [Self] {
        &[Self::Xai, Self::OpencodeGo, Self::OpenaiCompatible]
    }
}

impl std::str::FromStr for ModelProvider {
    type Err = UnknownModelProvider;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "xai" => Ok(Self::Xai),
            "opencode-go" | "opencodego" => Ok(Self::OpencodeGo),
            "openai-compatible" | "openai_compatible" | "local" => Ok(Self::OpenaiCompatible),
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
pub const DEFAULT_OPENCODE_GO_MODEL: &str = "glm-5.1";

pub fn default_model_id(provider: ModelProvider) -> Option<&'static str> {
    match provider {
        ModelProvider::Xai => Some(DEFAULT_XAI_MODEL),
        ModelProvider::OpencodeGo => Some(DEFAULT_OPENCODE_GO_MODEL),
        ModelProvider::OpenaiCompatible
        | ModelProvider::Openai
        | ModelProvider::Anthropic
        | ModelProvider::Openrouter => None,
    }
}

pub fn catalog_models(provider: ModelProvider) -> &'static [(&'static str, &'static str)] {
    match provider {
        ModelProvider::Xai => &[
            ("grok-4.6", "Grok 4.6"),
            ("grok-4.5", "Grok 4.5"),
            ("grok-4", "Grok 4"),
            ("grok-3", "Grok 3"),
            ("grok-3-mini", "Grok 3 Mini"),
            ("grok-3-fast", "Grok 3 Fast"),
        ],
        ModelProvider::OpencodeGo => &[
            ("glm-5.1", "GLM-5.1"),
            ("glm-5", "GLM-5"),
            ("glm-5.2", "GLM-5.2"),
            ("kimi-k2.6", "Kimi K2.6"),
            ("kimi-k2.5", "Kimi K2.5"),
            ("kimi-k2.7-code", "Kimi K2.7 Code"),
            ("deepseek-v4-flash", "DeepSeek V4 Flash"),
            ("deepseek-v4-pro", "DeepSeek V4 Pro"),
            ("mimo-v2.5-pro", "MiMo V2.5 Pro"),
            ("minimax-m2.7", "MiniMax M2.7"),
            ("qwen3.6-plus", "Qwen3.6 Plus"),
        ],
        ModelProvider::OpenaiCompatible
        | ModelProvider::Openai
        | ModelProvider::Anthropic
        | ModelProvider::Openrouter => &[],
    }
}

/// Best-effort catalog. Unknown ids for a wired provider default to "no vision"
/// so we never shove screenshots at a model that may not accept them.
pub fn model_capabilities(provider: ModelProvider, model_id: &str) -> ModelCapabilities {
    let id = model_id.to_ascii_lowercase();
    let vision = match provider {
        ModelProvider::Xai => {
            id.starts_with("grok-4")
                || id.contains("vision")
                || id.starts_with("grok-2")
                || id.starts_with("grok-3")
        }
        ModelProvider::OpencodeGo => id.contains("vision") || id.contains("omni"),
        // OpenAI-compatible is also how OpenRouter / LiteLLM / vLLM gateways
        // are reached, so the id may be any vendor's multimodal model.
        ModelProvider::OpenaiCompatible | ModelProvider::Openai => {
            id.contains("gpt-4o")
                || id.contains("gpt-4.1")
                || id.contains("gpt-5")
                || id.starts_with("o3")
                || id.starts_with("o4")
                || id.contains("vision")
                || id.contains("llava")
                || id.contains("omni")
                || id.contains("claude")
                || id.contains("gemini")
                || id.contains("grok-4")
                || (id.contains("qwen") && (id.contains("vl") || id.contains("qwen3")))
                || id.contains("glm-4v")
                || id.contains("glm-4.5v")
                || id.contains("glm-5v")
                || id.contains("ui-tars")
                || id.contains("pixtral")
                || id.contains("internvl")
                || id.contains("llama-4")
                || id.contains("kimi-k2.5")
                || id.contains("kimi-k2.6")
                || id.contains("kimi-k2.7")
                || id.contains("computer-use")
        }
        ModelProvider::Anthropic => id.contains("claude"),
        ModelProvider::Openrouter => {
            id.contains("vision")
                || id.contains("gpt-4o")
                || id.contains("claude")
                || id.contains("grok")
        }
    };
    ModelCapabilities { vision }
}
