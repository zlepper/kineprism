use crate::pyramid::{ImagePyramid, PyramidLevel};
use crate::{Alignment, Offset};

const HYPOTHESIS_COUNT: usize = 8;
const REFINEMENT_RADIUS: i32 = 3;
const MIN_GLOBAL_IMPROVEMENT: f64 = 0.03;
const MIN_RUNNER_UP_SEPARATION: f64 = 0.02;
const MAX_ACCEPTED_SCORE: f64 = 6.0;

#[derive(Clone, Copy, Debug)]
struct Hypothesis {
    offset: Offset,
    score: f64,
}

#[derive(Clone, Copy)]
struct HypothesisSet {
    entries: [Option<Hypothesis>; HYPOTHESIS_COUNT],
}

impl HypothesisSet {
    fn new() -> Self {
        Self {
            entries: [None; HYPOTHESIS_COUNT],
        }
    }

    fn consider(&mut self, candidate: Hypothesis, distinct_distance: u64) {
        if let Some(index) = self.entries.iter().position(|entry| {
            entry.is_some_and(|existing| {
                offset_distance(existing.offset, candidate.offset) < distinct_distance
            })
        }) {
            if self.entries[index].is_none_or(|existing| better(candidate, existing)) {
                self.entries[index] = Some(candidate);
            }
            return;
        }
        if let Some(empty) = self.entries.iter().position(Option::is_none) {
            self.entries[empty] = Some(candidate);
            return;
        }
        let worst = self
            .entries
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| {
                left.expect("full hypothesis set")
                    .score
                    .total_cmp(&right.expect("full hypothesis set").score)
            })
            .map(|(index, _)| index)
            .expect("hypothesis set is non-empty");
        if self.entries[worst].is_none_or(|existing| better(candidate, existing)) {
            self.entries[worst] = Some(candidate);
        }
    }

    fn best(self) -> Option<Hypothesis> {
        self.entries
            .into_iter()
            .flatten()
            .min_by(|left, right| hypothesis_order(*left, *right))
    }

    fn runner_up(self, best: Hypothesis) -> Option<Hypothesis> {
        self.entries
            .into_iter()
            .flatten()
            .filter(|candidate| offset_distance(candidate.offset, best.offset) >= 3)
            .min_by(|left, right| hypothesis_order(*left, *right))
    }
}

pub(crate) fn estimate(
    expected: &ImagePyramid,
    actual: &ImagePyramid,
    maximum_offset: u32,
    raw_pixels_match: bool,
) -> Alignment {
    if raw_pixels_match {
        return Alignment {
            offset: Offset::default(),
            confidence: 1.0,
        };
    }
    let common_levels = expected.levels().len().min(actual.levels().len());
    let Some(coarsest_index) = common_levels.checked_sub(1) else {
        return zero_alignment();
    };
    let mut hypotheses = exhaustive_hypotheses(
        &expected.levels()[coarsest_index],
        &actual.levels()[coarsest_index],
        maximum_offset.div_ceil(level_scale(coarsest_index)),
    );
    for level_index in (0..coarsest_index).rev() {
        hypotheses = refine_hypotheses(
            &expected.levels()[level_index],
            &actual.levels()[level_index],
            hypotheses,
            maximum_offset.div_ceil(level_scale(level_index)),
        );
    }

    let Some(best) = hypotheses.best() else {
        return zero_alignment();
    };
    let base_expected = &expected.levels()[0];
    let base_actual = &actual.levels()[0];
    let zero_score = score(base_expected, base_actual, Offset::default()).unwrap_or(f64::MAX);
    let improvement = relative_improvement(zero_score, best.score);
    let separation = hypotheses.runner_up(best).map_or(1.0, |runner| {
        ((runner.score - best.score) / runner.score.max(0.01)).clamp(0.0, 1.0)
    });
    let quality = (1.0 - best.score / MAX_ACCEPTED_SCORE).clamp(0.0, 1.0);
    if best.offset == Offset::default()
        || best.score > MAX_ACCEPTED_SCORE
        || improvement < MIN_GLOBAL_IMPROVEMENT
        || separation < MIN_RUNNER_UP_SEPARATION
        || !tile_consensus(base_expected, base_actual, best.offset)
    {
        return zero_alignment();
    }

    Alignment {
        offset: best.offset,
        confidence: ((improvement + separation + quality) / 3.0).clamp(0.0, 1.0),
    }
}

fn exhaustive_hypotheses(
    expected: &PyramidLevel,
    actual: &PyramidLevel,
    requested_limit: u32,
) -> HypothesisSet {
    let (x_limit, y_limit) = offset_limits(expected, actual, requested_limit);
    let mut result = HypothesisSet::new();
    for y in -y_limit..=y_limit {
        for x in -x_limit..=x_limit {
            consider_scored(expected, actual, Offset { x, y }, 2, &mut result);
        }
    }
    result
}

fn refine_hypotheses(
    expected: &PyramidLevel,
    actual: &PyramidLevel,
    previous: HypothesisSet,
    requested_limit: u32,
) -> HypothesisSet {
    let (x_limit, y_limit) = offset_limits(expected, actual, requested_limit);
    let mut result = HypothesisSet::new();
    for hypothesis in previous.entries.into_iter().flatten() {
        let center = Offset {
            x: hypothesis.offset.x.saturating_mul(2),
            y: hypothesis.offset.y.saturating_mul(2),
        };
        for delta_y in -REFINEMENT_RADIUS..=REFINEMENT_RADIUS {
            for delta_x in -REFINEMENT_RADIUS..=REFINEMENT_RADIUS {
                let candidate = Offset {
                    x: center.x.saturating_add(delta_x).clamp(-x_limit, x_limit),
                    y: center.y.saturating_add(delta_y).clamp(-y_limit, y_limit),
                };
                consider_scored(expected, actual, candidate, 3, &mut result);
            }
        }
    }
    result
}

fn consider_scored(
    expected: &PyramidLevel,
    actual: &PyramidLevel,
    offset: Offset,
    distinct_distance: u64,
    hypotheses: &mut HypothesisSet,
) {
    if let Some(candidate_score) = score(expected, actual, offset) {
        hypotheses.consider(
            Hypothesis {
                offset,
                score: candidate_score,
            },
            distinct_distance,
        );
    }
}

fn score(expected: &PyramidLevel, actual: &PyramidLevel, offset: Offset) -> Option<f64> {
    let expected_area = u64::from(expected.width()) * u64::from(expected.height());
    let actual_area = u64::from(actual.width()) * u64::from(actual.height());
    let reference_area = expected_area.min(actual_area);
    let overlap = overlap_bounds(expected, actual, offset)?;
    let overlap_area = u64::from(overlap.width) * u64::from(overlap.height);
    if overlap_area.saturating_mul(4) < reference_area {
        return None;
    }
    let mut error = 0.0_f64;
    let mut weight_sum = 0.0_f64;
    for y in overlap.y..overlap.y + overlap.height {
        for x in overlap.x..overlap.x + overlap.width {
            let actual_x = u32::try_from(i64::from(x) + i64::from(offset.x)).ok()?;
            let actual_y = u32::try_from(i64::from(y) + i64::from(offset.y)).ok()?;
            let left = expected.feature(x, y);
            let right = actual.feature(actual_x, actual_y);
            let edge_weight = f64::from(left.edge.max(right.edge));
            let weight = 0.15 + edge_weight * 6.0;
            let color_error = left
                .color
                .iter()
                .zip(right.color)
                .map(|(a, b)| f64::from((*a - b).abs()))
                .sum::<f64>()
                / 4.0;
            error += weight
                * color_error.mul_add(50.0, f64::from((left.edge - right.edge).abs()) * 20.0);
            weight_sum += weight;
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let coverage = overlap_area as f64 / reference_area.max(1) as f64;
    Some(error / weight_sum.max(f64::EPSILON) + (1.0 - coverage) * 2.0)
}

fn tile_consensus(expected: &PyramidLevel, actual: &PyramidLevel, candidate: Offset) -> bool {
    const TILE: u32 = 32;
    let mut candidate_wins = 0_u32;
    let mut decisive_tiles = 0_u32;
    for tile_y in (0..expected.height()).step_by(TILE as usize) {
        for tile_x in (0..expected.width()).step_by(TILE as usize) {
            let bounds = TileBounds {
                x: tile_x,
                y: tile_y,
                width: TILE.min(expected.width() - tile_x),
                height: TILE.min(expected.height() - tile_y),
            };
            if !textured(expected, bounds) {
                continue;
            }
            let (Some(zero), Some(shifted)) = (
                tile_score(expected, actual, bounds, Offset::default()),
                tile_score(expected, actual, bounds, candidate),
            ) else {
                continue;
            };
            let difference = zero - shifted;
            if difference.abs() <= 0.01 {
                continue;
            }
            decisive_tiles += 1;
            candidate_wins += u32::from(difference > 0.0);
        }
    }
    decisive_tiles > 0 && candidate_wins.saturating_mul(4) >= decisive_tiles.saturating_mul(3)
}

#[derive(Clone, Copy)]
struct TileBounds {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

fn textured(level: &PyramidLevel, bounds: TileBounds) -> bool {
    let mut edge = 0.0_f64;
    let mut luma_sum = 0.0_f64;
    let mut luma_squared = 0.0_f64;
    let count = u64::from(bounds.width) * u64::from(bounds.height);
    for y in bounds.y..bounds.y + bounds.height {
        for x in bounds.x..bounds.x + bounds.width {
            let feature = level.feature(x, y);
            let luma = f64::from(luminance(feature.color));
            edge += f64::from(feature.edge);
            luma_sum += luma;
            luma_squared += luma * luma;
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let count = count.max(1) as f64;
    let mean = luma_sum / count;
    edge / count > 0.002 || luma_squared / count - mean * mean > 0.000_02
}

fn tile_score(
    expected: &PyramidLevel,
    actual: &PyramidLevel,
    bounds: TileBounds,
    offset: Offset,
) -> Option<f64> {
    let mut error = 0.0_f64;
    let mut count = 0_u64;
    for y in bounds.y..bounds.y + bounds.height {
        for x in bounds.x..bounds.x + bounds.width {
            let actual_x = i64::from(x) + i64::from(offset.x);
            let actual_y = i64::from(y) + i64::from(offset.y);
            if actual_x < 0
                || actual_y < 0
                || actual_x >= i64::from(actual.width())
                || actual_y >= i64::from(actual.height())
            {
                continue;
            }
            let right =
                actual.feature(u32::try_from(actual_x).ok()?, u32::try_from(actual_y).ok()?);
            let left = expected.feature(x, y);
            error += left
                .color
                .iter()
                .zip(right.color)
                .map(|(a, b)| f64::from((*a - b).abs()))
                .sum::<f64>();
            error += f64::from((left.edge - right.edge).abs()) * 2.0;
            count += 1;
        }
    }
    (count > 0).then(|| {
        #[allow(clippy::cast_precision_loss)]
        let count = count as f64;
        error / count
    })
}

fn overlap_bounds(
    expected: &PyramidLevel,
    actual: &PyramidLevel,
    offset: Offset,
) -> Option<TileBounds> {
    let left = (-i64::from(offset.x)).max(0);
    let top = (-i64::from(offset.y)).max(0);
    let right = i64::from(expected.width()).min(i64::from(actual.width()) - i64::from(offset.x));
    let bottom = i64::from(expected.height()).min(i64::from(actual.height()) - i64::from(offset.y));
    if right <= left || bottom <= top {
        return None;
    }
    Some(TileBounds {
        x: u32::try_from(left).ok()?,
        y: u32::try_from(top).ok()?,
        width: u32::try_from(right - left).ok()?,
        height: u32::try_from(bottom - top).ok()?,
    })
}

fn offset_limits(expected: &PyramidLevel, actual: &PyramidLevel, requested: u32) -> (i32, i32) {
    let x = requested.min(expected.width().max(actual.width()).saturating_sub(1));
    let y = requested.min(expected.height().max(actual.height()).saturating_sub(1));
    (
        i32::try_from(x).unwrap_or(i32::MAX),
        i32::try_from(y).unwrap_or(i32::MAX),
    )
}

fn relative_improvement(baseline: f64, candidate: f64) -> f64 {
    if !baseline.is_finite() || !candidate.is_finite() {
        return 0.0;
    }
    (baseline - candidate) / baseline.max(0.01)
}

fn zero_alignment() -> Alignment {
    Alignment {
        offset: Offset::default(),
        confidence: 0.0,
    }
}

fn better(left: Hypothesis, right: Hypothesis) -> bool {
    hypothesis_order(left, right).is_lt()
}

fn hypothesis_order(left: Hypothesis, right: Hypothesis) -> std::cmp::Ordering {
    left.score
        .total_cmp(&right.score)
        .then_with(|| offset_magnitude(left.offset).cmp(&offset_magnitude(right.offset)))
        .then_with(|| left.offset.y.cmp(&right.offset.y))
        .then_with(|| left.offset.x.cmp(&right.offset.x))
}

fn level_scale(index: usize) -> u32 {
    1_u32
        .checked_shl(u32::try_from(index).unwrap_or(u32::MAX))
        .unwrap_or(u32::MAX)
}

fn offset_magnitude(offset: Offset) -> u64 {
    u64::from(offset.x.unsigned_abs()) + u64::from(offset.y.unsigned_abs())
}

fn offset_distance(left: Offset, right: Offset) -> u64 {
    (i64::from(left.x) - i64::from(right.x)).unsigned_abs()
        + (i64::from(left.y) - i64::from(right.y)).unsigned_abs()
}

fn luminance(color: [f32; 4]) -> f32 {
    color[0].mul_add(0.2126, color[1].mul_add(0.7152, color[2] * 0.0722))
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};

    use crate::color::NormalizedImage;

    use super::*;

    fn normalized(image: &RgbaImage) -> NormalizedImage {
        NormalizedImage::try_new(image).expect("normalize")
    }

    #[test]
    fn finds_clipped_negative_translation() {
        let mut expected = RgbaImage::from_pixel(80, 50, Rgba([240, 240, 240, 255]));
        let mut actual = RgbaImage::from_pixel(80, 50, Rgba([240, 240, 240, 255]));
        for y in 8..42 {
            for x in 12..70 {
                expected.put_pixel(x, y, Rgba([30, 80, 160, 255]));
            }
        }
        for y in 5..39 {
            for x in 8..66 {
                actual.put_pixel(x, y, Rgba([30, 80, 160, 255]));
            }
        }
        let expected = normalized(&expected);
        let actual = normalized(&actual);
        let alignment = estimate(
            &ImagePyramid::try_new(&expected).expect("expected pyramid"),
            &ImagePyramid::try_new(&actual).expect("actual pyramid"),
            16,
            false,
        );

        assert_eq!(alignment.offset, Offset { x: -4, y: -3 });
    }

    #[test]
    fn huge_requested_offset_is_bounded_by_tiny_images() {
        let expected = normalized(&RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 255])));
        let actual = normalized(&RgbaImage::from_pixel(1, 1, Rgba([255, 255, 255, 255])));
        let alignment = estimate(
            &ImagePyramid::try_new(&expected).expect("expected pyramid"),
            &ImagePyramid::try_new(&actual).expect("actual pyramid"),
            i32::MAX as u32,
            false,
        );

        assert_eq!(alignment.offset, Offset::default());
    }

    #[test]
    fn repeated_pattern_is_ambiguous() {
        let mut expected = RgbaImage::new(64, 32);
        let mut actual = RgbaImage::new(64, 32);
        for y in 0..32 {
            for x in 0..64 {
                let value = if (x / 8) % 2 == 0 { 40 } else { 220 };
                expected.put_pixel(x, y, Rgba([value, value, value, 255]));
                let shifted_x = (x + 8) % 64;
                actual.put_pixel(shifted_x, y, Rgba([value, value, value, 255]));
            }
        }
        let expected = normalized(&expected);
        let actual = normalized(&actual);
        let alignment = estimate(
            &ImagePyramid::try_new(&expected).expect("expected pyramid"),
            &ImagePyramid::try_new(&actual).expect("actual pyramid"),
            24,
            false,
        );

        assert_eq!(alignment.offset, Offset::default());
        assert!(alignment.confidence <= f64::EPSILON);
    }
}
