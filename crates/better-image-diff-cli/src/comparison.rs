use std::path::{Path, PathBuf};

use better_image_diff_core::{CompareOptions, compare, render_artifacts};

use crate::artifacts::ArtifactPaths;
use crate::error::CliError;
use crate::report::CliReport;

pub(crate) struct ComparisonRequest {
    pub(crate) expected: PathBuf,
    pub(crate) actual: PathBuf,
    pub(crate) output_dir: PathBuf,
    pub(crate) options: CompareOptions,
    pub(crate) force: bool,
}

pub(crate) struct ComparisonResult {
    pub(crate) equivalent: bool,
    pub(crate) report_json: Vec<u8>,
}

pub(crate) fn run(request: &ComparisonRequest) -> Result<ComparisonResult, CliError> {
    request.options.validate()?;

    let artifact_paths = ArtifactPaths::new(&request.output_dir);
    artifact_paths.preflight(&request.expected, &request.actual, request.force)?;
    let expected = load_png(&request.expected)?;
    let actual = load_png(&request.actual)?;
    let comparison = compare(&expected, &actual, &request.options)?;
    let rendered = render_artifacts(&expected, &actual, &comparison)?;
    let report = CliReport::new(
        &request.expected,
        &request.actual,
        &artifact_paths,
        &comparison,
    );
    let mut report_json = serde_json::to_vec_pretty(&report)?;
    report_json.push(b'\n');

    artifact_paths.write(&rendered, &report_json, request.force)?;
    Ok(ComparisonResult {
        equivalent: comparison.equivalent,
        report_json,
    })
}

fn load_png(path: &Path) -> Result<image::RgbaImage, CliError> {
    use image::{ImageFormat, ImageReader};

    let reader = ImageReader::open(path).map_err(|source| CliError::Io {
        action: "open input",
        path: path.to_owned(),
        source,
    })?;
    let reader = reader
        .with_guessed_format()
        .map_err(|source| CliError::Io {
            action: "inspect input",
            path: path.to_owned(),
            source,
        })?;
    if reader.format() != Some(ImageFormat::Png) {
        return Err(CliError::NotPng(path.to_owned()));
    }
    reader
        .decode()
        .map(image::DynamicImage::into_rgba8)
        .map_err(|source| CliError::Decode {
            path: path.to_owned(),
            source,
        })
}
