//! Structural image comparison primitives used by the CLI and other Rust tools.

mod color;
mod compare;
mod error;
mod geometry;
mod mapping;
mod mask;
mod metrics;
mod render;
mod report;

pub use compare::compare;
pub use error::{CompareError, RenderError};
pub use render::render_artifacts;
pub use report::{
    Alignment, Bounds, CompareOptions, Comparison, ComparisonSummary, Difference, DifferenceKind,
    ImageDimensions, MetricSet, Offset, RenderedArtifacts, SimilarityMetrics,
};
