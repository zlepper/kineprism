use crate::color::NormalizedImage;
use crate::mask::Mask;
use crate::{Bounds, CompareError, CompareOptions, Difference, DifferenceKind, Offset};

pub(crate) fn append_unhandled(
    expected: &NormalizedImage,
    actual: &NormalizedImage,
    alignment: Offset,
    options: &CompareOptions,
    handled_expected: &[Bounds],
    handled_actual: &[Bounds],
    differences: &mut Vec<Difference>,
) -> Result<(), CompareError> {
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

    for component in expected_components {
        let actual_bounds = translated_clipped(component.bounds, alignment, actual);
        if let Some(mapped) = actual_bounds {
            for (index, actual_component) in actual_components.iter().enumerate() {
                if mapped.intersection(actual_component.bounds).is_some() {
                    actual_used[index] = true;
                }
            }
        }
        differences.push(changed_component(
            Some(component.bounds),
            actual_bounds,
            component.area,
        ));
    }
    for (index, component) in actual_components.into_iter().enumerate() {
        if !actual_used[index] {
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
    Ok(())
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
