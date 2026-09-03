use lazyboy_contracts::{
    default_model_id, model_capabilities, ModelCapabilities, ModelProvider, DEFAULT_XAI_MODEL,
};
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ModelError {
    #[error("unsupported_provider:{provider}")]
    UnsupportedProvider { provider: String },
    #[error("missing credential for {provider} ({env_key})")]
    MissingCredential {
        provider: String,
        env_key: String,
    },
    #[error("unknown model provider: {0}")]
    UnknownProvider(String),
    #[error("model client error: {0}")]
    ProviderClient(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialChain {
    pub bot: Option<String>,
    pub space: Option<String>,
    pub env: Option<String>,
}

impl CredentialChain {
    pub fn resolve(&self) -> Option<&str> {
        self.bot
            .as_deref()
            .filter(|value| !value.is_empty())
            .or(self.space.as_deref().filter(|value| !value.is_empty()))
            .or(self.env.as_deref().filter(|value| !value.is_empty()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveModelRequest {
    pub provider: ModelProvider,
    pub model_id: Option<String>,
    pub credentials: CredentialChain,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedBackend {
    pub provider: ModelProvider,
    pub model_id: String,
    pub capabilities: ModelCapabilities,
    pub api_key: String,
    pub base_url: String,
}

/// Pick a backend without talking to the network.
///
/// v1 only constructs an xAI client. Other providers stay in the enum so a later
/// mapping can land without changing Computer tools.
pub fn resolve_backend(request: ResolveModelRequest) -> Result<ResolvedBackend, ModelError> {
    match request.provider {
        ModelProvider::Xai => {}
        other => {
            return Err(ModelError::UnsupportedProvider {
                provider: other.as_str().to_string(),
            });
        }
    }

    let api_key = request.credentials.resolve().ok_or(ModelError::MissingCredential {
        provider: request.provider.as_str().to_string(),
        env_key: request.provider.env_key_name().to_string(),
    })?;

    let model_id = request
        .model_id
        .filter(|value| !value.is_empty())
        .or_else(|| default_model_id(request.provider).map(str::to_string))
        .unwrap_or_else(|| DEFAULT_XAI_MODEL.to_string());

    let base_url = request
        .provider
        .default_base_url()
        .unwrap_or("https://api.x.ai/v1")
        .to_string();

    Ok(ResolvedBackend {
        provider: request.provider,
        model_id: model_id.clone(),
        capabilities: model_capabilities(request.provider, &model_id),
        api_key: api_key.to_string(),
        base_url,
    })
}

pub fn credential_from_env(provider: ModelProvider) -> Option<String> {
    std::env::var(provider.env_key_name()).ok().filter(|value| !value.is_empty())
}

/// Prove the xAI Rig client can be constructed from a resolved backend.
pub fn connect_xai(backend: &ResolvedBackend) -> Result<rig_core::providers::xai::Client, ModelError> {
    if backend.provider != ModelProvider::Xai {
        return Err(ModelError::UnsupportedProvider {
            provider: backend.provider.as_str().to_string(),
        });
    }
    rig_core::providers::xai::Client::new(&backend.api_key).map_err(|error| {
        ModelError::ProviderClient(error.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xai_request(key: Option<&str>) -> ResolveModelRequest {
        ResolveModelRequest {
            provider: ModelProvider::Xai,
            model_id: None,
            credentials: CredentialChain {
                bot: None,
                space: None,
                env: key.map(str::to_string),
            },
        }
    }

    #[test]
    fn xai_resolves_with_default_vision_model() {
        let backend = resolve_backend(xai_request(Some("test-key"))).unwrap();
        assert_eq!(backend.provider, ModelProvider::Xai);
        assert_eq!(backend.model_id, DEFAULT_XAI_MODEL);
        assert!(backend.capabilities.vision);
        assert_eq!(backend.base_url, "https://api.x.ai/v1");
        assert!(connect_xai(&backend).is_ok());
    }

    #[test]
    fn credentials_prefer_bot_then_space_then_env() {
        let backend = resolve_backend(ResolveModelRequest {
            provider: ModelProvider::Xai,
            model_id: Some("grok-4.6".into()),
            credentials: CredentialChain {
                bot: Some("bot-key".into()),
                space: Some("space-key".into()),
                env: Some("env-key".into()),
            },
        })
        .unwrap();
        assert_eq!(backend.api_key, "bot-key");
    }

    #[test]
    fn missing_credential_is_explicit() {
        let error = resolve_backend(xai_request(None)).unwrap_err();
        assert!(matches!(error, ModelError::MissingCredential { .. }));
    }

    #[test]
    fn other_providers_are_reserved_not_silent_fallback() {
        for provider in [
            ModelProvider::Openai,
            ModelProvider::Anthropic,
            ModelProvider::Openrouter,
        ] {
            let error = resolve_backend(ResolveModelRequest {
                provider,
                model_id: Some("whatever".into()),
                credentials: CredentialChain {
                    bot: None,
                    space: None,
                    env: Some("sk-test".into()),
                },
            })
            .unwrap_err();
            assert_eq!(
                error,
                ModelError::UnsupportedProvider {
                    provider: provider.as_str().to_string()
                }
            );
        }
    }

    #[test]
    fn unknown_provider_strings_fail_to_parse() {
        let error = "gemini".parse::<ModelProvider>().unwrap_err();
        assert_eq!(error.0, "gemini");
    }
}
