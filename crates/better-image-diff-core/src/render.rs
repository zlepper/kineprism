use image::{Rgba, RgbaImage};

use crate::color::rgba_perceptual_distance;
use crate::render_font::text;
use crate::render_primitives::{arrow, fill_mask_pixel, rectangle};
use crate::{Bounds, Comparison, Difference, DifferenceKind, RenderError, RenderedArtifacts};

const MAX_RENDER_PIXELS: u64 = 100_000_000;
const MOVED: Rgba<u8> = Rgba([32, 115, 230, 255]);
const RESIZED: Rgba<u8> = Rgba([145, 70, 210, 255]);
const ADDED: Rgba<u8> = Rgba([35, 160, 85, 255]);
const REMOVED: Rgba<u8> = Rgba([230, 135, 35, 255]);
const CHANGED: Rgba<u8> = Rgba([220, 45, 55, 255]);
const CANVAS: Rgba<u8> = Rgba([100, 105, 115, 255]);

/// Renders annotated source images and a white diagnostic canvas in memory.
///
/// Expected-side evidence is marked on `expected`, actual-side evidence on `actual`, and both
/// coordinate systems are combined on `diff`. The diagnostic image uses dashed expected bounds,
/// solid actual bounds, stable finding IDs, signed movement labels, and movement arrows.
///
/// # Errors
///
/// Returns [`RenderError`] when the comparison does not belong to the images, allocation fails,
/// or the diagnostic canvas exceeds the renderer's safe allocation limit.
pub fn render_artifacts(
    expected: &RgbaImage,
    actual: &RgbaImage,
    comparison: &Comparison,
) -> Result<RenderedArtifacts, RenderError> {
    validate_inputs(expected, actual, comparison)?;
    let width = expected.width().max(actual.width());
    let height = expected.height().max(actual.height());
    validate_area(width, height)?;
    let mut expected_artifact = try_clone(expected)?;
    let mut actual_artifact = try_clone(actual)?;
    let mut diff = try_blank(width, height, Rgba([255, 255, 255, 255]))?;

    if comparison.expected != comparison.actual {
        draw_canvas_boundaries(&mut diff, expected, actual);
    }
    for difference in &comparison.differences {
        if difference.kind == DifferenceKind::CanvasSize {
            text(&mut diff, 2, 2, &difference.id, CANVAS);
            continue;
        }
        draw_source_annotation(
            &mut expected_artifact,
            difference.expected_bounds,
            difference,
        );
        draw_source_annotation(&mut actual_artifact, difference.actual_bounds, difference);
        draw_diagnostic(
            &mut diff,
            expected,
            actual,
            difference,
            comparison.settings.color_threshold,
        );
    }

    Ok(RenderedArtifacts {
        expected: expected_artifact,
        actual: actual_artifact,
        diff,
    })
}

fn validate_inputs(
    expected: &RgbaImage,
    actual: &RgbaImage,
    comparison: &Comparison,
) -> Result<(), RenderError> {
    if comparison.expected.width != expected.width()
        || comparison.expected.height != expected.height()
        || comparison.actual.width != actual.width()
        || comparison.actual.height != actual.height()
    {
        return Err(RenderError::ComparisonImageMismatch);
    }
    Ok(())
}

fn validate_area(width: u32, height: u32) -> Result<(), RenderError> {
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or(RenderError::ImageTooLarge)?;
    if pixels > MAX_RENDER_PIXELS {
        return Err(RenderError::ImageTooLarge);
    }
    Ok(())
}

fn try_clone(image: &RgbaImage) -> Result<RgbaImage, RenderError> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(image.as_raw().len())
        .map_err(|_error| RenderError::ImageTooLarge)?;
    bytes.extend_from_slice(image.as_raw());
    RgbaImage::from_raw(image.width(), image.height(), bytes).ok_or(RenderError::ImageTooLarge)
}

fn try_blank(width: u32, height: u32, color: Rgba<u8>) -> Result<RgbaImage, RenderError> {
    let byte_count = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .and_then(|bytes| usize::try_from(bytes).ok())
        .ok_or(RenderError::ImageTooLarge)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(byte_count)
        .map_err(|_error| RenderError::ImageTooLarge)?;
    for _ in 0..u64::from(width) * u64::from(height) {
        bytes.extend_from_slice(&color.0);
    }
    RgbaImage::from_raw(width, height, bytes).ok_or(RenderError::ImageTooLarge)
}

fn draw_canvas_boundaries(diff: &mut RgbaImage, expected: &RgbaImage, actual: &RgbaImage) {
    rectangle(
        diff,
        Bounds {
            x: 0,
            y: 0,
            width: expected.width(),
            height: expected.height(),
        },
        CANVAS,
        true,
    );
    rectangle(
        diff,
        Bounds {
            x: 0,
            y: 0,
            width: actual.width(),
            height: actual.height(),
        },
        CANVAS,
        false,
    );
}

fn draw_source_annotation(image: &mut RgbaImage, bounds: Option<Bounds>, difference: &Difference) {
    let Some(bounds) = bounds else {
        return;
    };
    let color = translucent(category_color(difference.kind), 210);
    rectangle(image, bounds, color, false);
    let (label_x, label_y) = label_position(bounds, image);
    text(image, label_x, label_y, &difference.id, color);
}

fn draw_diagnostic(
    diff: &mut RgbaImage,
    expected: &RgbaImage,
    actual: &RgbaImage,
    difference: &Difference,
    color_threshold: f64,
) {
    let color = category_color(difference.kind);
    if difference.kind == DifferenceKind::Changed {
        draw_residual_shape(diff, expected, actual, difference, color, color_threshold);
    }
    if let Some(bounds) = difference.expected_bounds {
        rectangle(diff, bounds, color, true);
    }
    if let Some(bounds) = difference.actual_bounds {
        rectangle(diff, bounds, color, false);
    }
    if difference.kind == DifferenceKind::Moved
        && let (Some(expected_bounds), Some(actual_bounds), Some(offset)) = (
            difference.expected_bounds,
            difference.actual_bounds,
            difference.offset,
        )
    {
        let start = expected_bounds.center();
        let end = actual_bounds.center();
        arrow(diff, start, end, color);
        let label = format!("{} DX{:+} DY{:+}", difference.id, offset.x, offset.y);
        text(
            diff,
            start.0.saturating_add(end.0) / 2,
            start.1.saturating_add(end.1) / 2,
            &label,
            color,
        );
        return;
    }
    let bounds = difference.expected_bounds.or(difference.actual_bounds);
    if let Some(bounds) = bounds {
        let (x, y) = label_position(bounds, diff);
        text(diff, x, y, &difference.id, color);
    }
}

fn draw_residual_shape(
    diff: &mut RgbaImage,
    expected: &RgbaImage,
    actual: &RgbaImage,
    difference: &Difference,
    color: Rgba<u8>,
    color_threshold: f64,
) {
    let (Some(expected_bounds), Some(actual_bounds)) =
        (difference.expected_bounds, difference.actual_bounds)
    else {
        return;
    };
    let width = expected_bounds
        .width
        .min(actual_bounds.width)
        .min(expected.width().saturating_sub(expected_bounds.x))
        .min(actual.width().saturating_sub(actual_bounds.x))
        .min(diff.width().saturating_sub(expected_bounds.x));
    let height = expected_bounds
        .height
        .min(actual_bounds.height)
        .min(expected.height().saturating_sub(expected_bounds.y))
        .min(actual.height().saturating_sub(actual_bounds.y))
        .min(diff.height().saturating_sub(expected_bounds.y));
    let mask_color = translucent(color, 90);
    for y in 0..height {
        for x in 0..width {
            let expected_x = expected_bounds.x + x;
            let expected_y = expected_bounds.y + y;
            let actual_x = actual_bounds.x + x;
            let actual_y = actual_bounds.y + y;
            if rgba_perceptual_distance(
                *expected.get_pixel(expected_x, expected_y),
                *actual.get_pixel(actual_x, actual_y),
            ) > color_threshold
            {
                fill_mask_pixel(diff, expected_x, expected_y, mask_color);
            }
        }
    }
}

fn label_position(bounds: Bounds, image: &RgbaImage) -> (u32, u32) {
    let x = bounds
        .x
        .saturating_add(2)
        .min(image.width().saturating_sub(1));
    let y = if bounds.y >= 9 {
        bounds.y - 8
    } else {
        bounds.y.saturating_add(2)
    }
    .min(image.height().saturating_sub(1));
    (x, y)
}

fn category_color(kind: DifferenceKind) -> Rgba<u8> {
    match kind {
        DifferenceKind::CanvasSize => CANVAS,
        DifferenceKind::Moved => MOVED,
        DifferenceKind::Resized => RESIZED,
        DifferenceKind::Added => ADDED,
        DifferenceKind::Removed => REMOVED,
        DifferenceKind::Changed => CHANGED,
    }
}

fn translucent(mut color: Rgba<u8>, alpha: u8) -> Rgba<u8> {
    color[3] = alpha;
    color
}
