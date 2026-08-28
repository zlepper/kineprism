use better_image_diff_core::{CompareOptions, DifferenceKind, Offset, compare};
use image::{Rgba, RgbaImage};

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

fn draw_card(image: &mut RgbaImage, x: u32, y: u32, width: u32, height: u32, fill: Rgba<u8>) {
    fill_rect(image, x, y, width, height, Rgba([40, 48, 64, 255]));
    if width > 2 && height > 2 {
        fill_rect(image, x + 1, y + 1, width - 2, height - 2, fill);
    }
    let line_width = width.saturating_sub(8).max(1);
    for line_y in [y + 5, y + 9] {
        if line_y < y + height.saturating_sub(2) {
            fill_rect(
                image,
                x + 4,
                line_y,
                line_width,
                1,
                Rgba([70, 82, 104, 255]),
            );
        }
    }
}

fn findings_of_kind(
    comparison: &better_image_diff_core::Comparison,
    kind: DifferenceKind,
) -> Vec<&better_image_diff_core::Difference> {
    comparison
        .differences
        .iter()
        .filter(|difference| difference.kind == kind)
        .collect()
}

#[test]
fn a_card_shifted_five_pixels_is_one_movement() {
    let mut expected = canvas(120, 80);
    let mut actual = canvas(120, 80);
    let card_fill = Rgba([255, 255, 255, 255]);
    draw_card(&mut expected, 8, 8, 28, 18, card_fill);
    draw_card(&mut actual, 8, 8, 28, 18, card_fill);
    draw_card(&mut expected, 48, 36, 42, 24, card_fill);
    draw_card(&mut actual, 53, 36, 42, 24, card_fill);

    let comparison = compare(&expected, &actual, &CompareOptions::default()).expect("compare");
    let moved = findings_of_kind(&comparison, DifferenceKind::Moved);

    assert_eq!(moved.len(), 1, "{:?}", comparison.differences);
    assert_eq!(moved[0].offset, Some(Offset { x: 5, y: 0 }));
    assert_eq!(comparison.summary.changed, 0);
    assert_eq!(comparison.summary.added, 0);
    assert_eq!(comparison.summary.removed, 0);
    assert!(
        comparison
            .metrics
            .structural_aligned
            .mae
            .expect("structural MAE")
            < comparison.metrics.raw.mae.expect("raw MAE")
    );
}

#[test]
fn independent_card_movements_remain_separate() {
    let mut expected = canvas(140, 100);
    let mut actual = canvas(140, 100);
    draw_card(&mut expected, 5, 5, 25, 18, Rgba([255, 255, 255, 255]));
    draw_card(&mut actual, 5, 5, 25, 18, Rgba([255, 255, 255, 255]));
    draw_card(&mut expected, 45, 25, 30, 20, Rgba([220, 235, 255, 255]));
    draw_card(&mut actual, 50, 25, 30, 20, Rgba([220, 235, 255, 255]));
    draw_card(&mut expected, 90, 65, 32, 22, Rgba([235, 225, 255, 255]));
    draw_card(&mut actual, 90, 73, 32, 22, Rgba([235, 225, 255, 255]));

    let comparison = compare(&expected, &actual, &CompareOptions::default()).expect("compare");
    let moved = findings_of_kind(&comparison, DifferenceKind::Moved);
    let offsets: Vec<_> = moved.iter().map(|difference| difference.offset).collect();

    assert_eq!(moved.len(), 2, "{:?}", comparison.differences);
    assert!(offsets.contains(&Some(Offset { x: 5, y: 0 })));
    assert!(offsets.contains(&Some(Offset { x: 0, y: 8 })));
}

#[test]
fn whole_layout_translation_is_reported_across_different_canvases() {
    let mut expected = canvas(90, 65);
    let mut actual = canvas(110, 80);
    draw_card(&mut expected, 8, 7, 28, 18, Rgba([255, 255, 255, 255]));
    draw_card(&mut expected, 47, 35, 32, 20, Rgba([225, 238, 255, 255]));
    draw_card(&mut actual, 14, 11, 28, 18, Rgba([255, 255, 255, 255]));
    draw_card(&mut actual, 53, 39, 32, 20, Rgba([225, 238, 255, 255]));

    let comparison = compare(&expected, &actual, &CompareOptions::default()).expect("compare");

    assert_eq!(comparison.alignment.offset, Offset { x: 6, y: 4 });
    assert_eq!(comparison.summary.canvas_size, 1);
    assert_eq!(comparison.summary.moved, 1);
    assert!(
        comparison.metrics.global_aligned.mae.expect("aligned MAE")
            < comparison.metrics.raw.mae.expect("raw MAE")
    );
}

#[test]
fn movement_respects_the_configured_search_limit() {
    let mut expected = canvas(100, 70);
    let mut actual = canvas(100, 70);
    draw_card(&mut expected, 5, 5, 25, 18, Rgba([255, 255, 255, 255]));
    draw_card(&mut actual, 5, 5, 25, 18, Rgba([255, 255, 255, 255]));
    draw_card(&mut expected, 40, 35, 28, 20, Rgba([225, 238, 255, 255]));
    draw_card(&mut actual, 52, 35, 28, 20, Rgba([225, 238, 255, 255]));
    let limited = CompareOptions {
        max_offset: 5,
        ..CompareOptions::default()
    };

    let comparison = compare(&expected, &actual, &limited).expect("compare");

    assert!(
        findings_of_kind(&comparison, DifferenceKind::Moved).is_empty(),
        "alignment={:?}, differences={:?}",
        comparison.alignment,
        comparison.differences
    );
    assert!(comparison.summary.changed + comparison.summary.added + comparison.summary.removed > 0);
}

#[test]
fn resized_added_removed_and_changed_cards_are_classified() {
    let mut resize_expected = canvas(110, 75);
    let mut resize_actual = canvas(110, 75);
    draw_card(
        &mut resize_expected,
        10,
        20,
        30,
        20,
        Rgba([255, 255, 255, 255]),
    );
    draw_card(
        &mut resize_actual,
        10,
        20,
        42,
        27,
        Rgba([255, 255, 255, 255]),
    );
    let resized = compare(&resize_expected, &resize_actual, &CompareOptions::default())
        .expect("resize comparison");
    assert_eq!(resized.summary.resized, 1, "{:?}", resized.differences);

    let mut expected = canvas(130, 85);
    let mut actual = canvas(130, 85);
    draw_card(&mut expected, 8, 8, 24, 18, Rgba([255, 255, 255, 255]));
    draw_card(&mut actual, 8, 8, 24, 18, Rgba([255, 255, 255, 255]));
    draw_card(&mut expected, 42, 45, 25, 18, Rgba([220, 235, 255, 255]));
    draw_card(&mut actual, 92, 45, 27, 18, Rgba([245, 225, 220, 255]));
    let options = CompareOptions {
        max_offset: 10,
        ..CompareOptions::default()
    };
    let added_removed = compare(&expected, &actual, &options).expect("add/remove comparison");
    assert_eq!(
        added_removed.summary.added, 1,
        "{:?}",
        added_removed.differences
    );
    assert_eq!(
        added_removed.summary.removed, 1,
        "{:?}",
        added_removed.differences
    );

    let mut changed_actual = expected.clone();
    draw_card(
        &mut changed_actual,
        42,
        45,
        25,
        18,
        Rgba([255, 220, 220, 255]),
    );
    let changed = compare(&expected, &changed_actual, &CompareOptions::default())
        .expect("changed comparison");
    assert_eq!(changed.summary.changed, 1, "{:?}", changed.differences);
}

#[test]
fn movement_at_the_default_limit_is_detected() {
    let mut expected = canvas(320, 100);
    let mut actual = canvas(320, 100);
    draw_card(&mut expected, 5, 5, 28, 18, Rgba([255, 255, 255, 255]));
    draw_card(&mut actual, 5, 5, 28, 18, Rgba([255, 255, 255, 255]));
    draw_card(&mut expected, 60, 45, 34, 22, Rgba([220, 235, 255, 255]));
    draw_card(&mut actual, 188, 45, 34, 22, Rgba([220, 235, 255, 255]));

    let comparison = compare(&expected, &actual, &CompareOptions::default()).expect("compare");
    let moved = findings_of_kind(&comparison, DifferenceKind::Moved);

    assert_eq!(moved.len(), 1, "{:?}", comparison.differences);
    assert_eq!(moved[0].offset, Some(Offset { x: 128, y: 0 }));
}

#[test]
fn ambiguous_repeated_cards_are_not_claimed_as_movements() {
    let mut expected = canvas(130, 65);
    let mut actual = canvas(130, 65);
    let fill = Rgba([255, 255, 255, 255]);
    draw_card(&mut expected, 20, 20, 25, 18, fill);
    draw_card(&mut expected, 60, 20, 25, 18, fill);
    draw_card(&mut actual, 40, 20, 25, 18, fill);
    draw_card(&mut actual, 80, 20, 25, 18, fill);
    let options = CompareOptions {
        max_offset: 40,
        ..CompareOptions::default()
    };

    let comparison = compare(&expected, &actual, &options).expect("compare");

    assert!(
        findings_of_kind(&comparison, DifferenceKind::Moved).is_empty(),
        "alignment={:?}, differences={:?}",
        comparison.alignment,
        comparison.differences
    );
    assert!(comparison.summary.changed > 0);
}

#[test]
fn findings_and_ids_are_deterministic() {
    let mut expected = canvas(120, 80);
    let mut actual = canvas(120, 80);
    draw_card(&mut expected, 10, 10, 30, 20, Rgba([255, 255, 255, 255]));
    draw_card(&mut actual, 15, 10, 30, 20, Rgba([255, 255, 255, 255]));

    let first = compare(&expected, &actual, &CompareOptions::default()).expect("first");
    let second = compare(&expected, &actual, &CompareOptions::default()).expect("second");

    assert_eq!(first, second);
    assert_eq!(
        first
            .differences
            .iter()
            .map(|difference| difference.id.as_str())
            .collect::<Vec<_>>(),
        vec!["D1"],
        "{:?}",
        first.differences
    );
}
