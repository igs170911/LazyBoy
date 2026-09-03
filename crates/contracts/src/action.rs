use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PointerType {
    Move,
    Down,
    Up,
    Click,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PointerButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScrollDirection {
    Up,
    Down,
}

/// Canonical actions the control plane understands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ComputerAction {
    Pointer {
        x: u32,
        y: u32,
        #[serde(rename = "type")]
        pointer_type: PointerType,
        #[serde(default)]
        button: Option<PointerButton>,
    },
    Clipboard {
        text: String,
    },
    Key {
        key: String,
        #[serde(default)]
        modifiers: Option<Vec<String>>,
    },
    Scroll {
        direction: ScrollDirection,
        #[serde(default)]
        amount: Option<u32>,
    },
    Wait {
        ms: u32,
    },
    Open {
        path: String,
    },
    Launch {
        application: String,
        #[serde(default)]
        uri: Option<String>,
    },
    Focus {
        title: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CursorPosition {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveWindow {
    pub id: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputerObservation {
    pub frame_id: String,
    pub captured_at: String,
    pub mime_type: String,
    #[serde(with = "serde_bytes_opt")]
    pub image: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub cursor: Option<CursorPosition>,
    pub active_window: Option<ActiveWindow>,
}

mod serde_bytes_opt {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        STANDARD.decode(text).map_err(serde::de::Error::custom)
    }
}
