use lazyboy_contracts::{
    BrowserProfileMode, ComputerCapabilities, MULTI_SCREEN_UNAVAILABLE, PROFILE_LOCKED,
    TEAM_SCREENS_FULL,
};
pub use lazyboy_contracts::TEAM_SCREEN_LIMIT;
use thiserror::Error;

use crate::HOME;

pub const PRIMARY_DISPLAY: &str = ":1";
pub const PRIMARY_VIEW_PORT: u16 = 6080;
pub const PRIMARY_VNC_PORT: u16 = 5900;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenLayout {
    pub slot: u32,
    pub display: String,
    pub display_number: u32,
    pub view_port: u16,
    pub vnc_port: u16,
    pub is_primary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuiBlock {
    MultiScreenUnavailable { busy_bot_name: Option<String> },
    ScreensFull,
    ProfileLocked { bot_name: String },
}

impl GuiBlock {
    pub fn message(&self) -> String {
        match self {
            Self::MultiScreenUnavailable { busy_bot_name } => match busy_bot_name {
                Some(name) => format!("{MULTI_SCREEN_UNAVAILABLE} Currently in use by {name}."),
                None => MULTI_SCREEN_UNAVAILABLE.to_string(),
            },
            Self::ScreensFull => TEAM_SCREENS_FULL.to_string(),
            Self::ProfileLocked { bot_name } => {
                format!("{PROFILE_LOCKED} Currently in use by {bot_name}.")
            }
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("{0}")]
pub struct ScreenError(pub String);

pub fn screen_layout(slot: u32) -> Result<ScreenLayout, ScreenError> {
    if slot >= TEAM_SCREEN_LIMIT {
        return Err(ScreenError(format!("screen slot {slot} is out of range")));
    }
    let display_number = slot + 1;
    Ok(ScreenLayout {
        slot,
        display: format!(":{display_number}"),
        display_number,
        view_port: PRIMARY_VIEW_PORT + slot as u16,
        vnc_port: PRIMARY_VNC_PORT + slot as u16,
        is_primary: slot == 0,
    })
}

pub fn allocate_slot(used: &[u32]) -> Option<u32> {
    (0..TEAM_SCREEN_LIMIT).find(|slot| !used.contains(slot))
}

pub fn browser_profile_path(mode: BrowserProfileMode, bot_id: &str, run_id: Option<&str>) -> String {
    match mode {
        BrowserProfileMode::Shared => format!("{HOME}/.browser-profiles/chromium"),
        BrowserProfileMode::PerBot => format!("{HOME}/.browser-profiles/bots/{bot_id}"),
        BrowserProfileMode::PerTask => {
            let task = run_id.filter(|value| !value.is_empty()).unwrap_or("task");
            format!("{HOME}/.browser-profiles/tasks/{task}")
        }
    }
}

pub fn profile_lock_key(mode: BrowserProfileMode, bot_id: &str, run_id: Option<&str>) -> String {
    match mode {
        BrowserProfileMode::Shared => "shared".into(),
        BrowserProfileMode::PerBot => format!("bot:{bot_id}"),
        BrowserProfileMode::PerTask => {
            format!("task:{}", run_id.filter(|value| !value.is_empty()).unwrap_or("task"))
        }
    }
}

pub fn can_take_profile_lock(holder_bot_id: Option<&str>, incoming_bot_id: &str) -> bool {
    match holder_bot_id {
        None => true,
        Some(holder) if holder == incoming_bot_id => true,
        Some(_) => false,
    }
}

/// Decide whether this bot may drive a GUI session.
///
/// Shell and files are never gated here. Whole-machine locks are not a
/// concurrency model: only a missing multi-screen capability, a full slot
/// table, or a shared profile lock can block desktop tools.
pub fn admit_gui(
    capabilities: &ComputerCapabilities,
    requester_bot_id: &str,
    requester_has_screen: bool,
    other_gui_bot_name: Option<&str>,
    profile_lock_holder: Option<&str>,
    profile_lock_holder_name: Option<&str>,
) -> Result<(), GuiBlock> {
    if !capabilities.multi_screen && other_gui_bot_name.is_some() && !requester_has_screen {
        return Err(GuiBlock::MultiScreenUnavailable {
            busy_bot_name: other_gui_bot_name.map(str::to_string),
        });
    }
    if !can_take_profile_lock(profile_lock_holder, requester_bot_id) {
        return Err(GuiBlock::ProfileLocked {
            bot_name: profile_lock_holder_name
                .unwrap_or("another bot")
                .to_string(),
        });
    }
    Ok(())
}

pub fn admit_new_screen(
    capabilities: &ComputerCapabilities,
    used_slots: &[u32],
    requester_existing_slot: Option<u32>,
) -> Result<u32, GuiBlock> {
    if let Some(slot) = requester_existing_slot {
        return Ok(slot);
    }
    if !capabilities.multi_screen && !used_slots.is_empty() {
        return Err(GuiBlock::MultiScreenUnavailable { busy_bot_name: None });
    }
    allocate_slot(used_slots).ok_or(GuiBlock::ScreensFull)
}

pub fn normalize_display(display: &str) -> &str {
    if display.is_empty() {
        PRIMARY_DISPLAY
    } else {
        display
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenTarget {
    pub display: String,
    pub profile_path: Option<String>,
    pub slot: u32,
}

impl Default for ScreenTarget {
    fn default() -> Self {
        Self {
            display: PRIMARY_DISPLAY.into(),
            profile_path: None,
            slot: 0,
        }
    }
}

impl ScreenTarget {
    pub fn from_parts(display: Option<&str>, profile_path: Option<&str>, slot: Option<u32>) -> Self {
        Self {
            display: normalize_display(display.unwrap_or_default()).to_string(),
            profile_path: profile_path.filter(|value| !value.is_empty()).map(str::to_string),
            slot: slot.unwrap_or(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_map_to_independent_displays() {
        let a = screen_layout(0).unwrap();
        let b = screen_layout(1).unwrap();
        assert_eq!(a.display, ":1");
        assert_eq!(b.display, ":2");
        assert_ne!(a.view_port, b.view_port);
        assert_ne!(a.vnc_port, b.vnc_port);
        assert!(a.is_primary);
        assert!(!b.is_primary);
    }

    #[test]
    fn two_bots_get_distinct_slots() {
        let first = allocate_slot(&[]).unwrap();
        let second = allocate_slot(&[first]).unwrap();
        assert_eq!(first, 0);
        assert_eq!(second, 1);
        assert_ne!(
            screen_layout(first).unwrap().display,
            screen_layout(second).unwrap().display
        );
    }

    #[test]
    fn per_bot_profiles_do_not_share_cookie_dirs() {
        let a = browser_profile_path(BrowserProfileMode::PerBot, "bot-a", None);
        let b = browser_profile_path(BrowserProfileMode::PerBot, "bot-b", None);
        assert_ne!(a, b);
        assert!(a.ends_with("/bots/bot-a"));
        assert_eq!(
            browser_profile_path(BrowserProfileMode::Shared, "bot-a", None),
            browser_profile_path(BrowserProfileMode::Shared, "bot-b", None)
        );
        assert_ne!(
            profile_lock_key(BrowserProfileMode::PerBot, "bot-a", None),
            profile_lock_key(BrowserProfileMode::PerBot, "bot-b", None)
        );
        assert_eq!(
            profile_lock_key(BrowserProfileMode::Shared, "bot-a", None),
            profile_lock_key(BrowserProfileMode::Shared, "bot-b", None)
        );
    }

    #[test]
    fn multi_screen_false_blocks_a_second_bot_gui_only() {
        let caps = ComputerCapabilities { multi_screen: false };
        let err = admit_new_screen(&caps, &[0], None).unwrap_err();
        assert!(matches!(err, GuiBlock::MultiScreenUnavailable { .. }));
        assert!(admit_new_screen(&caps, &[0], Some(0)).is_ok());
        assert!(admit_new_screen(&ComputerCapabilities { multi_screen: true }, &[0], None).is_ok());
    }

    #[test]
    fn shared_profile_lock_is_exclusive() {
        assert!(can_take_profile_lock(None, "a"));
        assert!(can_take_profile_lock(Some("a"), "a"));
        assert!(!can_take_profile_lock(Some("a"), "b"));
        let err = admit_gui(
            &ComputerCapabilities { multi_screen: true },
            "b",
            true,
            None,
            Some("a"),
            Some("Alpha"),
        )
        .unwrap_err();
        assert!(matches!(err, GuiBlock::ProfileLocked { .. }));
        assert!(err.message().contains("Alpha"));
    }

    #[test]
    fn different_bots_are_not_serialized_by_a_machine_lock() {
        let caps = ComputerCapabilities { multi_screen: true };
        assert!(admit_gui(&caps, "a", true, Some("b"), Some("a"), Some("A")).is_ok());
        assert!(admit_gui(&caps, "b", true, Some("a"), Some("b"), Some("B")).is_ok());
    }
}
