use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use image::{ImageFormat, Rgba, RgbaImage};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "better-image-diff-test-{}-{sequence}",
            std::process::id()
        ));
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
    Command::new(env!("CARGO_BIN_EXE_better-image-diff"))
}

#[test]
fn help_describes_the_public_arguments() {
    let output = command().arg("--help").output().expect("run CLI");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 help");
    assert!(stdout.contains("<EXPECTED>"));
    assert!(stdout.contains("<ACTUAL>"));
    assert!(stdout.contains("--output-dir"));
    assert!(stdout.contains("--max-offset"));
    assert!(stdout.contains("--color-threshold"));
    assert!(stdout.contains("--min-region-area"));
    assert!(stdout.contains("--force"));
}

#[test]
fn missing_arguments_use_the_documented_error_exit_code() {
    let output = command().output().expect("run CLI");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
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
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/realistic-ui/expected.png");
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
