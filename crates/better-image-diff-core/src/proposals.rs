use std::collections::BTreeMap;

use image::{Rgba, RgbaImage};

use crate::color::{NormalizedImage, NormalizedPixel};
use crate::mask::Mask;
use crate::{Bounds, CompareError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Proposal {
    pub(crate) bounds: Bounds,
}

pub(crate) fn extract(
    image: &RgbaImage,
    normalized: &NormalizedImage,
    threshold: f64,
    minimum_area: u32,
) -> Result<Vec<Proposal>, CompareError> {
    if image.width() == 0 || image.height() == 0 {
        return Ok(Vec::new());
    }
    let (dominant, dominant_samples, total_samples) = dominant_color(image);
    if dominant_samples.saturating_mul(20) < total_samples {
        return Ok(Vec::new());
    }
    let background = normalized_color(dominant)?;
    let mut mask = Mask::try_new(image.width(), image.height())?;
    let background_threshold = threshold.max(4.0);
    for y in 0..image.height() {
        for x in 0..image.width() {
            if normalized.pixel(x, y).perceptual_distance(background) > background_threshold {
                mask.set(x, y, true);
            }
        }
    }
    let components = mask.components(minimum_area)?;
    let mut proposals: Vec<Proposal> = components
        .into_iter()
        .map(|component| Proposal {
            bounds: component.bounds,
        })
        .collect();
    remove_contained(&mut proposals);
    Ok(proposals)
}

pub(crate) fn scaled_patch_score(
    expected: &NormalizedImage,
    expected_bounds: Bounds,
    actual: &NormalizedImage,
    actual_bounds: Bounds,
) -> f64 {
    sampled_patch_score(expected, expected_bounds, actual, actual_bounds, 12)
}

pub(crate) fn coarse_patch_score(
    expected: &NormalizedImage,
    expected_bounds: Bounds,
    actual: &NormalizedImage,
    actual_bounds: Bounds,
) -> f64 {
    sampled_patch_score(expected, expected_bounds, actual, actual_bounds, 4)
}

fn sampled_patch_score(
    expected: &NormalizedImage,
    expected_bounds: Bounds,
    actual: &NormalizedImage,
    actual_bounds: Bounds,
    grid: u32,
) -> f64 {
    let mut total = 0.0;
    let mut samples = 0_u32;
    for grid_y in 0..grid {
        for grid_x in 0..grid {
            let expected_x =
                sample_coordinate(expected_bounds.x, expected_bounds.width, grid_x, grid);
            let expected_y =
                sample_coordinate(expected_bounds.y, expected_bounds.height, grid_y, grid);
            let actual_x = sample_coordinate(actual_bounds.x, actual_bounds.width, grid_x, grid);
            let actual_y = sample_coordinate(actual_bounds.y, actual_bounds.height, grid_y, grid);
            total += expected
                .pixel(expected_x, expected_y)
                .perceptual_distance(actual.pixel(actual_x, actual_y))
                .min(100.0);
            samples += 1;
        }
    }
    total / f64::from(samples)
}

fn sample_coordinate(origin: u32, length: u32, grid_position: u32, grid: u32) -> u32 {
    if length <= 1 {
        return origin;
    }
    let offset =
        u64::from(length - 1) * u64::from(grid_position) / u64::from(grid.saturating_sub(1).max(1));
    origin + u32::try_from(offset).unwrap_or(u32::MAX)
}

fn dominant_color(image: &RgbaImage) -> (Rgba<u8>, u64, u64) {
    let mut colors: BTreeMap<[u8; 4], u64> = BTreeMap::new();
    let step = (image.width().max(image.height()) / 256).max(1) as usize;
    for y in (0..image.height()).step_by(step) {
        for x in (0..image.width()).step_by(step) {
            let color = image.get_pixel(x, y).0;
            *colors.entry(color).or_default() += 1;
        }
    }
    let total = colors.values().copied().sum();
    colors
        .into_iter()
        .max_by(|(left_color, left_count), (right_color, right_count)| {
            left_count
                .cmp(right_count)
                .then_with(|| right_color.cmp(left_color))
        })
        .map_or((Rgba([0, 0, 0, 0]), 0, total), |(color, count)| {
            (Rgba(color), count, total)
        })
}

fn normalized_color(color: Rgba<u8>) -> Result<NormalizedPixel, CompareError> {
    let image = RgbaImage::from_pixel(1, 1, color);
    Ok(NormalizedImage::try_new(&image)?.pixel(0, 0))
}

fn remove_contained(proposals: &mut Vec<Proposal>) {
    proposals.sort_by(|left, right| {
        right
            .bounds
            .area()
            .cmp(&left.bounds.area())
            .then_with(|| left.bounds.y.cmp(&right.bounds.y))
            .then_with(|| left.bounds.x.cmp(&right.bounds.x))
    });
    let mut retained: Vec<Proposal> = Vec::new();
    for proposal in proposals.iter().copied() {
        if retained
            .iter()
            .any(|outer| contains(outer.bounds, proposal.bounds))
        {
            continue;
        }
        retained.push(proposal);
    }
    retained.sort_by_key(|proposal| (proposal.bounds.y, proposal.bounds.x));
    *proposals = retained;
}

fn contains(outer: Bounds, inner: Bounds) -> bool {
    outer.x <= inner.x
        && outer.y <= inner.y
        && outer.right() >= inner.right()
        && outer.bottom() >= inner.bottom()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dominant_background_is_not_a_proposal() {
        let mut image = RgbaImage::from_pixel(20, 20, Rgba([245, 245, 245, 255]));
        for y in 5..15 {
            for x in 4..16 {
                image.put_pixel(x, y, Rgba([20, 30, 40, 255]));
            }
        }
        let normalized = NormalizedImage::try_new(&image).expect("normalize");
        let proposals = extract(&image, &normalized, 2.3, 16).expect("proposals");

        assert_eq!(proposals.len(), 1);
        assert_eq!(
            proposals[0].bounds,
            Bounds {
                x: 4,
                y: 5,
                width: 12,
                height: 10,
            }
        );
    }

    #[test]
    fn identical_patches_have_zero_score() {
        let image = RgbaImage::from_pixel(10, 10, Rgba([20, 30, 40, 255]));
        let normalized = NormalizedImage::try_new(&image).expect("normalize");
        let bounds = Bounds {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        };
        assert!(scaled_patch_score(&normalized, bounds, &normalized, bounds) < f64::EPSILON);
    }
}
