use image::RgbaImage;

use crate::classify;
use crate::color::NormalizedImage;
use crate::mapping::{MovementMapping, PixelMapping};
use crate::metrics;
use crate::pyramid::ImagePyramid;
use crate::{
    Bounds, CompareError, CompareOptions, Comparison, ComparisonSummary, Difference,
    DifferenceKind, ImageDimensions, MetricSet, Offset, SimilarityMetrics,
};

const MAX_ESTIMATED_WORKING_BYTES: u64 = 1_610_612_736;
const EXPECTED_BYTES_PER_PIXEL: u64 = 96;
const ACTUAL_BYTES_PER_PIXEL: u64 = 64;

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

    let expected_dimensions = dimensions(expected);
    let actual_dimensions = dimensions(actual);
    let Some(region) = options.region else {
        validate_working_set_dimensions(
            expected.width(),
            expected.height(),
            actual.width(),
            actual.height(),
        )?;
        return compare_images(expected, actual, options);
    };
    validate_region(region, expected_dimensions, actual_dimensions)?;
    validate_working_set_dimensions(region.width, region.height, region.width, region.height)?;
    let (expected_region, actual_region) = rayon::join(
        || extract_region(expected, region),
        || extract_region(actual, region),
    );
    let expected_region = expected_region?;
    let actual_region = actual_region?;
    let mut comparison = compare_images(&expected_region, &actual_region, options)?;
    comparison.expected = expected_dimensions;
    comparison.actual = actual_dimensions;
    for difference in &mut comparison.differences {
        difference.expected_bounds = difference
            .expected_bounds
            .map(|bounds| rebase_bounds(bounds, region));
        difference.actual_bounds = difference
            .actual_bounds
            .map(|bounds| rebase_bounds(bounds, region));
    }
    Ok(comparison)
}

fn compare_images(
    expected: &RgbaImage,
    actual: &RgbaImage,
    options: &CompareOptions,
) -> Result<Comparison, CompareError> {
    let expected_dimensions = dimensions(expected);
    let actual_dimensions = dimensions(actual);
    let (expected_preparation, actual_preparation) =
        rayon::join(|| prepare_image(expected), || prepare_image(actual));
    let (normalized_expected, expected_pyramid) = expected_preparation?;
    let (normalized_actual, actual_pyramid) = actual_preparation?;
    let raw_mapping =
        PixelMapping::translated(&normalized_expected, &normalized_actual, Offset::default())?;
    let pending_raw_metrics = metrics::calculate_pending(
        &normalized_expected,
        &normalized_actual,
        &raw_mapping,
        options.color_threshold,
    );
    let raw_pixels_match = pending_raw_metrics
        .changed_pixel_ratio()
        .is_some_and(|ratio| ratio <= f64::EPSILON);
    let (raw_ssim, analysis) = rayon::join(
        || metrics::calculate_ssim(&normalized_expected, &normalized_actual, &raw_mapping),
        || {
            let alignment = crate::alignment::estimate(
                &expected_pyramid,
                &actual_pyramid,
                options.max_offset,
                raw_pixels_match,
            );
            if raw_pixels_match && expected_dimensions == actual_dimensions {
                Ok(empty_analysis(alignment))
            } else {
                classify::analyze(
                    expected,
                    &normalized_expected,
                    actual,
                    &normalized_actual,
                    options,
                    alignment,
                )
            }
        },
    );
    let raw_metrics = pending_raw_metrics.finish(raw_ssim);
    let analysis = analysis?;
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

    differences.extend(analysis.differences);

    differences.sort_by_key(difference_sort_key);
    for (index, difference) in differences.iter_mut().enumerate() {
        difference.id = format!("D{}", index + 1);
        difference.message = format!("{}: {}", difference.id, difference.message);
    }
    let summary = summarize(&differences);
    let equivalent = differences.is_empty();
    let (global_metrics, structural_metrics) = calculate_aligned_metrics(
        &normalized_expected,
        &normalized_actual,
        options,
        analysis.alignment.offset,
        &analysis.movements,
        &raw_metrics,
    )?;

    Ok(Comparison {
        expected: expected_dimensions,
        actual: actual_dimensions,
        settings: options.clone(),
        alignment: analysis.alignment,
        metrics: SimilarityMetrics {
            raw: raw_metrics,
            global_aligned: global_metrics,
            structural_aligned: structural_metrics,
        },
        equivalent,
        summary,
        suppression: analysis.suppression,
        differences,
    })
}

fn validate_region(
    region: Bounds,
    expected: ImageDimensions,
    actual: ImageDimensions,
) -> Result<(), CompareError> {
    let fits = |dimensions: ImageDimensions| {
        region
            .x
            .checked_add(region.width)
            .is_some_and(|right| right <= dimensions.width)
            && region
                .y
                .checked_add(region.height)
                .is_some_and(|bottom| bottom <= dimensions.height)
    };
    if !fits(expected) || !fits(actual) {
        return Err(CompareError::RegionOutOfBounds {
            region,
            expected,
            actual,
        });
    }
    Ok(())
}

fn extract_region(image: &RgbaImage, region: Bounds) -> Result<RgbaImage, CompareError> {
    let byte_count = region
        .area()
        .checked_mul(4)
        .and_then(|bytes| usize::try_from(bytes).ok())
        .ok_or(CompareError::ImageTooLarge)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(byte_count)
        .map_err(|_error| CompareError::ImageTooLarge)?;
    let source_width = u64::from(image.width());
    let row_bytes = usize::try_from(u64::from(region.width) * 4)
        .map_err(|_error| CompareError::ImageTooLarge)?;
    for y in region.y..region.bottom() {
        let start =
            usize::try_from((u64::from(y) * source_width + u64::from(region.x)).saturating_mul(4))
                .map_err(|_error| CompareError::ImageTooLarge)?;
        let end = start
            .checked_add(row_bytes)
            .ok_or(CompareError::ImageTooLarge)?;
        bytes.extend_from_slice(
            image
                .as_raw()
                .get(start..end)
                .ok_or(CompareError::ImageTooLarge)?,
        );
    }
    RgbaImage::from_raw(region.width, region.height, bytes).ok_or(CompareError::ImageTooLarge)
}

fn rebase_bounds(bounds: Bounds, region: Bounds) -> Bounds {
    Bounds {
        x: region.x.saturating_add(bounds.x),
        y: region.y.saturating_add(bounds.y),
        width: bounds.width,
        height: bounds.height,
    }
}

fn prepare_image(image: &RgbaImage) -> Result<(NormalizedImage, ImagePyramid), CompareError> {
    let normalized = NormalizedImage::try_new(image)?;
    let pyramid = ImagePyramid::try_new(&normalized)?;
    Ok((normalized, pyramid))
}

fn calculate_aligned_metrics(
    expected: &NormalizedImage,
    actual: &NormalizedImage,
    options: &CompareOptions,
    alignment_offset: Offset,
    movements: &[MovementMapping],
    raw_metrics: &MetricSet,
) -> Result<(MetricSet, MetricSet), CompareError> {
    let global_metrics = if alignment_offset == Offset::default() {
        raw_metrics.clone()
    } else {
        let global_mapping = PixelMapping::translated(expected, actual, alignment_offset)?;
        metrics::calculate(expected, actual, &global_mapping, options.color_threshold)
    };
    let structural_metrics = if movements.is_empty() {
        global_metrics.clone()
    } else {
        let structural_mapping =
            PixelMapping::structural(expected, actual, alignment_offset, movements)?;
        metrics::calculate(
            expected,
            actual,
            &structural_mapping,
            options.color_threshold,
        )
    };
    Ok((global_metrics, structural_metrics))
}

fn empty_analysis(alignment: crate::Alignment) -> classify::StructuralAnalysis {
    classify::StructuralAnalysis {
        alignment,
        differences: Vec::new(),
        movements: Vec::new(),
        suppression: crate::SuppressionSummary::default(),
    }
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

type DifferenceSortKey = (
    u8,
    u32,
    u32,
    DifferenceKind,
    (u32, u32, u32, u32),
    (u32, u32, u32, u32),
    (i32, i32),
);

fn difference_sort_key(difference: &Difference) -> DifferenceSortKey {
    let bounds = difference.expected_bounds.or(difference.actual_bounds);
    (
        u8::from(difference.kind != DifferenceKind::CanvasSize),
        bounds.map_or(0, |value| value.y),
        bounds.map_or(0, |value| value.x),
        difference.kind,
        bounds_key(difference.expected_bounds),
        bounds_key(difference.actual_bounds),
        difference
            .offset
            .map_or((0, 0), |offset| (offset.x, offset.y)),
    )
}

fn bounds_key(bounds: Option<crate::Bounds>) -> (u32, u32, u32, u32) {
    bounds.map_or((0, 0, 0, 0), |bounds| {
        (bounds.y, bounds.x, bounds.width, bounds.height)
    })
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
