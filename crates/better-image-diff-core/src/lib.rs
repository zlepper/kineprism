//! Structural image comparison primitives used by the CLI and other Rust tools.

mod compare;
mod error;
mod render;
mod report;

pub use compare::compare;
pub use error::{CompareError, RenderError};
pub use render::render_artifacts;
pub use report::{
    Alignment, Bounds, CompareOptions, Comparison, ComparisonSummary, Difference, DifferenceKind,
    ImageDimensions, MetricSet, Offset, RenderedArtifacts, SimilarityMetrics,
};
