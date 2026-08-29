use better_image_diff_core::{CompareOptions, DifferenceKind, compare, render_artifacts};
use image::{Rgba, RgbaImage};

fn fill_rect(image: &mut RgbaImage, x: u32, y: u32, width: u32, height: u32, color: Rgba<u8>) {
    for pixel_y in y..y + height {
        for pixel_x in x..x + width {
            image.put_pixel(pixel_x, pixel_y, color);
        }
    }
}

fn blended(source: Rgba<u8>, overlay: Rgba<u8>) -> Rgba<u8> {
    let alpha = u16::from(overlay[3]);
    let inverse = 255 - alpha;
    let mut result = source;
    for channel in 0..3 {
        let value = u16::from(overlay[channel]) * alpha + u16::from(source[channel]) * inverse;
        result[channel] = u8::try_from((value + 127) / 255).expect("blended channel");
    }
    result[3] = 255;
    result
}

#[test]
fn movement_artifacts_share_blue_annotations_and_are_deterministic() {
    let background = Rgba([245, 247, 250, 255]);
    let mut expected = RgbaImage::from_pixel(100, 70, background);
    let mut actual = expected.clone();
    fill_rect(&mut expected, 20, 24, 36, 22, Rgba([40, 60, 90, 255]));
    fill_rect(&mut actual, 25, 24, 36, 22, Rgba([40, 60, 90, 255]));
    let comparison = compare(&expected, &actual, &CompareOptions::default()).expect("compare");
    assert!(
        comparison
            .differences
            .iter()
            .any(|difference| { difference.kind == DifferenceKind::Moved })
    );

    let first = render_artifacts(&expected, &actual, &comparison).expect("render");
    let second = render_artifacts(&expected, &actual, &comparison).expect("render again");

    assert_eq!(first, second);
    assert_ne!(first.expected, expected);
    assert_ne!(first.actual, actual);
    assert_eq!(first.diff.dimensions(), (100, 70));
    assert!(
        first
            .diff
            .pixels()
            .any(|pixel| pixel.0 == [32, 115, 230, 255])
    );
    let movement = comparison
        .differences
        .iter()
        .find(|difference| difference.kind == DifferenceKind::Moved)
        .expect("movement");
    assert_eq!(movement.id, "D1");
    for (artifact, source, bounds) in [
        (
            &first.expected,
            &expected,
            movement.expected_bounds.unwrap(),
        ),
        (&first.actual, &actual, movement.actual_bounds.unwrap()),
    ] {
        let label = (bounds.x + 2, bounds.y - 8);
        let blue = Rgba([32, 115, 230, 210]);
        assert_eq!(
            *artifact.get_pixel(label.0, label.1),
            blended(*source.get_pixel(label.0, label.1), blue),
            "D glyph should use the movement color"
        );
        assert_eq!(
            *artifact.get_pixel(label.0 + 8, label.1),
            blended(*source.get_pixel(label.0 + 8, label.1), blue),
            "1 glyph should correlate the D1 ID"
        );
    }
}

#[test]
fn changed_regions_render_a_residual_shape_inside_the_box() {
    let mut expected = RgbaImage::from_pixel(40, 32, Rgba([255, 255, 255, 255]));
    let mut actual = expected.clone();
    fill_rect(&mut expected, 12, 10, 8, 8, Rgba([30, 80, 180, 255]));
    fill_rect(&mut actual, 12, 10, 8, 8, Rgba([20, 140, 70, 255]));
    let options = CompareOptions {
        min_region_area: 1,
        ..CompareOptions::default()
    };
    let comparison = compare(&expected, &actual, &options).expect("compare");

    let rendered = render_artifacts(&expected, &actual, &comparison).expect("render");
    let center = rendered.diff.get_pixel(15, 13);

    assert!(center[0] > center[1] && center[0] > center[2]);
    assert_ne!(*center, Rgba([255, 255, 255, 255]));
}

#[test]
fn residual_shape_uses_comparison_threshold_and_ignores_hidden_rgb() {
    let mut expected = RgbaImage::from_pixel(30, 30, Rgba([255, 255, 255, 255]));
    let mut actual = expected.clone();
    fill_rect(&mut expected, 8, 8, 14, 14, Rgba([30, 80, 180, 255]));
    fill_rect(&mut actual, 8, 8, 14, 14, Rgba([20, 140, 70, 255]));
    expected.put_pixel(14, 18, Rgba([255, 0, 0, 0]));
    actual.put_pixel(14, 18, Rgba([0, 255, 255, 0]));
    expected.put_pixel(16, 18, Rgba([100, 100, 100, 255]));
    actual.put_pixel(16, 18, Rgba([101, 101, 101, 255]));
    let options = CompareOptions {
        min_region_area: 1,
        ..CompareOptions::default()
    };
    let comparison = compare(&expected, &actual, &options).expect("compare");
    assert!(
        comparison
            .differences
            .iter()
            .any(|difference| difference.kind == DifferenceKind::Changed)
    );

    let rendered = render_artifacts(&expected, &actual, &comparison).expect("render");

    assert_ne!(*rendered.diff.get_pixel(18, 18), Rgba([255, 255, 255, 255]));
    assert_eq!(*rendered.diff.get_pixel(14, 18), Rgba([255, 255, 255, 255]));
    assert_eq!(*rendered.diff.get_pixel(16, 18), Rgba([255, 255, 255, 255]));
}

#[test]
fn renderer_clips_mutated_public_bounds_to_the_image() {
    let expected = RgbaImage::from_pixel(4, 4, Rgba([0, 0, 0, 255]));
    let actual = RgbaImage::from_pixel(4, 4, Rgba([255, 255, 255, 255]));
    let options = CompareOptions {
        min_region_area: 1,
        ..CompareOptions::default()
    };
    let mut comparison = compare(&expected, &actual, &options).expect("compare");
    let difference = comparison
        .differences
        .first_mut()
        .expect("changed difference");
    let unbounded = better_image_diff_core::Bounds {
        x: 0,
        y: 0,
        width: u32::MAX,
        height: u32::MAX,
    };
    difference.expected_bounds = Some(unbounded);
    difference.actual_bounds = Some(unbounded);

    let rendered = render_artifacts(&expected, &actual, &comparison).expect("bounded render");

    assert_eq!(rendered.diff.dimensions(), (4, 4));
}

#[test]
fn differing_canvas_boundaries_are_visible_on_the_maximum_canvas() {
    let expected = RgbaImage::from_pixel(30, 20, Rgba([255, 255, 255, 255]));
    let actual = RgbaImage::from_pixel(36, 24, Rgba([255, 255, 255, 255]));
    let comparison = compare(&expected, &actual, &CompareOptions::default()).expect("compare");

    let rendered = render_artifacts(&expected, &actual, &comparison).expect("render");

    assert_eq!(rendered.diff.dimensions(), (36, 24));
    assert!(
        rendered
            .diff
            .pixels()
            .any(|pixel| pixel.0 == [100, 105, 115, 255])
    );
    assert_eq!(
        *rendered.diff.get_pixel(2, 2),
        Rgba([100, 105, 115, 255]),
        "the canvas finding ID should be drawn inside the boundaries"
    );
}

#[test]
fn masked_comparisons_mark_the_full_size_artifacts_with_a_dashed_cyan_boundary() {
    let expected = RgbaImage::from_pixel(20, 16, Rgba([30, 40, 50, 255]));
    let actual = expected.clone();
    let region = better_image_diff_core::Bounds {
        x: 3,
        y: 4,
        width: 10,
        height: 8,
    };
    let options = CompareOptions {
        region: Some(region),
        ..CompareOptions::default()
    };
    let comparison = compare(&expected, &actual, &options).expect("compare region");

    let rendered = render_artifacts(&expected, &actual, &comparison).expect("render region");
    let cyan = Rgba([0, 180, 210, 255]);
    let source = Rgba([30, 40, 50, 255]);

    assert!(comparison.equivalent);
    assert_eq!(rendered.expected.dimensions(), expected.dimensions());
    assert_eq!(rendered.actual.dimensions(), actual.dimensions());
    assert_eq!(rendered.diff.dimensions(), expected.dimensions());
    for artifact in [&rendered.expected, &rendered.actual, &rendered.diff] {
        assert_eq!(*artifact.get_pixel(region.x, region.y), cyan);
        assert_eq!(*artifact.get_pixel(region.x, region.y + 1), cyan);
    }
    assert_eq!(*rendered.expected.get_pixel(region.x + 4, region.y), source);
    assert_eq!(*rendered.actual.get_pixel(region.x + 4, region.y), source);
    assert_eq!(
        *rendered.diff.get_pixel(region.x + 4, region.y),
        Rgba([255, 255, 255, 255])
    );
    assert_eq!(*rendered.expected.get_pixel(0, 0), source);
    assert_eq!(*rendered.actual.get_pixel(0, 0), source);
    assert_eq!(*rendered.diff.get_pixel(0, 0), Rgba([255, 255, 255, 255]));
}
