use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, Rgb, RgbImage};
use lazyboy_contracts::UiElement;

const YELLOW: Rgb<u8> = Rgb([255, 214, 0]);
const BLACK: Rgb<u8> = Rgb([16, 16, 16]);
const SCALE: u32 = 6;

/// 5x7 bitmap digits, MSB is the left pixel of each row.
const FONT: [[u8; 7]; 10] = [
    [
        0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
    ],
    [
        0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
    ],
    [
        0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111,
    ],
    [
        0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110,
    ],
    [
        0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
    ],
    [
        0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
    ],
    [
        0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
    ],
    [
        0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
    ],
    [
        0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
    ],
    [
        0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
    ],
];

/// Draw numbered boxes on a screenshot for the model. The live VNC feed is unchanged.
/// Empty element lists and undecodable bytes are returned as-is.
pub fn overlay_elements(bytes: &[u8], elements: &[UiElement]) -> Vec<u8> {
    if elements.is_empty() {
        return bytes.to_vec();
    }
    let Ok(dynamic) = image::load_from_memory(bytes) else {
        return bytes.to_vec();
    };
    let mut img = dynamic.to_rgb8();
    let (width, height) = img.dimensions();
    if width == 0 || height == 0 {
        return bytes.to_vec();
    }
    for element in elements.iter().take(50) {
        paint_element(&mut img, element, width, height);
    }
    encode_jpeg(&img).unwrap_or_else(|| bytes.to_vec())
}

fn paint_element(img: &mut RgbImage, element: &UiElement, width: u32, height: u32) {
    let x1 = element.x.min(width.saturating_sub(1));
    let y1 = element.y.min(height.saturating_sub(1));
    let x2 = element
        .x
        .saturating_add(element.w.max(1))
        .saturating_sub(1)
        .min(width.saturating_sub(1));
    let y2 = element
        .y
        .saturating_add(element.h.max(1))
        .saturating_sub(1)
        .min(height.saturating_sub(1));
    if x2 <= x1 || y2 <= y1 {
        draw_badge(img, x1, y1, element.id);
        return;
    }
    let area = u64::from(element.w.saturating_mul(element.h.max(1)));
    let full = u64::from(width) * u64::from(height);
    if full == 0 || area.saturating_mul(10) < full.saturating_mul(7) {
        stroke_rect(img, x1, y1, x2, y2, BLACK, 6);
        stroke_rect(img, x1, y1, x2, y2, YELLOW, 3);
    }
    draw_badge(img, x1, y1, element.id);
}

fn stroke_rect(
    img: &mut RgbImage,
    x1: u32,
    y1: u32,
    x2: u32,
    y2: u32,
    color: Rgb<u8>,
    thickness: u32,
) {
    for t in 0..thickness {
        let left = x1.saturating_add(t).min(x2);
        let right = x2.saturating_sub(t).max(x1);
        let top = y1.saturating_add(t).min(y2);
        let bottom = y2.saturating_sub(t).max(y1);
        for x in left..=right {
            put(img, x, top, color);
            put(img, x, bottom, color);
        }
        for y in top..=bottom {
            put(img, left, y, color);
            put(img, right, y, color);
        }
    }
}

fn draw_badge(img: &mut RgbImage, x: u32, y: u32, id: u32) {
    let label = id.to_string();
    let digits = label.len() as u32;
    let pad = 4;
    let digit_w = 5 * SCALE + 2;
    let digit_h = 7 * SCALE;
    let bw = pad * 2 + digits * digit_w - 2;
    let bh = pad * 2 + digit_h;
    let (width, height) = img.dimensions();
    let bx = x.min(width.saturating_sub(bw.max(1)));
    let by = if y >= bh + 2 {
        y.saturating_sub(bh + 2)
    } else {
        y.min(height.saturating_sub(bh.max(1)))
    };
    fill_rect(
        img,
        bx,
        by,
        bx.saturating_add(bw.saturating_sub(1)),
        by.saturating_add(bh.saturating_sub(1)),
        YELLOW,
    );
    stroke_rect(
        img,
        bx,
        by,
        bx.saturating_add(bw.saturating_sub(1)),
        by.saturating_add(bh.saturating_sub(1)),
        BLACK,
        2,
    );
    for (i, ch) in label.bytes().enumerate() {
        if !(b'0'..=b'9').contains(&ch) {
            continue;
        }
        let dx = bx + pad + i as u32 * digit_w;
        let dy = by + pad;
        draw_digit(img, dx, dy, (ch - b'0') as usize);
    }
}

fn fill_rect(img: &mut RgbImage, x1: u32, y1: u32, x2: u32, y2: u32, color: Rgb<u8>) {
    for y in y1..=y2 {
        for x in x1..=x2 {
            put(img, x, y, color);
        }
    }
}

fn draw_digit(img: &mut RgbImage, x: u32, y: u32, digit: usize) {
    let Some(rows) = FONT.get(digit) else {
        return;
    };
    for (row, bits) in rows.iter().enumerate() {
        for col in 0..5u32 {
            if bits & (1 << (4 - col)) == 0 {
                continue;
            }
            for oy in 0..SCALE {
                for ox in 0..SCALE {
                    put(
                        img,
                        x.saturating_add(col * SCALE + ox),
                        y.saturating_add(row as u32 * SCALE + oy),
                        BLACK,
                    );
                }
            }
        }
    }
}

fn put(img: &mut RgbImage, x: u32, y: u32, color: Rgb<u8>) {
    if x < img.width() && y < img.height() {
        img.put_pixel(x, y, color);
    }
}

fn encode_jpeg(img: &RgbImage) -> Option<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    let mut encoder = JpegEncoder::new_with_quality(&mut out, 60);
    encoder
        .encode(
            img.as_raw(),
            img.width(),
            img.height(),
            ExtendedColorType::Rgb8,
        )
        .ok()?;
    Some(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    fn jpeg_of(width: u32, height: u32, color: Rgb<u8>) -> Vec<u8> {
        let img = RgbImage::from_pixel(width, height, color);
        encode_jpeg(&img).expect("jpeg")
    }

    #[test]
    fn empty_elements_keeps_bytes() {
        let bytes = vec![1, 2, 3, 4];
        assert_eq!(overlay_elements(&bytes, &[]), bytes);
    }

    #[test]
    fn paints_a_yellow_badge() {
        let src = jpeg_of(160, 100, Rgb([8, 12, 40]));
        let out = overlay_elements(
            &src,
            &[UiElement {
                id: 7,
                title: "Submit".into(),
                x: 20,
                y: 30,
                w: 50,
                h: 24,
                ..UiElement::default()
            }],
        );
        assert!(out.starts_with(&[0xFF, 0xD8, 0xFF]));
        let painted = image::load_from_memory(&out).unwrap().to_rgb8();
        assert!(
            painted
                .pixels()
                .any(|pixel| pixel[0] > 200 && pixel[1] > 180 && pixel[2] < 40),
            "expected a yellow mark on the overlay"
        );
    }

    #[test]
    fn junk_bytes_pass_through() {
        let junk = b"not-an-image";
        let out = overlay_elements(
            junk,
            &[UiElement {
                id: 1,
                title: "x".into(),
                x: 0,
                y: 0,
                w: 10,
                h: 10,
                ..UiElement::default()
            }],
        );
        assert_eq!(out, junk);
    }
}
