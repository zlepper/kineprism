use crate::color::NormalizedImage;
use crate::{CompareError, Offset};

const UNMAPPED: u64 = u64::MAX;

#[derive(Debug, Clone)]
pub(crate) struct PixelMapping {
    expected_width: u32,
    expected_height: u32,
    actual_indices: Vec<u64>,
    compared_pixels: u64,
}

impl PixelMapping {
    pub(crate) fn translated(
        expected: &NormalizedImage,
        actual: &NormalizedImage,
        offset: Offset,
    ) -> Result<Self, CompareError> {
        let area = u64::from(expected.width()) * u64::from(expected.height());
        let area = usize::try_from(area).map_err(|_error| CompareError::ImageTooLarge)?;
        let mut actual_indices = Vec::new();
        actual_indices
            .try_reserve_exact(area)
            .map_err(|_error| CompareError::ImageTooLarge)?;
        actual_indices.resize(area, UNMAPPED);
        let mut compared_pixels = 0_u64;

        for expected_y in 0..expected.height() {
            for expected_x in 0..expected.width() {
                let actual_x = i64::from(expected_x) + i64::from(offset.x);
                let actual_y = i64::from(expected_y) + i64::from(offset.y);
                if actual_x < 0
                    || actual_y < 0
                    || actual_x >= i64::from(actual.width())
                    || actual_y >= i64::from(actual.height())
                {
                    continue;
                }
                let actual_x = u32::try_from(actual_x).expect("checked actual x");
                let actual_y = u32::try_from(actual_y).expect("checked actual y");
                let expected_index = expected.index(expected_x, expected_y);
                actual_indices[expected_index] =
                    u64::try_from(actual.index(actual_x, actual_y)).expect("actual index fits u64");
                compared_pixels += 1;
            }
        }

        Ok(Self {
            expected_width: expected.width(),
            expected_height: expected.height(),
            actual_indices,
            compared_pixels,
        })
    }

    pub(crate) fn actual_index(&self, expected_x: u32, expected_y: u32) -> Option<usize> {
        if expected_x >= self.expected_width || expected_y >= self.expected_height {
            return None;
        }
        let index = usize::try_from(
            u64::from(expected_y) * u64::from(self.expected_width) + u64::from(expected_x),
        )
        .expect("validated expected index");
        let actual = self.actual_indices[index];
        (actual != UNMAPPED).then(|| usize::try_from(actual).expect("actual index fits usize"))
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.actual_indices
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, actual)| *actual != UNMAPPED)
            .map(|(expected, actual)| {
                (
                    expected,
                    usize::try_from(actual).expect("actual index fits usize"),
                )
            })
    }

    pub(crate) fn compared_pixels(&self) -> u64 {
        self.compared_pixels
    }

    pub(crate) fn dimensions(&self) -> (u32, u32) {
        (self.expected_width, self.expected_height)
    }
}

#[cfg(test)]
mod tests {
    use image::RgbaImage;

    use super::*;

    fn normalized(width: u32, height: u32) -> NormalizedImage {
        NormalizedImage::try_new(&RgbaImage::new(width, height)).expect("normalize")
    }

    #[test]
    fn positive_offset_maps_expected_to_actual_and_crops_the_right_edge() {
        let expected = normalized(3, 1);
        let actual = normalized(3, 1);
        let mapping =
            PixelMapping::translated(&expected, &actual, Offset { x: 1, y: 0 }).expect("mapping");

        assert_eq!(mapping.compared_pixels(), 2);
        assert_eq!(mapping.actual_index(0, 0), Some(1));
        assert_eq!(mapping.actual_index(1, 0), Some(2));
        assert_eq!(mapping.actual_index(2, 0), None);
    }

    #[test]
    fn negative_offset_crops_the_left_edge() {
        let expected = normalized(3, 1);
        let actual = normalized(3, 1);
        let mapping =
            PixelMapping::translated(&expected, &actual, Offset { x: -1, y: 0 }).expect("mapping");

        assert_eq!(mapping.compared_pixels(), 2);
        assert_eq!(mapping.actual_index(0, 0), None);
        assert_eq!(mapping.actual_index(1, 0), Some(0));
        assert_eq!(mapping.actual_index(2, 0), Some(1));
    }
}
