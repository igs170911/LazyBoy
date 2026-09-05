use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenLease {
    pub owner_id: String,
    pub fence: u32,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("computer is busy")]
pub struct ComputerBusyError;

pub fn screen_lease_id(run_id: &str, fence: u32) -> String {
    format!("{run_id}:{fence}")
}

pub fn parse_screen_lease_id(lease_id: &str) -> ScreenLease {
    match lease_id.rsplit_once(':') {
        Some((owner, fence)) if !owner.is_empty() => {
            if let Ok(fence) = fence.parse::<u32>() {
                return ScreenLease {
                    owner_id: owner.to_string(),
                    fence,
                };
            }
        }
        _ => {}
    }
    ScreenLease {
        owner_id: lease_id.to_string(),
        fence: 0,
    }
}

pub fn can_take_screen_lease(existing: Option<&str>, incoming: Option<&str>) -> bool {
    let Some(incoming) = incoming else {
        return false;
    };
    match existing {
        None => true,
        Some(existing) if existing == incoming => true,
        Some(existing) => {
            parse_screen_lease_id(incoming).fence > parse_screen_lease_id(existing).fence
        }
    }
}

pub fn can_release_screen_lease(existing: Option<&str>, incoming: Option<&str>) -> bool {
    match (existing, incoming) {
        (_, None) | (None, _) => true,
        (Some(existing), Some(incoming)) if existing == incoming => true,
        (Some(existing), Some(incoming)) => {
            let current = parse_screen_lease_id(existing);
            let next = parse_screen_lease_id(incoming);
            next.owner_id == current.owner_id && next.fence >= current.fence
        }
    }
}

pub fn next_fence(current: u32) -> u32 {
    current.saturating_add(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_fence_takes_the_screen() {
        assert!(can_take_screen_lease(None, Some("run-a:1")));
        assert!(can_take_screen_lease(Some("run-a:1"), Some("run-a:1")));
        assert!(can_take_screen_lease(Some("run-a:1"), Some("run-b:2")));
        assert!(!can_take_screen_lease(Some("run-a:2"), Some("run-b:1")));
        assert!(!can_take_screen_lease(Some("run-a:1"), None));
    }

    #[test]
    fn release_requires_same_owner_and_non_decreasing_fence() {
        assert!(can_release_screen_lease(Some("run-a:1"), Some("run-a:1")));
        assert!(can_release_screen_lease(Some("run-a:1"), Some("run-a:2")));
        assert!(!can_release_screen_lease(Some("run-a:2"), Some("run-b:3")));
        assert!(!can_release_screen_lease(Some("run-a:2"), Some("run-a:1")));
    }
}
