use crate::Bounds;

pub(crate) fn nearby(left: Bounds, right: Bounds, gap: u32) -> bool {
    left.x <= right.right().saturating_add(gap)
        && right.x <= left.right().saturating_add(gap)
        && left.y <= right.bottom().saturating_add(gap)
        && right.y <= left.bottom().saturating_add(gap)
}

pub(crate) fn vertical_overlap(left: Bounds, right: Bounds) -> bool {
    left.y < right.bottom() && right.y < left.bottom()
}

pub(crate) fn horizontal_gap(left: Bounds, right: Bounds) -> u32 {
    if left.right() < right.x {
        right.x - left.right()
    } else if right.right() < left.x {
        left.x - right.right()
    } else {
        0
    }
}

pub(crate) fn padded(bounds: Bounds, padding: u32, width: u32, height: u32) -> Bounds {
    let x = bounds.x.saturating_sub(padding);
    let y = bounds.y.saturating_sub(padding);
    Bounds {
        x,
        y,
        width: bounds.right().saturating_add(padding).min(width) - x,
        height: bounds.bottom().saturating_add(padding).min(height) - y,
    }
}

pub(crate) fn union(left: Bounds, right: Bounds) -> Bounds {
    let x = left.x.min(right.x);
    let y = left.y.min(right.y);
    Bounds {
        x,
        y,
        width: left.right().max(right.right()) - x,
        height: left.bottom().max(right.bottom()) - y,
    }
}
