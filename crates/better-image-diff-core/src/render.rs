use image::{Rgba, RgbaImage};

use crate::{Comparison, RenderError, RenderedArtifacts};

const MAX_RENDER_PIXELS: u64 = 100_000_000;

/// Renders annotated source images and a white diagnostic canvas in memory.
///
/// # Errors
///
/// Returns [`RenderError`] when the comparison does not belong to the images or when the
/// diagnostic canvas would exceed the renderer's safe allocation limit.
pub fn render_artifacts(
    expected: &RgbaImage,
    actual: &RgbaImage,
    comparison: &Comparison,
) -> Result<RenderedArtifacts, RenderError> {
    if comparison.expected.width != expected.width()
        || comparison.expected.height != expected.height()
        || comparison.actual.width != actual.width()
        || comparison.actual.height != actual.height()
    {
        return Err(RenderError::ComparisonImageMismatch);
    }
    let width = expected.width().max(actual.width());
    let height = expected.height().max(actual.height());
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or(RenderError::ImageTooLarge)?;
    if pixels > MAX_RENDER_PIXELS {
        return Err(RenderError::ImageTooLarge);
    }
    let diff = RgbaImage::from_pixel(width, height, Rgba([255, 255, 255, 255]));

    Ok(RenderedArtifacts {
        expected: expected.clone(),
        actual: actual.clone(),
        diff,
    })
}
