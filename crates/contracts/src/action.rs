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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RefVerb {
    Click,
    Focus,
    SetValue,
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
    /// Semantic target (DOM selector or AT-SPI path). Execute via CDP/a11y, not xdotool.
    Ref {
        verb: RefVerb,
        target: String,
        #[serde(default, rename = "refKind")]
        ref_kind: String,
        #[serde(default)]
        text: Option<String>,
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

/// A numbered on-screen target the model can click without guessing pixels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UiElement {
    pub id: u32,
    pub title: String,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    /// CSS selector (DOM) or AT-SPI path (a11y). Native windows leave this empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    /// "dom" for page controls, "a11y" for AT-SPI widgets, "window" for native windows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

impl UiElement {
    pub fn has_ref(&self) -> bool {
        self.selector
            .as_deref()
            .is_some_and(|value| !value.is_empty())
            && matches!(self.kind.as_deref(), Some("dom") | Some("a11y"))
    }

    pub fn center(&self) -> (u32, u32) {
        (
            self.x.saturating_add(self.w / 2),
            self.y.saturating_add(self.h / 2),
        )
    }

    /// Page controls outside the viewport are reported with zero size: they
    /// can be clicked through the DOM but have no pixel position.
    pub fn is_offscreen(&self) -> bool {
        self.w == 0 && self.h == 0 && self.selector.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputerObservation {
    /// The controller already populated native semantics; skip legacy enrichment.
    #[serde(default)]
    pub native_observation_complete: bool,
    pub frame_id: String,
    pub captured_at: String,
    pub mime_type: String,
    #[serde(with = "serde_bytes_opt")]
    pub image: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub cursor: Option<CursorPosition>,
    pub active_window: Option<ActiveWindow>,
    #[serde(default)]
    pub elements: Vec<UiElement>,
}

mod serde_bytes_opt {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        STANDARD.decode(text).map_err(serde::de::Error::custom)
    }
}
