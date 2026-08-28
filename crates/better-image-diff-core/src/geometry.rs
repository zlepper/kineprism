use crate::{Bounds, Offset};

impl Bounds {
    /// Returns the exclusive right edge, saturated at [`u32::MAX`] for externally constructed
    /// bounds that do not fit the coordinate domain.
    #[must_use]
    pub fn right(self) -> u32 {
        self.x.saturating_add(self.width)
    }

    /// Returns the exclusive bottom edge, saturated at [`u32::MAX`] for externally constructed
    /// bounds that do not fit the coordinate domain.
    #[must_use]
    pub fn bottom(self) -> u32 {
        self.y.saturating_add(self.height)
    }

    /// Returns the rectangle area in pixels.
    #[must_use]
    pub fn area(self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    /// Returns the intersection of two non-empty half-open rectangles.
    #[must_use]
    pub fn intersection(self, other: Self) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        (right > x && bottom > y).then_some(Self {
            x,
            y,
            width: right - x,
            height: bottom - y,
        })
    }

    /// Translates the bounds and clips them to a canvas.
    #[must_use]
    pub fn translated_clipped(
        self,
        offset: Offset,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Option<Self> {
        let left = i64::from(self.x) + i64::from(offset.x);
        let top = i64::from(self.y) + i64::from(offset.y);
        let right = left + i64::from(self.width);
        let bottom = top + i64::from(self.height);
        let clipped_left = left.clamp(0, i64::from(canvas_width));
        let clipped_top = top.clamp(0, i64::from(canvas_height));
        let clipped_right = right.clamp(0, i64::from(canvas_width));
        let clipped_bottom = bottom.clamp(0, i64::from(canvas_height));
        (clipped_right > clipped_left && clipped_bottom > clipped_top).then_some(Self {
            x: clamped_u32(clipped_left),
            y: clamped_u32(clipped_top),
            width: clamped_u32(clipped_right - clipped_left),
            height: clamped_u32(clipped_bottom - clipped_top),
        })
    }

    /// Returns an integer center suitable for annotation placement.
    #[must_use]
    pub fn center(self) -> (u32, u32) {
        (
            u32::try_from(u64::from(self.x) + u64::from(self.width / 2)).unwrap_or(u32::MAX),
            u32::try_from(u64::from(self.y) + u64::from(self.height / 2)).unwrap_or(u32::MAX),
        )
    }
}

fn clamped_u32(value: i64) -> u32 {
    u32::try_from(value).unwrap_or(if value < 0 { 0 } else { u32::MAX })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersection_uses_half_open_edges() {
        let first = Bounds {
            x: 1,
            y: 2,
            width: 4,
            height: 3,
        };
        let touching = Bounds {
            x: 5,
            y: 2,
            width: 2,
            height: 2,
        };
        assert_eq!(first.intersection(touching), None);
    }

    #[test]
    fn translation_clips_negative_coordinates() {
        let bounds = Bounds {
            x: 2,
            y: 2,
            width: 4,
            height: 4,
        };
        assert_eq!(
            bounds.translated_clipped(Offset { x: -4, y: -3 }, 10, 10),
            Some(Bounds {
                x: 0,
                y: 0,
                width: 2,
                height: 3,
            })
        );
    }

    #[test]
    fn center_saturates_instead_of_overflowing_for_external_bounds() {
        let bounds = Bounds {
            x: u32::MAX,
            y: u32::MAX,
            width: u32::MAX,
            height: u32::MAX,
        };
        assert_eq!(bounds.center(), (u32::MAX, u32::MAX));
    }
}
