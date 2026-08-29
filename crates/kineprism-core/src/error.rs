use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::{Bounds, ImageDimensions};

/// An invalid comparison request.
#[derive(Debug, Clone, PartialEq)]
pub enum CompareError {
    /// The maximum offset cannot be represented by the matching implementation.
    MaxOffsetTooLarge(u32),
    /// The perceptual threshold is negative or not finite.
    InvalidColorThreshold(f64),
    /// Regions must contain at least one pixel.
    InvalidMinimumRegionArea(u32),
    /// A selected comparison region must have nonzero width and height.
    InvalidRegionSize(Bounds),
    /// A selected comparison region must fit completely inside both images.
    RegionOutOfBounds {
        /// Requested full-image comparison region.
        region: Bounds,
        /// Expected image dimensions.
        expected: ImageDimensions,
        /// Actual image dimensions.
        actual: ImageDimensions,
    },
    /// An image dimension or area cannot be represented safely.
    ImageTooLarge,
}

impl Display for CompareError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::MaxOffsetTooLarge(value) => {
                write!(formatter, "max offset {value} exceeds the supported range")
            }
            Self::InvalidColorThreshold(value) => {
                write!(
                    formatter,
                    "color threshold must be finite and non-negative, got {value}"
                )
            }
            Self::InvalidMinimumRegionArea(value) => {
                write!(
                    formatter,
                    "minimum region area must be positive, got {value}"
                )
            }
            Self::InvalidRegionSize(region) => write!(
                formatter,
                "comparison region must have positive width and height, got {}x{} at ({}, {})",
                region.width, region.height, region.x, region.y
            ),
            Self::RegionOutOfBounds {
                region,
                expected,
                actual,
            } => write!(
                formatter,
                "comparison region {}x{} at ({}, {}) must fit inside expected {}x{} and actual {}x{}",
                region.width,
                region.height,
                region.x,
                region.y,
                expected.width,
                expected.height,
                actual.width,
                actual.height
            ),
            Self::ImageTooLarge => formatter.write_str("image dimensions are too large"),
        }
    }
}

impl Error for CompareError {}

/// A failure while constructing annotated image buffers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    /// The diagnostic canvas dimensions cannot be represented safely.
    ImageTooLarge,
    /// The comparison dimensions do not match the supplied source images.
    ComparisonImageMismatch,
}

impl Display for RenderError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ImageTooLarge => formatter.write_str("rendered image dimensions are too large"),
            Self::ComparisonImageMismatch => {
                formatter.write_str("comparison dimensions do not match the supplied images")
            }
        }
    }
}

impl Error for RenderError {}
