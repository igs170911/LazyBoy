use base64::Engine;
use chrono::Utc;
use lazyboy_contracts::{ActiveWindow, ComputerObservation, CursorPosition, UiElement};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub fn observation_from_png(
    image: Vec<u8>,
    width: u32,
    height: u32,
    cursor: Option<CursorPosition>,
    active_window: Option<ActiveWindow>,
) -> ComputerObservation {
    ComputerObservation {
        native_observation_complete: false,
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

pub fn observation_to_control_json(observation: &ComputerObservation) -> Value {
    let mut body = json!({
        "png_base64": base64::engine::general_purpose::STANDARD.encode(&observation.image),
        "native_observation_complete": observation.native_observation_complete,
    });
    if let Some(cursor) = &observation.cursor {
        body["cursor"] = json!({ "x": cursor.x, "y": cursor.y });
    }
    if let Some(window) = &observation.active_window {
        body["activeWindow"] = json!({ "id": window.id, "title": window.title });
    }
    if !observation.elements.is_empty()
        && let Ok(value) = serde_json::to_value(&observation.elements)
    {
        body["elements"] = value;
    }
    body
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

const SIGNATURE_W: u32 = 32;
const SIGNATURE_H: u32 = 18;

/// Coarse grayscale thumbnail of a screenshot. Two frames whose signatures
/// are `similar` differ only in small details (panel clock, caret blink),
/// which a byte-level `frame_id` comparison would treat as a new frame.
pub fn frame_signature(image: &[u8]) -> Option<Vec<u8>> {
    let dynamic = image::load_from_memory(image).ok()?;
    let thumb = dynamic
        .resize_exact(
            SIGNATURE_W,
            SIGNATURE_H,
            image::imageops::FilterType::Triangle,
        )
        .to_luma8();
    Some(thumb.into_raw())
}

/// True when fewer than 2% of the coarse cells moved by a visible amount.
pub fn signatures_similar(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() || a.is_empty() {
        return false;
    }
    let changed = a
        .iter()
        .zip(b)
        .filter(|(x, y)| x.abs_diff(**y) > 24)
        .count();
    changed * 50 < a.len()
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
    fn signature_ignores_tiny_changes_but_sees_big_ones() {
        use image::{ImageEncoder, Rgb, RgbImage};
        fn png(paint: impl Fn(u32, u32) -> Rgb<u8>) -> Vec<u8> {
            let img = RgbImage::from_fn(320, 180, paint);
            let mut out = Vec::new();
            image::codecs::png::PngEncoder::new(&mut out)
                .write_image(img.as_raw(), 320, 180, image::ExtendedColorType::Rgb8)
                .unwrap();
            out
        }
        let base = frame_signature(&png(|_, _| Rgb([240, 240, 240]))).unwrap();
        // A panel clock flipping digits touches a couple of pixels only.
        let clock = frame_signature(&png(|x, y| {
            if x < 6 && y < 6 {
                Rgb([0, 0, 0])
            } else {
                Rgb([240, 240, 240])
            }
        }))
        .unwrap();
        // A dialog covering a quarter of the screen.
        let dialog = frame_signature(&png(|x, y| {
            if x < 160 && y < 90 {
                Rgb([20, 20, 20])
            } else {
                Rgb([240, 240, 240])
            }
        }))
        .unwrap();
        assert!(signatures_similar(&base, &clock));
        assert!(!signatures_similar(&base, &dialog));
        assert!(frame_signature(b"not an image").is_none());
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
