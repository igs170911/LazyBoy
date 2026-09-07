use std::sync::Arc;

use async_trait::async_trait;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use lazyboy_contracts::{
    VoiceProvider, catalog_voice_models, catalog_voices, computer_voice_tools,
};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::sync::{Mutex, mpsc};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use crate::{CredentialChain, ModelError};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum VoiceError {
    #[error("{0}")]
    Message(String),
    #[error("missing credential for {provider} ({env_key})")]
    MissingCredential { provider: String, env_key: String },
    #[error("unknown voice provider: {0}")]
    UnknownProvider(String),
}

impl From<ModelError> for VoiceError {
    fn from(error: ModelError) -> Self {
        match error {
            ModelError::MissingCredential { provider, env_key } => {
                Self::MissingCredential { provider, env_key }
            }
            other => Self::Message(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceCatalogEntry {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceCatalog {
    pub provider: VoiceProvider,
    pub models: Vec<VoiceCatalogEntry>,
    pub voices: Vec<VoiceCatalogEntry>,
}

pub fn voice_catalog(provider: VoiceProvider) -> VoiceCatalog {
    VoiceCatalog {
        provider,
        models: catalog_voice_models(provider)
            .iter()
            .map(|(id, name)| VoiceCatalogEntry {
                id: (*id).into(),
                name: (*name).into(),
            })
            .collect(),
        voices: catalog_voices(provider)
            .iter()
            .map(|(id, name)| VoiceCatalogEntry {
                id: (*id).into(),
                name: (*name).into(),
            })
            .collect(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveVoiceRequest {
    pub provider: VoiceProvider,
    pub model_id: Option<String>,
    pub voice_id: Option<String>,
    pub credentials: CredentialChain,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedVoice {
    pub provider: VoiceProvider,
    pub model_id: String,
    pub voice_id: String,
    pub api_key: String,
}

pub fn resolve_voice(request: ResolveVoiceRequest) -> Result<ResolvedVoice, VoiceError> {
    let api_key = match request.credentials.resolve() {
        Some(key) => key.to_string(),
        None if request.provider.requires_api_key() => {
            return Err(VoiceError::MissingCredential {
                provider: request.provider.as_str().to_string(),
                env_key: request.provider.env_key_name().to_string(),
            });
        }
        None => String::new(),
    };
    let model_id = request
        .model_id
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| request.provider.default_model_id().to_string());
    let voice_id = request
        .voice_id
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| request.provider.default_voice_id().to_string());
    Ok(ResolvedVoice {
        provider: request.provider,
        model_id,
        voice_id,
        api_key,
    })
}

#[derive(Debug, Clone)]
pub enum VoiceEvent {
    AudioPcm(Vec<u8>),
    SpeechStarted,
    SpeechStopped,
    InputTranscript {
        text: String,
        final_: bool,
    },
    OutputTranscript {
        text: String,
        final_: bool,
    },
    FunctionCall {
        call_id: String,
        name: String,
        arguments: String,
    },
    FunctionCallOutput {
        call_id: String,
        output: String,
    },
    SpeakNow {
        text: String,
    },
    InjectContext {
        text: String,
    },
    ResponseCreate,
    ResponseStarted,
    ResponseFinished,
    CancelResponse,
    Error {
        message: String,
    },
}

#[derive(Debug, Clone)]
pub struct VoiceConnectRequest {
    pub api_key: String,
    pub model_id: String,
    pub voice_id: String,
    pub instructions: String,
    pub tools: Vec<Value>,
    pub history: Vec<(String, String)>,
}

impl VoiceConnectRequest {
    pub fn from_resolved(resolved: &ResolvedVoice, instructions: String) -> Self {
        Self {
            api_key: resolved.api_key.clone(),
            model_id: resolved.model_id.clone(),
            voice_id: resolved.voice_id.clone(),
            instructions,
            tools: computer_voice_tools(),
            history: Vec::new(),
        }
    }
}

#[async_trait]
pub trait VoiceRealtime: Send + Sync {
    fn provider(&self) -> VoiceProvider;
    async fn connect(
        &self,
        request: VoiceConnectRequest,
    ) -> Result<Box<dyn VoiceSocket>, VoiceError>;
}

#[async_trait]
pub trait VoiceSocket: Send {
    async fn send(&self, event: VoiceEvent) -> Result<(), VoiceError>;
    async fn recv(&mut self) -> Result<Option<VoiceEvent>, VoiceError>;
}

pub fn create_voice(provider: VoiceProvider) -> Box<dyn VoiceRealtime> {
    match provider {
        VoiceProvider::Scripted => Box::new(ScriptedVoice),
        other => Box::new(HostedVoice { provider: other }),
    }
}

pub fn scripted_voice_enabled() -> bool {
    matches!(
        std::env::var("LAZYBOY_VOICE_SCRIPTED").as_deref(),
        Ok("1" | "true" | "yes")
    )
}

fn native_roots() -> Result<rustls::RootCertStore, VoiceError> {
    let mut roots = rustls::RootCertStore::empty();
    let certs = rustls_native_certs::load_native_certs();
    roots.add_parsable_certificates(certs.certs);
    if roots.is_empty() {
        return Err(VoiceError::Message(
            "No trusted TLS certificates available".into(),
        ));
    }
    Ok(roots)
}

struct HostedVoice {
    provider: VoiceProvider,
}

#[async_trait]
impl VoiceRealtime for HostedVoice {
    fn provider(&self) -> VoiceProvider {
        self.provider
    }

    async fn connect(
        &self,
        request: VoiceConnectRequest,
    ) -> Result<Box<dyn VoiceSocket>, VoiceError> {
        let url = self.provider.realtime_url(&request.model_id);
        let mut http_request = url
            .as_str()
            .into_client_request()
            .map_err(|error| VoiceError::Message(error.to_string()))?;
        let header = format!("Bearer {}", request.api_key);
        http_request.headers_mut().insert(
            "Authorization",
            http::HeaderValue::from_str(&header)
                .map_err(|error| VoiceError::Message(error.to_string()))?,
        );
        // Choose explicitly: the dependency graph enables both ring and aws-lc-rs.
        // Rustls's automatic provider selection panics in that configuration.
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|error| VoiceError::Message(error.to_string()))?
        .with_root_certificates(native_roots()?)
        .with_no_client_auth();
        let (stream, _) = tokio_tungstenite::connect_async_tls_with_config(
            http_request,
            None,
            false,
            Some(tokio_tungstenite::Connector::Rustls(Arc::new(tls))),
        )
        .await
        .map_err(|error| VoiceError::Message(error.to_string()))?;
        let (write, read) = stream.split();
        let socket = HostedSocket {
            provider: self.provider,
            write: Mutex::new(write),
            read,
        };
        socket
            .send_raw(Message::Text(
                session_update_json(self.provider, &request).into(),
            ))
            .await?;
        for (role, text) in &request.history {
            if text.trim().is_empty() {
                continue;
            }
            let item = json!({
                "type": "conversation.item.create",
                "item": {
                    "type": "message",
                    "role": role,
                    "content": [{
                        "type": if *role == "assistant" { "output_text" } else { "input_text" },
                        "text": text
                    }]
                }
            });
            socket
                .send_raw(Message::Text(item.to_string().into()))
                .await?;
        }
        Ok(Box::new(socket))
    }
}

type HostedWrite = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    Message,
>;
type HostedRead = futures_util::stream::SplitStream<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
>;

struct HostedSocket {
    provider: VoiceProvider,
    write: Mutex<HostedWrite>,
    read: HostedRead,
}

impl HostedSocket {
    async fn send_raw(&self, message: Message) -> Result<(), VoiceError> {
        self.write
            .lock()
            .await
            .send(message)
            .await
            .map_err(|error| VoiceError::Message(error.to_string()))
    }
}

#[async_trait]
impl VoiceSocket for HostedSocket {
    async fn send(&self, event: VoiceEvent) -> Result<(), VoiceError> {
        match encode_provider_event(self.provider, &event) {
            Some(message) => self.send_raw(message).await,
            None => Ok(()),
        }
    }

    async fn recv(&mut self) -> Result<Option<VoiceEvent>, VoiceError> {
        loop {
            match self.read.next().await {
                None => return Ok(None),
                Some(Err(error)) => return Err(VoiceError::Message(error.to_string())),
                Some(Ok(Message::Close(_))) => return Ok(None),
                Some(Ok(Message::Ping(payload))) => {
                    let _ = self.send_raw(Message::Pong(payload)).await;
                }
                Some(Ok(Message::Pong(_))) => {}
                Some(Ok(Message::Frame(_))) => {}
                Some(Ok(Message::Binary(bytes))) => {
                    return Ok(Some(VoiceEvent::AudioPcm(bytes.to_vec())));
                }
                Some(Ok(Message::Text(text))) => {
                    if let Some(event) = parse_provider_event(&text) {
                        return Ok(Some(event));
                    }
                }
            }
        }
    }
}

fn session_update_json(provider: VoiceProvider, request: &VoiceConnectRequest) -> String {
    let mut session = json!({
        "voice": request.voice_id,
        "instructions": request.instructions,
        "turn_detection": { "type": "server_vad" },
        "tools": request.tools,
        "audio": {
            "input": {
                "format": { "type": "audio/pcm", "rate": 24000 },
                "transport": "binary"
            },
            "output": {
                "format": { "type": "audio/pcm", "rate": 24000 },
                "transport": "binary"
            }
        }
    });
    if provider == VoiceProvider::Openai {
        session.as_object_mut().unwrap().remove("voice");
        session.as_object_mut().unwrap().remove("turn_detection");
        session["type"] = json!("realtime");
        session["audio"] = json!({
            "input": {
                "format": { "type": "audio/pcm", "rate": 24000 },
                "turn_detection": { "type": "server_vad" }
            },
            "output": {
                "format": { "type": "audio/pcm", "rate": 24000 },
                "voice": request.voice_id
            }
        });
    }
    json!({ "type": "session.update", "session": session }).to_string()
}

pub fn encode_provider_event(provider: VoiceProvider, event: &VoiceEvent) -> Option<Message> {
    match event {
        VoiceEvent::AudioPcm(bytes) if provider == VoiceProvider::Openai => Some(Message::Text(
            json!({ "type": "input_audio_buffer.append", "audio": base64::engine::general_purpose::STANDARD.encode(bytes) }).to_string().into(),
        )),
        VoiceEvent::AudioPcm(bytes) => Some(Message::Binary(bytes.clone().into())),
        VoiceEvent::FunctionCallOutput { call_id, output } => Some(Message::Text(
            json!({
                "type": "conversation.item.create",
                "item": {
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": output
                }
            })
            .to_string()
            .into(),
        )),
        VoiceEvent::ResponseCreate => Some(Message::Text(
            json!({ "type": "response.create" }).to_string().into(),
        )),
        VoiceEvent::CancelResponse => Some(Message::Text(
            json!({ "type": "response.cancel" }).to_string().into(),
        )),
        VoiceEvent::InjectContext { text } => Some(Message::Text(
            json!({
                "type": "conversation.item.create",
                "item": {
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": text }]
                }
            })
            .to_string()
            .into(),
        )),
        VoiceEvent::SpeakNow { text } if provider == VoiceProvider::Xai => Some(Message::Text(
            json!({
                "type": "conversation.item.create",
                "item": {
                    "type": "force_message",
                    "role": "assistant",
                    "interruptible": true,
                    "content": [{ "type": "output_text", "text": text }]
                }
            })
            .to_string()
            .into(),
        )),
        VoiceEvent::SpeakNow { text } => Some(Message::Text(
            json!({
                "type": "conversation.item.create",
                "item": {
                    "type": "message",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": text }]
                }
            })
            .to_string()
            .into(),
        )),
        _ => None,
    }
}

pub fn parse_provider_event(text: &str) -> Option<VoiceEvent> {
    let event: Value = serde_json::from_str(text).ok()?;
    let kind = event.get("type")?.as_str()?;
    match kind {
        "input_audio_buffer.speech_started" => Some(VoiceEvent::SpeechStarted),
        "input_audio_buffer.speech_stopped" => Some(VoiceEvent::SpeechStopped),
        "response.created" => Some(VoiceEvent::ResponseStarted),
        "response.done" | "response.completed" | "response.cancelled" => {
            Some(VoiceEvent::ResponseFinished)
        }
        "conversation.item.input_audio_transcription.delta"
        | "response.input_audio_transcription.delta" => {
            let text = event.get("delta")?.as_str()?.to_string();
            Some(VoiceEvent::InputTranscript {
                text,
                final_: false,
            })
        }
        "conversation.item.input_audio_transcription.completed"
        | "conversation.item.input_audio_transcription.done"
        | "response.input_audio_transcription.completed" => {
            let text = event
                .get("transcript")
                .and_then(Value::as_str)
                .or_else(|| event.get("text").and_then(Value::as_str))?
                .to_string();
            Some(VoiceEvent::InputTranscript { text, final_: true })
        }
        "response.output_audio.delta" | "response.audio.delta" => {
            let encoded = event.get("delta").and_then(Value::as_str)?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .ok()?;
            Some(VoiceEvent::AudioPcm(bytes))
        }
        "response.output_audio_transcript.delta" | "response.audio_transcript.delta" => {
            let text = event.get("delta")?.as_str()?.to_string();
            Some(VoiceEvent::OutputTranscript {
                text,
                final_: false,
            })
        }
        "response.output_audio_transcript.done" | "response.audio_transcript.done" => {
            let text = event
                .get("transcript")
                .and_then(Value::as_str)
                .or_else(|| event.get("text").and_then(Value::as_str))
                .unwrap_or("")
                .to_string();
            Some(VoiceEvent::OutputTranscript { text, final_: true })
        }
        "response.function_call_arguments.done" => {
            let call_id = event.get("call_id")?.as_str()?.to_string();
            let name = event.get("name")?.as_str()?.to_string();
            let arguments = event
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}")
                .to_string();
            Some(VoiceEvent::FunctionCall {
                call_id,
                name,
                arguments,
            })
        }
        "error" => {
            let message = event
                .pointer("/error/message")
                .and_then(Value::as_str)
                .or_else(|| event.get("message").and_then(Value::as_str))
                .unwrap_or("voice error")
                .to_string();
            Some(VoiceEvent::Error { message })
        }
        _ => None,
    }
}

struct ScriptedVoice;

#[async_trait]
impl VoiceRealtime for ScriptedVoice {
    fn provider(&self) -> VoiceProvider {
        VoiceProvider::Scripted
    }

    async fn connect(
        &self,
        _request: VoiceConnectRequest,
    ) -> Result<Box<dyn VoiceSocket>, VoiceError> {
        Ok(Box::new(ScriptedSocket::new()))
    }
}

struct ScriptedSocket {
    incoming: Mutex<mpsc::UnboundedSender<VoiceEvent>>,
    outgoing: mpsc::UnboundedReceiver<VoiceEvent>,
    heard: Arc<Mutex<bool>>,
}

impl ScriptedSocket {
    fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            incoming: Mutex::new(tx),
            outgoing: rx,
            heard: Arc::new(Mutex::new(false)),
        }
    }

    fn beep() -> Vec<u8> {
        // 80 ms of 24 kHz PCM16 silence so tests have a non-empty clip.
        vec![0; 24000 / 12 * 2]
    }
}

#[async_trait]
impl VoiceSocket for ScriptedSocket {
    async fn send(&self, event: VoiceEvent) -> Result<(), VoiceError> {
        match event {
            VoiceEvent::AudioPcm(bytes) if !bytes.is_empty() => {
                let mut heard = self.heard.lock().await;
                if !*heard {
                    *heard = true;
                    let tx = self.incoming.lock().await;
                    let _ = tx.send(VoiceEvent::SpeechStarted);
                    let _ = tx.send(VoiceEvent::InputTranscript {
                        text: "hello from the test microphone".into(),
                        final_: true,
                    });
                    let _ = tx.send(VoiceEvent::FunctionCall {
                        call_id: "call_scripted".into(),
                        name: "start_computer_task".into(),
                        arguments: json!({ "prompt": "open the browser" }).to_string(),
                    });
                }
            }
            VoiceEvent::FunctionCallOutput { .. } => {
                let tx = self.incoming.lock().await;
                let _ = tx.send(VoiceEvent::OutputTranscript {
                    text: "好，我去電腦上開。".into(),
                    final_: true,
                });
                let _ = tx.send(VoiceEvent::AudioPcm(Self::beep()));
            }
            VoiceEvent::SpeakNow { text } | VoiceEvent::InjectContext { text } => {
                let tx = self.incoming.lock().await;
                let _ = tx.send(VoiceEvent::OutputTranscript { text, final_: true });
                let _ = tx.send(VoiceEvent::AudioPcm(Self::beep()));
            }
            VoiceEvent::ResponseCreate => {}
            _ => {}
        }
        Ok(())
    }

    async fn recv(&mut self) -> Result<Option<VoiceEvent>, VoiceError> {
        Ok(self.outgoing.recv().await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_tls_config_uses_an_explicit_provider() {
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(native_roots().unwrap())
        .with_no_client_auth();
        assert!(!config.crypto_provider().cipher_suites.is_empty());
    }

    #[test]
    fn openai_audio_uses_json_and_nested_session_settings() {
        let request = VoiceConnectRequest {
            api_key: String::new(),
            model_id: "gpt-realtime".into(),
            voice_id: "marin".into(),
            instructions: "test".into(),
            tools: vec![],
            history: vec![],
        };
        let session: Value =
            serde_json::from_str(&session_update_json(VoiceProvider::Openai, &request)).unwrap();
        assert!(session["session"].get("voice").is_none());
        assert!(session["session"].get("turn_detection").is_none());
        assert_eq!(
            session.pointer("/session/audio/input/format/rate"),
            Some(&json!(24000))
        );
        assert_eq!(
            session.pointer("/session/audio/input/turn_detection/type"),
            Some(&json!("server_vad"))
        );
        let event = VoiceEvent::AudioPcm(vec![0, 1, 2, 3]);
        let Some(Message::Text(text)) = encode_provider_event(VoiceProvider::Openai, &event) else {
            panic!("expected JSON audio")
        };
        let encoded: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(encoded["type"], "input_audio_buffer.append");
        assert_eq!(encoded["audio"], "AAECAw==");
        assert!(matches!(
            encode_provider_event(VoiceProvider::Xai, &event),
            Some(Message::Binary(_))
        ));
    }

    #[test]
    fn resolve_voice_defaults_and_requires_key() {
        let missing = resolve_voice(ResolveVoiceRequest {
            provider: VoiceProvider::Xai,
            model_id: None,
            voice_id: None,
            credentials: CredentialChain {
                bot: None,
                space: None,
                env: None,
            },
        });
        assert!(matches!(missing, Err(VoiceError::MissingCredential { .. })));

        let ok = resolve_voice(ResolveVoiceRequest {
            provider: VoiceProvider::Xai,
            model_id: None,
            voice_id: None,
            credentials: CredentialChain {
                bot: None,
                space: Some("sk".into()),
                env: None,
            },
        })
        .unwrap();
        assert_eq!(ok.model_id, "grok-voice-latest");
        assert_eq!(ok.voice_id, "eve");
        assert_eq!(ok.api_key, "sk");
    }

    #[test]
    fn parses_xai_and_openai_event_aliases() {
        let started = parse_provider_event(r#"{"type":"input_audio_buffer.speech_started"}"#);
        assert!(matches!(started, Some(VoiceEvent::SpeechStarted)));

        let transcript = parse_provider_event(
            r#"{"type":"conversation.item.input_audio_transcription.completed","transcript":"hello"}"#,
        );
        match transcript {
            Some(VoiceEvent::InputTranscript { text, final_ }) => {
                assert_eq!(text, "hello");
                assert!(final_);
            }
            other => panic!("{other:?}"),
        }

        let old_audio = parse_provider_event(r#"{"type":"response.audio.delta","delta":"AQID"}"#);
        assert!(matches!(old_audio, Some(VoiceEvent::AudioPcm(_))));

        let tool = parse_provider_event(
            r#"{"type":"response.function_call_arguments.done","call_id":"1","name":"start_computer_task","arguments":"{}"}"#,
        );
        match tool {
            Some(VoiceEvent::FunctionCall { name, .. }) => {
                assert_eq!(name, "start_computer_task");
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn scripted_voice_emits_transcript_and_tool() {
        let provider = create_voice(VoiceProvider::Scripted);
        let mut socket = provider
            .connect(VoiceConnectRequest {
                api_key: String::new(),
                model_id: "scripted-voice".into(),
                voice_id: "scripted".into(),
                instructions: "test".into(),
                tools: computer_voice_tools(),
                history: Vec::new(),
            })
            .await
            .unwrap();
        socket
            .send(VoiceEvent::AudioPcm(vec![0, 1, 2, 3]))
            .await
            .unwrap();
        let mut kinds = Vec::new();
        for _ in 0..3 {
            match socket.recv().await.unwrap() {
                Some(VoiceEvent::SpeechStarted) => kinds.push("start"),
                Some(VoiceEvent::InputTranscript { final_: true, .. }) => kinds.push("in"),
                Some(VoiceEvent::FunctionCall { name, .. }) => {
                    assert_eq!(name, "start_computer_task");
                    kinds.push("tool");
                }
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(kinds, ["start", "in", "tool"]);
        socket
            .send(VoiceEvent::FunctionCallOutput {
                call_id: "call_scripted".into(),
                output: json!({"status":"queued"}).to_string(),
            })
            .await
            .unwrap();
        match socket.recv().await.unwrap() {
            Some(VoiceEvent::OutputTranscript { text, final_ }) => {
                assert!(final_);
                assert!(text.contains("電腦"));
            }
            other => panic!("{other:?}"),
        }
        match socket.recv().await.unwrap() {
            Some(VoiceEvent::AudioPcm(bytes)) => assert!(!bytes.is_empty()),
            other => panic!("{other:?}"),
        }
    }
}
