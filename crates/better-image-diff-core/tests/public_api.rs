use better_image_diff_core::{
    CompareError, CompareOptions, DifferenceKind, Offset, RenderError, compare, render_artifacts,
};
use image::{Rgba, RgbaImage};

#[test]
fn default_options_match_the_cli_contract() {
    let options = CompareOptions::default();

    assert_eq!(options.max_offset, 128);
    assert!((options.color_threshold - 2.3).abs() < f64::EPSILON);
    assert_eq!(options.min_region_area, 16);
    assert_eq!(options.region, None);
}

#[test]
fn report_domain_types_are_straightforward_to_consume() {
    let offset = Offset { x: 5, y: -2 };

    assert_eq!(offset.x, 5);
    assert_eq!(offset.y, -2);
    assert_eq!(DifferenceKind::Moved.to_string(), "moved");
}

#[test]
fn identical_in_memory_images_can_be_compared_and_rendered() {
    let image = RgbaImage::from_pixel(4, 3, Rgba([20, 30, 40, 255]));

    let comparison = compare(&image, &image, &CompareOptions::default()).expect("compare");
    assert!(comparison.equivalent);
    assert!(comparison.differences.is_empty());
    assert_eq!(comparison.suppression.movement_border_regions, 0);
    assert_eq!(comparison.suppression.movement_border_pixels, 0);
    assert!(comparison.suppression.message.is_none());

    let rendered = render_artifacts(&image, &image, &comparison).expect("render");
    assert_eq!(rendered.expected.dimensions(), (4, 3));
    assert_eq!(rendered.actual.dimensions(), (4, 3));
    assert_eq!(rendered.diff.dimensions(), (4, 3));
}

#[test]
fn invalid_options_are_rejected_by_the_library() {
    let image = RgbaImage::new(1, 1);

    let zero_area = CompareOptions {
        min_region_area: 0,
        ..CompareOptions::default()
    };
    assert_eq!(
        compare(&image, &image, &zero_area),
        Err(CompareError::InvalidMinimumRegionArea(0))
    );

    let nan_threshold = CompareOptions {
        color_threshold: f64::NAN,
        ..CompareOptions::default()
    };
    assert!(matches!(
        compare(&image, &image, &nan_threshold),
        Err(CompareError::InvalidColorThreshold(value)) if value.is_nan()
    ));
}

#[test]
fn a_different_canvas_and_pixels_are_summarized() {
    let expected = RgbaImage::from_pixel(4, 3, Rgba([10, 20, 30, 255]));
    let actual = RgbaImage::from_pixel(5, 3, Rgba([30, 20, 10, 255]));

    let options = CompareOptions {
        min_region_area: 1,
        ..CompareOptions::default()
    };
    let comparison = compare(&expected, &actual, &options).expect("compare");

    assert!(!comparison.equivalent);
    assert_eq!(comparison.summary.total, 3);
    assert_eq!(comparison.summary.canvas_size, 1);
    assert_eq!(comparison.summary.changed, 2);
    assert!(comparison.differences.iter().any(|difference| {
        difference.expected_bounds.is_none()
            && difference
                .actual_bounds
                .is_some_and(|bounds| bounds.x == 4 && bounds.width == 1)
    }));
    assert!(comparison.metrics.raw.mae.expect("MAE") > 0.0);
    assert!(comparison.metrics.raw.psnr_db.is_some());
}

#[test]
fn rendering_rejects_images_that_do_not_belong_to_the_comparison() {
    let expected = RgbaImage::new(4, 3);
    let actual = RgbaImage::new(4, 3);
    let comparison = compare(&expected, &actual, &CompareOptions::default()).expect("compare");
    let wrong_actual = RgbaImage::new(5, 3);

    assert_eq!(
        render_artifacts(&expected, &wrong_actual, &comparison),
        Err(RenderError::ComparisonImageMismatch)
    );
}

#[test]
fn rendering_rejects_an_excessive_combined_canvas() {
    let expected = RgbaImage::new(20_000, 1);
    let actual = RgbaImage::new(1, 20_000);
    let mut comparison = compare(&expected, &actual, &CompareOptions::default()).expect("compare");
    comparison.actual.width = actual.width();
    comparison.actual.height = actual.height();

    assert_eq!(
        render_artifacts(&expected, &actual, &comparison),
        Err(RenderError::ImageTooLarge)
    );
}

#[test]
fn alignment_confidence_is_not_derived_from_filtered_findings() {
    let expected = RgbaImage::from_pixel(5, 5, Rgba([255, 255, 255, 255]));
    let mut noisy = expected.clone();
    noisy.put_pixel(2, 2, Rgba([0, 0, 0, 255]));

    let ignored_noise = compare(&expected, &noisy, &CompareOptions::default()).expect("compare");
    assert!(ignored_noise.equivalent);
    assert!(ignored_noise.alignment.confidence.abs() < f64::EPSILON);

    let wider = RgbaImage::from_pixel(6, 5, Rgba([255, 255, 255, 255]));
    let canvas_only = compare(&expected, &wider, &CompareOptions::default()).expect("compare");
    assert!(!canvas_only.equivalent);
    assert!((canvas_only.alignment.confidence - 1.0).abs() < f64::EPSILON);
}
