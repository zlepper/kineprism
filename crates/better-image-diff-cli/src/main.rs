mod artifacts;
mod error;
mod report;

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use better_image_diff_core::{CompareOptions, compare, render_artifacts};
use clap::Parser;
use image::{ImageFormat, ImageReader, RgbaImage};

use artifacts::ArtifactPaths;
use error::CliError;
use report::CliReport;

#[derive(Debug, Parser)]
#[command(
    name = "better-image-diff",
    version,
    about = "Structurally compare two UI screenshots"
)]
struct Args {
    /// Target or reference PNG.
    expected: PathBuf,
    /// Implementation PNG.
    actual: PathBuf,
    /// Directory for expected.png, actual.png, and diff.png.
    #[arg(long)]
    output_dir: PathBuf,
    /// Largest translation to search on each axis.
    #[arg(long, default_value_t = CompareOptions::default().max_offset)]
    max_offset: u32,
    /// Perceptual distance treated as equivalent.
    #[arg(long, default_value_t = CompareOptions::default().color_threshold)]
    color_threshold: f64,
    /// Smallest significant connected region.
    #[arg(long, default_value_t = CompareOptions::default().min_region_area)]
    min_region_area: u32,
    /// Replace the three known artifacts if they exist.
    #[arg(long)]
    force: bool,
}

fn main() -> ExitCode {
    match run(&Args::parse()) {
        Ok(equivalent) => {
            if equivalent {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(2)
        }
    }
}

fn run(arguments: &Args) -> Result<bool, CliError> {
    let options = CompareOptions {
        max_offset: arguments.max_offset,
        color_threshold: arguments.color_threshold,
        min_region_area: arguments.min_region_area,
    };
    options.validate()?;

    let artifact_paths = ArtifactPaths::new(&arguments.output_dir);
    artifact_paths.preflight(&arguments.expected, &arguments.actual, arguments.force)?;
    let expected = load_png(&arguments.expected)?;
    let actual = load_png(&arguments.actual)?;
    let comparison = compare(&expected, &actual, &options)?;
    let rendered = render_artifacts(&expected, &actual, &comparison)?;
    let report = CliReport::new(
        &arguments.expected,
        &arguments.actual,
        &artifact_paths,
        &comparison,
    );
    let json = serde_json::to_vec_pretty(&report)?;

    artifact_paths.write(&rendered, arguments.force)?;
    io::stdout().write_all(&json)?;
    io::stdout().write_all(b"\n")?;
    Ok(comparison.equivalent)
}

fn load_png(path: &Path) -> Result<RgbaImage, CliError> {
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
