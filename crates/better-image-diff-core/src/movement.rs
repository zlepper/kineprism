use crate::mapping::MovementMapping;
use crate::{Bounds, CompareError, Difference, Offset};

const MERGE_GAP: u32 = 10;

pub(crate) fn coalesce(
    differences: &mut Vec<Difference>,
    movements: &mut Vec<MovementMapping>,
) -> Result<(), CompareError> {
    let mut removed_movements = vec![false; movements.len()];
    let mut removed_differences = vec![false; differences.len()];
    for left in 0..movements.len() {
        if removed_movements[left] {
            continue;
        }
        for right in left + 1..movements.len() {
            if removed_movements[right]
                || movements[left].offset != movements[right].offset
                || !nearby(movements[left].bounds, movements[right].bounds)
            {
                continue;
            }
            let left_order = movements[left].order;
            let right_order = movements[right].order;
            let expected_bounds = union(movements[left].bounds, movements[right].bounds);
            let actual_bounds = union(
                differences[left_order]
                    .actual_bounds
                    .unwrap_or_else(|| translated(expected_bounds, movements[left].offset)),
                differences[right_order]
                    .actual_bounds
                    .unwrap_or_else(|| translated(expected_bounds, movements[left].offset)),
            );
            let confidence = movements[left].confidence.min(movements[right].confidence);
            movements[left].bounds = expected_bounds;
            movements[left].confidence = confidence;
            differences[left_order].expected_bounds = Some(expected_bounds);
            differences[left_order].actual_bounds = Some(actual_bounds);
            differences[left_order].confidence = confidence;
            differences[left_order].message = format!(
                "Adjacent visual fragments appear {} as one region.",
                describe_offset(movements[left].offset)
            );
            removed_movements[right] = true;
            removed_differences[right_order] = true;
        }
    }

    let mut old_to_new = vec![None; differences.len()];
    let mut retained_differences = Vec::new();
    retained_differences
        .try_reserve_exact(differences.len())
        .map_err(|_error| CompareError::ImageTooLarge)?;
    for (old_index, difference) in differences.drain(..).enumerate() {
        if !removed_differences[old_index] {
            old_to_new[old_index] = Some(retained_differences.len());
            retained_differences.push(difference);
        }
    }
    *differences = retained_differences;

    let mut retained_movements = Vec::new();
    retained_movements
        .try_reserve_exact(movements.len())
        .map_err(|_error| CompareError::ImageTooLarge)?;
    for (index, mut movement) in movements.drain(..).enumerate() {
        if removed_movements[index] {
            continue;
        }
        movement.order = old_to_new[movement.order].expect("retained movement finding");
        retained_movements.push(movement);
    }
    *movements = retained_movements;
    Ok(())
}

fn nearby(left: Bounds, right: Bounds) -> bool {
    left.x <= right.right().saturating_add(MERGE_GAP)
        && right.x <= left.right().saturating_add(MERGE_GAP)
        && left.y <= right.bottom().saturating_add(MERGE_GAP)
        && right.y <= left.bottom().saturating_add(MERGE_GAP)
}

fn union(left: Bounds, right: Bounds) -> Bounds {
    let x = left.x.min(right.x);
    let y = left.y.min(right.y);
    Bounds {
        x,
        y,
        width: left.right().max(right.right()) - x,
        height: left.bottom().max(right.bottom()) - y,
    }
}

fn translated(bounds: Bounds, offset: Offset) -> Bounds {
    bounds
        .translated_clipped(offset, u32::MAX, u32::MAX)
        .unwrap_or(bounds)
}

fn describe_offset(offset: Offset) -> String {
    match (offset.x, offset.y) {
        (x, 0) if x > 0 => format!("{x} px right"),
        (x, 0) => format!("{} px left", x.unsigned_abs()),
        (0, y) if y > 0 => format!("{y} px down"),
        (0, y) => format!("{} px up", y.unsigned_abs()),
        (x, y) => format!("at offset ({x}, {y})"),
    }
}
