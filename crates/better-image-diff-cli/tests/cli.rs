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
fn identical_pngs_emit_json_and_three_artifacts() {
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

    for name in ["expected.png", "actual.png", "diff.png"] {
        let artifact = output_directory.join(name);
        assert!(artifact.is_file(), "missing {}", artifact.display());
        assert_eq!(
            image::ImageReader::open(artifact)
                .expect("open artifact")
                .with_guessed_format()
                .expect("guess artifact")
                .format(),
            Some(ImageFormat::Png)
        );
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
    assert!(report["summary"]["changed"].as_u64().unwrap() >= 1);
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
fn missing_and_corrupt_inputs_exit_two() {
    let directory = TestDirectory::new();
    let missing = command()
        .args(["missing-expected.png", "missing-actual.png"])
        .arg("--output-dir")
        .arg(directory.path())
        .output()
        .expect("run CLI");
    assert_eq!(missing.status.code(), Some(2));
    assert!(missing.stdout.is_empty());

    let corrupt = directory.path().join("corrupt.png");
    let valid = directory.path().join("valid.png");
    fs::write(&corrupt, b"not a PNG").expect("write corrupt input");
    RgbaImage::new(1, 1).save(&valid).expect("save valid input");
    let corrupt_result = command()
        .args([corrupt.as_os_str(), valid.as_os_str()])
        .arg("--output-dir")
        .arg(directory.path().join("corrupt-output"))
        .output()
        .expect("run CLI");
    assert_eq!(corrupt_result.status.code(), Some(2));
    assert!(corrupt_result.stdout.is_empty());
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
    fs::create_dir(output_directory.join("expected.png")).expect("create blocking directory");
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
    let leftovers: Vec<_> = fs::read_dir(&output_directory)
        .expect("read output directory")
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with('.') && name.ends_with(".tmp")
        })
        .collect();
    assert!(leftovers.is_empty(), "temporary files left behind");
}
