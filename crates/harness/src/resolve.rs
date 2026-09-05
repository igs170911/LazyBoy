use lazyboy_contracts::{ModelCapabilities, ModelProvider, default_model_id, model_capabilities};
use rig_core::client::CompletionClient;
use rig_core::providers::{openai, xai};
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ModelError {
    #[error("unsupported_provider:{provider}")]
    UnsupportedProvider { provider: String },
    #[error("missing credential for {provider} ({env_key})")]
    MissingCredential { provider: String, env_key: String },
    #[error("missing base URL for {provider}")]
    MissingBaseUrl { provider: String },
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
    pub base_url: Option<String>,
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
pub fn resolve_backend(request: ResolveModelRequest) -> Result<ResolvedBackend, ModelError> {
    match request.provider {
        ModelProvider::Xai | ModelProvider::OpencodeGo | ModelProvider::OpenaiCompatible => {}
        other => {
            return Err(ModelError::UnsupportedProvider {
                provider: other.as_str().to_string(),
            });
        }
    }

    let api_key = match request.credentials.resolve() {
        Some(key) => key.to_string(),
        None if request.provider.requires_api_key() => {
            return Err(ModelError::MissingCredential {
                provider: request.provider.as_str().to_string(),
                env_key: request.provider.env_key_name().to_string(),
            });
        }
        None => String::new(),
    };

    let model_id = request
        .model_id
        .filter(|value| !value.is_empty())
        .or_else(|| default_model_id(request.provider).map(str::to_string))
        .ok_or_else(|| ModelError::ProviderClient("missing model id".into()))?;

    let base_url = request
        .base_url
        .filter(|value| !value.trim().is_empty())
        .or_else(|| request.provider.default_base_url().map(str::to_string))
        .ok_or_else(|| ModelError::MissingBaseUrl {
            provider: request.provider.as_str().to_string(),
        })?;
    let base_url = rewrite_loopback_host(&normalize_base_url(&base_url));

    Ok(ResolvedBackend {
        provider: request.provider,
        model_id: model_id.clone(),
        capabilities: model_capabilities(request.provider, &model_id),
        api_key,
        base_url,
    })
}

fn normalize_base_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

fn rewrite_loopback_host(url: &str) -> String {
    let supervisor = std::env::var("SANDBOX_SUPERVISOR_URL").unwrap_or_default();
    if !supervisor.contains("://supervisor") {
        return url.to_string();
    }
    let Ok(mut parsed) = reqwest::Url::parse(url) else {
        return url.to_string();
    };
    if matches!(parsed.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")) {
        let _ = parsed.set_host(Some("host.docker.internal"));
    }
    parsed.to_string().trim_end_matches('/').to_string()
}

pub enum DynModel {
    Xai(xai::completion::CompletionModel),
    OpenAi(openai::completion::CompletionModel),
    OpenAiResponses(openai::responses_api::ResponsesCompletionModel),
}

fn uses_responses_api(provider: ModelProvider, model_id: &str) -> bool {
    let id = model_id.to_ascii_lowercase();
    provider == ModelProvider::OpencodeGo
        && (id.starts_with("gpt-") || id.starts_with("grok-") || id.starts_with("muse-"))
}

pub fn connect_model(backend: &ResolvedBackend) -> Result<DynModel, ModelError> {
    match backend.provider {
        ModelProvider::Xai => {
            let client = xai::Client::new(&backend.api_key)
                .map_err(|error| ModelError::ProviderClient(error.to_string()))?;
            Ok(DynModel::Xai(client.completion_model(&backend.model_id)))
        }
        ModelProvider::OpencodeGo | ModelProvider::OpenaiCompatible => {
            let key = if backend.api_key.is_empty() {
                "local"
            } else {
                backend.api_key.as_str()
            };
            if uses_responses_api(backend.provider, &backend.model_id) {
                let client = openai::Client::builder()
                    .api_key(key.to_string())
                    .base_url(&backend.base_url)
                    .build()
                    .map_err(|error| ModelError::ProviderClient(error.to_string()))?;
                Ok(DynModel::OpenAiResponses(
                    client.completion_model(&backend.model_id),
                ))
            } else {
                let client = openai::CompletionsClient::builder()
                    .api_key(key.to_string())
                    .base_url(&backend.base_url)
                    .build()
                    .map_err(|error| ModelError::ProviderClient(error.to_string()))?;
                Ok(DynModel::OpenAi(client.completion_model(&backend.model_id)))
            }
        }
        other => Err(ModelError::UnsupportedProvider {
            provider: other.as_str().to_string(),
        }),
    }
}

pub fn credential_from_env(provider: ModelProvider) -> Option<String> {
    std::env::var(provider.env_key_name())
        .ok()
        .filter(|value| !value.is_empty())
}

/// Prove the xAI Rig client can be constructed from a resolved backend.
pub fn connect_xai(
    backend: &ResolvedBackend,
) -> Result<rig_core::providers::xai::Client, ModelError> {
    if backend.provider != ModelProvider::Xai {
        return Err(ModelError::UnsupportedProvider {
            provider: backend.provider.as_str().to_string(),
        });
    }
    rig_core::providers::xai::Client::new(&backend.api_key)
        .map_err(|error| ModelError::ProviderClient(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lazyboy_contracts::DEFAULT_XAI_MODEL;

    fn xai_request(key: Option<&str>) -> ResolveModelRequest {
        ResolveModelRequest {
            provider: ModelProvider::Xai,
            model_id: None,
            base_url: None,
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
            base_url: None,
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
                base_url: None,
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

    #[test]
    fn opencode_go_uses_zen_go_endpoint() {
        let backend = resolve_backend(ResolveModelRequest {
            provider: ModelProvider::OpencodeGo,
            model_id: None,
            base_url: None,
            credentials: CredentialChain {
                bot: None,
                space: None,
                env: Some("go-key".into()),
            },
        })
        .unwrap();
        assert_eq!(backend.model_id, "glm-5.1");
        assert_eq!(backend.base_url, "https://opencode.ai/zen/go/v1");
        assert!(connect_model(&backend).is_ok());
    }

    #[test]
    fn opencode_routes_responses_models_to_the_responses_api() {
        assert!(uses_responses_api(
            ModelProvider::OpencodeGo,
            "gpt-5.6-luna"
        ));
        assert!(uses_responses_api(ModelProvider::OpencodeGo, "grok-4.6"));
        assert!(!uses_responses_api(ModelProvider::OpencodeGo, "glm-5.1"));
        assert!(!uses_responses_api(
            ModelProvider::OpenaiCompatible,
            "gpt-5.6-luna"
        ));
    }

    #[test]
    fn openai_compatible_allows_empty_key_and_custom_url() {
        let backend = resolve_backend(ResolveModelRequest {
            provider: ModelProvider::OpenaiCompatible,
            model_id: Some("qwen2.5".into()),
            base_url: Some("http://127.0.0.1:8000/v1/".into()),
            credentials: CredentialChain {
                bot: None,
                space: None,
                env: None,
            },
        })
        .unwrap();
        assert_eq!(backend.api_key, "");
        assert_eq!(backend.base_url, "http://127.0.0.1:8000/v1");
        assert!(connect_model(&backend).is_ok());
    }

    #[test]
    fn openai_compatible_requires_base_url() {
        let error = resolve_backend(ResolveModelRequest {
            provider: ModelProvider::OpenaiCompatible,
            model_id: Some("local".into()),
            base_url: None,
            credentials: CredentialChain {
                bot: None,
                space: None,
                env: None,
            },
        })
        .unwrap_err();
        assert!(matches!(error, ModelError::MissingBaseUrl { .. }));
    }
}
