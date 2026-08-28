use image::{Rgba, RgbaImage};

use crate::Bounds;

pub(crate) fn blend_pixel(image: &mut RgbaImage, x: i64, y: i64, color: Rgba<u8>) {
    if x < 0 || y < 0 || x >= i64::from(image.width()) || y >= i64::from(image.height()) {
        return;
    }
    let x = u32::try_from(x).expect("checked render x");
    let y = u32::try_from(y).expect("checked render y");
    let destination = image.get_pixel_mut(x, y);
    let alpha = u16::from(color[3]);
    let inverse = 255_u16 - alpha;
    for channel in 0..3 {
        let value = u16::from(color[channel]) * alpha + u16::from(destination[channel]) * inverse;
        destination[channel] = u8::try_from((value + 127) / 255).unwrap_or(255);
    }
    destination[3] = 255;
}

pub(crate) fn rectangle(image: &mut RgbaImage, bounds: Bounds, color: Rgba<u8>, dashed: bool) {
    if bounds.width == 0 || bounds.height == 0 || image.width() == 0 || image.height() == 0 {
        return;
    }
    let left = bounds.x.min(image.width());
    let top = bounds.y.min(image.height());
    let right_exclusive = bounds.right().min(image.width());
    let bottom_exclusive = bounds.bottom().min(image.height());
    if left >= right_exclusive || top >= bottom_exclusive {
        return;
    }
    let left = i64::from(left);
    let top = i64::from(top);
    let right = i64::from(right_exclusive - 1);
    let bottom = i64::from(bottom_exclusive - 1);
    for thickness in 0..2_i64 {
        for x in left..=right {
            if stroke_visible(x - left, dashed) {
                blend_pixel(image, x, top + thickness, color);
                blend_pixel(image, x, bottom - thickness, color);
            }
        }
        for y in top..=bottom {
            if stroke_visible(y - top, dashed) {
                blend_pixel(image, left + thickness, y, color);
                blend_pixel(image, right - thickness, y, color);
            }
        }
    }
}

pub(crate) fn line(image: &mut RgbaImage, start: (u32, u32), end: (u32, u32), color: Rgba<u8>) {
    if image.width() == 0 || image.height() == 0 {
        return;
    }
    let mut x = i64::from(start.0.min(image.width() - 1));
    let mut y = i64::from(start.1.min(image.height() - 1));
    let end_x = i64::from(end.0.min(image.width() - 1));
    let end_y = i64::from(end.1.min(image.height() - 1));
    let delta_x = (end_x - x).abs();
    let step_x = if x < end_x { 1 } else { -1 };
    let delta_y = -(end_y - y).abs();
    let step_y = if y < end_y { 1 } else { -1 };
    let mut error = delta_x + delta_y;
    loop {
        blend_pixel(image, x, y, color);
        if x == end_x && y == end_y {
            break;
        }
        let doubled = error.saturating_mul(2);
        if doubled >= delta_y {
            error += delta_y;
            x += step_x;
        }
        if doubled <= delta_x {
            error += delta_x;
            y += step_y;
        }
    }
}

pub(crate) fn arrow(image: &mut RgbaImage, start: (u32, u32), end: (u32, u32), color: Rgba<u8>) {
    line(image, start, end, color);
    let delta_x = i64::from(end.0) - i64::from(start.0);
    let delta_y = i64::from(end.1) - i64::from(start.1);
    if delta_x == 0 && delta_y == 0 {
        return;
    }
    let length = delta_x.abs().max(delta_y.abs()).max(1);
    let backward_x = -delta_x * 6 / length;
    let backward_y = -delta_y * 6 / length;
    let perpendicular_x = -delta_y * 3 / length;
    let perpendicular_y = delta_x * 3 / length;
    let end_x = i64::from(end.0);
    let end_y = i64::from(end.1);
    let first = (
        clamped_coordinate(end_x + backward_x + perpendicular_x),
        clamped_coordinate(end_y + backward_y + perpendicular_y),
    );
    let second = (
        clamped_coordinate(end_x + backward_x - perpendicular_x),
        clamped_coordinate(end_y + backward_y - perpendicular_y),
    );
    line(image, end, first, color);
    line(image, end, second, color);
}

pub(crate) fn fill_mask_pixel(image: &mut RgbaImage, x: u32, y: u32, color: Rgba<u8>) {
    blend_pixel(image, i64::from(x), i64::from(y), color);
}

fn stroke_visible(position: i64, dashed: bool) -> bool {
    !dashed || position.rem_euclid(8) < 4
}

fn clamped_coordinate(value: i64) -> u32 {
    u32::try_from(value.max(0)).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_clipped_primitives_do_not_panic() {
        let mut image = RgbaImage::new(2, 2);
        rectangle(
            &mut image,
            Bounds {
                x: u32::MAX,
                y: u32::MAX,
                width: u32::MAX,
                height: u32::MAX,
            },
            Rgba([255, 0, 0, 255]),
            true,
        );
        arrow(
            &mut image,
            (u32::MAX, u32::MAX),
            (0, 0),
            Rgba([0, 0, 255, 255]),
        );

        rectangle(
            &mut image,
            Bounds {
                x: 0,
                y: 0,
                width: u32::MAX,
                height: u32::MAX,
            },
            Rgba([255, 0, 0, 255]),
            false,
        );
        assert_eq!(*image.get_pixel(1, 1), Rgba([255, 0, 0, 255]));
    }
}
