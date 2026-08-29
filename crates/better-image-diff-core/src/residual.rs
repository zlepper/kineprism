use crate::color::NormalizedImage;
use crate::local_geometry::padded;
use crate::mapping::MovementMapping;
use crate::mask::{Component, Mask};
use crate::{
    Bounds, CompareError, CompareOptions, Difference, DifferenceKind, Offset, SuppressionSummary,
};

#[derive(Clone, Copy)]
pub(crate) struct AnalysisContext<'a> {
    pub(crate) expected: &'a NormalizedImage,
    pub(crate) actual: &'a NormalizedImage,
    pub(crate) alignment: Offset,
    pub(crate) options: &'a CompareOptions,
    pub(crate) handled_expected: &'a [Bounds],
    pub(crate) handled_actual: &'a [Bounds],
    pub(crate) movements: &'a [MovementMapping],
}

pub(crate) fn append_unhandled(
    context: AnalysisContext<'_>,
    differences: &mut Vec<Difference>,
) -> Result<SuppressionSummary, CompareError> {
    let AnalysisContext {
        expected,
        actual,
        alignment,
        options,
        handled_expected,
        handled_actual,
        movements,
    } = context;
    let mut expected_mask = aligned_mask(expected, actual, alignment, options)?;
    let reverse = Offset {
        x: alignment.x.saturating_neg(),
        y: alignment.y.saturating_neg(),
    };
    let mut actual_mask = aligned_mask(actual, expected, reverse, options)?;
    clear_bounds(&mut expected_mask, handled_expected, expected);
    clear_translated_bounds(&mut expected_mask, handled_actual, reverse, expected);
    clear_bounds(&mut actual_mask, handled_actual, actual);
    clear_translated_bounds(&mut actual_mask, handled_expected, alignment, actual);
    let expected_components = expected_mask.components(options.min_region_area)?;
    let actual_components = actual_mask.components(options.min_region_area)?;
    let mut actual_used = vec![false; actual_components.len()];
    let mut suppression = SuppressionSummary::default();

    for component in expected_components {
        let actual_bounds = translated_clipped(component.bounds, alignment, actual);
        if let Some(mapped) = actual_bounds {
            for (index, actual_component) in actual_components.iter().enumerate() {
                if mapped.intersection(actual_component.bounds).is_some() {
                    actual_used[index] = true;
                }
            }
        }
        if movement_border_suppression(&component, true, movements, alignment, expected, actual) {
            suppression.record_movement_border(component.area);
            continue;
        }
        differences.push(changed_component(
            Some(component.bounds),
            actual_bounds,
            component.area,
        ));
    }
    for (index, component) in actual_components.into_iter().enumerate() {
        if !actual_used[index] {
            if movement_border_suppression(
                &component, false, movements, alignment, expected, actual,
            ) {
                suppression.record_movement_border(component.area);
                continue;
            }
            differences.push(changed_component(
                translated_clipped(component.bounds, reverse, expected),
                Some(component.bounds),
                component.area,
            ));
        }
    }
    append_unmatched_canvas_regions(
        expected,
        actual,
        alignment,
        options.min_region_area,
        true,
        differences,
    );
    append_unmatched_canvas_regions(
        actual,
        expected,
        reverse,
        options.min_region_area,
        false,
        differences,
    );
    Ok(suppression)
}

fn movement_border_suppression(
    component: &Component,
    component_is_expected: bool,
    movements: &[MovementMapping],
    alignment: Offset,
    expected: &NormalizedImage,
    actual: &NormalizedImage,
) -> bool {
    let mut matched = None;
    for (index, movement) in movements.iter().enumerate() {
        if !component_borders_movement(
            component.bounds,
            component_is_expected,
            *movement,
            alignment,
            expected,
            actual,
        ) {
            continue;
        }
        if matched.is_some() {
            return false;
        }
        matched = Some(index);
    }
    matched.is_some_and(|index| component.area <= suppression_area(movements[index].bounds))
}

fn component_borders_movement(
    component: Bounds,
    component_is_expected: bool,
    movement: MovementMapping,
    alignment: Offset,
    expected: &NormalizedImage,
    actual: &NormalizedImage,
) -> bool {
    let radius = suppression_radius(movement.bounds);
    let actual_bounds =
        movement
            .bounds
            .translated_clipped(movement.offset, actual.width(), actual.height());
    let reverse_alignment = Offset {
        x: alignment.x.saturating_neg(),
        y: alignment.y.saturating_neg(),
    };
    if component_is_expected {
        borders(
            component,
            movement.bounds,
            radius,
            expected.width(),
            expected.height(),
        ) || actual_bounds
            .and_then(|bounds| {
                bounds.translated_clipped(reverse_alignment, expected.width(), expected.height())
            })
            .is_some_and(|bounds| {
                borders(
                    component,
                    bounds,
                    radius,
                    expected.width(),
                    expected.height(),
                )
            })
    } else {
        actual_bounds.is_some_and(|bounds| {
            borders(component, bounds, radius, actual.width(), actual.height())
        }) || movement
            .bounds
            .translated_clipped(alignment, actual.width(), actual.height())
            .is_some_and(|bounds| {
                borders(component, bounds, radius, actual.width(), actual.height())
            })
    }
}

fn borders(component: Bounds, movement: Bounds, radius: u32, width: u32, height: u32) -> bool {
    movement.intersection(component).is_none()
        && contains(padded(movement, radius, width, height), component)
}

fn contains(outer: Bounds, inner: Bounds) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.right() <= outer.right()
        && inner.bottom() <= outer.bottom()
}

fn suppression_radius(bounds: Bounds) -> u32 {
    let root = bounds.area().isqrt();
    u32::try_from(root.div_ceil(128).clamp(1, 8)).unwrap_or(8)
}

fn suppression_area(bounds: Bounds) -> u32 {
    u32::try_from((bounds.area() / 512).clamp(16, 1024)).unwrap_or(1024)
}

pub(crate) fn significant_pair(
    expected: &NormalizedImage,
    expected_bounds: Bounds,
    actual: &NormalizedImage,
    actual_bounds: Bounds,
    threshold: f64,
    minimum_area: u32,
) -> bool {
    let width = expected_bounds.width.min(actual_bounds.width);
    let height = expected_bounds.height.min(actual_bounds.height);
    let Ok(mut mask) = Mask::try_new(width, height) else {
        return true;
    };
    for y in 0..height {
        for x in 0..width {
            if expected
                .pixel(expected_bounds.x + x, expected_bounds.y + y)
                .perceptual_distance(actual.pixel(actual_bounds.x + x, actual_bounds.y + y))
                > threshold
            {
                mask.set(x, y, true);
            }
        }
    }
    mask.components(minimum_area)
        .map_or(true, |components| !components.is_empty())
}

fn aligned_mask(
    source: &NormalizedImage,
    target: &NormalizedImage,
    offset: Offset,
    options: &CompareOptions,
) -> Result<Mask, CompareError> {
    let mut mask = Mask::try_new(source.width(), source.height())?;
    for y in 0..source.height() {
        for x in 0..source.width() {
            let target_x = i64::from(x) + i64::from(offset.x);
            let target_y = i64::from(y) + i64::from(offset.y);
            if target_x < 0
                || target_y < 0
                || target_x >= i64::from(target.width())
                || target_y >= i64::from(target.height())
            {
                continue;
            }
            let target_x = u32::try_from(target_x).expect("checked residual x");
            let target_y = u32::try_from(target_y).expect("checked residual y");
            if source
                .pixel(x, y)
                .perceptual_distance(target.pixel(target_x, target_y))
                > options.color_threshold
            {
                mask.set(x, y, true);
            }
        }
    }
    Ok(mask)
}

fn append_unmatched_canvas_regions(
    source: &NormalizedImage,
    target: &NormalizedImage,
    offset: Offset,
    minimum_area: u32,
    source_is_expected: bool,
    differences: &mut Vec<Difference>,
) {
    for bounds in unmatched_canvas_bounds(source, target, offset) {
        let area = bounds.width.saturating_mul(bounds.height);
        if area < minimum_area {
            continue;
        }
        let (expected_bounds, actual_bounds) = if source_is_expected {
            (Some(bounds), None)
        } else {
            (None, Some(bounds))
        };
        differences.push(changed_component(expected_bounds, actual_bounds, area));
    }
}

fn unmatched_canvas_bounds(
    source: &NormalizedImage,
    target: &NormalizedImage,
    offset: Offset,
) -> Vec<Bounds> {
    let source_width = i64::from(source.width());
    let source_height = i64::from(source.height());
    let valid_left = (-i64::from(offset.x)).clamp(0, source_width);
    let valid_top = (-i64::from(offset.y)).clamp(0, source_height);
    let valid_right = (i64::from(target.width()) - i64::from(offset.x)).clamp(0, source_width);
    let valid_bottom = (i64::from(target.height()) - i64::from(offset.y)).clamp(0, source_height);
    if valid_left >= valid_right || valid_top >= valid_bottom {
        return nonzero_bounds(0, 0, source.width(), source.height())
            .into_iter()
            .collect();
    }
    let left = u32::try_from(valid_left).expect("clamped valid left");
    let top = u32::try_from(valid_top).expect("clamped valid top");
    let right = u32::try_from(valid_right).expect("clamped valid right");
    let bottom = u32::try_from(valid_bottom).expect("clamped valid bottom");
    [
        nonzero_bounds(0, 0, source.width(), top),
        nonzero_bounds(0, bottom, source.width(), source.height() - bottom),
        nonzero_bounds(0, top, left, bottom - top),
        nonzero_bounds(right, top, source.width() - right, bottom - top),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn nonzero_bounds(x: u32, y: u32, width: u32, height: u32) -> Option<Bounds> {
    (width > 0 && height > 0).then_some(Bounds {
        x,
        y,
        width,
        height,
    })
}

fn clear_bounds(mask: &mut Mask, bounds: &[Bounds], image: &NormalizedImage) {
    for bounds in bounds {
        clear_one_bounds(mask, *bounds, image);
    }
}

fn clear_one_bounds(mask: &mut Mask, bounds: Bounds, image: &NormalizedImage) {
    for y in bounds.y..bounds.bottom().min(image.height()) {
        for x in bounds.x..bounds.right().min(image.width()) {
            mask.set(x, y, false);
        }
    }
}

fn clear_translated_bounds(
    mask: &mut Mask,
    bounds: &[Bounds],
    offset: Offset,
    image: &NormalizedImage,
) {
    for bounds in bounds {
        if let Some(translated) = translated_clipped(*bounds, offset, image) {
            clear_one_bounds(mask, translated, image);
        }
    }
}

fn translated_clipped(bounds: Bounds, offset: Offset, image: &NormalizedImage) -> Option<Bounds> {
    bounds.translated_clipped(offset, image.width(), image.height())
}

fn changed_component(
    expected_bounds: Option<Bounds>,
    actual_bounds: Option<Bounds>,
    area: u32,
) -> Difference {
    Difference {
        id: String::new(),
        kind: DifferenceKind::Changed,
        expected_bounds,
        actual_bounds,
        offset: None,
        confidence: 0.7,
        message: format!("A {area} px region contains otherwise unexplained visual differences."),
    }
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};

    use super::*;
    use crate::color::NormalizedImage;

    fn movement(bounds: Bounds, offset: Offset) -> MovementMapping {
        MovementMapping {
            bounds,
            offset,
            confidence: 1.0,
            order: 0,
        }
    }

    fn fill(image: &mut RgbaImage, bounds: Bounds, color: Rgba<u8>) {
        for y in bounds.y..bounds.bottom() {
            for x in bounds.x..bounds.right() {
                image.put_pixel(x, y, color);
            }
        }
    }

    #[test]
    fn suppression_limits_scale_with_movement_area() {
        let small = Bounds {
            x: 0,
            y: 0,
            width: 64,
            height: 64,
        };
        let large = Bounds {
            x: 0,
            y: 0,
            width: 1024,
            height: 512,
        };

        assert_eq!(suppression_radius(small), 1);
        assert_eq!(suppression_area(small), 16);
        assert_eq!(suppression_radius(large), 6);
        assert_eq!(suppression_area(large), 1024);
    }

    #[test]
    fn suppression_requires_one_unambiguous_bordering_movement_and_a_small_component() {
        let expected_pixels = RgbaImage::from_pixel(900, 400, Rgba([250, 250, 250, 255]));
        let actual_pixels = expected_pixels.clone();
        let expected = NormalizedImage::try_new(&expected_pixels).expect("expected image");
        let actual = NormalizedImage::try_new(&actual_pixels).expect("actual image");
        let primary = movement(
            Bounds {
                x: 20,
                y: 20,
                width: 256,
                height: 128,
            },
            Offset { x: 300, y: 0 },
        );
        let fringe = Component {
            bounds: Bounds {
                x: 18,
                y: 40,
                width: 1,
                height: 20,
            },
            area: 20,
        };

        assert!(movement_border_suppression(
            &fringe,
            true,
            &[primary],
            Offset::default(),
            &expected,
            &actual,
        ));

        let too_large = Component {
            area: 65,
            ..fringe.clone()
        };
        assert!(!movement_border_suppression(
            &too_large,
            true,
            &[primary],
            Offset::default(),
            &expected,
            &actual,
        ));

        let outside_halo = Component {
            bounds: Bounds {
                x: 17,
                ..fringe.bounds
            },
            ..fringe.clone()
        };
        assert!(!movement_border_suppression(
            &outside_halo,
            true,
            &[primary],
            Offset::default(),
            &expected,
            &actual,
        ));

        let adjacent = movement(
            Bounds {
                x: 15,
                y: 20,
                width: 3,
                height: 128,
            },
            Offset { x: 300, y: 0 },
        );
        assert!(!movement_border_suppression(
            &fringe,
            true,
            &[primary, adjacent],
            Offset::default(),
            &expected,
            &actual,
        ));
    }

    #[test]
    fn residual_analysis_defers_border_fringes_but_keeps_distant_changes() {
        let background = Rgba([250, 250, 250, 255]);
        let content = Rgba([40, 80, 140, 255]);
        let shadow = Rgba([225, 225, 228, 255]);
        let changed = Rgba([220, 40, 40, 255]);
        let mut expected_pixels = RgbaImage::from_pixel(620, 220, background);
        let mut actual_pixels = expected_pixels.clone();
        let expected_bounds = Bounds {
            x: 40,
            y: 40,
            width: 256,
            height: 128,
        };
        let actual_bounds = Bounds {
            x: 340,
            ..expected_bounds
        };
        fill(&mut expected_pixels, expected_bounds, content);
        fill(&mut actual_pixels, actual_bounds, content);
        fill(
            &mut expected_pixels,
            Bounds {
                x: 38,
                y: 70,
                width: 1,
                height: 20,
            },
            shadow,
        );
        fill(
            &mut actual_pixels,
            Bounds {
                x: 338,
                y: 70,
                width: 1,
                height: 20,
            },
            shadow,
        );
        fill(
            &mut actual_pixels,
            Bounds {
                x: 610,
                y: 190,
                width: 4,
                height: 5,
            },
            changed,
        );
        let expected = NormalizedImage::try_new(&expected_pixels).expect("expected image");
        let actual = NormalizedImage::try_new(&actual_pixels).expect("actual image");
        let movement = movement(expected_bounds, Offset { x: 300, y: 0 });
        let mut differences = Vec::new();

        let options = CompareOptions::default();
        let suppression = append_unhandled(
            AnalysisContext {
                expected: &expected,
                actual: &actual,
                alignment: Offset::default(),
                options: &options,
                handled_expected: &[expected_bounds],
                handled_actual: &[actual_bounds],
                movements: &[movement],
            },
            &mut differences,
        )
        .expect("residual analysis");

        assert_eq!(suppression.movement_border_regions, 2);
        assert_eq!(suppression.movement_border_pixels, 40);
        assert_eq!(differences.len(), 1, "{differences:?}");
        assert_eq!(differences[0].kind, DifferenceKind::Changed);
        assert_eq!(
            differences[0].actual_bounds,
            Some(Bounds {
                x: 610,
                y: 190,
                width: 4,
                height: 5,
            })
        );
    }
}
