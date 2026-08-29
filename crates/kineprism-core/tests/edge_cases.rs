use image::{Rgba, RgbaImage};
use kineprism_core::{CompareOptions, DifferenceKind, Offset, compare};

const BACKGROUND: Rgba<u8> = Rgba([242, 244, 248, 255]);

fn fill_rect(image: &mut RgbaImage, x: u32, y: u32, width: u32, height: u32, color: Rgba<u8>) {
    for pixel_y in y..(y + height).min(image.height()) {
        for pixel_x in x..(x + width).min(image.width()) {
            image.put_pixel(pixel_x, pixel_y, color);
        }
    }
}

fn patterned_card(image: &mut RgbaImage, x: u32, y: u32, color: Rgba<u8>) {
    fill_rect(image, x, y, 30, 22, Rgba([35, 45, 65, color[3]]));
    fill_rect(image, x + 1, y + 1, 28, 20, color);
    fill_rect(image, x + 5, y + 6, 17, 2, Rgba([20, 90, 160, color[3]]));
    fill_rect(image, x + 5, y + 12, 11, 2, Rgba([180, 70, 40, color[3]]));
}

#[test]
fn partially_transparent_card_can_be_matched_as_a_movement() {
    let mut expected = RgbaImage::from_pixel(110, 75, BACKGROUND);
    let mut actual = expected.clone();
    patterned_card(&mut expected, 8, 8, Rgba([255, 255, 255, 255]));
    patterned_card(&mut actual, 8, 8, Rgba([255, 255, 255, 255]));
    patterned_card(&mut expected, 48, 38, Rgba([190, 220, 255, 140]));
    patterned_card(&mut actual, 55, 38, Rgba([190, 220, 255, 140]));

    let comparison = compare(&expected, &actual, &CompareOptions::default()).expect("compare");

    assert!(
        comparison.differences.iter().any(|difference| {
            difference.kind == DifferenceKind::Moved
                && difference.offset == Some(Offset { x: 7, y: 0 })
        }),
        "{:?}",
        comparison.differences
    );
}

#[test]
fn repeated_flat_regions_do_not_invent_a_movement() {
    let mut expected = RgbaImage::from_pixel(120, 60, BACKGROUND);
    let mut actual = expected.clone();
    for x in [15, 55] {
        fill_rect(&mut expected, x, 20, 20, 20, Rgba([80, 120, 180, 255]));
    }
    for x in [35, 75] {
        fill_rect(&mut actual, x, 20, 20, 20, Rgba([80, 120, 180, 255]));
    }

    let comparison = compare(&expected, &actual, &CompareOptions::default()).expect("compare");

    assert_eq!(comparison.summary.moved, 0, "{:?}", comparison.differences);
    assert!(!comparison.equivalent);
}

#[test]
fn movement_left_and_up_near_the_canvas_edge_has_signed_offset() {
    let mut expected = RgbaImage::from_pixel(100, 70, BACKGROUND);
    let mut actual = expected.clone();
    patterned_card(&mut expected, 55, 38, Rgba([255, 255, 255, 255]));
    patterned_card(&mut actual, 55, 38, Rgba([255, 255, 255, 255]));
    patterned_card(&mut expected, 8, 9, Rgba([220, 235, 255, 255]));
    patterned_card(&mut actual, 2, 4, Rgba([220, 235, 255, 255]));

    let comparison = compare(&expected, &actual, &CompareOptions::default()).expect("compare");

    assert!(
        comparison.differences.iter().any(|difference| {
            difference.kind == DifferenceKind::Moved
                && difference.offset == Some(Offset { x: -6, y: -5 })
        }),
        "{:?}",
        comparison.differences
    );
}

#[test]
fn lowering_minimum_region_area_reveals_small_components() {
    let expected = RgbaImage::from_pixel(40, 30, BACKGROUND);
    let mut actual = expected.clone();
    fill_rect(&mut actual, 20, 15, 3, 3, Rgba([10, 20, 30, 255]));

    let default = compare(&expected, &actual, &CompareOptions::default()).expect("default compare");
    let sensitive = compare(
        &expected,
        &actual,
        &CompareOptions {
            min_region_area: 1,
            ..CompareOptions::default()
        },
    )
    .expect("sensitive compare");

    assert!(default.equivalent, "{:?}", default.differences);
    assert!(!sensitive.equivalent);
}

#[test]
fn cropped_non_background_content_remains_residual_evidence() {
    let mut expected = RgbaImage::from_pixel(100, 60, BACKGROUND);
    let mut actual = RgbaImage::from_pixel(80, 60, BACKGROUND);
    patterned_card(&mut expected, 10, 10, Rgba([255, 255, 255, 255]));
    patterned_card(&mut actual, 10, 10, Rgba([255, 255, 255, 255]));
    fill_rect(&mut expected, 86, 20, 10, 20, Rgba([40, 110, 190, 255]));

    let comparison = compare(&expected, &actual, &CompareOptions::default()).expect("compare");

    assert!(
        comparison.differences.iter().any(|difference| {
            difference
                .expected_bounds
                .is_some_and(|bounds| bounds.x >= 80)
                && difference.actual_bounds.is_none()
        }),
        "{:?}",
        comparison.differences
    );
}
