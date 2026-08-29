mod artifacts;
mod comparison;
mod error;
mod mcp;
mod report;

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args as ClapArgs, Parser, Subcommand};
use kineprism_core::{Bounds, CompareOptions};

use comparison::ComparisonRequest;
use error::CliError;

#[derive(Debug, Parser)]
#[command(
    name = "kineprism",
    version,
    about = "Structurally compare two UI screenshots"
)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
    #[command(flatten)]
    comparison: ComparisonArgs,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run a Model Context Protocol server over standard input and output.
    Mcp,
}

#[derive(Debug, ClapArgs)]
struct ComparisonArgs {
    /// Target or reference PNG.
    expected: Option<PathBuf>,
    /// Implementation PNG.
    actual: Option<PathBuf>,
    /// Directory for report.json and the three annotated PNGs.
    #[arg(long)]
    output_dir: Option<PathBuf>,
    /// Largest translation to search on each axis.
    #[arg(long, default_value_t = CompareOptions::default().max_offset)]
    max_offset: u32,
    /// Perceptual distance treated as equivalent.
    #[arg(long, default_value_t = CompareOptions::default().color_threshold)]
    color_threshold: f64,
    /// Smallest significant connected region.
    #[arg(long, default_value_t = CompareOptions::default().min_region_area)]
    min_region_area: u32,
    /// Left edge of the optional comparison region.
    #[arg(
        long,
        requires_all = ["region_y", "region_width", "region_height"]
    )]
    region_x: Option<u32>,
    /// Top edge of the optional comparison region.
    #[arg(long, requires = "region_x")]
    region_y: Option<u32>,
    /// Width of the optional comparison region.
    #[arg(long, requires = "region_x")]
    region_width: Option<u32>,
    /// Height of the optional comparison region.
    #[arg(long, requires = "region_x")]
    region_height: Option<u32>,
    /// Replace the four known artifacts if they exist.
    #[arg(long)]
    force: bool,
}

fn main() -> ExitCode {
    let arguments = Args::parse();
    if matches!(arguments.command, Some(Command::Mcp)) {
        return run_mcp();
    }

    match run(&arguments.comparison) {
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

fn run_mcp() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("error: failed to start MCP runtime: {error}");
            return ExitCode::from(2);
        }
    };
    match runtime.block_on(mcp::run()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: MCP server failed: {error}");
            ExitCode::from(2)
        }
    }
}

fn run(arguments: &ComparisonArgs) -> Result<bool, CliError> {
    let expected = arguments
        .expected
        .as_ref()
        .ok_or(CliError::MissingArgument("expected PNG"))?;
    let actual = arguments
        .actual
        .as_ref()
        .ok_or(CliError::MissingArgument("actual PNG"))?;
    let output_dir = arguments
        .output_dir
        .as_ref()
        .ok_or(CliError::MissingArgument("--output-dir"))?;
    let options = CompareOptions {
        max_offset: arguments.max_offset,
        color_threshold: arguments.color_threshold,
        min_region_area: arguments.min_region_area,
        region: comparison_region(arguments),
    };
    let request = ComparisonRequest {
        expected: expected.clone(),
        actual: actual.clone(),
        output_dir: output_dir.clone(),
        options,
        force: arguments.force,
    };
    let result = comparison::run(&request)?;
    io::stdout().write_all(&result.report_json)?;
    Ok(result.equivalent)
}

fn comparison_region(arguments: &ComparisonArgs) -> Option<Bounds> {
    arguments.region_x.map(|x| Bounds {
        x,
        y: arguments
            .region_y
            .expect("Clap requires region y when region x is present"),
        width: arguments
            .region_width
            .expect("Clap requires region width when region x is present"),
        height: arguments
            .region_height
            .expect("Clap requires region height when region x is present"),
    })
}
