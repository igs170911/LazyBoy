use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub bot_id: String,
    pub title: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub next_message_seq: i32,
    pub history_summary: String,
    pub history_summary_seq: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionInput {
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSessionInput {
    pub title: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessage {
    pub id: String,
    pub session_id: String,
    pub seq: i32,
    pub role: String,
    pub body: String,
    pub blocks: Value,
    pub run_id: Option<String>,
    pub client_nonce: Option<String>,
    pub created_at: DateTime<Utc>,
    pub speaker_bot_id: Option<String>,
    pub speaker_name: Option<String>,
    pub speaker_color: Option<String>,
    pub speaker_shape: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendSessionMessageInput {
    pub text: String,
    pub client_nonce: Option<String>,
    #[serde(default)]
    pub blocks: Vec<Value>,
    #[serde(default)]
    pub attachments: Vec<SessionAttachment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionAttachment {
    pub name: String,
    #[serde(default)]
    pub mime_type: String,
    #[serde(alias = "contentBase64")]
    pub content: String,
}
