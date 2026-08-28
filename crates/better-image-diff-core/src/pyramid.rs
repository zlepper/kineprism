use crate::CompareError;
use crate::color::NormalizedImage;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Feature {
    pub(crate) color: [f32; 4],
    pub(crate) edge: f32,
}

#[derive(Debug, Clone)]
pub(crate) struct PyramidLevel {
    width: u32,
    height: u32,
    features: Vec<Feature>,
}

impl PyramidLevel {
    pub(crate) fn width(&self) -> u32 {
        self.width
    }

    pub(crate) fn height(&self) -> u32 {
        self.height
    }

    pub(crate) fn feature(&self, x: u32, y: u32) -> Feature {
        self.features[index(self.width, x, y)]
    }

    pub(crate) fn row(&self, y: u32) -> &[Feature] {
        let start = index(self.width, 0, y);
        let end = start
            .checked_add(usize::try_from(self.width).expect("pyramid width fits usize"))
            .expect("validated pyramid row");
        self.features
            .get(start..end)
            .expect("validated pyramid row")
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ImagePyramid {
    levels: Vec<PyramidLevel>,
}

impl ImagePyramid {
    pub(crate) fn try_new(image: &NormalizedImage) -> Result<Self, CompareError> {
        let mut levels = Vec::new();
        levels
            .try_reserve(1)
            .map_err(|_error| CompareError::ImageTooLarge)?;
        levels.push(base_level(image)?);
        while levels
            .last()
            .is_some_and(|level| level.width > 32 || level.height > 32)
        {
            let next = downsample(levels.last().expect("pyramid has a base level"))?;
            if next.width == levels.last().expect("pyramid has a base level").width
                && next.height == levels.last().expect("pyramid has a base level").height
            {
                break;
            }
            levels
                .try_reserve(1)
                .map_err(|_error| CompareError::ImageTooLarge)?;
            levels.push(next);
        }
        Ok(Self { levels })
    }

    pub(crate) fn levels(&self) -> &[PyramidLevel] {
        &self.levels
    }
}

fn base_level(image: &NormalizedImage) -> Result<PyramidLevel, CompareError> {
    let area = usize::try_from(u64::from(image.width()) * u64::from(image.height()))
        .map_err(|_error| CompareError::ImageTooLarge)?;
    let mut features = Vec::new();
    features
        .try_reserve_exact(area)
        .map_err(|_error| CompareError::ImageTooLarge)?;
    for y in 0..image.height() {
        for x in 0..image.width() {
            let pixel = image.pixel(x, y);
            features.push(Feature {
                color: [
                    pixel.channel_f32(0),
                    pixel.channel_f32(1),
                    pixel.channel_f32(2),
                    pixel.channel_f32(3),
                ],
                edge: 0.0,
            });
        }
    }
    let mut level = PyramidLevel {
        width: image.width(),
        height: image.height(),
        features,
    };
    calculate_edges(&mut level);
    Ok(level)
}

fn downsample(source: &PyramidLevel) -> Result<PyramidLevel, CompareError> {
    let width = source.width.div_ceil(2);
    let height = source.height.div_ceil(2);
    let area = usize::try_from(u64::from(width) * u64::from(height))
        .map_err(|_error| CompareError::ImageTooLarge)?;
    let mut features = Vec::new();
    features
        .try_reserve_exact(area)
        .map_err(|_error| CompareError::ImageTooLarge)?;
    for y in 0..height {
        for x in 0..width {
            let mut color = [0.0_f32; 4];
            let mut samples = 0.0_f32;
            for source_y in y * 2..(y * 2 + 2).min(source.height) {
                for source_x in x * 2..(x * 2 + 2).min(source.width) {
                    let feature = source.feature(source_x, source_y);
                    for (sum, value) in color.iter_mut().zip(feature.color) {
                        *sum += value;
                    }
                    samples += 1.0;
                }
            }
            for value in &mut color {
                *value /= samples;
            }
            features.push(Feature { color, edge: 0.0 });
        }
    }
    let mut level = PyramidLevel {
        width,
        height,
        features,
    };
    calculate_edges(&mut level);
    Ok(level)
}

fn calculate_edges(level: &mut PyramidLevel) {
    if level.width == 0 || level.height == 0 {
        return;
    }
    for y in 0..level.height {
        for x in 0..level.width {
            let left = luminance(level.features[index(level.width, x.saturating_sub(1), y)].color);
            let right = luminance(
                level.features[index(level.width, (x + 1).min(level.width - 1), y)].color,
            );
            let top = luminance(level.features[index(level.width, x, y.saturating_sub(1))].color);
            let bottom = luminance(
                level.features[index(level.width, x, (y + 1).min(level.height - 1))].color,
            );
            let horizontal = right - left;
            let vertical = bottom - top;
            level.features[index(level.width, x, y)].edge = horizontal.hypot(vertical);
        }
    }
}

fn luminance(color: [f32; 4]) -> f32 {
    color[0].mul_add(0.2126, color[1].mul_add(0.7152, color[2] * 0.0722))
}

fn index(width: u32, x: u32, y: u32) -> usize {
    usize::try_from(u64::from(y) * u64::from(width) + u64::from(x))
        .expect("validated pyramid index")
}

#[cfg(test)]
mod tests {
    use image::RgbaImage;

    use super::*;

    #[test]
    fn odd_dimensions_reduce_with_ceiling_division() {
        let image = RgbaImage::new(65, 33);
        let normalized = NormalizedImage::try_new(&image).expect("normalize");
        let pyramid = ImagePyramid::try_new(&normalized).expect("pyramid");
        let sizes: Vec<_> = pyramid
            .levels()
            .iter()
            .map(|level| (level.width(), level.height()))
            .collect();

        assert_eq!(sizes, vec![(65, 33), (33, 17), (17, 9)]);
    }
}
