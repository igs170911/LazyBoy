use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use lazyboy_contracts::{VoiceProvider, catalog_voices, computer_voice_tools};
use lazyboy_harness::{
    CredentialChain, ResolveVoiceRequest, resolve_voice, scripted_voice_enabled, voice_catalog,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::db::{Actor, SpaceRow};
use crate::state::AppState;
use crate::voice_call;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/voice/settings", get(get_settings).patch(update_settings))
        .route("/api/sessions/{id}/call", get(voice_call::call_ws))
}

async fn actor(state: &AppState) -> Result<Actor, StatusCode> {
    state
        .bootstrap()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub fn voice_credential_chain(
    provider: VoiceProvider,
    voice_api_key: Option<&str>,
    text_provider: &str,
    text_api_key: Option<&str>,
    env_key: Option<&str>,
) -> CredentialChain {
    let nonempty = |value: Option<&str>| {
        value
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
    };
    let space = nonempty(voice_api_key).or_else(|| {
        if text_provider == provider.as_str() {
            nonempty(text_api_key)
        } else {
            None
        }
    });
    CredentialChain {
        bot: None,
        space,
        env: nonempty(env_key),
    }
}

pub fn voice_instructions(bot_name: &str, bot_instructions: &str) -> String {
    let mut text = format!(
        "You are {bot_name}, on a live voice call. Speak shortly and naturally in the user's language. Do not read markdown aloud.\n\
         You cannot see the screen. Computer progress arrives as text from tools.\n\
         If the user wants you to operate the computer, answer yes or no immediately, then call start_computer_task. Never pretend you already clicked.\n\
         If a task is already running and they change their mind, call follow_up_computer_task or stop_computer_task.\n\
         If a tool returns blocked because they have the screen, tell them to finish on the right-hand desktop."
    );
    let extra = bot_instructions.trim();
    if !extra.is_empty() {
        text.push_str("\n\nBot instructions:\n");
        text.push_str(extra);
    }
    text
}

fn listed_providers() -> Vec<VoiceProvider> {
    let mut providers = VoiceProvider::selectable().to_vec();
    if scripted_voice_enabled() {
        providers.push(VoiceProvider::Scripted);
    }
    providers
}

fn settings_json(space: &SpaceRow) -> Value {
    let requested = space
        .voice_provider
        .as_deref()
        .unwrap_or("xai")
        .parse::<VoiceProvider>()
        .unwrap_or(VoiceProvider::Xai);
    let provider = if requested == VoiceProvider::Scripted && !scripted_voice_enabled() {
        VoiceProvider::Xai
    } else {
        requested
    };
    let env_key = std::env::var(provider.env_key_name())
        .ok()
        .filter(|value| !value.is_empty());
    let credentials = voice_credential_chain(
        provider,
        space.voice_api_key.as_deref(),
        &space.default_model_provider,
        space.default_model_api_key.as_deref(),
        env_key.as_deref(),
    );
    let resolved = resolve_voice(ResolveVoiceRequest {
        provider,
        model_id: space.voice_model_id.clone(),
        voice_id: space.voice_id.clone(),
        credentials,
    });
    let (model_id, voice_id, ready, missing) = match &resolved {
        Ok(value) => (value.model_id.clone(), value.voice_id.clone(), true, None),
        Err(error) => (
            space
                .voice_model_id
                .clone()
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| provider.default_model_id().to_string()),
            space
                .voice_id
                .clone()
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| provider.default_voice_id().to_string()),
            false,
            Some(error.to_string()),
        ),
    };
    let catalog = voice_catalog(provider);
    json!({
        "provider": provider.as_str(),
        "modelId": model_id,
        "voiceId": voice_id,
        "enabled": space.voice_enabled,
        "ready": space.voice_enabled && ready,
        "missing": missing,
        "apiKeySet": space.voice_api_key.as_deref().is_some_and(|value| !value.is_empty()),
        "envKeySet": env_key.is_some(),
        "envKeyName": provider.env_key_name(),
        "reusesTextKey": provider.as_str() == space.default_model_provider
            && space.voice_api_key.as_deref().is_none_or(|value| value.is_empty())
            && space.default_model_api_key.as_deref().is_some_and(|value| !value.is_empty()),
        "providers": listed_providers().iter().map(|item| json!({
            "id": item.as_str(),
            "name": match item {
                VoiceProvider::Xai => "xAI",
                VoiceProvider::Openai => "OpenAI",
                VoiceProvider::Scripted => "Scripted",
            },
            "envKeyName": item.env_key_name(),
        })).collect::<Vec<_>>(),
        "models": catalog.models.iter().map(|item| json!({"id": item.id, "name": item.name})).collect::<Vec<_>>(),
        "voices": catalog.voices.iter().map(|item| json!({"id": item.id, "name": item.name})).collect::<Vec<_>>(),
        "tools": computer_voice_tools(),
    })
}

async fn get_settings(State(state): State<AppState>) -> Result<Json<Value>, StatusCode> {
    let actor = actor(&state).await?;
    let space = state
        .db
        .get_space(&actor)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(settings_json(&space)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateVoiceSettings {
    #[serde(default)]
    enabled: Option<bool>,
    provider: String,
    model_id: String,
    voice_id: String,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    clear_api_key: bool,
}

async fn update_settings(
    State(state): State<AppState>,
    Json(input): Json<UpdateVoiceSettings>,
) -> Result<Json<Value>, StatusCode> {
    let actor = actor(&state).await?;
    let provider: VoiceProvider = input
        .provider
        .parse()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    if !listed_providers().contains(&provider) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let model_id = input.model_id.trim();
    let voice_id = input.voice_id.trim();
    if model_id.is_empty() || voice_id.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if catalog_voices(provider).iter().all(|(id, _)| *id != voice_id)
        && provider != VoiceProvider::Scripted
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let supplied = input
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let api_key = if input.clear_api_key {
        Some(None)
    } else {
        supplied.map(Some)
    };
    state
        .db
        .update_voice_settings(&actor, input.enabled, provider.as_str(), model_id, voice_id, api_key)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    get_settings(State(state)).await
}

pub fn resolve_space_voice(
    space: &SpaceRow,
) -> Result<(VoiceProvider, lazyboy_harness::ResolvedVoice), String> {
    if !space.voice_enabled {
        return Err("語音通話尚未啟用，請到「設定 → 語音」開啟。".into());
    }
    let requested = space
        .voice_provider
        .as_deref()
        .unwrap_or("xai")
        .parse::<VoiceProvider>()
        .unwrap_or(VoiceProvider::Xai);
    let provider = if requested == VoiceProvider::Scripted && !scripted_voice_enabled() {
        VoiceProvider::Xai
    } else {
        requested
    };
    let env_key = std::env::var(provider.env_key_name())
        .ok()
        .filter(|value| !value.is_empty());
    let credentials = voice_credential_chain(
        provider,
        space.voice_api_key.as_deref(),
        &space.default_model_provider,
        space.default_model_api_key.as_deref(),
        env_key.as_deref(),
    );
    let resolved = resolve_voice(ResolveVoiceRequest {
        provider,
        model_id: space.voice_model_id.clone(),
        voice_id: space.voice_id.clone(),
        credentials,
    })
    .map_err(|error| error.to_string())?;
    Ok((provider, resolved))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_key_reuses_matching_text_provider_and_does_not_cross_providers() {
        let reused = voice_credential_chain(
            VoiceProvider::Xai,
            None,
            "xai",
            Some("text-key"),
            Some("env-key"),
        );
        assert_eq!(reused.resolve(), Some("text-key"));

        let env_only = voice_credential_chain(
            VoiceProvider::Xai,
            None,
            "opencode-go",
            Some("go-key"),
            Some("env-xai"),
        );
        assert_eq!(env_only.resolve(), Some("env-xai"));

        let dedicated = voice_credential_chain(
            VoiceProvider::Openai,
            Some("voice-openai"),
            "xai",
            Some("text-xai"),
            Some("env-openai"),
        );
        assert_eq!(dedicated.resolve(), Some("voice-openai"));
    }

    #[test]
    fn instructions_include_bot_name_and_custom_text() {
        let text = voice_instructions("阿明", "Always prefer the browser.");
        assert!(text.contains("阿明"));
        assert!(text.contains("start_computer_task"));
        assert!(text.contains("Always prefer the browser."));
    }
}
