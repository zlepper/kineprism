#![allow(
    deprecated,
    reason = "the server intentionally uses MCP Roots as its workspace access boundary"
)]

use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use image::{ImageFormat, Rgba, RgbaImage};
use rayon::ThreadPoolBuilder;
use rmcp::{
    ClientHandler, ServiceExt,
    model::{
        CallToolRequestParams, ClientCapabilities, ClientInfo, Implementation, ListRootsResult,
        Root,
    },
    service::{RequestContext, RoleClient},
    transport::{ConfigureCommandExt, TokioChildProcess},
};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("kineprism-test-{}-{sequence}", std::process::id()));
        fs::create_dir(&path).expect("create test directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _result = fs::remove_dir_all(&self.0);
    }
}

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kineprism"))
}

struct McpTestClient {
    root_uri: Option<String>,
}

impl ClientHandler for McpTestClient {
    fn get_info(&self) -> ClientInfo {
        let capabilities = if self.root_uri.is_some() {
            ClientCapabilities::builder().enable_roots().build()
        } else {
            ClientCapabilities::builder().build()
        };
        ClientInfo::new(capabilities, Implementation::new("kineprism-test", "1"))
    }

    fn list_roots(
        &self,
        _context: RequestContext<RoleClient>,
    ) -> impl Future<Output = Result<ListRootsResult, rmcp::ErrorData>> {
        let roots = self
            .root_uri
            .as_deref()
            .map_or_else(Vec::new, |uri| vec![Root::new(uri)]);
        std::future::ready(Ok(ListRootsResult::new(roots)))
    }
}

async fn mcp_client(
    root: &Path,
) -> Result<rmcp::service::RunningService<RoleClient, McpTestClient>, Box<dyn std::error::Error>> {
    mcp_client_with_roots(root, Some(file_uri(root))).await
}

async fn mcp_client_without_roots(
    root: &Path,
) -> Result<rmcp::service::RunningService<RoleClient, McpTestClient>, Box<dyn std::error::Error>> {
    mcp_client_with_roots(root, None).await
}

async fn mcp_client_with_roots(
    root: &Path,
    root_uri: Option<String>,
) -> Result<rmcp::service::RunningService<RoleClient, McpTestClient>, Box<dyn std::error::Error>> {
    let transport = TokioChildProcess::new(
        tokio::process::Command::new(env!("CARGO_BIN_EXE_kineprism")).configure(|command| {
            command.arg("mcp").current_dir(root);
        }),
    )?;
    let client = McpTestClient { root_uri }.serve(transport).await?;
    Ok(client)
}

fn file_uri(path: &Path) -> String {
    format!("file://{}", path.display())
}

#[test]
fn help_describes_the_public_arguments() {
    let output = command().arg("--help").output().expect("run CLI");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 help");
    assert!(stdout.contains("EXPECTED"));
    assert!(stdout.contains("ACTUAL"));
    assert!(stdout.contains("--output-dir"));
    assert!(stdout.contains("--max-offset"));
    assert!(stdout.contains("--color-threshold"));
    assert!(stdout.contains("--min-region-area"));
    assert!(stdout.contains("--region-x"));
    assert!(stdout.contains("--region-y"));
    assert!(stdout.contains("--region-width"));
    assert!(stdout.contains("--region-height"));
    assert!(stdout.contains("--force"));
    assert!(stdout.contains("mcp"));
}

#[tokio::test]
async fn mcp_compares_ui_images_within_roots_and_preserves_artifact_safety()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new();
    let expected_path = directory.path().join("expected.png");
    let actual_path = directory.path().join("actual.png");
    let output_directory = directory.path().join("output");
    let expected = RgbaImage::from_pixel(8, 6, Rgba([20, 30, 40, 255]));
    expected.save(&expected_path)?;
    expected.save(&actual_path)?;

    let client = mcp_client(directory.path()).await?;
    let tools = client.list_all_tools().await?;
    let tool = tools
        .iter()
        .find(|tool| tool.name == "compare_ui_images")
        .expect("compare_ui_images tool");
    assert!(
        tool.description
            .as_deref()
            .expect("tool description")
            .contains("UI screenshot")
    );
    assert!(
        tool.input_schema["required"]
            .as_array()
            .expect("required inputs")
            .iter()
            .any(|parameter| parameter == "expected_path")
    );

    let arguments = serde_json::json!({
        "expected_path": expected_path,
        "actual_path": actual_path,
        "output_dir": output_directory,
    });
    let first = client
        .call_tool(
            CallToolRequestParams::new("compare_ui_images").with_arguments(json_object(&arguments)),
        )
        .await?;
    assert!(!first.is_error.unwrap_or(false), "{first:?}");
    let response = first.content[0]
        .as_text()
        .expect("report text")
        .text
        .as_str();
    assert!(response.starts_with("Comparison: equivalent"), "{response}");
    assert!(response.contains("MAE: raw="), "{response}");
    assert!(response.contains("Structural metrics: SSIM="), "{response}");
    assert!(response.contains("Findings: none"), "{response}");
    for name in ["expected.png", "actual.png", "diff.png", "report.json"] {
        let artifact = output_directory.join(name);
        assert!(artifact.is_file(), "missing {name}");
        assert!(
            response.contains(&artifact.display().to_string()),
            "response omitted artifact path '{}': {response}",
            artifact.display()
        );
    }
    let detailed_report: serde_json::Value =
        serde_json::from_slice(&fs::read(output_directory.join("report.json"))?)?;
    assert_eq!(detailed_report["equivalent"], true);

    let repeated = client
        .call_tool(
            CallToolRequestParams::new("compare_ui_images").with_arguments(json_object(&arguments)),
        )
        .await?;
    assert_eq!(repeated.is_error, Some(true));
    assert!(
        repeated.content[0]
            .as_text()
            .expect("error text")
            .text
            .contains("already exists")
    );

    let forced = client
        .call_tool(
            CallToolRequestParams::new("compare_ui_images").with_arguments(json_object(
                &serde_json::json!({
                    "expected_path": expected_path,
                    "actual_path": actual_path,
                    "output_dir": output_directory,
                    "force": true,
                }),
            )),
        )
        .await?;
    assert!(!forced.is_error.unwrap_or(false), "{forced:?}");
    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn mcp_text_result_reports_metrics_and_findings() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new();
    let expected_path = directory.path().join("expected.png");
    let actual_path = directory.path().join("actual.png");
    let output_directory = directory.path().join("output");
    let expected = RgbaImage::from_pixel(12, 10, Rgba([245, 245, 245, 255]));
    let mut actual = expected.clone();
    for y in 3..7 {
        for x in 4..8 {
            actual.put_pixel(x, y, Rgba([20, 30, 40, 255]));
        }
    }
    expected.save(&expected_path)?;
    actual.save(&actual_path)?;

    let client = mcp_client(directory.path()).await?;
    let result = client
        .call_tool(
            CallToolRequestParams::new("compare_ui_images").with_arguments(json_object(
                &serde_json::json!({
                    "expected_path": expected_path,
                    "actual_path": actual_path,
                    "output_dir": output_directory,
                }),
            )),
        )
        .await?;

    assert!(!result.is_error.unwrap_or(false), "{result:?}");
    let response = result.content[0]
        .as_text()
        .expect("report text")
        .text
        .as_str();
    assert!(response.starts_with("Comparison: different"), "{response}");
    assert!(response.contains("MAE: raw="), "{response}");
    assert!(response.contains("global-aligned="), "{response}");
    assert!(response.contains("structural-aligned="), "{response}");
    assert!(response.contains("Structural metrics: SSIM="), "{response}");
    assert!(response.contains("changed-pixel-ratio="), "{response}");
    assert!(response.contains("Findings: "), "{response}");
    assert!(response.contains("- D1 "), "{response}");
    assert!(response.contains("confidence="), "{response}");
    assert!(!response.trim_start().starts_with('{'), "{response}");

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn mcp_uses_its_working_directory_when_the_client_supplies_no_roots()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new();
    let expected_path = directory.path().join("expected.png");
    let actual_path = directory.path().join("actual.png");
    let output_directory = directory.path().join("output");
    let image = RgbaImage::from_pixel(8, 6, Rgba([20, 30, 40, 255]));
    image.save(&expected_path)?;
    image.save(&actual_path)?;

    let client = mcp_client_without_roots(directory.path()).await?;
    let result = client
        .call_tool(
            CallToolRequestParams::new("compare_ui_images").with_arguments(json_object(
                &serde_json::json!({
                    "expected_path": expected_path,
                    "actual_path": actual_path,
                    "output_dir": output_directory,
                }),
            )),
        )
        .await?;

    assert!(!result.is_error.unwrap_or(false), "{result:?}");
    let response = result.content[0]
        .as_text()
        .expect("report text")
        .text
        .as_str();
    assert!(response.starts_with("Comparison: equivalent"), "{response}");
    assert!(output_directory.join("report.json").is_file());

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn mcp_rejects_paths_outside_workspace_roots() -> Result<(), Box<dyn std::error::Error>> {
    let root = TestDirectory::new();
    let outside = TestDirectory::new();
    let actual_path = root.path().join("actual.png");
    let expected_path = outside.path().join("expected.png");
    RgbaImage::new(2, 2).save(&actual_path)?;
    RgbaImage::new(2, 2).save(&expected_path)?;

    let client = mcp_client(root.path()).await?;
    let result = client
        .call_tool(
            CallToolRequestParams::new("compare_ui_images").with_arguments(json_object(
                &serde_json::json!({
                    "expected_path": expected_path,
                    "actual_path": actual_path,
                    "output_dir": root.path().join("output"),
                }),
            )),
        )
        .await?;

    assert_eq!(result.is_error, Some(true));
    assert!(
        result.content[0]
            .as_text()
            .expect("error text")
            .text
            .contains("outside the MCP workspace roots")
    );
    client.cancel().await?;
    Ok(())
}

fn json_object(value: &serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    value.as_object().expect("JSON object").clone()
}

#[test]
fn missing_arguments_use_the_documented_error_exit_code() {
    let output = command().output().expect("run CLI");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}

#[test]
fn comparison_region_flags_are_all_or_none() {
    let output = command()
        .args([
            "expected.png",
            "actual.png",
            "--output-dir",
            "output",
            "--region-x",
            "4",
        ])
        .output()
        .expect("run CLI");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--region-y"));
}

#[test]
fn masked_cli_comparison_ignores_outside_changes_and_marks_full_size_artifacts() {
    let directory = TestDirectory::new();
    let expected_path = directory.path().join("expected.png");
    let actual_path = directory.path().join("actual.png");
    let output_directory = directory.path().join("output");
    let expected = RgbaImage::from_pixel(30, 20, Rgba([245, 245, 245, 255]));
    let mut actual = expected.clone();
    for y in 1..5 {
        for x in 2..8 {
            actual.put_pixel(x, y, Rgba([20, 30, 40, 255]));
        }
    }
    expected.save(&expected_path).expect("save expected");
    actual.save(&actual_path).expect("save actual");

    let output = command()
        .args([expected_path.as_os_str(), actual_path.as_os_str()])
        .arg("--output-dir")
        .arg(&output_directory)
        .args([
            "--region-x",
            "10",
            "--region-y",
            "6",
            "--region-width",
            "12",
            "--region-height",
            "10",
        ])
        .output()
        .expect("run masked CLI comparison");

    assert_eq!(output.status.code(), Some(0), "{:?}", output.stderr);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON report");
    assert_eq!(
        report["settings"]["region"],
        serde_json::json!({ "x": 10, "y": 6, "width": 12, "height": 10 })
    );
    assert_eq!(report["metrics"]["raw"]["compared_pixels"], 120);
    assert!(
        report["differences"]
            .as_array()
            .expect("differences")
            .is_empty()
    );

    for name in ["expected.png", "actual.png", "diff.png"] {
        let artifact = image::open(output_directory.join(name))
            .expect("open masked artifact")
            .to_rgba8();
        assert_eq!(artifact.dimensions(), (30, 20));
        assert_eq!(*artifact.get_pixel(10, 6), Rgba([0, 180, 210, 255]));
    }
    let diff = image::open(output_directory.join("diff.png"))
        .expect("open diff")
        .to_rgba8();
    assert_eq!(*diff.get_pixel(0, 0), Rgba([255, 255, 255, 255]));
}

#[test]
fn an_out_of_bounds_cli_region_fails_without_artifacts() {
    let directory = TestDirectory::new();
    let expected_path = directory.path().join("expected.png");
    let actual_path = directory.path().join("actual.png");
    let output_directory = directory.path().join("output");
    let image = RgbaImage::new(10, 10);
    image.save(&expected_path).expect("save expected");
    image.save(&actual_path).expect("save actual");

    let output = command()
        .args([expected_path.as_os_str(), actual_path.as_os_str()])
        .arg("--output-dir")
        .arg(&output_directory)
        .args([
            "--region-x",
            "5",
            "--region-y",
            "5",
            "--region-width",
            "6",
            "--region-height",
            "5",
        ])
        .output()
        .expect("run invalid masked comparison");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("must fit inside"));
    assert!(!output_directory.join("report.json").exists());
}

#[test]
fn identical_pngs_emit_json_and_four_artifacts() {
    let directory = TestDirectory::new();
    let expected = directory.path().join("reference.png");
    let actual = directory.path().join("implementation.png");
    let output_directory = directory.path().join("output");
    let image = RgbaImage::from_pixel(8, 6, Rgba([20, 30, 40, 255]));
    image.save(&expected).expect("save expected");
    image.save(&actual).expect("save actual");

    let output = command()
        .args([expected.as_os_str(), actual.as_os_str()])
        .arg("--output-dir")
        .arg(&output_directory)
        .output()
        .expect("run CLI");

    assert_eq!(output.status.code(), Some(0), "{:?}", output.stderr);
    assert!(output.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON report");
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["equivalent"], true);
    assert_eq!(report["settings"]["max_offset"], 128);
    assert_eq!(report["settings"]["color_threshold"], 2.3);
    assert_eq!(report["settings"]["min_region_area"], 16);
    assert!(report["settings"].get("region").is_none());
    assert_eq!(report["suppression"]["movement_border_regions"], 0);
    assert_eq!(report["suppression"]["movement_border_pixels"], 0);
    assert!(report["suppression"].get("message").is_none());
    assert_eq!(
        fs::read(output_directory.join("report.json")).expect("read JSON report"),
        output.stdout
    );

    for name in ["expected.png", "actual.png", "diff.png"] {
        let artifact = output_directory.join(name);
        assert!(artifact.is_file(), "missing {}", artifact.display());
        let decoded = image::ImageReader::open(&artifact)
            .expect("open artifact")
            .with_guessed_format()
            .expect("guess artifact");
        assert_eq!(decoded.format(), Some(ImageFormat::Png));
        assert_eq!(
            decoded
                .decode()
                .expect("decode artifact")
                .to_rgba8()
                .dimensions(),
            (8, 6)
        );
        assert_eq!(
            report["artifacts"][name.trim_end_matches(".png")],
            artifact.to_string_lossy().as_ref()
        );
    }
}

#[test]
fn generated_dashboard_card_shift_is_reported_structurally() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/deterministic-ui/expected.png");
    let dashboard = image::open(&fixture)
        .unwrap_or_else(|error| panic!("open {}: {error}", fixture.display()))
        .to_rgba8();
    let expected = image::imageops::crop_imm(&dashboard, 260, 90, 800, 360).to_image();
    let mut actual = expected.clone();
    let card = image::imageops::crop_imm(&expected, 410, 32, 364, 285).to_image();
    for y in 32..317 {
        let background = *expected.get_pixel(400, y);
        for x in 410..774 {
            actual.put_pixel(x, y, background);
        }
    }
    image::imageops::overlay(&mut actual, &card, 410, 44);
    let directory = TestDirectory::new();
    let expected_path = directory.path().join("dashboard-expected.png");
    let actual_path = directory.path().join("dashboard-actual.png");
    expected.save(&expected_path).expect("save expected crop");
    actual.save(&actual_path).expect("save actual crop");

    let output = command()
        .args([expected_path.as_os_str(), actual_path.as_os_str()])
        .arg("--output-dir")
        .arg(directory.path().join("output"))
        .output()
        .expect("run CLI");

    assert_eq!(output.status.code(), Some(1), "{:?}", output.stderr);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON report");
    assert!(
        report["differences"]
            .as_array()
            .expect("differences")
            .iter()
            .any(|finding| {
                finding["kind"] == "moved"
                    && finding["offset"]["x"] == 0
                    && finding["offset"]["y"] == 12
            }),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        report["metrics"]["structural_aligned"]["mae"]
            .as_f64()
            .expect("structural MAE")
            < report["metrics"]["global_aligned"]["mae"]
                .as_f64()
                .expect("global MAE")
    );
}

#[test]
fn realistic_comparison_is_identical_across_rayon_thread_counts() {
    let fixture_directory =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/deterministic-ui");
    let expected_fixture = image::open(fixture_directory.join("expected.png"))
        .expect("open deterministic expected fixture")
        .to_rgba8();
    let actual_fixture = image::open(fixture_directory.join("actual.png"))
        .expect("open deterministic actual fixture")
        .to_rgba8();
    let expected = image::imageops::crop_imm(&expected_fixture, 640, 90, 430, 410).to_image();
    let actual = image::imageops::crop_imm(&actual_fixture, 640, 90, 430, 410).to_image();
    let options = kineprism_core::CompareOptions::default();
    let serial_pool = ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .expect("build serial Rayon pool");
    let parallel_pool = ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .expect("build parallel Rayon pool");

    for (name, comparison_actual) in [("identical", &expected), ("changed", &actual)] {
        let serial = serial_pool
            .install(|| kineprism_core::compare(&expected, comparison_actual, &options))
            .unwrap_or_else(|error| panic!("serial {name} comparison: {error}"));
        let parallel = parallel_pool
            .install(|| kineprism_core::compare(&expected, comparison_actual, &options))
            .unwrap_or_else(|error| panic!("parallel {name} comparison: {error}"));

        assert_eq!(
            serial, parallel,
            "{name} comparison changed with thread count"
        );
        assert_eq!(serial.equivalent, name == "identical");
    }
}

#[test]
fn meaningful_differences_exit_one() {
    let directory = TestDirectory::new();
    let expected_path = directory.path().join("expected.png");
    let actual_path = directory.path().join("actual.png");
    let expected = RgbaImage::from_pixel(8, 8, Rgba([255, 255, 255, 255]));
    let mut actual = expected.clone();
    for y in 2..6 {
        for x in 2..6 {
            actual.put_pixel(x, y, Rgba([0, 0, 0, 255]));
        }
    }
    expected.save(&expected_path).expect("save expected");
    actual.save(&actual_path).expect("save actual");

    let output = command()
        .args([expected_path.as_os_str(), actual_path.as_os_str()])
        .arg("--output-dir")
        .arg(directory.path().join("output"))
        .output()
        .expect("run CLI");

    assert_eq!(output.status.code(), Some(1), "{:?}", output.stderr);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON report");
    assert_eq!(report["equivalent"], false);
    assert!(report["summary"]["total"].as_u64().unwrap() >= 1);
    let difference = &report["differences"][0];
    let id = difference["id"].as_str().expect("finding ID");
    assert!(
        difference["message"]
            .as_str()
            .expect("message")
            .contains(id)
    );
    assert!(difference.get("expected_bounds").is_none());
    assert!(difference.get("offset").is_none());
}

#[test]
fn invalid_options_fail_before_input_io() {
    let directory = TestDirectory::new();
    let output = command()
        .args(["missing-expected.png", "missing-actual.png"])
        .arg("--output-dir")
        .arg(directory.path())
        .args(["--min-region-area", "0"])
        .output()
        .expect("run CLI");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("minimum region area"));
}

#[test]
fn missing_non_png_and_corrupt_png_inputs_exit_two() {
    let directory = TestDirectory::new();
    let missing = command()
        .args(["missing-expected.png", "missing-actual.png"])
        .arg("--output-dir")
        .arg(directory.path())
        .output()
        .expect("run CLI");
    assert_eq!(missing.status.code(), Some(2));
    assert!(missing.stdout.is_empty());

    let non_png = directory.path().join("not-an-image.gif");
    let valid = directory.path().join("valid.png");
    fs::write(&non_png, b"GIF89a").expect("write non-PNG input");
    RgbaImage::new(1, 1).save(&valid).expect("save valid input");
    let non_png_result = command()
        .args([non_png.as_os_str(), valid.as_os_str()])
        .arg("--output-dir")
        .arg(directory.path().join("non-png-output"))
        .output()
        .expect("run CLI");
    assert_eq!(non_png_result.status.code(), Some(2));
    assert!(non_png_result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&non_png_result.stderr).contains("not a PNG"));

    let corrupt = directory.path().join("corrupt.png");
    fs::write(&corrupt, b"\x89PNG\r\n\x1a\ntruncated").expect("write corrupt PNG");
    let corrupt_result = command()
        .args([corrupt.as_os_str(), valid.as_os_str()])
        .arg("--output-dir")
        .arg(directory.path().join("corrupt-output"))
        .output()
        .expect("run CLI");
    assert_eq!(corrupt_result.status.code(), Some(2));
    assert!(corrupt_result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&corrupt_result.stderr).contains("decode PNG"));
}

#[test]
fn invalid_output_directory_exits_two_without_json() {
    let directory = TestDirectory::new();
    let expected = directory.path().join("expected.png");
    let actual = directory.path().join("actual.png");
    let blocked_output = directory.path().join("output-is-a-file");
    let image = RgbaImage::from_pixel(2, 2, Rgba([20, 30, 40, 255]));
    image.save(&expected).expect("save expected");
    image.save(&actual).expect("save actual");
    fs::write(&blocked_output, b"not a directory").expect("create blocking file");

    let output = command()
        .args([expected.as_os_str(), actual.as_os_str()])
        .arg("--output-dir")
        .arg(&blocked_output)
        .output()
        .expect("run CLI");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("create output directory"));
}

#[test]
fn force_does_not_overwrite_an_input_reached_through_a_path_alias() {
    let directory = TestDirectory::new();
    let subdirectory = directory.path().join("sub");
    fs::create_dir(&subdirectory).expect("create alias directory");
    let expected = directory.path().join("expected.png");
    let actual = directory.path().join("source-actual.png");
    let pixels = RgbaImage::from_pixel(3, 2, Rgba([12, 34, 56, 255]));
    pixels.save(&expected).expect("save expected");
    pixels.save(&actual).expect("save actual");
    let original = fs::read(&expected).expect("read original");
    let aliased_expected = subdirectory.join("..").join("expected.png");

    let output = command()
        .args([aliased_expected.as_os_str(), actual.as_os_str()])
        .arg("--output-dir")
        .arg(directory.path())
        .arg("--force")
        .output()
        .expect("run CLI");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("overwrite an input"));
    assert_eq!(
        fs::read(expected).expect("read expected after run"),
        original
    );
}

#[test]
fn failed_forced_replacement_cleans_up_temporary_artifacts() {
    let directory = TestDirectory::new();
    let expected = directory.path().join("source-expected.png");
    let actual = directory.path().join("source-actual.png");
    let output_directory = directory.path().join("output");
    fs::create_dir(&output_directory).expect("create output directory");
    let prior_expected = b"prior expected artifact";
    fs::write(output_directory.join("expected.png"), prior_expected).expect("write prior artifact");
    fs::create_dir(output_directory.join("report.json")).expect("create blocking directory");
    let pixels = RgbaImage::from_pixel(3, 2, Rgba([12, 34, 56, 255]));
    pixels.save(&expected).expect("save expected");
    pixels.save(&actual).expect("save actual");

    let output = command()
        .args([expected.as_os_str(), actual.as_os_str()])
        .arg("--output-dir")
        .arg(&output_directory)
        .arg("--force")
        .output()
        .expect("run CLI");

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        fs::read(output_directory.join("expected.png")).expect("restored prior artifact"),
        prior_expected
    );
    let leftovers: Vec<_> = fs::read_dir(&output_directory)
        .expect("read output directory")
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with('.') && (name.ends_with(".tmp") || name.ends_with(".bak"))
        })
        .collect();
    assert!(leftovers.is_empty(), "transaction files left behind");
}

#[test]
fn force_replaces_only_known_artifacts_and_is_deterministic() {
    let directory = TestDirectory::new();
    let expected_path = directory.path().join("source-expected.png");
    let actual_path = directory.path().join("source-actual.png");
    let output_directory = directory.path().join("output");
    let mut expected = RgbaImage::from_pixel(40, 30, Rgba([250, 250, 250, 255]));
    let mut actual = expected.clone();
    for y in 8..20 {
        for x in 10..24 {
            expected.put_pixel(x, y, Rgba([30, 60, 100, 255]));
            actual.put_pixel(x + 4, y, Rgba([30, 60, 100, 255]));
        }
    }
    expected.save(&expected_path).expect("save expected");
    actual.save(&actual_path).expect("save actual");

    let first = command()
        .args([expected_path.as_os_str(), actual_path.as_os_str()])
        .arg("--output-dir")
        .arg(&output_directory)
        .output()
        .expect("first run");
    assert_eq!(first.status.code(), Some(1), "{:?}", first.stderr);
    let first_artifacts: Vec<_> = ["expected.png", "actual.png", "diff.png", "report.json"]
        .into_iter()
        .map(|name| fs::read(output_directory.join(name)).expect("read first artifact"))
        .collect();
    fs::write(output_directory.join("keep.txt"), b"untouched").expect("write unrelated file");

    let collision = command()
        .args([expected_path.as_os_str(), actual_path.as_os_str()])
        .arg("--output-dir")
        .arg(&output_directory)
        .output()
        .expect("collision run");
    assert_eq!(collision.status.code(), Some(2));

    let forced = command()
        .args([expected_path.as_os_str(), actual_path.as_os_str()])
        .arg("--output-dir")
        .arg(&output_directory)
        .arg("--force")
        .output()
        .expect("forced run");
    assert_eq!(forced.status.code(), Some(1), "{:?}", forced.stderr);
    assert_eq!(first.stdout, forced.stdout);
    assert_eq!(
        fs::read(output_directory.join("keep.txt")).expect("read unrelated file"),
        b"untouched"
    );
    for (index, name) in ["expected.png", "actual.png", "diff.png", "report.json"]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            fs::read(output_directory.join(name)).expect("read replaced artifact"),
            first_artifacts[index]
        );
    }
}
