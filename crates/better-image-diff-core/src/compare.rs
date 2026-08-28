use image::RgbaImage;

use crate::{
    Alignment, Bounds, CompareError, CompareOptions, Comparison, ComparisonSummary, Difference,
    DifferenceKind, ImageDimensions, MetricSet, Offset, SimilarityMetrics,
};

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
    let identical = expected == actual;
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

    if !identical {
        differences.push(Difference {
            id: String::new(),
            kind: DifferenceKind::Changed,
            expected_bounds: full_bounds(expected),
            actual_bounds: full_bounds(actual),
            offset: None,
            confidence: 1.0,
            message: "Images contain visual differences.".to_owned(),
        });
    }

    for (index, difference) in differences.iter_mut().enumerate() {
        difference.id = format!("D{}", index + 1);
    }
    let summary = summarize(&differences);
    let empty_metrics = initial_metrics(expected, actual, identical);

    Ok(Comparison {
        expected: expected_dimensions,
        actual: actual_dimensions,
        settings: options.clone(),
        alignment: Alignment {
            offset: Offset::default(),
            confidence: f64::from(identical),
        },
        metrics: SimilarityMetrics {
            raw: empty_metrics.clone(),
            global_aligned: empty_metrics.clone(),
            structural_aligned: empty_metrics,
        },
        equivalent: differences.is_empty(),
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

fn dimensions(image: &RgbaImage) -> ImageDimensions {
    ImageDimensions {
        width: image.width(),
        height: image.height(),
    }
}

fn full_bounds(image: &RgbaImage) -> Option<Bounds> {
    (image.width() > 0 && image.height() > 0).then_some(Bounds {
        x: 0,
        y: 0,
        width: image.width(),
        height: image.height(),
    })
}

fn initial_metrics(expected: &RgbaImage, actual: &RgbaImage, identical: bool) -> MetricSet {
    let pixels = u64::from(expected.width().min(actual.width()))
        * u64::from(expected.height().min(actual.height()));
    let expected_area = u64::from(expected.width()) * u64::from(expected.height());
    let actual_area = u64::from(actual.width()) * u64::from(actual.height());
    let exact_score = (pixels > 0 && identical).then_some(0.0);
    MetricSet {
        compared_pixels: pixels,
        expected_coverage: coverage(pixels, expected_area),
        actual_coverage: coverage(pixels, actual_area),
        mae: exact_score,
        rmse: exact_score,
        psnr_db: None,
        ssim: (pixels > 0 && identical).then_some(1.0),
        changed_pixel_ratio: exact_score,
    }
}

#[allow(clippy::cast_precision_loss)]
fn coverage(pixels: u64, area: u64) -> f64 {
    if area == 0 {
        0.0
    } else {
        pixels as f64 / area as f64
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
