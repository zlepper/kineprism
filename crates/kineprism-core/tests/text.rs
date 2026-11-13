use image::{Rgba, RgbaImage};
use kineprism_core::{CompareOptions, Comparison, DifferenceKind, compare};

const PAGE: Rgba<u8> = Rgba([242, 244, 248, 255]);
const CARD: Rgba<u8> = Rgba([255, 255, 255, 255]);
const BORDER: Rgba<u8> = Rgba([40, 48, 64, 255]);
const INK: Rgba<u8> = Rgba([30, 34, 40, 255]);

fn canvas(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_pixel(width, height, PAGE)
}

fn fill_rect(image: &mut RgbaImage, x: u32, y: u32, width: u32, height: u32, color: Rgba<u8>) {
    for pixel_y in y..(y + height).min(image.height()) {
        for pixel_x in x..(x + width).min(image.width()) {
            image.put_pixel(pixel_x, pixel_y, color);
        }
    }
}

fn draw_card(image: &mut RgbaImage, x: u32, y: u32, width: u32, height: u32) {
    fill_rect(image, x, y, width, height, BORDER);
    fill_rect(image, x + 1, y + 1, width - 2, height - 2, CARD);
}

fn glyph_rows(character: char) -> [u8; 7] {
    match character {
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        ':' => [0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x0C, 0x00],
        _ => [0; 7],
    }
}

/// Text rendering options; `edge_alpha` blends the ink into background pixels bordering a glyph in
/// proportion to how much ink surrounds them, imitating a rasterizer's coverage-based antialiasing.
#[derive(Clone, Copy)]
struct Style {
    scale: u32,
    color: Rgba<u8>,
    edge_alpha: f32,
    /// Extra pixels added to the right of every stroke, imitating a bolder weight.
    weight: u32,
}

const STYLE: Style = Style {
    scale: 2,
    color: INK,
    edge_alpha: 0.35,
    weight: 0,
};

fn draw_text(image: &mut RgbaImage, x: u32, y: u32, value: &str, style: Style) {
    let scale = style.scale;
    let advance = 6 * scale;
    let text_width = advance * u32::try_from(value.chars().count()).expect("short text");
    let text_height = 7 * scale;
    let mut ink = vec![false; (text_width * text_height) as usize];
    for (index, character) in value.chars().enumerate() {
        let origin = advance * u32::try_from(index).expect("short text");
        for (row, bits) in glyph_rows(character).into_iter().enumerate() {
            for column in 0..5_u32 {
                if bits & (1 << (4 - column)) == 0 {
                    continue;
                }
                let row = u32::try_from(row).expect("glyph row");
                for dy in 0..scale {
                    for dx in 0..scale + style.weight {
                        let ink_x = origin + column * scale + dx;
                        let ink_y = row * scale + dy;
                        ink[(ink_y * text_width + ink_x) as usize] = true;
                    }
                }
            }
        }
    }
    let is_ink = |ink_x: i64, ink_y: i64| {
        ink_x >= 0
            && ink_y >= 0
            && ink_x < i64::from(text_width)
            && ink_y < i64::from(text_height)
            && ink[usize::try_from(ink_y * i64::from(text_width) + ink_x).expect("index")]
    };
    for ink_y in -1..=i64::from(text_height) {
        for ink_x in -1..=i64::from(text_width) {
            let Ok(pixel_x) = u32::try_from(i64::from(x) + ink_x) else {
                continue;
            };
            let Ok(pixel_y) = u32::try_from(i64::from(y) + ink_y) else {
                continue;
            };
            if pixel_x >= image.width() || pixel_y >= image.height() {
                continue;
            }
            if is_ink(ink_x, ink_y) {
                image.put_pixel(pixel_x, pixel_y, style.color);
                continue;
            }
            let coverage: f32 = [(-1, 0, 2.0), (1, 0, 2.0), (0, -1, 2.0), (0, 1, 2.0)]
                .into_iter()
                .chain([(-1, -1, 1.0), (1, -1, 1.0), (-1, 1, 1.0), (1, 1, 1.0)])
                .filter(|(dx, dy, _)| is_ink(ink_x + dx, ink_y + dy))
                .map(|(.., weight)| weight)
                .sum();
            if coverage >= 2.0 {
                let background = *image.get_pixel(pixel_x, pixel_y);
                let alpha = (style.edge_alpha * coverage / 4.0).min(0.9);
                image.put_pixel(pixel_x, pixel_y, blend(background, style.color, alpha));
            }
        }
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped to the u8 range"
)]
fn blend(background: Rgba<u8>, foreground: Rgba<u8>, alpha: f32) -> Rgba<u8> {
    let mix = |channel: usize| {
        let value = f32::from(background[channel])
            + (f32::from(foreground[channel]) - f32::from(background[channel])) * alpha;
        value.round().clamp(0.0, 255.0) as u8
    };
    Rgba([mix(0), mix(1), mix(2), 255])
}

fn dashboard(label: &str, style: Style) -> RgbaImage {
    let mut image = canvas(200, 120);
    draw_card(&mut image, 10, 10, 180, 44);
    draw_text(&mut image, 22, 24, label, style);
    draw_text(&mut image, 22, 80, "42", STYLE);
    image
}

fn kinds(comparison: &Comparison) -> Vec<DifferenceKind> {
    comparison
        .differences
        .iter()
        .map(|difference| difference.kind)
        .collect()
}

fn classifying_text() -> CompareOptions {
    CompareOptions {
        classify_text: true,
        ..CompareOptions::default()
    }
}

fn ignoring_text() -> CompareOptions {
    CompareOptions {
        ignore_text: true,
        ..CompareOptions::default()
    }
}

fn ignoring_antialiasing() -> CompareOptions {
    CompareOptions {
        ignore_antialiasing: true,
        ..CompareOptions::default()
    }
}

fn softer() -> Style {
    Style {
        edge_alpha: 0.6,
        ..STYLE
    }
}

#[test]
fn text_is_only_classified_when_requested() {
    let expected = dashboard("12:01", STYLE);
    let actual = dashboard("12:59", STYLE);

    let comparison = compare(&expected, &actual, &CompareOptions::default()).expect("compare");

    assert!(!comparison.differences.is_empty());
    assert!(!kinds(&comparison).contains(&DifferenceKind::TextChanged));
    assert_eq!(comparison.suppression.text_regions, 0);
}

#[test]
fn changed_digits_in_a_card_are_one_text_change_covering_the_label() {
    let expected = dashboard("12:01", STYLE);
    let actual = dashboard("12:59", STYLE);

    let comparison = compare(&expected, &actual, &classifying_text()).expect("compare");

    assert_eq!(kinds(&comparison), [DifferenceKind::TextChanged]);
    assert_eq!(comparison.summary.text_changed, 1);
    let difference = &comparison.differences[0];
    let bounds = difference.expected_bounds.expect("expected bounds");
    assert_eq!((bounds.x, bounds.y), (23, 23));
    assert!(bounds.width >= 50 && bounds.height <= 18, "{bounds:?}");
    assert!(!comparison.equivalent);
}

#[test]
fn ignored_text_changes_are_counted_but_not_reported() {
    let expected = dashboard("12:01", STYLE);
    let actual = dashboard("12:59", STYLE);

    let comparison = compare(&expected, &actual, &ignoring_text()).expect("compare");

    assert!(comparison.equivalent, "{:?}", comparison.differences);
    assert_eq!(comparison.suppression.text_regions, 1);
    assert!(comparison.suppression.text_pixels > 0);
    assert!(
        comparison
            .suppression
            .message
            .as_deref()
            .is_some_and(|message| message.contains("text"))
    );
}

#[test]
fn text_directly_on_the_page_is_classified_even_when_its_length_changes() {
    let expected = dashboard("12:01", STYLE);
    let mut actual = dashboard("12:01", STYLE);
    fill_rect(&mut actual, 21, 79, 40, 18, PAGE);
    draw_text(&mut actual, 22, 80, "418", STYLE);

    let comparison = compare(&expected, &actual, &classifying_text()).expect("compare");

    assert_eq!(kinds(&comparison), [DifferenceKind::TextChanged]);
    let difference = &comparison.differences[0];
    assert_eq!(difference.expected_bounds.map(|bounds| bounds.x), Some(21));
    assert_eq!(difference.actual_bounds.map(|bounds| bounds.x), Some(21));
}

#[test]
fn antialiasing_differences_are_tolerated_only_when_requested() {
    let expected = dashboard("12:01", STYLE);
    let actual = dashboard("12:01", softer());

    let strict = compare(&expected, &actual, &CompareOptions::default()).expect("compare");
    let tolerant = compare(&expected, &actual, &ignoring_antialiasing()).expect("compare");

    assert!(!strict.equivalent);
    assert_eq!(strict.suppression.antialiased_pixels, 0);
    assert!(tolerant.equivalent, "{:?}", tolerant.differences);
    assert!(tolerant.suppression.antialiased_pixels > 0);
    assert_eq!(
        tolerant.metrics.raw.changed_pixel_ratio, strict.metrics.raw.changed_pixel_ratio,
        "metrics stay literal"
    );
}

#[test]
fn a_new_line_beside_text_is_reported_while_tolerating_antialiasing() {
    let expected = dashboard("12:01", STYLE);
    let mut actual = dashboard("12:01", softer());
    fill_rect(&mut actual, 22, 44, 60, 1, INK);

    let comparison = compare(
        &expected,
        &actual,
        &CompareOptions {
            ignore_antialiasing: true,
            ignore_text: true,
            ..CompareOptions::default()
        },
    )
    .expect("compare");

    assert!(!comparison.equivalent);
    assert!(
        comparison
            .differences
            .iter()
            .all(|difference| difference.kind != DifferenceKind::TextChanged)
    );
}

#[test]
fn recolored_text_is_not_a_text_change() {
    let expected = dashboard("12:01", STYLE);
    let actual = dashboard(
        "12:01",
        Style {
            color: Rgba([200, 30, 40, 255]),
            ..STYLE
        },
    );

    let comparison = compare(&expected, &actual, &ignoring_text()).expect("compare");

    assert!(!comparison.equivalent);
    assert!(!kinds(&comparison).contains(&DifferenceKind::TextChanged));
    assert_eq!(comparison.suppression.text_regions, 0);
}

#[test]
fn resized_text_is_not_a_text_change() {
    let expected = dashboard("12:01", STYLE);
    let mut actual = dashboard("12:01", STYLE);
    fill_rect(&mut actual, 21, 79, 40, 18, PAGE);
    draw_text(&mut actual, 22, 80, "42", Style { scale: 3, ..STYLE });

    let comparison = compare(&expected, &actual, &ignoring_text()).expect("compare");

    assert!(!comparison.equivalent);
    assert!(!kinds(&comparison).contains(&DifferenceKind::TextChanged));
}

#[test]
fn a_shifted_label_is_not_a_text_change() {
    let expected = dashboard("12:01", STYLE);
    let mut actual = canvas(200, 120);
    draw_card(&mut actual, 10, 10, 180, 44);
    draw_text(&mut actual, 27, 24, "12:01", STYLE);
    draw_text(&mut actual, 22, 80, "42", STYLE);

    let comparison = compare(&expected, &actual, &ignoring_text()).expect("compare");

    assert!(!comparison.equivalent);
    assert!(!kinds(&comparison).contains(&DifferenceKind::TextChanged));
}

#[test]
fn a_changed_gradient_badge_is_not_a_text_change() {
    let mut expected = dashboard("12:01", STYLE);
    let mut actual = expected.clone();
    for offset in 0..14 {
        let shade = u8::try_from(40 + offset * 12).expect("shade");
        fill_rect(
            &mut expected,
            120 + offset,
            24,
            1,
            14,
            Rgba([shade, 90, 200, 255]),
        );
        fill_rect(
            &mut actual,
            120 + offset,
            24,
            1,
            14,
            Rgba([200, shade, 90, 255]),
        );
    }

    let comparison = compare(&expected, &actual, &ignoring_text()).expect("compare");

    assert!(!comparison.equivalent);
    assert!(!kinds(&comparison).contains(&DifferenceKind::TextChanged));
}

#[test]
fn a_single_changed_glyph_on_its_own_is_left_as_a_generic_change() {
    let mut expected = canvas(120, 60);
    let mut actual = canvas(120, 60);
    draw_text(&mut expected, 40, 20, "7", STYLE);
    draw_text(&mut actual, 40, 20, "8", STYLE);

    let comparison = compare(&expected, &actual, &ignoring_text()).expect("compare");

    assert!(!comparison.equivalent);
    assert!(!kinds(&comparison).contains(&DifferenceKind::TextChanged));
}

#[test]
fn text_changes_are_classified_alongside_a_moved_card() {
    let mut expected = dashboard("12:01", STYLE);
    let mut actual = dashboard("12:59", STYLE);
    draw_card(&mut expected, 110, 70, 60, 30);
    draw_card(&mut actual, 116, 70, 60, 30);

    let comparison = compare(&expected, &actual, &classifying_text()).expect("compare");

    let kinds = kinds(&comparison);
    assert!(kinds.contains(&DifferenceKind::TextChanged), "{kinds:?}");
    assert!(kinds.contains(&DifferenceKind::Moved), "{kinds:?}");
}

fn label_options(ignore_text: bool, ignore_antialiasing: bool) -> CompareOptions {
    CompareOptions {
        classify_text: true,
        ignore_text,
        ignore_antialiasing,
        ..CompareOptions::default()
    }
}

/// Every flag combination, with text classification on, must keep reporting a change that is not
/// a text change.
fn assert_never_hidden(expected: &RgbaImage, actual: &RgbaImage) {
    for (ignore_text, ignore_antialiasing) in
        [(false, false), (true, false), (false, true), (true, true)]
    {
        let comparison = compare(
            expected,
            actual,
            &label_options(ignore_text, ignore_antialiasing),
        )
        .expect("compare");
        assert!(
            !comparison.equivalent,
            "hidden with ignore_text={ignore_text}, ignore_antialiasing={ignore_antialiasing}"
        );
        assert!(
            !kinds(&comparison).contains(&DifferenceKind::TextChanged),
            "{:?}",
            comparison.differences
        );
    }
}

fn label_at(x: u32, y: u32, style: Style) -> RgbaImage {
    let mut image = canvas(200, 120);
    draw_card(&mut image, 10, 10, 180, 44);
    draw_text(&mut image, x, y, "12:01", style);
    draw_text(&mut image, 22, 80, "42", STYLE);
    image
}

#[test]
fn labels_shifted_by_a_pixel_or_two_are_not_text_changes() {
    let expected = dashboard("12:01", STYLE);
    for (x, y) in [(23, 24), (24, 24), (22, 25), (22, 26)] {
        assert_never_hidden(&expected, &label_at(x, y, STYLE));
    }
}

#[test]
fn one_recolored_glyph_is_not_a_text_change() {
    let expected = dashboard("12:01", STYLE);
    let mut actual = dashboard("12:0", STYLE);
    draw_text(
        &mut actual,
        22 + 4 * 12,
        24,
        "1",
        Style {
            color: Rgba([200, 30, 40, 255]),
            ..STYLE
        },
    );

    assert_never_hidden(&expected, &actual);
}

#[test]
fn bolder_text_is_not_a_text_change() {
    let expected = dashboard("12:01", STYLE);
    let actual = dashboard("12:01", Style { weight: 1, ..STYLE });

    assert_never_hidden(&expected, &actual);
}

#[test]
fn a_strikethrough_across_the_glyphs_is_never_hidden() {
    let expected = dashboard("12:01", STYLE);
    let mut actual = expected.clone();
    fill_rect(&mut actual, 46, 30, 34, 3, INK);

    assert_never_hidden(&expected, &actual);
}

#[test]
fn ignoring_antialiasing_keeps_a_text_change_as_one_finding() {
    let expected = dashboard("1111111", STYLE);
    let actual = dashboard("7171717", STYLE);

    for options in [label_options(false, false), label_options(false, true)] {
        let comparison = compare(&expected, &actual, &options).expect("compare");
        assert_eq!(kinds(&comparison), [DifferenceKind::TextChanged]);
    }
    let both = compare(&expected, &actual, &label_options(true, true)).expect("compare");
    assert!(both.equivalent, "{:?}", both.differences);
    assert_eq!(both.suppression.text_regions, 1);
}

#[test]
fn suppressed_text_pixels_are_counted_once() {
    let expected = dashboard("1111111", STYLE);
    let actual = dashboard("7171717", STYLE);

    let comparison = compare(&expected, &actual, &ignoring_text()).expect("compare");

    let changed = expected
        .pixels()
        .zip(actual.pixels())
        .filter(|(left, right)| left != right)
        .count();
    assert_eq!(
        comparison.suppression.text_pixels,
        u64::try_from(changed).expect("count")
    );
}

#[test]
fn antialiasing_inside_a_reported_finding_is_not_counted_as_tolerated() {
    let mut expected = canvas(80, 60);
    fill_rect(&mut expected, 20, 15, 30, 20, INK);
    let mut actual = expected.clone();
    for x in 19..51 {
        actual.put_pixel(x, 14, Rgba([136, 139, 144, 255]));
        actual.put_pixel(x, 35, Rgba([136, 139, 144, 255]));
    }
    for y in 14..36 {
        actual.put_pixel(19, y, Rgba([136, 139, 144, 255]));
        actual.put_pixel(50, y, Rgba([136, 139, 144, 255]));
    }

    let comparison = compare(&expected, &actual, &ignoring_antialiasing()).expect("compare");

    assert!(!comparison.equivalent);
    assert_eq!(comparison.suppression.antialiased_pixels, 0);
}

#[test]
fn text_changes_are_found_under_a_global_offset_and_canvas_change() {
    let expected = dashboard("12:01", STYLE);
    let shifted = dashboard("12:59", STYLE);
    let mut actual = RgbaImage::from_pixel(210, 130, PAGE);
    image::imageops::replace(&mut actual, &shifted, 5, 7);

    for options in [label_options(false, false), label_options(false, true)] {
        let comparison = compare(&expected, &actual, &options).expect("compare");
        let kinds = kinds(&comparison);
        assert!(kinds.contains(&DifferenceKind::TextChanged), "{kinds:?}");
        // Only the strips the canvas grew by remain as generic changes.
        assert!(
            comparison
                .differences
                .iter()
                .filter(|difference| difference.kind == DifferenceKind::Changed)
                .all(|difference| difference.expected_bounds.is_none()),
            "{:?}",
            comparison.differences
        );
    }
}

#[test]
fn text_at_the_image_corner_is_suppressed_with_either_flag_combination() {
    let mut expected = canvas(120, 60);
    let mut actual = canvas(120, 60);
    draw_text(&mut expected, 0, 0, "12:01", STYLE);
    draw_text(&mut actual, 0, 0, "12:59", STYLE);

    for options in [label_options(true, false), label_options(true, true)] {
        let comparison = compare(&expected, &actual, &options).expect("compare");
        assert!(comparison.equivalent, "{:?}", comparison.differences);
    }
}

#[test]
fn tiny_images_compare_with_every_flag() {
    for (width, height) in [(1, 1), (1, 2), (2, 1), (3, 3), (1, 30), (30, 1)] {
        let expected = canvas(width, height);
        let mut actual = expected.clone();
        actual.put_pixel(width - 1, height - 1, INK);
        for (ignore_text, ignore_antialiasing) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let options = CompareOptions {
                min_region_area: 1,
                ..label_options(ignore_text, ignore_antialiasing)
            };
            let comparison = compare(&expected, &actual, &options).expect("compare");
            assert!(!comparison.equivalent, "{width}x{height}");
        }
    }
}

#[test]
fn many_text_changes_are_each_recognized() {
    let mut expected = canvas(1260, 540);
    let mut actual = canvas(1260, 540);
    for row in 0..14 {
        for column in 0..12 {
            let (x, y) = (12 + column * 104, 12 + row * 38);
            draw_text(&mut expected, x, y, "12:01", STYLE);
            draw_text(&mut actual, x, y, "12:59", STYLE);
        }
    }

    let comparison = compare(&expected, &actual, &classifying_text()).expect("compare");

    assert_eq!(
        comparison.summary.text_changed, 168,
        "{:?}",
        comparison.summary
    );
    assert_eq!(comparison.summary.total, 168);
}

/// Draws a `size` square outline with a 2px stroke, like an empty checkbox or icon frame.
fn draw_box(image: &mut RgbaImage, x: u32, y: u32, size: u32) {
    fill_rect(image, x, y, size, size, INK);
    fill_rect(image, x + 2, y + 2, size - 4, size - 4, CARD);
}

#[test]
fn a_toggled_checkbox_beside_a_label_is_not_a_text_change() {
    let mut expected = dashboard("12:01", STYLE);
    draw_box(&mut expected, 100, 24, 14);
    let mut filled = expected.clone();
    fill_rect(&mut filled, 100, 24, 14, 14, INK);
    let mut ticked = expected.clone();
    fill_rect(&mut ticked, 105, 29, 4, 4, INK);

    assert_never_hidden(&expected, &filled);
    assert_never_hidden(&expected, &ticked);
}

#[test]
fn a_removed_icon_before_a_label_is_not_a_text_change() {
    let mut expected = canvas(200, 120);
    draw_card(&mut expected, 10, 10, 180, 44);
    draw_box(&mut expected, 22, 24, 14);
    draw_text(&mut expected, 42, 24, "12:01", STYLE);
    let mut actual = expected.clone();
    fill_rect(&mut actual, 22, 24, 14, 14, CARD);

    assert_never_hidden(&expected, &actual);
}

#[test]
fn a_swapped_icon_in_a_row_of_icons_is_not_a_text_change() {
    let mut expected = canvas(200, 120);
    draw_card(&mut expected, 10, 10, 180, 44);
    for index in 0..4 {
        draw_box(&mut expected, 22 + index * 20, 24, 14);
    }
    let mut actual = expected.clone();
    fill_rect(&mut actual, 62, 24, 14, 14, CARD);
    fill_rect(&mut actual, 68, 24, 2, 14, INK);
    fill_rect(&mut actual, 62, 30, 14, 2, INK);

    assert_never_hidden(&expected, &actual);
}

#[test]
fn a_moved_icon_after_a_label_is_not_a_text_change() {
    let mut expected = dashboard("12:01", STYLE);
    let mut actual = expected.clone();
    draw_box(&mut expected, 90, 24, 14);
    draw_box(&mut actual, 104, 24, 14);

    assert_never_hidden(&expected, &actual);
}

#[test]
fn text_in_an_outlined_chip_is_classified_without_the_chip() {
    let mut expected = canvas(200, 120);
    let mut actual = canvas(200, 120);
    for (image, label) in [(&mut expected, "12:01"), (&mut actual, "12:59")] {
        fill_rect(image, 20, 20, 100, 36, INK);
        fill_rect(image, 22, 22, 96, 32, CARD);
        draw_text(image, 30, 31, label, STYLE);
    }

    let comparison = compare(&expected, &actual, &classifying_text()).expect("compare");

    assert_eq!(kinds(&comparison), [DifferenceKind::TextChanged]);
    let bounds = comparison.differences[0]
        .expected_bounds
        .expect("expected bounds");
    assert!(bounds.height <= 18 && bounds.y > 22, "{bounds:?}");
}

fn large_label(style: Style) -> RgbaImage {
    let mut image = canvas(260, 120);
    draw_text(&mut image, 20, 20, "12:01", style);
    image
}

#[test]
fn large_text_changing_size_or_weight_is_not_a_text_change() {
    let large = Style { scale: 5, ..STYLE };
    let expected = large_label(large);

    assert_never_hidden(&expected, &large_label(Style { scale: 4, ..large }));
    assert_never_hidden(&expected, &large_label(Style { weight: 1, ..large }));
}

#[test]
fn subtly_recolored_text_is_not_a_text_change() {
    let expected = dashboard("12:01", STYLE);
    let actual = dashboard(
        "12:01",
        Style {
            color: Rgba([36, 40, 46, 255]),
            ..STYLE
        },
    );

    assert_never_hidden(&expected, &actual);
}

#[test]
fn respaced_or_partly_shifted_text_is_not_a_text_change() {
    let expected = dashboard("12:01", STYLE);
    let draw = |positions: [(u32, u32); 5]| {
        let mut image = dashboard("", STYLE);
        for (character, (x, y)) in "12:01".chars().zip(positions) {
            draw_text(&mut image, x, y, &character.to_string(), STYLE);
        }
        image
    };

    let spaced = draw([(22, 24), (36, 24), (50, 24), (64, 24), (78, 24)]);
    let shifted = draw([(22, 24), (34, 24), (46, 24), (62, 24), (74, 24)]);
    let dropped = draw([(22, 24), (34, 24), (46, 24), (58, 27), (70, 27)]);

    assert_never_hidden(&expected, &spaced);
    assert_never_hidden(&expected, &shifted);
    assert_never_hidden(&expected, &dropped);
}

#[test]
fn a_recolored_one_pixel_border_is_not_antialiasing() {
    let mut expected = canvas(120, 80);
    fill_rect(&mut expected, 20, 20, 60, 40, Rgba([128, 128, 128, 255]));
    fill_rect(&mut expected, 21, 21, 58, 38, INK);
    let mut actual = expected.clone();
    fill_rect(&mut actual, 20, 20, 60, 40, Rgba([200, 40, 40, 255]));
    fill_rect(&mut actual, 21, 21, 58, 38, INK);

    let comparison = compare(&expected, &actual, &ignoring_antialiasing()).expect("compare");

    assert!(!comparison.equivalent);
}

#[test]
fn antialiasing_inside_suppressed_text_is_counted_as_text() {
    let expected = dashboard("12:01", STYLE);
    let actual = dashboard(
        "12:59",
        Style {
            edge_alpha: 0.45,
            ..STYLE
        },
    );

    let comparison = compare(&expected, &actual, &label_options(true, true)).expect("compare");

    assert!(comparison.equivalent, "{:?}", comparison.differences);
    assert_eq!(comparison.suppression.text_regions, 1);
    assert_eq!(comparison.suppression.antialiased_pixels, 0);
}
