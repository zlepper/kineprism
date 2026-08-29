use std::path::Path;

use better_image_diff_core::{
    Alignment, CompareOptions, Comparison, ComparisonSummary, Difference, SimilarityMetrics,
    SuppressionSummary,
};
use serde::Serialize;

use crate::artifacts::ArtifactPaths;

#[derive(Serialize)]
pub(crate) struct CliReport<'a> {
    schema_version: u32,
    equivalent: bool,
    expected: InputImage<'a>,
    actual: InputImage<'a>,
    settings: &'a CompareOptions,
    alignment: &'a Alignment,
    metrics: &'a SimilarityMetrics,
    summary: &'a ComparisonSummary,
    suppression: &'a SuppressionSummary,
    differences: &'a [Difference],
    artifacts: ReportArtifacts<'a>,
}

#[derive(Serialize)]
struct InputImage<'a> {
    path: &'a Path,
    width: u32,
    height: u32,
}

#[derive(Serialize)]
struct ReportArtifacts<'a> {
    expected: &'a Path,
    actual: &'a Path,
    diff: &'a Path,
}

impl<'a> CliReport<'a> {
    pub(crate) fn new(
        expected_path: &'a Path,
        actual_path: &'a Path,
        artifact_paths: &'a ArtifactPaths,
        comparison: &'a Comparison,
    ) -> Self {
        Self {
            schema_version: 1,
            equivalent: comparison.equivalent,
            expected: InputImage {
                path: expected_path,
                width: comparison.expected.width,
                height: comparison.expected.height,
            },
            actual: InputImage {
                path: actual_path,
                width: comparison.actual.width,
                height: comparison.actual.height,
            },
            settings: &comparison.settings,
            alignment: &comparison.alignment,
            metrics: &comparison.metrics,
            summary: &comparison.summary,
            suppression: &comparison.suppression,
            differences: &comparison.differences,
            artifacts: ReportArtifacts {
                expected: &artifact_paths.expected,
                actual: &artifact_paths.actual,
                diff: &artifact_paths.diff,
            },
        }
    }
}
