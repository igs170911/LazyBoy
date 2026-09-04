use chrono::Utc;
use lazyboy_contracts::{ActiveWindow, ComputerObservation, CursorPosition, UiElement};
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
        mime_type: sniff_image_mime(&image).to_string(),
        image,
        width,
        height,
        cursor,
        active_window,
        elements: Vec::new(),
    }
}

pub fn observation_with_elements(
    mut observation: ComputerObservation,
    elements: Vec<UiElement>,
) -> ComputerObservation {
    observation.elements = elements;
    observation
}

pub fn format_ui_elements(elements: &[UiElement]) -> String {
    if elements.is_empty() {
        return "none".into();
    }
    elements
        .iter()
        .map(|element| format!("[{}] {}", element.id, element.title))
        .collect::<Vec<_>>()
        .join(" ")
}

fn sniff_image_mime(image: &[u8]) -> &'static str {
    if image.len() >= 3 && image[0] == 0xFF && image[1] == 0xD8 && image[2] == 0xFF {
        "image/jpeg"
    } else {
        "image/png"
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

    #[test]
    fn jpeg_magic_sets_mime() {
        let observation = observation_from_png(vec![0xFF, 0xD8, 0xFF, 0x00], 1, 1, None, None);
        assert_eq!(observation.mime_type, "image/jpeg");
    }

    #[test]
    fn formats_numbered_windows() {
        let elements = vec![UiElement {
            id: 1,
            title: "Chromium".into(),
            x: 0,
            y: 0,
            w: 100,
            h: 40,
            ..UiElement::default()
        }];
        assert_eq!(format_ui_elements(&elements), "[1] Chromium");
        assert_eq!(format_ui_elements(&[]), "none");
    }
}
