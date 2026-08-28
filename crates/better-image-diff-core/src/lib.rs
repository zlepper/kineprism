//! Structural image comparison primitives used by the CLI and other Rust tools.

mod alignment;
mod classify;
mod color;
mod compare;
mod error;
mod geometry;
mod local;
mod local_geometry;
mod mapping;
mod mask;
mod matching;
mod metrics;
mod movement;
mod proposals;
mod pyramid;
mod render;
mod report;
mod residual;

pub use compare::compare;
pub use error::{CompareError, RenderError};
pub use render::render_artifacts;
pub use report::{
    Alignment, Bounds, CompareOptions, Comparison, ComparisonSummary, Difference, DifferenceKind,
    ImageDimensions, MetricSet, Offset, RenderedArtifacts, SimilarityMetrics,
};
