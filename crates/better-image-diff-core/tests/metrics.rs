use better_image_diff_core::{CompareOptions, compare};
use image::{Rgba, RgbaImage};

fn assert_close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn identical_images_have_perfect_metrics() {
    let image = RgbaImage::from_pixel(3, 2, Rgba([20, 40, 60, 255]));

    let comparison = compare(&image, &image, &CompareOptions::default()).expect("compare");
    let metrics = &comparison.metrics.raw;

    assert_eq!(metrics.compared_pixels, 6);
    assert_close(metrics.expected_coverage, 1.0, f64::EPSILON);
    assert_close(metrics.actual_coverage, 1.0, f64::EPSILON);
    assert_eq!(metrics.mae, Some(0.0));
    assert_eq!(metrics.rmse, Some(0.0));
    assert_eq!(metrics.psnr_db, None);
    assert_eq!(metrics.ssim, Some(1.0));
    assert_eq!(metrics.changed_pixel_ratio, Some(0.0));
}

#[test]
fn black_and_white_have_hand_calculable_error_metrics() {
    let black = RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 255]));
    let white = RgbaImage::from_pixel(1, 1, Rgba([255, 255, 255, 255]));

    let comparison = compare(&black, &white, &CompareOptions::default()).expect("compare");
    let metrics = &comparison.metrics.raw;

    assert_close(metrics.mae.expect("MAE"), 0.75, 1e-12);
    assert_close(metrics.rmse.expect("RMSE"), 0.75_f64.sqrt(), 1e-12);
    assert_close(
        metrics.psnr_db.expect("PSNR"),
        10.0 * (1.0_f64 / 0.75).log10(),
        1e-12,
    );
    assert_eq!(metrics.changed_pixel_ratio, Some(1.0));
    let constant_channel_score = 0.000_1 / 1.000_1;
    assert_close(
        metrics.ssim.expect("SSIM"),
        (3.0 * constant_channel_score + 1.0) / 4.0,
        1e-12,
    );
}

#[test]
fn hidden_rgb_does_not_affect_metrics() {
    let first = RgbaImage::from_pixel(1, 1, Rgba([255, 0, 0, 0]));
    let second = RgbaImage::from_pixel(1, 1, Rgba([0, 255, 255, 0]));

    let comparison = compare(&first, &second, &CompareOptions::default()).expect("compare");
    let metrics = &comparison.metrics.raw;

    assert_eq!(metrics.mae, Some(0.0));
    assert_eq!(metrics.rmse, Some(0.0));
    assert_eq!(metrics.changed_pixel_ratio, Some(0.0));
}

#[test]
fn metrics_report_overlap_coverage_for_different_canvases() {
    let expected = RgbaImage::new(4, 2);
    let actual = RgbaImage::new(2, 2);

    let comparison = compare(&expected, &actual, &CompareOptions::default()).expect("compare");
    let metrics = &comparison.metrics.raw;

    assert_eq!(metrics.compared_pixels, 4);
    assert_close(metrics.expected_coverage, 0.5, f64::EPSILON);
    assert_close(metrics.actual_coverage, 1.0, f64::EPSILON);
}

#[test]
fn changed_pixel_ratio_uses_the_perceptual_threshold() {
    let expected = RgbaImage::from_pixel(1, 1, Rgba([100, 100, 100, 255]));
    let actual = RgbaImage::from_pixel(1, 1, Rgba([101, 101, 101, 255]));
    let permissive = CompareOptions {
        color_threshold: 100.0,
        ..CompareOptions::default()
    };
    let strict = CompareOptions {
        color_threshold: 0.0,
        ..CompareOptions::default()
    };

    let permissive_result = compare(&expected, &actual, &permissive).expect("compare");
    let strict_result = compare(&expected, &actual, &strict).expect("compare");

    assert_eq!(permissive_result.metrics.raw.changed_pixel_ratio, Some(0.0));
    assert_eq!(strict_result.metrics.raw.changed_pixel_ratio, Some(1.0));
}

#[test]
fn alpha_changes_contribute_to_premultiplied_error_metrics() {
    let transparent = RgbaImage::from_pixel(1, 1, Rgba([255, 255, 255, 0]));
    let translucent = RgbaImage::from_pixel(1, 1, Rgba([255, 255, 255, 128]));

    let comparison =
        compare(&transparent, &translucent, &CompareOptions::default()).expect("compare");
    let alpha = 128.0 / 255.0;

    assert_close(comparison.metrics.raw.mae.expect("MAE"), alpha, 1e-7);
    assert_close(comparison.metrics.raw.rmse.expect("RMSE"), alpha, 1e-7);
    assert_eq!(comparison.metrics.raw.changed_pixel_ratio, Some(1.0));
}

#[test]
fn empty_overlap_has_zero_coverage_and_unavailable_scores() {
    let empty = RgbaImage::new(0, 0);
    let actual = RgbaImage::new(2, 2);

    let comparison = compare(&empty, &actual, &CompareOptions::default()).expect("compare");
    let metrics = &comparison.metrics.raw;

    assert_eq!(metrics.compared_pixels, 0);
    assert_close(metrics.expected_coverage, 0.0, f64::EPSILON);
    assert_close(metrics.actual_coverage, 0.0, f64::EPSILON);
    assert!(metrics.mae.is_none());
    assert!(metrics.rmse.is_none());
    assert!(metrics.psnr_db.is_none());
    assert!(metrics.ssim.is_none());
    assert!(metrics.changed_pixel_ratio.is_none());
}

#[test]
fn structural_change_degrades_ssim() {
    let mut expected = RgbaImage::from_pixel(16, 16, Rgba([255, 255, 255, 255]));
    let actual = expected.clone();
    for y in 0..8 {
        for x in 0..8 {
            expected.put_pixel(x, y, Rgba([0, 0, 0, 255]));
        }
    }

    let comparison = compare(&expected, &actual, &CompareOptions::default()).expect("compare");

    assert!(comparison.metrics.raw.ssim.expect("SSIM") < 0.95);
}
