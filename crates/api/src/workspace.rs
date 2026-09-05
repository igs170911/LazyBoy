use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use lazyboy_contracts::{ModelProvider, catalog_models, default_model_id};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::db::Actor;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspace/settings",
            get(get_settings).patch(update_settings),
        )
        .route("/api/workspace/models", get(list_models))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderInfo {
    id: &'static str,
    name: &'static str,
    needs_base_url: bool,
    needs_key: bool,
    default_base_url: Option<&'static str>,
    default_model: Option<&'static str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelChoice {
    id: String,
    name: String,
}

fn provider_info(provider: ModelProvider) -> ProviderInfo {
    ProviderInfo {
        id: provider.as_str(),
        name: match provider {
            ModelProvider::Xai => "xAI",
            ModelProvider::OpencodeGo => "OpenCode Go",
            ModelProvider::OpenaiCompatible => "OpenAI 相容",
            ModelProvider::Openai => "OpenAI",
            ModelProvider::Anthropic => "Anthropic",
            ModelProvider::Openrouter => "OpenRouter",
        },
        needs_base_url: provider.requires_base_url(),
        needs_key: provider.requires_api_key(),
        default_base_url: provider.default_base_url(),
        default_model: default_model_id(provider),
    }
}

async fn actor(state: &AppState) -> Result<Actor, StatusCode> {
    state
        .bootstrap()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn get_settings(State(state): State<AppState>) -> Result<Json<Value>, StatusCode> {
    let actor = actor(&state).await?;
    let space = state
        .db
        .get_space(&actor)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let provider = space
        .default_model_provider
        .parse::<ModelProvider>()
        .unwrap_or(ModelProvider::Xai);
    let env_key_set = std::env::var(provider.env_key_name())
        .ok()
        .filter(|value| !value.is_empty())
        .is_some();
    Ok(Json(json!({
        "provider": provider.as_str(),
        "modelId": space.default_model_id,
        "baseUrl": space.default_model_base_url.unwrap_or_default(),
        "apiKeySet": space.default_model_api_key.as_deref().is_some_and(|value| !value.is_empty()),
        "envKeySet": env_key_set,
        "envKeyName": provider.env_key_name(),
        "providers": ModelProvider::selectable().iter().copied().map(provider_info).collect::<Vec<_>>(),
        "models": catalog_models(provider).iter().map(|(id, name)| ModelChoice { id: (*id).into(), name: (*name).into() }).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateSettings {
    provider: String,
    model_id: String,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    clear_api_key: bool,
}

async fn update_settings(
    State(state): State<AppState>,
    Json(input): Json<UpdateSettings>,
) -> Result<Json<Value>, StatusCode> {
    let actor = actor(&state).await?;
    let provider = input
        .provider
        .parse::<ModelProvider>()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    if !ModelProvider::selectable().contains(&provider) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let model_id = input.model_id.trim();
    if model_id.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let base_url = input
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if provider.requires_base_url() && base_url.is_none() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let current = state
        .db
        .get_space(&actor)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let provider_changed = current
        .as_ref()
        .is_some_and(|space| space.default_model_provider != provider.as_str());
    let supplied = input
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    // A stored key belongs to the provider it was entered for. Carrying it
    // over to a new provider shadows the env key and every run fails with
    // "Incorrect API key", so drop it unless a new one is supplied.
    let api_key = if input.clear_api_key || (provider_changed && supplied.is_none()) {
        Some(None)
    } else {
        supplied.map(Some)
    };
    state
        .db
        .update_workspace_model(&actor, provider.as_str(), model_id, base_url, api_key)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    get_settings(State(state)).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelsQuery {
    provider: String,
    #[serde(default)]
    base_url: Option<String>,
}

async fn list_models(
    State(state): State<AppState>,
    Query(query): Query<ModelsQuery>,
) -> Result<Json<Value>, StatusCode> {
    let actor = actor(&state).await?;
    let provider = query
        .provider
        .parse::<ModelProvider>()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let space = state.db.get_space(&actor).await.ok().flatten();
    let fallback = catalog_models(provider)
        .iter()
        .map(|(id, name)| ModelChoice {
            id: (*id).into(),
            name: (*name).into(),
        })
        .collect::<Vec<_>>();
    let env_key = std::env::var(provider.env_key_name())
        .ok()
        .filter(|value| !value.is_empty());
    let live = fetch_remote_models(
        provider,
        query.base_url.as_deref().or(space
            .as_ref()
            .and_then(|row| row.default_model_base_url.as_deref())),
        space
            .as_ref()
            .and_then(|row| row.default_model_api_key.as_deref())
            .or(env_key.as_deref()),
    )
    .await
    .unwrap_or_default();
    let models = if live.is_empty() { fallback } else { live };
    Ok(Json(json!({ "models": models })))
}

async fn fetch_remote_models(
    provider: ModelProvider,
    base_url: Option<&str>,
    api_key: Option<&str>,
) -> Result<Vec<ModelChoice>, String> {
    let base = base_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_end_matches('/').to_string())
        .or_else(|| provider.default_base_url().map(str::to_string))
        .ok_or_else(|| "missing base URL".to_string())?;
    let url = format!("{base}/models");
    let mut request = reqwest::Client::new().get(url);
    if let Some(key) = api_key.filter(|value| !value.is_empty()) {
        request = request.bearer_auth(key);
    }
    let body: Value = request
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .await
        .map_err(|error| error.to_string())?;
    let items = body
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .or_else(|| body.as_array().cloned())
        .unwrap_or_default();
    Ok(items
        .into_iter()
        .filter_map(|item| {
            let id = item.get("id")?.as_str()?.to_string();
            let name = item
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(&id)
                .to_string();
            Some(ModelChoice { id, name })
        })
        .collect())
}
