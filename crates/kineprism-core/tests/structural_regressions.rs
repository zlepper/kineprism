use image::{Rgba, RgbaImage};
use kineprism_core::{CompareOptions, DifferenceKind, Offset, compare};

const BACKGROUND: Rgba<u8> = Rgba([242, 244, 248, 255]);

fn canvas(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_pixel(width, height, BACKGROUND)
}

fn fill_rect(image: &mut RgbaImage, x: u32, y: u32, width: u32, height: u32, color: Rgba<u8>) {
    for pixel_y in y..(y + height).min(image.height()) {
        for pixel_x in x..(x + width).min(image.width()) {
            image.put_pixel(pixel_x, pixel_y, color);
        }
    }
}

fn card(image: &mut RgbaImage, x: u32, y: u32, width: u32, height: u32, fill: Rgba<u8>) {
    fill_rect(image, x, y, width, height, Rgba([35, 44, 62, 255]));
    fill_rect(image, x + 1, y + 1, width - 2, height - 2, fill);
    fill_rect(image, x + 4, y + 5, width - 8, 2, Rgba([75, 88, 112, 255]));
}

#[test]
fn threshold_zero_reports_a_subtle_same_position_change() {
    let mut expected = canvas(90, 60);
    card(&mut expected, 16, 14, 48, 28, Rgba([220, 230, 240, 255]));
    let mut actual = canvas(90, 60);
    card(&mut actual, 16, 14, 48, 28, Rgba([221, 231, 241, 255]));
    let strict = CompareOptions {
        color_threshold: 0.0,
        ..CompareOptions::default()
    };

    let comparison = compare(&expected, &actual, &strict).expect("strict comparison");
    assert!(
        comparison.differences.iter().any(|difference| {
            difference.kind == DifferenceKind::Changed
                && difference.expected_bounds.is_some()
                && difference.actual_bounds.is_some()
        }),
        "{:?}",
        comparison.differences
    );

    let permissive =
        compare(&expected, &actual, &CompareOptions::default()).expect("permissive comparison");
    assert!(permissive.equivalent, "{:?}", permissive.differences);
}

#[test]
fn a_change_after_global_alignment_is_not_swallowed() {
    let mut expected = canvas(150, 90);
    let mut actual = canvas(150, 90);
    card(&mut expected, 12, 12, 42, 24, Rgba([230, 240, 255, 255]));
    card(&mut expected, 82, 50, 44, 25, Rgba([245, 232, 220, 255]));
    card(&mut actual, 18, 16, 42, 24, Rgba([230, 240, 255, 255]));
    card(&mut actual, 88, 54, 44, 25, Rgba([245, 232, 220, 255]));
    fill_rect(&mut actual, 100, 62, 4, 4, Rgba([255, 80, 80, 255]));
    let options = CompareOptions {
        color_threshold: 0.0,
        ..CompareOptions::default()
    };

    let comparison = compare(&expected, &actual, &options).expect("comparison");

    assert_eq!(comparison.alignment.offset, Offset { x: 6, y: 4 });
    assert_eq!(comparison.summary.moved, 1, "{:?}", comparison.differences);
    assert!(
        comparison.differences.iter().any(|difference| {
            difference.kind == DifferenceKind::Changed
                && difference.expected_bounds.is_some()
                && difference.actual_bounds.is_some()
        }),
        "{:?}",
        comparison.differences
    );
}

#[test]
fn a_local_move_is_measured_relative_to_a_large_global_shift() {
    let mut expected = canvas(520, 130);
    let mut actual = canvas(520, 130);
    for (x, color) in [
        (20, Rgba([220, 235, 255, 255])),
        (100, Rgba([235, 220, 255, 255])),
        (180, Rgba([255, 235, 220, 255])),
    ] {
        card(&mut expected, x, 40, 46, 28, color);
        card(&mut actual, x + 125, 40, 46, 28, color);
    }
    card(&mut expected, 260, 40, 46, 28, Rgba([220, 250, 230, 255]));
    card(&mut actual, 395, 40, 46, 28, Rgba([220, 250, 230, 255]));

    let comparison = compare(&expected, &actual, &CompareOptions::default()).expect("comparison");

    assert_eq!(comparison.alignment.offset, Offset { x: 125, y: 0 });
    assert!(
        comparison.differences.iter().any(|difference| {
            difference.kind == DifferenceKind::Moved
                && difference.offset == Some(Offset { x: 135, y: 0 })
        }),
        "{:?}",
        comparison.differences
    );
    assert!(
        comparison
            .metrics
            .structural_aligned
            .mae
            .expect("structural MAE")
            < comparison.metrics.global_aligned.mae.expect("global MAE")
    );
}

#[test]
fn a_large_static_panel_prevents_tiny_icons_from_becoming_global_alignment() {
    let mut expected = canvas(220, 130);
    let mut actual = canvas(220, 130);
    card(&mut expected, 8, 8, 150, 108, Rgba([255, 255, 255, 255]));
    card(&mut actual, 8, 8, 150, 108, Rgba([255, 255, 255, 255]));
    card(&mut expected, 175, 20, 18, 18, Rgba([220, 235, 255, 255]));
    card(&mut actual, 181, 20, 18, 18, Rgba([220, 235, 255, 255]));
    card(&mut expected, 175, 60, 18, 18, Rgba([235, 220, 255, 255]));
    card(&mut actual, 181, 60, 18, 18, Rgba([235, 220, 255, 255]));

    let comparison = compare(&expected, &actual, &CompareOptions::default()).expect("comparison");

    assert_eq!(comparison.alignment.offset, Offset::default());
}

#[test]
fn findings_are_sorted_by_position_before_kind() {
    let mut expected = canvas(120, 100);
    let mut actual = canvas(120, 100);
    card(&mut expected, 10, 8, 36, 22, Rgba([225, 238, 255, 255]));
    card(&mut actual, 10, 8, 36, 22, Rgba([255, 220, 220, 255]));
    card(&mut expected, 25, 60, 42, 24, Rgba([235, 225, 255, 255]));
    card(&mut actual, 31, 60, 42, 24, Rgba([235, 225, 255, 255]));

    let comparison = compare(&expected, &actual, &CompareOptions::default()).expect("comparison");

    assert_eq!(comparison.differences[0].kind, DifferenceKind::Changed);
    assert_eq!(comparison.differences[1].kind, DifferenceKind::Moved);
}

#[test]
fn minimum_region_area_applies_to_connected_noise() {
    let mut expected = canvas(100, 80);
    card(&mut expected, 8, 8, 82, 62, Rgba([230, 238, 248, 255]));
    let mut scattered = expected.clone();
    for row in 0..4 {
        for column in 0..4 {
            scattered.put_pixel(16 + column * 12, 18 + row * 12, Rgba([255, 80, 80, 255]));
        }
    }
    let options = CompareOptions {
        color_threshold: 0.0,
        min_region_area: 16,
        ..CompareOptions::default()
    };

    let ignored = compare(&expected, &scattered, &options).expect("scattered comparison");
    assert!(ignored.equivalent, "{:?}", ignored.differences);

    let mut connected = expected.clone();
    fill_rect(&mut connected, 30, 30, 4, 4, Rgba([255, 80, 80, 255]));
    let reported = compare(&expected, &connected, &options).expect("connected comparison");
    assert_eq!(reported.summary.changed, 1, "{:?}", reported.differences);
}

#[test]
fn full_canvas_texture_global_translation_is_always_reported() {
    let mut expected = RgbaImage::new(96, 64);
    for y in 0..expected.height() {
        for x in 0..expected.width() {
            expected.put_pixel(
                x,
                y,
                Rgba([
                    u8::try_from((x * 7 + y * 3) % 251).expect("red"),
                    u8::try_from((x * 2 + y * 11) % 251).expect("green"),
                    u8::try_from((x * 13 + y * 5) % 251).expect("blue"),
                    255,
                ]),
            );
        }
    }
    let mut actual = RgbaImage::new(96, 64);
    for expected_y in 3..64 {
        for expected_x in 0..91 {
            actual.put_pixel(
                expected_x + 5,
                expected_y - 3,
                *expected.get_pixel(expected_x, expected_y),
            );
        }
    }

    let comparison = compare(&expected, &actual, &CompareOptions::default()).expect("comparison");

    assert_eq!(comparison.alignment.offset, Offset { x: 5, y: -3 });
    assert_eq!(comparison.summary.moved, 1, "{:?}", comparison.differences);
    assert!(
        comparison
            .differences
            .iter()
            .filter(|difference| {
                difference.kind == DifferenceKind::Changed
                    && (difference.expected_bounds.is_none() || difference.actual_bounds.is_none())
            })
            .count()
            >= 4,
        "cropped bands should remain visible: {:?}",
        comparison.differences
    );
    assert!(
        comparison.metrics.global_aligned.mae.expect("aligned MAE")
            < comparison.metrics.raw.mae.expect("raw MAE")
    );
}

#[test]
fn a_gradient_card_moves_on_a_non_flat_background() {
    let background = |width: u32, height: u32| {
        let mut image = RgbaImage::new(width, height);
        for y in 0..height {
            for x in 0..width {
                image.put_pixel(
                    x,
                    y,
                    Rgba([
                        210 + u8::try_from(x % 23).expect("red"),
                        215 + u8::try_from(y % 19).expect("green"),
                        225 + u8::try_from((x + y) % 17).expect("blue"),
                        255,
                    ]),
                );
            }
        }
        image
    };
    let draw_gradient_card = |image: &mut RgbaImage, x: u32| {
        fill_rect(image, x + 3, 24, 52, 34, Rgba([100, 110, 130, 90]));
        for row in 0..30 {
            fill_rect(
                image,
                x,
                20 + row,
                52,
                1,
                Rgba([
                    245 - u8::try_from(row / 3).expect("gradient"),
                    248 - u8::try_from(row / 4).expect("gradient"),
                    252,
                    255,
                ]),
            );
        }
        fill_rect(image, x + 7, 29, 31, 2, Rgba([65, 80, 110, 255]));
        fill_rect(image, x + 7, 37, 23, 2, Rgba([90, 105, 130, 255]));
    };
    let mut expected = background(130, 80);
    let mut actual = background(130, 80);
    draw_gradient_card(&mut expected, 28);
    draw_gradient_card(&mut actual, 33);

    let comparison = compare(&expected, &actual, &CompareOptions::default()).expect("comparison");

    assert_eq!(comparison.alignment.offset, Offset::default());
    assert_eq!(comparison.summary.moved, 1, "{:?}", comparison.differences);
    assert_eq!(
        comparison.differences[0].offset,
        Some(Offset { x: 5, y: 0 })
    );
    assert_eq!(
        comparison.summary.changed, 0,
        "{:?}",
        comparison.differences
    );
}

#[test]
fn distinctive_cards_swapped_in_place_are_two_movements() {
    let mut expected = canvas(150, 75);
    let mut actual = canvas(150, 75);
    card(&mut expected, 18, 24, 42, 25, Rgba([210, 235, 255, 255]));
    card(&mut expected, 90, 24, 42, 25, Rgba([255, 225, 210, 255]));
    card(&mut actual, 18, 24, 42, 25, Rgba([255, 225, 210, 255]));
    card(&mut actual, 90, 24, 42, 25, Rgba([210, 235, 255, 255]));

    let comparison = compare(&expected, &actual, &CompareOptions::default()).expect("comparison");
    let offsets: Vec<_> = comparison
        .differences
        .iter()
        .filter(|difference| difference.kind == DifferenceKind::Moved)
        .filter_map(|difference| difference.offset)
        .collect();

    assert_eq!(comparison.summary.moved, 2, "{:?}", comparison.differences);
    assert!(offsets.contains(&Offset { x: 72, y: 0 }));
    assert!(offsets.contains(&Offset { x: -72, y: 0 }));
    assert_eq!(
        comparison.summary.changed, 0,
        "{:?}",
        comparison.differences
    );
}

#[test]
fn adjacent_icon_fragments_with_one_offset_are_one_movement() {
    let mut expected = canvas(150, 90);
    let mut actual = canvas(150, 90);
    card(&mut expected, 8, 8, 52, 28, Rgba([255, 255, 255, 255]));
    card(&mut actual, 8, 8, 52, 28, Rgba([255, 255, 255, 255]));
    for (x, color) in [
        (75, Rgba([30, 90, 180, 255])),
        (87, Rgba([170, 60, 150, 255])),
        (99, Rgba([40, 150, 100, 255])),
    ] {
        fill_rect(&mut expected, x, 55, 5, 5, color);
        fill_rect(&mut actual, x + 5, 55, 5, 5, color);
    }

    let comparison = compare(&expected, &actual, &CompareOptions::default()).expect("comparison");

    assert_eq!(comparison.alignment.offset, Offset::default());
    assert_eq!(comparison.summary.moved, 1, "{:?}", comparison.differences);
    assert_eq!(
        comparison
            .differences
            .iter()
            .find(|difference| difference.kind == DifferenceKind::Moved)
            .and_then(|difference| difference.offset),
        Some(Offset { x: 5, y: 0 })
    );
}

#[test]
fn identical_dense_component_grids_short_circuit_as_equivalent() {
    let mut image = RgbaImage::from_pixel(64, 64, Rgba([255, 255, 255, 255]));
    for row in 0..23 {
        for column in 0..23 {
            image.put_pixel(2 + column * 2, 2 + row * 2, Rgba([0, 0, 0, 255]));
        }
    }
    let options = CompareOptions {
        min_region_area: 1,
        ..CompareOptions::default()
    };

    let comparison = compare(&image, &image, &options).expect("dense comparison");

    assert!(comparison.equivalent, "{:?}", comparison.differences);
    assert!(comparison.differences.is_empty());
}

#[test]
fn nonidentical_dense_grids_fall_back_to_residual_changes() {
    let mut expected = RgbaImage::from_pixel(64, 64, Rgba([255, 255, 255, 255]));
    for row in 0..23 {
        for column in 0..23 {
            expected.put_pixel(2 + column * 2, 2 + row * 2, Rgba([0, 0, 0, 255]));
        }
    }
    let mut actual = expected.clone();
    actual.put_pixel(24, 24, Rgba([220, 20, 60, 255]));
    let options = CompareOptions {
        min_region_area: 1,
        ..CompareOptions::default()
    };

    let comparison = compare(&expected, &actual, &options).expect("dense comparison");

    assert_eq!(comparison.summary.added, 0, "{:?}", comparison.differences);
    assert_eq!(
        comparison.summary.removed, 0,
        "{:?}",
        comparison.differences
    );
    assert!(
        comparison.summary.changed > 0,
        "{:?}",
        comparison.differences
    );
}

#[test]
fn content_preselection_keeps_a_true_match_that_is_last_spatially() {
    let mut expected = canvas(150, 110);
    let mut actual = canvas(150, 110);
    card(&mut expected, 5, 5, 140, 42, Rgba([255, 255, 255, 255]));
    card(&mut actual, 5, 5, 140, 42, Rgba([255, 255, 255, 255]));
    let colors: Vec<_> = (0..17)
        .map(|index| {
            Rgba([
                20 + u8::try_from(index * 11).expect("red"),
                210 - u8::try_from(index * 7).expect("green"),
                30 + u8::try_from((index * 37) % 200).expect("blue"),
                255,
            ])
        })
        .collect();
    for (index, color) in colors.iter().copied().enumerate() {
        let x = 5 + u32::try_from(index).expect("index") * 7;
        fill_rect(&mut expected, x, 76, 5, 5, color);
        let actual_color = colors[(index + 1) % colors.len()];
        fill_rect(&mut actual, x, 76, 5, 5, actual_color);
    }

    let comparison = compare(&expected, &actual, &CompareOptions::default()).expect("comparison");

    assert_eq!(comparison.alignment.offset, Offset::default());
    assert_eq!(comparison.summary.added, 0, "{:?}", comparison.differences);
    assert_eq!(
        comparison.summary.removed, 0,
        "{:?}",
        comparison.differences
    );
    assert!(
        comparison.differences.iter().any(|difference| {
            difference.kind == DifferenceKind::Moved
                && difference.offset == Some(Offset { x: 112, y: 0 })
        }),
        "{:?}",
        comparison.differences
    );
}
