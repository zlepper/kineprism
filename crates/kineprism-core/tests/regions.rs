use image::{Rgba, RgbaImage};
use kineprism_core::{
    Bounds, CompareError, CompareOptions, DifferenceKind, ImageDimensions, Offset, compare,
};

const WHITE: Rgba<u8> = Rgba([255, 255, 255, 255]);
const DARK: Rgba<u8> = Rgba([25, 35, 50, 255]);

fn fill_rect(image: &mut RgbaImage, bounds: Bounds, color: Rgba<u8>) {
    for y in bounds.y..bounds.bottom() {
        for x in bounds.x..bounds.right() {
            image.put_pixel(x, y, color);
        }
    }
}

fn draw_card(image: &mut RgbaImage, bounds: Bounds) {
    fill_rect(image, bounds, DARK);
    fill_rect(
        image,
        Bounds {
            x: bounds.x + 1,
            y: bounds.y + 1,
            width: bounds.width - 2,
            height: bounds.height - 2,
        },
        Rgba([225, 235, 250, 255]),
    );
    fill_rect(
        image,
        Bounds {
            x: bounds.x + 4,
            y: bounds.y + 5,
            width: bounds.width - 8,
            height: 2,
        },
        Rgba([70, 85, 110, 255]),
    );
}

#[test]
fn changes_outside_the_region_are_ignored_and_metrics_cover_the_region() {
    let expected = RgbaImage::from_pixel(40, 30, WHITE);
    let mut actual = expected.clone();
    fill_rect(
        &mut actual,
        Bounds {
            x: 2,
            y: 3,
            width: 8,
            height: 7,
        },
        DARK,
    );
    let region = Bounds {
        x: 20,
        y: 12,
        width: 12,
        height: 10,
    };
    let options = CompareOptions {
        region: Some(region),
        ..CompareOptions::default()
    };

    let comparison = compare(&expected, &actual, &options).expect("masked comparison");

    assert!(comparison.equivalent);
    assert!(comparison.differences.is_empty());
    assert_eq!(
        comparison.expected,
        ImageDimensions {
            width: 40,
            height: 30
        }
    );
    assert_eq!(
        comparison.actual,
        ImageDimensions {
            width: 40,
            height: 30
        }
    );
    assert_eq!(comparison.settings.region, Some(region));
    assert_eq!(comparison.metrics.raw.compared_pixels, region.area());
    assert!((comparison.metrics.raw.expected_coverage - 1.0).abs() < f64::EPSILON);
    assert!((comparison.metrics.raw.actual_coverage - 1.0).abs() < f64::EPSILON);
    assert_eq!(comparison.metrics.raw.mae, Some(0.0));
}

#[test]
fn findings_inside_the_region_use_full_image_coordinates() {
    let expected = RgbaImage::from_pixel(40, 30, WHITE);
    let mut actual = expected.clone();
    let changed = Bounds {
        x: 23,
        y: 15,
        width: 4,
        height: 3,
    };
    fill_rect(&mut actual, changed, DARK);
    let region = Bounds {
        x: 20,
        y: 12,
        width: 12,
        height: 10,
    };
    let options = CompareOptions {
        min_region_area: 1,
        region: Some(region),
        ..CompareOptions::default()
    };

    let comparison = compare(&expected, &actual, &options).expect("masked comparison");

    assert!(!comparison.equivalent);
    assert!(comparison.summary.changed > 0 || comparison.summary.added > 0);
    assert!(comparison.differences.iter().all(|difference| {
        difference
            .expected_bounds
            .into_iter()
            .chain(difference.actual_bounds)
            .all(|bounds| {
                bounds.x >= region.x
                    && bounds.y >= region.y
                    && bounds.right() <= region.right()
                    && bounds.bottom() <= region.bottom()
            })
    }));
    assert!(
        comparison
            .differences
            .iter()
            .any(|difference| difference.actual_bounds == Some(changed)),
        "{:?}",
        comparison.differences
    );
}

#[test]
fn movement_inside_the_region_keeps_its_offset_and_global_bounds() {
    let mut expected = RgbaImage::from_pixel(160, 120, WHITE);
    let mut actual = expected.clone();
    let region = Bounds {
        x: 20,
        y: 15,
        width: 120,
        height: 80,
    };
    let static_card = Bounds {
        x: 28,
        y: 23,
        width: 28,
        height: 18,
    };
    let expected_moved = Bounds {
        x: 68,
        y: 51,
        width: 42,
        height: 24,
    };
    let actual_moved = Bounds {
        x: 73,
        ..expected_moved
    };
    draw_card(&mut expected, static_card);
    draw_card(&mut actual, static_card);
    draw_card(&mut expected, expected_moved);
    draw_card(&mut actual, actual_moved);
    let options = CompareOptions {
        region: Some(region),
        ..CompareOptions::default()
    };

    let comparison = compare(&expected, &actual, &options).expect("masked movement");
    let movement = comparison
        .differences
        .iter()
        .find(|difference| difference.kind == DifferenceKind::Moved)
        .unwrap_or_else(|| panic!("missing movement: {:?}", comparison.differences));

    assert_eq!(movement.offset, Some(Offset { x: 5, y: 0 }));
    assert_eq!(movement.expected_bounds, Some(expected_moved));
    assert_eq!(movement.actual_bounds, Some(actual_moved));
}

#[test]
fn a_shared_region_ignores_source_canvas_size_differences() {
    let expected = RgbaImage::from_pixel(30, 20, WHITE);
    let actual = RgbaImage::from_pixel(40, 28, WHITE);
    let options = CompareOptions {
        region: Some(Bounds {
            x: 5,
            y: 4,
            width: 20,
            height: 12,
        }),
        ..CompareOptions::default()
    };

    let comparison = compare(&expected, &actual, &options).expect("masked comparison");

    assert!(comparison.equivalent);
    assert_eq!(
        comparison.expected,
        ImageDimensions {
            width: 30,
            height: 20
        }
    );
    assert_eq!(
        comparison.actual,
        ImageDimensions {
            width: 40,
            height: 28
        }
    );
    assert_eq!(comparison.summary.canvas_size, 0);
}

#[test]
fn regions_must_be_nonempty_and_fit_both_images() {
    let expected = RgbaImage::new(20, 20);
    let actual = RgbaImage::new(18, 22);
    let zero_width = Bounds {
        x: 0,
        y: 0,
        width: 0,
        height: 5,
    };
    let zero_options = CompareOptions {
        region: Some(zero_width),
        ..CompareOptions::default()
    };
    assert_eq!(
        compare(&expected, &actual, &zero_options),
        Err(CompareError::InvalidRegionSize(zero_width))
    );

    let out_of_bounds = Bounds {
        x: 10,
        y: 10,
        width: 9,
        height: 8,
    };
    let invalid_options = CompareOptions {
        region: Some(out_of_bounds),
        ..CompareOptions::default()
    };
    assert_eq!(
        compare(&expected, &actual, &invalid_options),
        Err(CompareError::RegionOutOfBounds {
            region: out_of_bounds,
            expected: ImageDimensions {
                width: 20,
                height: 20,
            },
            actual: ImageDimensions {
                width: 18,
                height: 22,
            },
        })
    );
}

#[test]
fn a_region_may_touch_the_bottom_right_edge() {
    let expected = RgbaImage::from_pixel(20, 15, WHITE);
    let actual = expected.clone();
    let options = CompareOptions {
        region: Some(Bounds {
            x: 14,
            y: 10,
            width: 6,
            height: 5,
        }),
        ..CompareOptions::default()
    };

    assert!(
        compare(&expected, &actual, &options)
            .expect("edge region")
            .equivalent
    );
}
