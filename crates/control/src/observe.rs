use chrono::Utc;
use lazyboy_contracts::{ActiveWindow, ComputerObservation, CursorPosition};
use sha2::{Digest, Sha256};

pub fn observation_from_png(
    image: Vec<u8>,
    width: u32,
    height: u32,
    cursor: Option<CursorPosition>,
    active_window: Option<ActiveWindow>,
) -> ComputerObservation {
    ComputerObservation {
        frame_id: hex::encode(Sha256::digest(&image)),
        captured_at: Utc::now().to_rfc3339(),
        mime_type: "image/png".to_string(),
        image,
        width,
        height,
        cursor,
        active_window,
    }
}

pub fn frames_match(previous_frame_id: Option<&str>, observation: &ComputerObservation) -> bool {
    previous_frame_id == Some(observation.frame_id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_bytes_share_a_frame_id() {
        let a = observation_from_png(vec![1, 2, 3], 1, 1, None, None);
        let b = observation_from_png(vec![1, 2, 3], 1, 1, None, None);
        assert_eq!(a.frame_id, b.frame_id);
        assert!(frames_match(Some(&a.frame_id), &b));
        let c = observation_from_png(vec![1, 2, 4], 1, 1, None, None);
        assert!(!frames_match(Some(&a.frame_id), &c));
    }
}
