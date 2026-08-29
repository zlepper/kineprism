#![allow(
    deprecated,
    reason = "MCP Roots is the requested workspace access boundary despite its protocol deprecation"
)]

use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
};

use kineprism_core::{Bounds, CompareOptions, Difference};
use rmcp::{
    RoleServer, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ContentBlock, Root},
    schemars,
    service::RequestContext,
    tool, tool_handler, tool_router,
    transport::stdio,
};
use serde::Deserialize;

use crate::comparison::{self, ComparisonRequest};

#[derive(Clone)]
struct ImageDiffServer {
    tool_router: ToolRouter<Self>,
}

impl ImageDiffServer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CompareUiImagesRequest {
    /// Absolute path to the reference UI screenshot PNG.
    expected_path: String,
    /// Absolute path to the implementation UI screenshot PNG.
    actual_path: String,
    /// Absolute directory where the report and annotated PNG artifacts will be written.
    output_dir: String,
    /// Largest translation to search on each axis, in pixels. Defaults to 128.
    max_offset: Option<u32>,
    /// Perceptual distance treated as equivalent. Defaults to 2.3.
    color_threshold: Option<f64>,
    /// Smallest significant connected region, in pixels. Defaults to 16.
    min_region_area: Option<u32>,
    /// Optional rectangle restricting the comparison to a UI region.
    region: Option<Region>,
    /// Replace the four managed artifacts when they already exist. Defaults to false.
    #[serde(default)]
    force: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct Region {
    /// Left edge of the comparison region, in pixels.
    x: u32,
    /// Top edge of the comparison region, in pixels.
    y: u32,
    /// Width of the comparison region, in pixels.
    width: u32,
    /// Height of the comparison region, in pixels.
    height: u32,
}

#[tool_router]
impl ImageDiffServer {
    #[tool(
        name = "compare_ui_images",
        description = "Compare two UI screenshot PNGs, return a compact plain-text summary, and write a detailed JSON report plus annotated expected.png, actual.png, and diff.png artifacts.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn compare_ui_images(
        &self,
        Parameters(request): Parameters<CompareUiImagesRequest>,
        context: RequestContext<RoleServer>,
    ) -> CallToolResult {
        let roots = match context.peer.list_roots().await {
            Ok(result) => result.roots,
            Err(error) => {
                return tool_error(format!(
                    "could not obtain MCP workspace roots; this tool requires the client Roots capability: {error}"
                ));
            }
        };
        let request = match comparison_request(request, &roots) {
            Ok(request) => request,
            Err(error) => return tool_error(error),
        };

        match tokio::task::spawn_blocking(move || comparison::run(&request)).await {
            Ok(Ok(result)) => {
                CallToolResult::success(vec![ContentBlock::text(format_comparison_result(&result))])
            }
            Ok(Err(error)) => tool_error(error.to_string()),
            Err(error) => tool_error(format!("comparison task failed: {error}")),
        }
    }
}

#[allow(clippy::unused_async_trait_impl)]
#[tool_handler(
    router = self.tool_router,
    name = "kineprism",
    instructions = "Use compare_ui_images to compare PNG screenshots of user interfaces. All paths must be absolute and under an MCP workspace root."
)]
impl ServerHandler for ImageDiffServer {}

pub(crate) async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let service = ImageDiffServer::new().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

fn tool_error(message: String) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(message)])
}

fn format_comparison_result(result: &comparison::ComparisonResult) -> String {
    let comparison = &result.comparison;
    let mut output = String::new();
    let outcome = if result.equivalent {
        "equivalent"
    } else {
        "different"
    };
    writeln!(output, "Comparison: {outcome}").expect("write to string");
    write_summary(&mut output, &comparison.summary);
    writeln!(
        output,
        "MAE: raw={}; global-aligned={}; structural-aligned={}",
        metric_value(comparison.metrics.raw.mae),
        metric_value(comparison.metrics.global_aligned.mae),
        metric_value(comparison.metrics.structural_aligned.mae),
    )
    .expect("write to string");
    writeln!(
        output,
        "Structural metrics: SSIM={}; changed-pixel-ratio={}",
        metric_value(comparison.metrics.structural_aligned.ssim),
        percentage_value(comparison.metrics.structural_aligned.changed_pixel_ratio),
    )
    .expect("write to string");

    for difference in &comparison.differences {
        write_difference(&mut output, difference);
    }
    write_suppression(&mut output, &comparison.suppression);

    writeln!(output, "Artifacts:").expect("write to string");
    writeln!(
        output,
        "- annotated expected: {}",
        result.artifact_paths.expected.display()
    )
    .expect("write to string");
    writeln!(
        output,
        "- annotated actual: {}",
        result.artifact_paths.actual.display()
    )
    .expect("write to string");
    writeln!(output, "- diff: {}", result.artifact_paths.diff.display()).expect("write to string");
    writeln!(
        output,
        "- detailed report: {}",
        result.artifact_paths.report.display()
    )
    .expect("write to string");
    output
}

fn write_summary(output: &mut String, summary: &kineprism_core::ComparisonSummary) {
    if summary.total == 0 {
        writeln!(output, "Findings: none").expect("write to string");
        return;
    }

    let mut counts = Vec::new();
    for (kind, count) in [
        ("moved", summary.moved),
        ("resized", summary.resized),
        ("added", summary.added),
        ("removed", summary.removed),
        ("changed", summary.changed),
        ("canvas-size", summary.canvas_size),
    ] {
        if count > 0 {
            counts.push(format!("{kind}={count}"));
        }
    }
    writeln!(
        output,
        "Findings: {} total ({})",
        summary.total,
        counts.join(", ")
    )
    .expect("write to string");
}

fn write_difference(output: &mut String, difference: &Difference) {
    let message_prefix = format!("{}: ", difference.id);
    let message = difference
        .message
        .strip_prefix(&message_prefix)
        .unwrap_or(&difference.message);
    write!(output, "- {} {}: {message}", difference.id, difference.kind).expect("write to string");
    if let Some(bounds) = difference.expected_bounds {
        write!(
            output,
            "; expected=(x={},y={},width={},height={})",
            bounds.x, bounds.y, bounds.width, bounds.height
        )
        .expect("write to string");
    }
    if let Some(bounds) = difference.actual_bounds {
        write!(
            output,
            "; actual=(x={},y={},width={},height={})",
            bounds.x, bounds.y, bounds.width, bounds.height
        )
        .expect("write to string");
    }
    if let Some(offset) = difference.offset {
        write!(output, "; offset=({:+},{:+})", offset.x, offset.y).expect("write to string");
    }
    writeln!(output, "; confidence={:.3}", difference.confidence).expect("write to string");
}

fn write_suppression(output: &mut String, suppression: &kineprism_core::SuppressionSummary) {
    if suppression.movement_border_regions > 0 {
        writeln!(
            output,
            "Suppressed residuals: {} region(s), {} px; see the detailed report.",
            suppression.movement_border_regions, suppression.movement_border_pixels
        )
        .expect("write to string");
    }
}

fn metric_value(value: Option<f64>) -> String {
    value.map_or_else(|| "n/a".to_owned(), |value| format!("{value:.6}"))
}

fn percentage_value(value: Option<f64>) -> String {
    value.map_or_else(
        || "n/a".to_owned(),
        |value| format!("{:.2}%", value * 100.0),
    )
}

fn comparison_request(
    request: CompareUiImagesRequest,
    roots: &[Root],
) -> Result<ComparisonRequest, String> {
    let roots = canonical_root_paths(roots)?;
    let expected = validate_existing_path("expected_path", &request.expected_path, &roots)?;
    let actual = validate_existing_path("actual_path", &request.actual_path, &roots)?;
    let output_dir = validate_output_directory(&request.output_dir, &roots)?;
    let defaults = CompareOptions::default();

    Ok(ComparisonRequest {
        expected,
        actual,
        output_dir,
        options: CompareOptions {
            max_offset: request.max_offset.unwrap_or(defaults.max_offset),
            color_threshold: request.color_threshold.unwrap_or(defaults.color_threshold),
            min_region_area: request.min_region_area.unwrap_or(defaults.min_region_area),
            region: request.region.map(|region| Bounds {
                x: region.x,
                y: region.y,
                width: region.width,
                height: region.height,
            }),
        },
        force: request.force,
    })
}

fn canonical_root_paths(roots: &[Root]) -> Result<Vec<PathBuf>, String> {
    if roots.is_empty() {
        return Err("the client supplied no MCP workspace roots".to_string());
    }

    roots
        .iter()
        .map(|root| {
            let path = file_uri_to_path(&root.uri)?;
            if !path.is_absolute() {
                return Err(format!("MCP root '{}' is not an absolute path", root.uri));
            }
            let canonical = std::fs::canonicalize(&path)
                .map_err(|error| format!("could not resolve MCP root '{}': {error}", root.uri))?;
            if !canonical.is_dir() {
                return Err(format!("MCP root '{}' is not a directory", root.uri));
            }
            Ok(canonical)
        })
        .collect()
}

fn file_uri_to_path(uri: &str) -> Result<PathBuf, String> {
    let encoded_path = uri
        .strip_prefix("file://")
        .ok_or_else(|| format!("MCP root '{uri}' is not a file URI"))?;
    let encoded_path = encoded_path
        .strip_prefix('/')
        .ok_or_else(|| format!("MCP root '{uri}' has a remote host"))?;
    let decoded = percent_decode(encoded_path)?;
    Ok(PathBuf::from(format!("/{decoded}")))
}

fn percent_decode(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = *bytes
                .get(index + 1)
                .ok_or_else(|| format!("invalid percent encoding in '{value}'"))?;
            let low = *bytes
                .get(index + 2)
                .ok_or_else(|| format!("invalid percent encoding in '{value}'"))?;
            let high =
                hex_value(high).ok_or_else(|| format!("invalid percent encoding in '{value}'"))?;
            let low =
                hex_value(low).ok_or_else(|| format!("invalid percent encoding in '{value}'"))?;
            decoded.push(high * 16 + low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).map_err(|_| format!("MCP root '{value}' is not valid UTF-8"))
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn validate_existing_path(
    parameter: &str,
    value: &str,
    roots: &[PathBuf],
) -> Result<PathBuf, String> {
    let path = absolute_path(parameter, value)?;
    let canonical = std::fs::canonicalize(&path).map_err(|error| {
        format!(
            "could not resolve {parameter} '{}': {error}",
            path.display()
        )
    })?;
    ensure_within_roots(parameter, &canonical, roots)?;
    Ok(canonical)
}

fn validate_output_directory(value: &str, roots: &[PathBuf]) -> Result<PathBuf, String> {
    let path = absolute_path("output_dir", value)?;
    let ancestor = canonical_existing_ancestor(&path)?;
    ensure_within_roots("output_dir", &ancestor, roots)?;
    Ok(path)
}

fn absolute_path(parameter: &str, value: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(format!("{parameter} must be an absolute path"))
    }
}

fn canonical_existing_ancestor(path: &Path) -> Result<PathBuf, String> {
    let mut ancestor = path;
    loop {
        if ancestor.exists() {
            return std::fs::canonicalize(ancestor).map_err(|error| {
                format!(
                    "could not resolve output directory ancestor '{}': {error}",
                    ancestor.display()
                )
            });
        }
        ancestor = ancestor.parent().ok_or_else(|| {
            format!(
                "could not find an existing ancestor for output_dir '{}'",
                path.display()
            )
        })?;
    }
}

fn ensure_within_roots(parameter: &str, path: &Path, roots: &[PathBuf]) -> Result<(), String> {
    if roots.iter().any(|root| path.starts_with(root)) {
        Ok(())
    } else {
        Err(format!(
            "{parameter} '{}' is outside the MCP workspace roots",
            path.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uri_decoding_supports_spaces() {
        assert_eq!(
            file_uri_to_path("file:///tmp/ui%20images").expect("decode file URI"),
            PathBuf::from("/tmp/ui images")
        );
    }

    #[test]
    fn file_uri_rejects_remote_hosts() {
        assert!(file_uri_to_path("file://example.com/tmp").is_err());
    }
}
