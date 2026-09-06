use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VoiceProvider {
    Xai,
    Openai,
    Scripted,
}

impl VoiceProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Xai => "xai",
            Self::Openai => "openai",
            Self::Scripted => "scripted",
        }
    }

    pub fn env_key_name(self) -> &'static str {
        match self {
            Self::Xai => "XAI_API_KEY",
            Self::Openai => "OPENAI_API_KEY",
            Self::Scripted => "",
        }
    }

    pub fn requires_api_key(self) -> bool {
        !matches!(self, Self::Scripted)
    }

    pub fn default_model_id(self) -> &'static str {
        match self {
            Self::Xai => DEFAULT_XAI_VOICE_MODEL,
            Self::Openai => DEFAULT_OPENAI_VOICE_MODEL,
            Self::Scripted => "scripted-voice",
        }
    }

    pub fn default_voice_id(self) -> &'static str {
        match self {
            Self::Xai => "eve",
            Self::Openai => "marin",
            Self::Scripted => "scripted",
        }
    }

    pub fn realtime_url(self, model_id: &str) -> String {
        match self {
            Self::Xai => format!("wss://api.x.ai/v1/realtime?model={model_id}"),
            Self::Openai => format!("wss://api.openai.com/v1/realtime?model={model_id}"),
            Self::Scripted => String::new(),
        }
    }

    pub fn selectable() -> &'static [Self] {
        &[Self::Xai, Self::Openai]
    }
}

impl std::str::FromStr for VoiceProvider {
    type Err = UnknownVoiceProvider;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "xai" => Ok(Self::Xai),
            "openai" => Ok(Self::Openai),
            "scripted" => Ok(Self::Scripted),
            other => Err(UnknownVoiceProvider(other.to_string())),
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("unknown voice provider: {0}")]
pub struct UnknownVoiceProvider(pub String);

pub const DEFAULT_XAI_VOICE_MODEL: &str = "grok-voice-latest";
pub const DEFAULT_OPENAI_VOICE_MODEL: &str = "gpt-realtime";
pub const VOICE_SAMPLE_RATE: u32 = 24_000;

pub fn catalog_voice_models(provider: VoiceProvider) -> &'static [(&'static str, &'static str)] {
    match provider {
        VoiceProvider::Xai => &[
            ("grok-voice-latest", "Grok Voice Latest"),
            ("grok-voice-think-fast-2.0", "Grok Voice Think Fast 2.0"),
            ("grok-voice-think-fast-1.0", "Grok Voice Think Fast 1.0"),
        ],
        VoiceProvider::Openai => &[("gpt-realtime", "GPT Realtime")],
        VoiceProvider::Scripted => &[("scripted-voice", "Scripted")],
    }
}

pub fn catalog_voices(provider: VoiceProvider) -> &'static [(&'static str, &'static str)] {
    match provider {
        VoiceProvider::Xai => &[
            ("eve", "Eve"),
            ("ara", "Ara"),
            ("leo", "Leo"),
            ("rex", "Rex"),
            ("sal", "Sal"),
        ],
        VoiceProvider::Openai => &[
            ("marin", "Marin"),
            ("alloy", "Alloy"),
            ("verse", "Verse"),
            ("cedar", "Cedar"),
        ],
        VoiceProvider::Scripted => &[("scripted", "Scripted")],
    }
}

/// Custom functions the voice model may call. LazyBoy executes them; they return immediately.
pub fn computer_voice_tools() -> Vec<Value> {
    vec![
        function_tool(
            "start_computer_task",
            "Start a computer task on the user's Linux desktop. Returns immediately — speak to the user first, do not wait for the work to finish. Use when they want you to operate the computer (open a site, click, download, run a command).",
            json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "What to do on the computer, in the user's language."
                    }
                },
                "required": ["prompt"]
            }),
        ),
        function_tool(
            "follow_up_computer_task",
            "Add or change instructions for the computer task that is already running. Use when the user interrupts with a correction.",
            json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "The new or extra instruction."
                    }
                },
                "required": ["prompt"]
            }),
        ),
        function_tool(
            "stop_computer_task",
            "Cancel the computer task that is running.",
            json!({ "type": "object", "properties": {} }),
        ),
        function_tool(
            "computer_status",
            "Check whether a computer task is running, queued, needs the user to take over the screen, or is idle.",
            json!({ "type": "object", "properties": {} }),
        ),
    ]
}

fn function_tool(name: &str, description: &str, parameters: Value) -> Value {
    json!({
        "type": "function",
        "name": name,
        "description": description,
        "parameters": parameters
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_provider_roundtrip() {
        assert_eq!("xai".parse::<VoiceProvider>().unwrap(), VoiceProvider::Xai);
        assert_eq!(
            "openai".parse::<VoiceProvider>().unwrap(),
            VoiceProvider::Openai
        );
        assert!("anthropic".parse::<VoiceProvider>().is_err());
        assert_eq!(VoiceProvider::Xai.default_voice_id(), "eve");
    }

    #[test]
    fn computer_tools_are_named() {
        let tools = computer_voice_tools();
        let names: Vec<_> = tools
            .iter()
            .filter_map(|tool| tool.get("name")?.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "start_computer_task",
                "follow_up_computer_task",
                "stop_computer_task",
                "computer_status"
            ]
        );
    }
}
