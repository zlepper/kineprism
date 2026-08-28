use image::RgbaImage;

use crate::color::NormalizedImage;
use crate::mapping::PixelMapping;
use crate::mask::Mask;
use crate::metrics;
use crate::{
    Alignment, CompareError, CompareOptions, Comparison, ComparisonSummary, Difference,
    DifferenceKind, ImageDimensions, Offset, SimilarityMetrics,
};

const MAX_ESTIMATED_WORKING_BYTES: u64 = 1_073_741_824;
const EXPECTED_BYTES_PER_PIXEL: u64 = 64;
const ACTUAL_BYTES_PER_PIXEL: u64 = 32;

/// Compares two in-memory images without filesystem or process side effects.
///
/// # Errors
///
/// Returns [`CompareError`] when settings or image dimensions cannot be handled safely.
pub fn compare(
    expected: &RgbaImage,
    actual: &RgbaImage,
    options: &CompareOptions,
) -> Result<Comparison, CompareError> {
    options.validate()?;
    validate_area(expected)?;
    validate_area(actual)?;
    validate_working_set_dimensions(
        expected.width(),
        expected.height(),
        actual.width(),
        actual.height(),
    )?;

    let expected_dimensions = dimensions(expected);
    let actual_dimensions = dimensions(actual);
    let normalized_expected = NormalizedImage::try_new(expected)?;
    let normalized_actual = NormalizedImage::try_new(actual)?;
    let mut differences = Vec::new();

    if expected_dimensions != actual_dimensions {
        differences.push(Difference {
            id: String::new(),
            kind: DifferenceKind::CanvasSize,
            expected_bounds: None,
            actual_bounds: None,
            offset: None,
            confidence: 1.0,
            message: format!(
                "Canvas changed from {}x{} to {}x{}.",
                expected.width(),
                expected.height(),
                actual.width(),
                actual.height()
            ),
        });
    }

    for component in changed_components(
        &normalized_expected,
        &normalized_actual,
        options.color_threshold,
        options.min_region_area,
    )? {
        differences.push(Difference {
            id: String::new(),
            kind: DifferenceKind::Changed,
            expected_bounds: Some(component.bounds),
            actual_bounds: Some(component.bounds),
            offset: None,
            confidence: 1.0,
            message: format!(
                "A {} px region contains visual differences.",
                component.area
            ),
        });
    }

    for (index, difference) in differences.iter_mut().enumerate() {
        difference.id = format!("D{}", index + 1);
    }
    let summary = summarize(&differences);
    let equivalent = differences.is_empty();
    let mapping =
        PixelMapping::translated(&normalized_expected, &normalized_actual, Offset::default())?;
    let raw_metrics = metrics::calculate(
        &normalized_expected,
        &normalized_actual,
        &mapping,
        options.color_threshold,
    );

    Ok(Comparison {
        expected: expected_dimensions,
        actual: actual_dimensions,
        settings: options.clone(),
        alignment: Alignment {
            offset: Offset::default(),
            confidence: f64::from(
                raw_metrics
                    .changed_pixel_ratio
                    .is_some_and(|ratio| ratio <= f64::EPSILON),
            ),
        },
        metrics: SimilarityMetrics {
            raw: raw_metrics.clone(),
            global_aligned: raw_metrics.clone(),
            structural_aligned: raw_metrics,
        },
        equivalent,
        summary,
        differences,
    })
}

fn validate_area(image: &RgbaImage) -> Result<(), CompareError> {
    u64::from(image.width())
        .checked_mul(u64::from(image.height()))
        .and_then(|area| area.checked_mul(4))
        .ok_or(CompareError::ImageTooLarge)
        .map(|_| ())
}

fn validate_working_set_dimensions(
    expected_width: u32,
    expected_height: u32,
    actual_width: u32,
    actual_height: u32,
) -> Result<(), CompareError> {
    let expected_area = u64::from(expected_width)
        .checked_mul(u64::from(expected_height))
        .ok_or(CompareError::ImageTooLarge)?;
    let actual_area = u64::from(actual_width)
        .checked_mul(u64::from(actual_height))
        .ok_or(CompareError::ImageTooLarge)?;
    let estimated_bytes = expected_area
        .checked_mul(EXPECTED_BYTES_PER_PIXEL)
        .and_then(|expected_bytes| {
            actual_area
                .checked_mul(ACTUAL_BYTES_PER_PIXEL)
                .and_then(|actual_bytes| expected_bytes.checked_add(actual_bytes))
        })
        .ok_or(CompareError::ImageTooLarge)?;
    if estimated_bytes > MAX_ESTIMATED_WORKING_BYTES
        || expected_area > usize::MAX as u64
        || actual_area > usize::MAX as u64
    {
        return Err(CompareError::ImageTooLarge);
    }
    Ok(())
}

fn dimensions(image: &RgbaImage) -> ImageDimensions {
    ImageDimensions {
        width: image.width(),
        height: image.height(),
    }
}

fn changed_components(
    expected: &NormalizedImage,
    actual: &NormalizedImage,
    threshold: f64,
    minimum_area: u32,
) -> Result<Vec<crate::mask::Component>, CompareError> {
    let width = expected.width().min(actual.width());
    let height = expected.height().min(actual.height());
    let mut mask = Mask::try_new(width, height)?;
    for y in 0..height {
        for x in 0..width {
            if expected.pixel(x, y).perceptual_distance(actual.pixel(x, y)) > threshold {
                mask.set(x, y, true);
            }
        }
    }
    mask.components(minimum_area)
}

fn summarize(differences: &[Difference]) -> ComparisonSummary {
    let mut summary = ComparisonSummary {
        total: u32::try_from(differences.len()).unwrap_or(u32::MAX),
        ..ComparisonSummary::default()
    };
    for difference in differences {
        let count = match difference.kind {
            DifferenceKind::CanvasSize => &mut summary.canvas_size,
            DifferenceKind::Moved => &mut summary.moved,
            DifferenceKind::Resized => &mut summary.resized,
            DifferenceKind::Added => &mut summary.added,
            DifferenceKind::Removed => &mut summary.removed,
            DifferenceKind::Changed => &mut summary.changed,
        };
        *count = count.saturating_add(1);
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excessive_estimated_working_set_is_rejected_without_allocation() {
        assert_eq!(
            validate_working_set_dimensions(100_000, 100_000, 100_000, 100_000),
            Err(CompareError::ImageTooLarge)
        );
    }

    #[test]
    fn common_four_k_canvases_fit_the_working_set_policy() {
        assert_eq!(
            validate_working_set_dimensions(3840, 2160, 3840, 2160),
            Ok(())
        );
    }
}
