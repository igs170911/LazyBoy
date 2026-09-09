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

/// Pixel size of a captured frame, or `None` when the bytes are not decodable.
/// Capture inputs may use different codecs (imported frames may be JPEG
/// from `xwd | convert`, the Cua driver writes PNG), so the dimensions have to
/// come from the frame itself rather than from a configured constant.
pub fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    use image::ImageDecoder;

    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let decoder = reader.into_decoder().ok()?;
    Some(decoder.dimensions())
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

/// Titles longer than this are cut with an ellipsis. AT-SPI labels in chat and
/// Electron clients can be entire message bodies, and a label only has to
/// identify a control.
const MAX_ELEMENT_TITLE_CHARS: usize = 60;

/// One line per control: `[id] kind "title" @ x,y`. Models address controls by
/// id (see `actions::element_id`), so this list is meant to replace the element
/// array in a tool result, not to sit next to it; `max` keeps a dense desktop
/// inside the per-turn budget and says what was dropped.
pub fn format_ui_element_lines(elements: &[UiElement], max: usize) -> String {
    if elements.is_empty() {
        return "none".into();
    }
    let mut lines: Vec<String> = elements
        .iter()
        .take(max)
        .map(|element| {
            let (x, y) = element.center();
            let kind = element
                .role
                .as_deref()
                .or(element.kind.as_deref())
                .unwrap_or("control");
            let title: String = element
                .title
                .chars()
                .filter(|character| *character != '\n' && *character != '\r')
                .take(MAX_ELEMENT_TITLE_CHARS)
                .collect();
            let ellipsis = if element.title.chars().count() > MAX_ELEMENT_TITLE_CHARS {
                "…"
            } else {
                ""
            };
            format!("[{}] {kind} \"{title}{ellipsis}\" @ {x},{y}", element.id)
        })
        .collect();
    if let Some(dropped) = elements.len().checked_sub(max) {
        lines.push(format!(
            "+{dropped} more not listed: re-observe after scrolling, or aim at the coordinates above"
        ));
    }
    lines.join("\n")
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

/// How a capture differs from the frame the Agent last saw. `Identical` is the
/// transport fact: the same picture is already in the model's context. Byte
/// equality alone is too strict for advice, because a panel clock or a blinking
/// caret makes every capture a new sha256 and a click that did nothing would
/// never be recognised; `Similar` is the perceptual answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenChange {
    Identical,
    Similar,
    Changed,
}

/// Compare a capture against the previous frame id and signature. Skipping a
/// screenshot is still only correct for `Identical`: a change too small to see
/// is a change the model should be allowed to look at, while "nothing moved,
/// do not repeat this" may use `Similar`.
pub fn screen_change_between(
    previous_frame: Option<&str>,
    previous_signature: Option<&[u8]>,
    observation: &ComputerObservation,
) -> ScreenChange {
    if frames_match(previous_frame, observation) {
        return ScreenChange::Identical;
    }
    let (Some(previous), Some(current)) = (previous_signature, frame_signature(&observation.image))
    else {
        return ScreenChange::Changed;
    };
    if signatures_similar(previous, &current) {
        ScreenChange::Similar
    } else {
        ScreenChange::Changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::jpeg::JpegEncoder;
    use image::codecs::png::PngEncoder;
    use image::{ExtendedColorType, ImageEncoder, Rgb, RgbImage};
    use std::io::Cursor;

    #[test]
    fn dimensions_come_from_the_frame_for_both_codecs() {
        let frame = RgbImage::from_pixel(96, 48, Rgb([10, 20, 30]));

        let mut png = Cursor::new(Vec::new());
        PngEncoder::new(&mut png)
            .write_image(frame.as_raw(), 96, 48, ExtendedColorType::Rgb8)
            .unwrap();
        assert_eq!(image_dimensions(&png.into_inner()), Some((96, 48)));

        // Imported captures can use JPEG, so a hardcoded size would drift the
        // moment the Xvfb geometry changes.
        let mut jpeg = Cursor::new(Vec::new());
        JpegEncoder::new_with_quality(&mut jpeg, 60)
            .encode(frame.as_raw(), 96, 48, ExtendedColorType::Rgb8)
            .unwrap();
        assert_eq!(image_dimensions(&jpeg.into_inner()), Some((96, 48)));

        // A truncated capture must not invent a size.
        assert_eq!(image_dimensions(&[0xFF, 0xD8, 0xFF]), None);
        assert_eq!(image_dimensions(&[]), None);
    }

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
    fn screen_change_calls_a_clock_tick_similar_not_identical() {
        fn png(paint: impl Fn(u32, u32) -> Rgb<u8>) -> Vec<u8> {
            let img = RgbImage::from_fn(320, 180, paint);
            let mut out = Vec::new();
            PngEncoder::new(&mut out)
                .write_image(img.as_raw(), 320, 180, ExtendedColorType::Rgb8)
                .unwrap();
            out
        }
        let plain = png(|_, _| Rgb([240, 240, 240]));
        let clock = png(|x, y| {
            if x < 6 && y < 6 {
                Rgb([0, 0, 0])
            } else {
                Rgb([240, 240, 240])
            }
        });
        let dialog = png(|x, y| {
            if x < 160 && y < 90 {
                Rgb([20, 20, 20])
            } else {
                Rgb([240, 240, 240])
            }
        });

        let before = observation_from_png(plain.clone(), 320, 180, None, None);
        let signature = frame_signature(&plain);

        // Byte for byte the same frame: the model already holds this picture.
        let same = observation_from_png(plain.clone(), 320, 180, None, None);
        assert_eq!(
            screen_change_between(Some(before.frame_id.as_str()), signature.as_deref(), &same),
            ScreenChange::Identical
        );

        // A flipping panel clock is a new sha256 of the same screen. Only the
        // perceptual answer lets the "stop repeating this click" advice fire.
        let tick = observation_from_png(clock, 320, 180, None, None);
        assert_ne!(tick.frame_id, before.frame_id);
        assert_eq!(
            screen_change_between(Some(before.frame_id.as_str()), signature.as_deref(), &tick),
            ScreenChange::Similar
        );

        let covered = observation_from_png(dialog, 320, 180, None, None);
        assert_eq!(
            screen_change_between(
                Some(before.frame_id.as_str()),
                signature.as_deref(),
                &covered
            ),
            ScreenChange::Changed
        );

        // With no signature to compare, only byte equality counts.
        assert_eq!(
            screen_change_between(None, None, &same),
            ScreenChange::Changed
        );
    }

    #[test]
    fn element_lines_cap_the_count_and_long_labels() {
        let elements: Vec<UiElement> = (1..=5)
            .map(|index| UiElement {
                id: index,
                title: format!("Window {index}"),
                x: index * 10,
                y: index * 20,
                w: 40,
                h: 20,
                kind: Some("window".into()),
                ..UiElement::default()
            })
            .collect();
        let listing = format_ui_element_lines(&elements, 3);
        let lines: Vec<&str> = listing.lines().collect();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], "[1] window \"Window 1\" @ 30,30");
        assert!(listing.contains("+2 more not listed"));
        assert!(!listing.contains("Window 5"));

        // A label identifies a control; it is not a text channel.
        let chatty = vec![UiElement {
            id: 7,
            title: "x".repeat(200),
            ..UiElement::default()
        }];
        let line = format_ui_element_lines(&chatty, 10);
        assert_eq!(line.matches('x').count(), MAX_ELEMENT_TITLE_CHARS);
        assert!(line.ends_with("\" @ 0,0"));

        // A role tells the model more than the internal kind does.
        let control = vec![UiElement {
            id: 3,
            title: "Send".into(),
            kind: Some("dom".into()),
            role: Some("button".into()),
            ..UiElement::default()
        }];
        assert!(format_ui_element_lines(&control, 10).starts_with("[3] button \"Send\""));
        assert_eq!(format_ui_element_lines(&[], 10), "none");
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
