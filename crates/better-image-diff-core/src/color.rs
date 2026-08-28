use image::{Rgba, RgbaImage};

use crate::CompareError;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NormalizedPixel {
    channels: [f32; 4],
    perceptual: [f32; 4],
}

impl NormalizedPixel {
    pub(crate) fn channel(self, index: usize) -> f64 {
        f64::from(self.channels[index])
    }

    pub(crate) fn perceptual_distance(self, other: Self) -> f64 {
        self.perceptual
            .iter()
            .zip(other.perceptual)
            .map(|(left, right)| {
                let difference = f64::from(*left) - f64::from(right);
                difference * difference
            })
            .sum::<f64>()
            .sqrt()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct NormalizedImage {
    width: u32,
    height: u32,
    pixels: Vec<NormalizedPixel>,
}

impl NormalizedImage {
    pub(crate) fn try_new(image: &RgbaImage) -> Result<Self, CompareError> {
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(image.as_raw().len() / 4)
            .map_err(|_error| CompareError::ImageTooLarge)?;
        pixels.extend(image.pixels().copied().map(normalize));
        Ok(Self {
            width: image.width(),
            height: image.height(),
            pixels,
        })
    }

    pub(crate) fn width(&self) -> u32 {
        self.width
    }

    pub(crate) fn height(&self) -> u32 {
        self.height
    }

    pub(crate) fn pixel_by_index(&self, index: usize) -> NormalizedPixel {
        self.pixels[index]
    }

    pub(crate) fn pixel(&self, x: u32, y: u32) -> NormalizedPixel {
        self.pixels[self.index(x, y)]
    }

    pub(crate) fn index(&self, x: u32, y: u32) -> usize {
        usize::try_from(u64::from(y) * u64::from(self.width) + u64::from(x))
            .expect("validated image index fits usize")
    }
}

fn normalize(pixel: Rgba<u8>) -> NormalizedPixel {
    let alpha = f32::from(pixel[3]) / 255.0;
    let linear = [
        srgb_to_linear(pixel[0]),
        srgb_to_linear(pixel[1]),
        srgb_to_linear(pixel[2]),
    ];
    let channels = [
        linear[0] * alpha,
        linear[1] * alpha,
        linear[2] * alpha,
        alpha,
    ];
    let lab = linear_rgb_to_lab(linear);
    NormalizedPixel {
        channels,
        perceptual: [
            lab[0] * alpha,
            lab[1] * alpha,
            lab[2] * alpha,
            alpha * 100.0,
        ],
    }
}

fn srgb_to_linear(value: u8) -> f32 {
    let encoded = f32::from(value) / 255.0;
    if encoded <= 0.040_45 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_rgb_to_lab(rgb: [f32; 3]) -> [f32; 3] {
    let x = rgb[0].mul_add(
        0.412_456_4,
        rgb[1].mul_add(0.357_576_1, rgb[2] * 0.180_437_5),
    );
    let y = rgb[0].mul_add(0.212_672_9, rgb[1].mul_add(0.715_152_2, rgb[2] * 0.072_175));
    let z = rgb[0].mul_add(0.019_333_9, rgb[1].mul_add(0.119_192, rgb[2] * 0.950_304_1));
    let fx = lab_curve(x / 0.950_47);
    let fy = lab_curve(y);
    let fz = lab_curve(z / 1.088_83);
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

fn lab_curve(value: f32) -> f32 {
    const EPSILON: f32 = 216.0 / 24_389.0;
    const KAPPA: f32 = 24_389.0 / 27.0;
    if value > EPSILON {
        value.cbrt()
    } else {
        (KAPPA * value + 16.0) / 116.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparent_rgb_is_normalized_away() {
        let red = normalize(Rgba([255, 0, 0, 0]));
        let cyan = normalize(Rgba([0, 255, 255, 0]));

        assert_eq!(red, cyan);
        assert!(red.perceptual_distance(cyan).abs() < f64::EPSILON);
    }

    #[test]
    fn distance_is_symmetric_and_finite() {
        let black = normalize(Rgba([0, 0, 0, 255]));
        let white = normalize(Rgba([255, 255, 255, 255]));

        let forward = black.perceptual_distance(white);
        assert!(forward.is_finite());
        assert!((forward - white.perceptual_distance(black)).abs() < f64::EPSILON);
        assert!(forward > 0.0);
    }

    #[test]
    fn primary_colors_are_stable_and_finite() {
        for color in [
            Rgba([255, 0, 0, 255]),
            Rgba([0, 255, 0, 255]),
            Rgba([0, 0, 255, 255]),
        ] {
            let first = normalize(color);
            let second = normalize(color);
            assert_eq!(first, second);
            assert!(first.perceptual.iter().all(|value| value.is_finite()));
        }
    }

    #[test]
    fn alpha_change_has_perceptual_distance() {
        let opaque = normalize(Rgba([80, 120, 160, 255]));
        let translucent = normalize(Rgba([80, 120, 160, 128]));
        assert!(opaque.perceptual_distance(translucent) > 0.0);
    }
}
