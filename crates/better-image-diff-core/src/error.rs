use std::error::Error;
use std::fmt::{self, Display, Formatter};

/// An invalid comparison request.
#[derive(Debug, Clone, PartialEq)]
pub enum CompareError {
    /// The maximum offset cannot be represented by the matching implementation.
    MaxOffsetTooLarge(u32),
    /// The perceptual threshold is negative or not finite.
    InvalidColorThreshold(f64),
    /// Regions must contain at least one pixel.
    InvalidMinimumRegionArea(u32),
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
