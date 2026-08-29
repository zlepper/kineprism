use image::{Rgba, RgbaImage};

use crate::render_primitives::blend_pixel;

pub(crate) fn text(image: &mut RgbaImage, x: u32, y: u32, value: &str, color: Rgba<u8>) {
    let mut cursor = x;
    for character in value.chars() {
        glyph(
            image,
            cursor,
            y,
            character,
            Rgba([255, 255, 255, 220]),
            1,
            1,
        );
        glyph(image, cursor, y, character, color, 0, 0);
        cursor = cursor.saturating_add(6);
        if cursor >= image.width() {
            break;
        }
    }
}

fn glyph(
    image: &mut RgbaImage,
    x: u32,
    y: u32,
    character: char,
    color: Rgba<u8>,
    offset_x: i64,
    offset_y: i64,
) {
    let rows = pattern(character);
    for (row, bits) in rows.into_iter().enumerate() {
        for column in 0..5_u8 {
            if bits & (1 << (4 - column)) != 0 {
                blend_pixel(
                    image,
                    i64::from(x) + i64::from(column) + offset_x,
                    i64::from(y) + i64::try_from(row).unwrap_or(0) + offset_y,
                    color,
                );
            }
        }
    }
}

fn pattern(character: char) -> [u8; 7] {
    match character.to_ascii_uppercase() {
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
        '6' => [
            0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        '+' => [
            0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        ',' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00110, 0b00100, 0b01000,
        ],
        _ => [0; 7],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_deterministic() {
        let mut first = RgbaImage::new(40, 10);
        let mut second = RgbaImage::new(40, 10);
        text(&mut first, 0, 0, "D12", Rgba([20, 40, 80, 255]));
        text(&mut second, 0, 0, "D12", Rgba([20, 40, 80, 255]));
        assert_eq!(first, second);
        assert!(first.pixels().any(|pixel| pixel[3] != 0));
    }
}
