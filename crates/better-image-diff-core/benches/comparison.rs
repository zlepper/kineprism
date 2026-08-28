use std::hint::black_box;
use std::time::Duration;

use better_image_diff_core::{CompareOptions, Comparison, DifferenceKind, Offset, compare};
use criterion::{BenchmarkId, Criterion, Throughput};
use image::{Rgba, RgbaImage};

const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
const BACKGROUND: Rgba<u8> = Rgba([244, 246, 250, 255]);
const PANEL: Rgba<u8> = Rgba([255, 255, 255, 255]);
const BORDER: Rgba<u8> = Rgba([205, 212, 224, 255]);

#[derive(Clone, Copy)]
struct Rectangle {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

fn fill_rect(image: &mut RgbaImage, bounds: Rectangle, color: Rgba<u8>) {
    for y in bounds.y..bounds.y + bounds.height {
        for x in bounds.x..bounds.x + bounds.width {
            image.put_pixel(x, y, color);
        }
    }
}

fn draw_card(image: &mut RgbaImage, bounds: Rectangle, accent: Rgba<u8>, seed: u32) {
    fill_rect(image, bounds, BORDER);
    fill_rect(
        image,
        Rectangle {
            x: bounds.x + 2,
            y: bounds.y + 2,
            width: bounds.width - 4,
            height: bounds.height - 4,
        },
        PANEL,
    );
    fill_rect(
        image,
        Rectangle {
            x: bounds.x + 20,
            y: bounds.y + 22,
            width: 90 + seed % 70,
            height: 8,
        },
        Rgba([75, 84, 104, 255]),
    );
    fill_rect(
        image,
        Rectangle {
            x: bounds.x + 20,
            y: bounds.y + 52,
            width: 145 + seed % 90,
            height: 20,
        },
        Rgba([30, 38, 56, 255]),
    );
    fill_rect(
        image,
        Rectangle {
            x: bounds.x + 20,
            y: bounds.y + 88,
            width: 70 + seed % 60,
            height: 7,
        },
        accent,
    );
    for point in 0..9 {
        let chart_x = bounds.x + 20 + point * (bounds.width - 42) / 8;
        let chart_y = bounds.y + bounds.height - 28 - (seed + point * 11) % 42;
        fill_rect(
            image,
            Rectangle {
                x: chart_x,
                y: chart_y,
                width: 7,
                height: 7,
            },
            accent,
        );
    }
}

fn draw_dashboard() -> RgbaImage {
    let mut image = RgbaImage::from_pixel(WIDTH, HEIGHT, BACKGROUND);
    fill_rect(
        &mut image,
        Rectangle {
            x: 0,
            y: 0,
            width: 238,
            height: HEIGHT,
        },
        Rgba([13, 28, 52, 255]),
    );
    fill_rect(
        &mut image,
        Rectangle {
            x: 238,
            y: 0,
            width: WIDTH - 238,
            height: 94,
        },
        PANEL,
    );
    for row in 0..10 {
        fill_rect(
            &mut image,
            Rectangle {
                x: 28,
                y: 120 + row * 66,
                width: 160 - (row % 3) * 18,
                height: 12,
            },
            Rgba([150, 170, 202, 255]),
        );
    }
    for (index, x) in [278, 680, 1082, 1484].into_iter().enumerate() {
        draw_card(
            &mut image,
            Rectangle {
                x,
                y: 126,
                width: 360,
                height: 218,
            },
            [
                Rgba([38, 99, 220, 255]),
                Rgba([25, 155, 95, 255]),
                Rgba([132, 72, 210, 255]),
                Rgba([220, 125, 35, 255]),
            ][index],
            u32::try_from(index).expect("card index") * 17,
        );
    }
    draw_card(
        &mut image,
        Rectangle {
            x: 278,
            y: 382,
            width: 950,
            height: 630,
        },
        Rgba([38, 99, 220, 255]),
        83,
    );
    draw_card(
        &mut image,
        Rectangle {
            x: 1266,
            y: 382,
            width: 578,
            height: 630,
        },
        Rgba([25, 155, 95, 255]),
        131,
    );
    for row in 0..8 {
        fill_rect(
            &mut image,
            Rectangle {
                x: 1310,
                y: 500 + row * 57,
                width: 410 - (row % 4) * 38,
                height: 9,
            },
            Rgba([110, 120, 140, 255]),
        );
    }
    image
}

fn erase_card(image: &mut RgbaImage, bounds: Rectangle) {
    fill_rect(image, bounds, BACKGROUND);
}

fn single_element_change(expected: &RgbaImage) -> RgbaImage {
    let mut actual = expected.clone();
    let card = Rectangle {
        x: 680,
        y: 126,
        width: 360,
        height: 218,
    };
    erase_card(&mut actual, card);
    draw_card(
        &mut actual,
        Rectangle {
            x: card.x + 5,
            ..card
        },
        Rgba([25, 155, 95, 255]),
        17,
    );
    actual
}

fn many_changes(expected: &RgbaImage) -> RgbaImage {
    let mut actual = expected.clone();
    for (index, (x, offset_x, offset_y)) in
        [(278, 7, 4), (680, -9, 8), (1082, 12, -5), (1484, -6, 10)]
            .into_iter()
            .enumerate()
    {
        let original = Rectangle {
            x,
            y: 126,
            width: 360,
            height: 218,
        };
        erase_card(&mut actual, original);
        draw_card(
            &mut actual,
            Rectangle {
                x: x.checked_add_signed(offset_x).expect("shifted x"),
                y: 126_u32.checked_add_signed(offset_y).expect("shifted y"),
                ..original
            },
            [
                Rgba([190, 55, 75, 255]),
                Rgba([20, 130, 150, 255]),
                Rgba([95, 75, 200, 255]),
                Rgba([210, 105, 25, 255]),
            ][index],
            u32::try_from(index).expect("card index") * 29 + 7,
        );
    }
    fill_rect(
        &mut actual,
        Rectangle {
            x: 360,
            y: 520,
            width: 250,
            height: 80,
        },
        Rgba([235, 215, 220, 255]),
    );
    fill_rect(
        &mut actual,
        Rectangle {
            x: 1380,
            y: 760,
            width: 270,
            height: 90,
        },
        Rgba([215, 235, 225, 255]),
    );
    actual
}

fn validate_scenarios(
    expected: &RgbaImage,
    identical: &Comparison,
    single: &Comparison,
    many: &Comparison,
) {
    assert_eq!(expected.dimensions(), (WIDTH, HEIGHT));
    assert!(identical.equivalent);
    assert_eq!(identical.summary.total, 0);
    assert_eq!(identical.metrics.raw.mae, Some(0.0));
    assert_eq!(identical.metrics.raw.changed_pixel_ratio, Some(0.0));
    assert!(!single.equivalent);
    assert_eq!(single.summary.total, 1, "{:?}", single.differences);
    assert_eq!(single.summary.moved, 1, "{:?}", single.differences);
    assert_eq!(single.summary.resized, 0);
    assert_eq!(single.summary.added, 0);
    assert_eq!(single.summary.removed, 0);
    assert_eq!(single.summary.changed, 0);
    assert_eq!(single.differences[0].kind, DifferenceKind::Moved);
    assert_eq!(single.differences[0].offset, Some(Offset { x: 5, y: 0 }));
    assert!(
        single
            .metrics
            .structural_aligned
            .mae
            .expect("structural MAE")
            < single.metrics.global_aligned.mae.expect("global MAE")
    );
    assert!(!many.equivalent);
    assert_eq!(many.summary.total, 6, "{:?}", many.differences);
    assert_eq!(many.summary.moved, 0);
    assert_eq!(many.summary.resized, 0);
    assert_eq!(many.summary.added, 0);
    assert_eq!(many.summary.removed, 0);
    assert_eq!(many.summary.changed, 6);
    assert!(many.metrics.raw.mae.expect("raw MAE") > 0.0);
    assert!(many.metrics.raw.rmse.expect("raw RMSE") > 0.0);
    assert!(many.metrics.raw.changed_pixel_ratio.expect("changed ratio") > 0.0);
}

fn compare_benchmarks(criterion: &mut Criterion) {
    let expected = draw_dashboard();
    let single = single_element_change(&expected);
    let many = many_changes(&expected);
    let options = CompareOptions::default();
    let identical_result = compare(&expected, &expected, &options).expect("validate identical");
    let single_result = compare(&expected, &single, &options).expect("validate single change");
    let many_result = compare(&expected, &many, &options).expect("validate many changes");
    validate_scenarios(&expected, &identical_result, &single_result, &many_result);

    let mut group = criterion.benchmark_group("structural_compare_1920x1080");
    group.throughput(Throughput::Elements(u64::from(WIDTH) * u64::from(HEIGHT)));
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(12));
    for (name, actual) in [
        ("no_diff", &expected),
        ("single_element_change", &single),
        ("many_changes", &many),
    ] {
        group.bench_with_input(
            BenchmarkId::from_parameter(name),
            actual,
            |bencher, actual| {
                bencher.iter(|| {
                    compare(black_box(&expected), black_box(actual), black_box(&options))
                        .expect("benchmark comparison")
                });
            },
        );
    }
    group.finish();
}

fn main() {
    if cfg!(debug_assertions) {
        return;
    }
    let mut criterion = Criterion::default().configure_from_args();
    compare_benchmarks(&mut criterion);
    criterion.final_summary();
}
