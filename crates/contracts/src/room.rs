use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomMember {
    pub id: String,
    pub name: String,
    pub avatar_color: String,
    pub avatar_shape: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Room {
    pub id: String,
    pub name: String,
    pub members: Vec<RoomMember>,
    /// The member that answers what nobody was named for. `None` means "the
    /// first member", which is what rooms used before this column resolve to.
    pub host_bot_id: Option<String>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub last_preview: Option<String>,
    pub unread_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRoomInput {
    pub name: String,
    pub member_ids: Vec<String>,
    /// Optional from the first version: leaving it out makes the first member
    /// the host, which is what a group needs by default.
    #[serde(default)]
    pub host_bot_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRoomInput {
    /// Must be one of the room's members.
    #[serde(default)]
    pub host_bot_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomStatus {
    pub busy: Vec<RoomMember>,
}
