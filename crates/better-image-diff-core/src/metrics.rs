use std::sync::OnceLock;

use crate::MetricSet;
use crate::color::NormalizedImage;
use crate::mapping::PixelMapping;

const SSIM_WINDOW_SIZE: u32 = 11;
const SSIM_RADIUS: i32 = 5;
const SSIM_STRIDE: u32 = 8;
const SSIM_SIGMA: f64 = 1.5;
const SSIM_C1: f64 = 0.000_1;
const SSIM_C2: f64 = 0.000_9;

pub(crate) fn calculate(
    expected: &NormalizedImage,
    actual: &NormalizedImage,
    mapping: &PixelMapping,
    color_threshold: f64,
) -> MetricSet {
    let pending = calculate_pending(expected, actual, mapping, color_threshold);
    let ssim = calculate_ssim(expected, actual, mapping);
    pending.finish(ssim)
}

pub(crate) struct PendingMetrics(MetricSet);

impl PendingMetrics {
    pub(crate) fn changed_pixel_ratio(&self) -> Option<f64> {
        self.0.changed_pixel_ratio
    }

    pub(crate) fn finish(mut self, ssim: Option<f64>) -> MetricSet {
        self.0.ssim = ssim;
        self.0
    }
}

pub(crate) fn calculate_pending(
    expected: &NormalizedImage,
    actual: &NormalizedImage,
    mapping: &PixelMapping,
    color_threshold: f64,
) -> PendingMetrics {
    let compared_pixels = mapping.compared_pixels();
    let expected_area = u64::from(expected.width()) * u64::from(expected.height());
    let actual_area = u64::from(actual.width()) * u64::from(actual.height());
    if compared_pixels == 0 {
        return PendingMetrics(MetricSet {
            compared_pixels,
            expected_coverage: coverage(compared_pixels, expected_area),
            actual_coverage: coverage(compared_pixels, actual_area),
            mae: None,
            rmse: None,
            psnr_db: None,
            ssim: None,
            changed_pixel_ratio: None,
        });
    }

    let mut absolute_error = 0.0;
    let mut squared_error = 0.0;
    let mut changed_pixels = 0_u64;
    for (expected_index, actual_index) in mapping.iter() {
        let expected_pixel = expected.pixel_by_index(expected_index);
        let actual_pixel = actual.pixel_by_index(actual_index);
        for channel in 0..4 {
            let difference = expected_pixel.channel(channel) - actual_pixel.channel(channel);
            absolute_error += difference.abs();
            squared_error += difference * difference;
        }
        if expected_pixel.perceptual_distance(actual_pixel) > color_threshold {
            changed_pixels += 1;
        }
    }

    #[allow(clippy::cast_precision_loss)]
    let channel_count = compared_pixels as f64 * 4.0;
    let mae = absolute_error / channel_count;
    let mse = squared_error / channel_count;
    let rmse = mse.sqrt();
    let psnr_db = (mse > 0.0).then(|| 10.0 * (1.0 / mse).log10());

    PendingMetrics(MetricSet {
        compared_pixels,
        expected_coverage: coverage(compared_pixels, expected_area),
        actual_coverage: coverage(compared_pixels, actual_area),
        mae: Some(mae),
        rmse: Some(rmse),
        psnr_db,
        ssim: None,
        changed_pixel_ratio: Some(ratio(changed_pixels, compared_pixels)),
    })
}

#[allow(clippy::cast_precision_loss)]
pub(crate) fn calculate_ssim(
    expected: &NormalizedImage,
    actual: &NormalizedImage,
    mapping: &PixelMapping,
) -> Option<f64> {
    let (width, height) = mapping.dimensions();
    if width == 0 || height == 0 || mapping.compared_pixels() == 0 {
        return None;
    }

    let mut sum = 0.0;
    let mut scores = 0_u64;
    for center_y in window_centers(height) {
        for center_x in window_centers(width) {
            for channel in 0..4 {
                if let Some(score) =
                    window_ssim(expected, actual, mapping, center_x, center_y, channel)
                {
                    sum += score;
                    scores += 1;
                }
            }
        }
    }
    (scores > 0).then(|| (sum / scores as f64).clamp(-1.0, 1.0))
}

fn window_centers(length: u32) -> Vec<u32> {
    if length <= SSIM_WINDOW_SIZE {
        return vec![length / 2];
    }
    let final_center = length - 1 - SSIM_RADIUS as u32;
    let mut centers: Vec<u32> = (SSIM_RADIUS as u32..=final_center)
        .step_by(SSIM_STRIDE as usize)
        .collect();
    if centers.last().copied() != Some(final_center) {
        centers.push(final_center);
    }
    centers
}

fn window_ssim(
    expected: &NormalizedImage,
    actual: &NormalizedImage,
    mapping: &PixelMapping,
    center_x: u32,
    center_y: u32,
    channel: usize,
) -> Option<f64> {
    let (width, height) = mapping.dimensions();
    let mut weight_sum = 0.0;
    let mut expected_sum = 0.0;
    let mut actual_sum = 0.0;
    let mut expected_squared = 0.0;
    let mut actual_squared = 0.0;
    let mut product_sum = 0.0;
    let gaussian_weights = gaussian_weights();

    for delta_y in -SSIM_RADIUS..=SSIM_RADIUS {
        let y = i64::from(center_y) + i64::from(delta_y);
        if y < 0 || y >= i64::from(height) {
            continue;
        }
        for delta_x in -SSIM_RADIUS..=SSIM_RADIUS {
            let x = i64::from(center_x) + i64::from(delta_x);
            if x < 0 || x >= i64::from(width) {
                continue;
            }
            let x = u32::try_from(x).expect("checked window x");
            let y = u32::try_from(y).expect("checked window y");
            let Some(actual_index) = mapping.actual_index(x, y) else {
                continue;
            };
            let expected_value = expected.pixel(x, y).channel(channel);
            let actual_value = actual.pixel_by_index(actual_index).channel(channel);
            let weight = gaussian_weights[gaussian_index(delta_x, delta_y)];
            weight_sum += weight;
            expected_sum += weight * expected_value;
            actual_sum += weight * actual_value;
            expected_squared += weight * expected_value * expected_value;
            actual_squared += weight * actual_value * actual_value;
            product_sum += weight * expected_value * actual_value;
        }
    }

    if weight_sum == 0.0 {
        return None;
    }
    let expected_mean = expected_sum / weight_sum;
    let actual_mean = actual_sum / weight_sum;
    let expected_variance = (expected_squared / weight_sum - expected_mean.powi(2)).max(0.0);
    let actual_variance = (actual_squared / weight_sum - actual_mean.powi(2)).max(0.0);
    let covariance = product_sum / weight_sum - expected_mean * actual_mean;
    let numerator = (2.0 * expected_mean * actual_mean + SSIM_C1) * (2.0 * covariance + SSIM_C2);
    let denominator = (expected_mean.powi(2) + actual_mean.powi(2) + SSIM_C1)
        * (expected_variance + actual_variance + SSIM_C2);
    Some(if denominator == 0.0 {
        1.0
    } else {
        numerator / denominator
    })
}

fn gaussian_weights() -> &'static [f64; SSIM_WINDOW_SIZE as usize * SSIM_WINDOW_SIZE as usize] {
    static WEIGHTS: OnceLock<[f64; SSIM_WINDOW_SIZE as usize * SSIM_WINDOW_SIZE as usize]> =
        OnceLock::new();
    WEIGHTS.get_or_init(|| {
        std::array::from_fn(|index| {
            let x = index % SSIM_WINDOW_SIZE as usize;
            let y = index / SSIM_WINDOW_SIZE as usize;
            let delta_x = i32::try_from(x).expect("SSIM window x") - SSIM_RADIUS;
            let delta_y = i32::try_from(y).expect("SSIM window y") - SSIM_RADIUS;
            gaussian_weight(delta_x, delta_y)
        })
    })
}

fn gaussian_index(delta_x: i32, delta_y: i32) -> usize {
    let x = usize::try_from(delta_x + SSIM_RADIUS).expect("SSIM weight x");
    let y = usize::try_from(delta_y + SSIM_RADIUS).expect("SSIM weight y");
    y * SSIM_WINDOW_SIZE as usize + x
}

fn gaussian_weight(delta_x: i32, delta_y: i32) -> f64 {
    let squared_distance = f64::from(delta_x * delta_x + delta_y * delta_y);
    (-squared_distance / (2.0 * SSIM_SIGMA * SSIM_SIGMA)).exp()
}

#[allow(clippy::cast_precision_loss)]
fn coverage(pixels: u64, area: u64) -> f64 {
    if area == 0 {
        0.0
    } else {
        pixels as f64 / area as f64
    }
}

#[allow(clippy::cast_precision_loss)]
fn ratio(numerator: u64, denominator: u64) -> f64 {
    numerator as f64 / denominator as f64
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};

    use super::*;
    use crate::Offset;

    #[test]
    fn distance_equal_to_threshold_is_not_changed() {
        let expected =
            NormalizedImage::try_new(&RgbaImage::from_pixel(1, 1, Rgba([100, 100, 100, 255])))
                .expect("normalize expected");
        let actual =
            NormalizedImage::try_new(&RgbaImage::from_pixel(1, 1, Rgba([110, 110, 110, 255])))
                .expect("normalize actual");
        let threshold = expected.pixel(0, 0).perceptual_distance(actual.pixel(0, 0));
        let mapping =
            PixelMapping::translated(&expected, &actual, Offset::default()).expect("mapping");

        let metrics = calculate(&expected, &actual, &mapping, threshold);

        assert_eq!(metrics.changed_pixel_ratio, Some(0.0));
    }

    #[test]
    fn ssim_centers_use_only_full_windows_when_the_dimension_is_large_enough() {
        assert_eq!(window_centers(10), vec![5]);
        assert_eq!(window_centers(11), vec![5]);
        assert_eq!(window_centers(12), vec![5, 6]);
        assert_eq!(window_centers(16), vec![5, 10]);
        assert_eq!(window_centers(22), vec![5, 13, 16]);
    }
}
