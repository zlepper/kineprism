use std::fmt::{self, Display, Formatter};

use image::RgbaImage;
use serde::Serialize;

/// Settings controlling structural comparison.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CompareOptions {
    /// Largest absolute translation searched on either axis, in pixels.
    pub max_offset: u32,
    /// Perceptual distance at or below which pixels are treated as equivalent.
    pub color_threshold: f64,
    /// Smallest connected region that may become a reported difference.
    pub min_region_area: u32,
    /// Optional full-image rectangle restricting every comparison scope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<Bounds>,
}

impl Default for CompareOptions {
    fn default() -> Self {
        Self {
            max_offset: 128,
            color_threshold: 2.3,
            min_region_area: 16,
            region: None,
        }
    }
}

impl CompareOptions {
    /// Validates that all settings can be used safely by the comparison engine.
    ///
    /// # Errors
    ///
    /// Returns a typed error when an option is outside its documented range.
    pub fn validate(&self) -> Result<(), crate::CompareError> {
        if self.max_offset > i32::MAX as u32 {
            return Err(crate::CompareError::MaxOffsetTooLarge(self.max_offset));
        }
        if !self.color_threshold.is_finite() || self.color_threshold < 0.0 {
            return Err(crate::CompareError::InvalidColorThreshold(
                self.color_threshold,
            ));
        }
        if self.min_region_area == 0 {
            return Err(crate::CompareError::InvalidMinimumRegionArea(0));
        }
        if let Some(region) = self.region
            && (region.width == 0 || region.height == 0)
        {
            return Err(crate::CompareError::InvalidRegionSize(region));
        }
        Ok(())
    }
}

/// An image's pixel dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ImageDimensions {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// A half-open pixel rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Bounds {
    /// Left coordinate.
    pub x: u32,
    /// Top coordinate.
    pub y: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// Translation from expected coordinates to actual coordinates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Offset {
    /// Horizontal displacement: `actual_x - expected_x`.
    pub x: i32,
    /// Vertical displacement: `actual_y - expected_y`.
    pub y: i32,
}

/// Estimated whole-image content alignment.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Alignment {
    /// Translation from expected to actual content.
    pub offset: Offset,
    /// Match confidence in `[0, 1]`.
    pub confidence: f64,
}

/// A structural difference category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DifferenceKind {
    /// Source canvas dimensions differ.
    CanvasSize,
    /// Corresponding content appears at a translated position.
    Moved,
    /// Corresponding content has different dimensions.
    Resized,
    /// Content exists only in the actual image.
    Added,
    /// Content exists only in the expected image.
    Removed,
    /// Content differs without a more reliable explanation.
    Changed,
}

impl Display for DifferenceKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::CanvasSize => "canvas_size",
            Self::Moved => "moved",
            Self::Resized => "resized",
            Self::Added => "added",
            Self::Removed => "removed",
            Self::Changed => "changed",
        };
        formatter.write_str(value)
    }
}

/// Similarity values calculated over one coordinate mapping.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MetricSet {
    /// Number of valid expected/actual pixel pairs included in this scope.
    pub compared_pixels: u64,
    /// Compared pixels divided by the full expected canvas area, in `[0, 1]`.
    pub expected_coverage: f64,
    /// Compared pixels divided by the full actual canvas area, in `[0, 1]`.
    pub actual_coverage: f64,
    /// Mean absolute error in `[0, 1]` over linear, alpha-premultiplied RGBA channels with equal
    /// channel weight; `None` when the scope has no pairs.
    pub mae: Option<f64>,
    /// Root mean squared error in `[0, 1]` over the same channels as MAE; `None` when the scope has
    /// no pairs.
    pub rmse: Option<f64>,
    /// Peak signal-to-noise ratio in decibels using peak `1.0`; `None` represents either positive
    /// infinity for a perfect scope or an unavailable value when there are no pairs. Inspect
    /// `compared_pixels` to distinguish those cases.
    pub psnr_db: Option<f64>,
    /// Mean structural similarity in `[-1, 1]`, averaged across the four normalized channels.
    /// Uses an 11x11 Gaussian window (`sigma=1.5`, `K1=0.01`, `K2=0.03`, `L=1`) sampled every
    /// eight pixels, with one available-area window for smaller images.
    pub ssim: Option<f64>,
    /// Fraction of pairs whose Lab-plus-alpha distance is strictly greater than the configured
    /// perceptual threshold, in `[0, 1]`.
    pub changed_pixel_ratio: Option<f64>,
}

/// Literal and alignment-aware similarity metrics.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SimilarityMetrics {
    /// Same-coordinate overlap without alignment.
    pub raw: MetricSet,
    /// Valid overlap after applying the detected whole-image translation.
    pub global_aligned: MetricSet,
    /// Valid, uniquely paired pixels after detected global and validated local translations.
    pub structural_aligned: MetricSet,
}

/// One reported structural or visual difference.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Difference {
    /// Stable identifier assigned after deterministic sorting.
    pub id: String,
    /// Difference category.
    pub kind: DifferenceKind,
    /// Relevant region in expected coordinates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_bounds: Option<Bounds>,
    /// Relevant region in actual coordinates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_bounds: Option<Bounds>,
    /// Translation from expected to actual coordinates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<Offset>,
    /// Classification confidence in `[0, 1]`.
    pub confidence: f64,
    /// Concise human- and agent-readable explanation.
    pub message: String,
}

/// Counts derived from finalized differences.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ComparisonSummary {
    /// All differences.
    pub total: u32,
    /// Moved regions.
    pub moved: u32,
    /// Resized regions.
    pub resized: u32,
    /// Added regions.
    pub added: u32,
    /// Removed regions.
    pub removed: u32,
    /// Generically changed regions.
    pub changed: u32,
    /// Canvas-size records.
    pub canvas_size: u32,
}

/// Residual differences intentionally deferred so primary structural findings remain prominent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SuppressionSummary {
    /// Small residual regions suppressed because they border exactly one validated movement.
    pub movement_border_regions: u32,
    /// Connected changed pixels contained by those suppressed regions.
    pub movement_border_pixels: u64,
    /// Human- and agent-readable guidance when any residuals were suppressed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl SuppressionSummary {
    pub(crate) fn record_movement_border(&mut self, pixels: u32) {
        self.movement_border_regions = self.movement_border_regions.saturating_add(1);
        self.movement_border_pixels = self
            .movement_border_pixels
            .saturating_add(u64::from(pixels));
        self.message = Some(format!(
            "Suppressed {} small residual {} ({} px) bordering validated movements. Recheck {} after correcting the reported movements.",
            self.movement_border_regions,
            if self.movement_border_regions == 1 {
                "region"
            } else {
                "regions"
            },
            self.movement_border_pixels,
            if self.movement_border_regions == 1 {
                "it"
            } else {
                "them"
            }
        ));
    }
}

/// Complete path-independent result of a comparison.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Comparison {
    /// Expected image dimensions.
    pub expected: ImageDimensions,
    /// Actual image dimensions.
    pub actual: ImageDimensions,
    /// Effective settings.
    pub settings: CompareOptions,
    /// Estimated global content relationship.
    pub alignment: Alignment,
    /// Literal and alignment-aware metrics.
    pub metrics: SimilarityMetrics,
    /// Counts derived from differences.
    pub summary: ComparisonSummary,
    /// Pixel-level residuals deferred in favor of more valuable structural findings.
    pub suppression: SuppressionSummary,
    /// Sorted structural differences.
    pub differences: Vec<Difference>,
    /// Whether no meaningful differences remain.
    pub equivalent: bool,
}

/// Three in-memory annotated images.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedArtifacts {
    /// Annotated expected image.
    pub expected: RgbaImage,
    /// Annotated actual image.
    pub actual: RgbaImage,
    /// White-background diagnostics.
    pub diff: RgbaImage,
}
