use crate::color::NormalizedImage;
use crate::local_geometry::{horizontal_gap, nearby, padded, union, vertical_overlap};
use crate::mask::Mask;
use crate::proposals;
use crate::{Bounds, CompareError, CompareOptions, Offset};

const MAX_PROPOSALS: usize = 64;
const HYPOTHESIS_COUNT: usize = 8;
const CONTEXT: u32 = 4;
const GROUPING_GAP: u32 = 10;
const MAX_MATCH_SCORE: f64 = 6.0;
const MIN_BASELINE_IMPROVEMENT: f64 = 1.0;
const MIN_RUNNER_MARGIN: f64 = 0.2;

#[derive(Debug, Clone, Copy)]
pub(crate) struct LocalMovement {
    pub(crate) expected_bounds: Bounds,
    pub(crate) actual_bounds: Bounds,
    pub(crate) offset: Offset,
    pub(crate) confidence: f64,
}

#[derive(Clone, Copy, Debug)]
struct Candidate {
    offset: Offset,
    score: f64,
}

#[derive(Clone, Copy)]
struct Candidates {
    entries: [Option<Candidate>; HYPOTHESIS_COUNT],
}

impl Candidates {
    fn new() -> Self {
        Self {
            entries: [None; HYPOTHESIS_COUNT],
        }
    }

    fn consider(&mut self, candidate: Candidate) {
        if let Some(index) = self.entries.iter().position(|entry| {
            entry.is_some_and(|existing| distance(existing.offset, candidate.offset) < 1)
        }) {
            if self.entries[index].is_none_or(|existing| candidate_better(candidate, existing)) {
                self.entries[index] = Some(candidate);
            }
            return;
        }
        if let Some(index) = self.entries.iter().position(Option::is_none) {
            self.entries[index] = Some(candidate);
            return;
        }
        let worst = self
            .entries
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| {
                left.expect("full candidates")
                    .score
                    .total_cmp(&right.expect("full candidates").score)
            })
            .map(|(index, _)| index)
            .expect("candidate set is non-empty");
        if self.entries[worst].is_none_or(|existing| candidate_better(candidate, existing)) {
            self.entries[worst] = Some(candidate);
        }
    }

    fn best(self) -> Option<Candidate> {
        self.entries
            .into_iter()
            .flatten()
            .min_by(|left, right| candidate_order(*left, *right))
    }

    fn runner(self, best: Candidate) -> Option<Candidate> {
        self.entries
            .into_iter()
            .flatten()
            .filter(|candidate| distance(candidate.offset, best.offset) >= 3)
            .min_by(|left, right| candidate_order(*left, *right))
    }
}

pub(crate) fn detect(
    expected: &NormalizedImage,
    actual: &NormalizedImage,
    global: Offset,
    options: &CompareOptions,
) -> Result<Vec<LocalMovement>, CompareError> {
    let residuals = residual_proposals(expected, actual, global, options)?;
    if residuals.len() > MAX_PROPOSALS {
        return Ok(Vec::new());
    }
    let mut movements = Vec::new();
    for expected_bounds in &residuals {
        let expected_bounds = *expected_bounds;
        if !distinctive(expected, expected_bounds) {
            continue;
        }
        let candidates = search(
            expected,
            expected_bounds,
            actual,
            global,
            options.max_offset,
        );
        let candidates = full_resolution_candidates(expected, expected_bounds, actual, candidates);
        let Some(best) = candidates.best() else {
            continue;
        };
        let mut matched_bounds = expected_bounds;
        let mut match_score =
            full_patch_score(expected, expected_bounds, actual, best.offset).unwrap_or(100.0);
        let mut baseline =
            full_patch_score(expected, expected_bounds, actual, global).unwrap_or(100.0);
        let runner_margin = candidates
            .runner(best)
            .map_or(100.0, |runner| runner.score - best.score);
        if best.offset == global
            || match_score > MAX_MATCH_SCORE
            || baseline - match_score < MIN_BASELINE_IMPROVEMENT
            || runner_margin < MIN_RUNNER_MARGIN
        {
            continue;
        }
        for related in &residuals {
            if !vertical_overlap(matched_bounds, *related)
                || horizontal_gap(matched_bounds, *related) > 64
            {
                continue;
            }
            let combined = union(matched_bounds, *related);
            let Some(combined_score) = full_patch_score(expected, combined, actual, best.offset)
            else {
                continue;
            };
            let combined_baseline =
                full_patch_score(expected, combined, actual, global).unwrap_or(100.0);
            if combined_score <= 12.0
                && combined_baseline - combined_score >= MIN_BASELINE_IMPROVEMENT
            {
                matched_bounds = combined;
                match_score = combined_score;
                baseline = combined_baseline;
            }
        }
        let Some(actual_bounds) = translate_full(matched_bounds, best.offset, actual) else {
            continue;
        };
        if movements.iter().any(|movement: &LocalMovement| {
            movement.actual_bounds.intersection(actual_bounds).is_some()
        }) {
            continue;
        }
        let reverse_global = Offset {
            x: global.x.saturating_neg(),
            y: global.y.saturating_neg(),
        };
        let reverse = search(
            actual,
            actual_bounds,
            expected,
            reverse_global,
            options.max_offset,
        )
        .best();
        let expected_reverse = Offset {
            x: best.offset.x.saturating_neg(),
            y: best.offset.y.saturating_neg(),
        };
        if reverse.is_none_or(|candidate| distance(candidate.offset, expected_reverse) > 2) {
            continue;
        }
        let quality = (1.0 - match_score / MAX_MATCH_SCORE).clamp(0.0, 1.0);
        let improvement = ((baseline - match_score) / baseline.max(0.01)).clamp(0.0, 1.0);
        let separation = (runner_margin / (match_score + runner_margin).max(0.01)).clamp(0.0, 1.0);
        movements
            .try_reserve(1)
            .map_err(|_error| CompareError::ImageTooLarge)?;
        movements.push(LocalMovement {
            expected_bounds: matched_bounds,
            actual_bounds,
            offset: best.offset,
            confidence: ((quality + improvement + separation + 1.0) / 4.0).clamp(0.0, 1.0),
        });
    }
    Ok(movements)
}

fn residual_proposals(
    expected: &NormalizedImage,
    actual: &NormalizedImage,
    global: Offset,
    options: &CompareOptions,
) -> Result<Vec<Bounds>, CompareError> {
    let mut mask = Mask::try_new(expected.width(), expected.height())?;
    for y in 0..expected.height() {
        for x in 0..expected.width() {
            let actual_x = i64::from(x) + i64::from(global.x);
            let actual_y = i64::from(y) + i64::from(global.y);
            if actual_x < 0
                || actual_y < 0
                || actual_x >= i64::from(actual.width())
                || actual_y >= i64::from(actual.height())
            {
                continue;
            }
            let actual_x = u32::try_from(actual_x).expect("checked local residual x");
            let actual_y = u32::try_from(actual_y).expect("checked local residual y");
            if expected
                .pixel(x, y)
                .perceptual_distance(actual.pixel(actual_x, actual_y))
                > options.color_threshold
            {
                mask.set(x, y, true);
            }
        }
    }
    let components = mask.components(options.min_region_area)?;
    let mut grouped = Vec::new();
    for component in components {
        let mut bounds = component.bounds;
        let mut index = 0;
        while index < grouped.len() {
            if nearby(bounds, grouped[index], GROUPING_GAP) {
                bounds = union(bounds, grouped.swap_remove(index));
                index = 0;
            } else {
                index += 1;
            }
        }
        grouped
            .try_reserve(1)
            .map_err(|_error| CompareError::ImageTooLarge)?;
        grouped.push(bounds);
    }
    let initial_count = grouped.len();
    for left in 0..initial_count {
        for right in left + 1..initial_count {
            if vertical_overlap(grouped[left], grouped[right])
                && horizontal_gap(grouped[left], grouped[right]) <= 64
            {
                grouped
                    .try_reserve(1)
                    .map_err(|_error| CompareError::ImageTooLarge)?;
                grouped.push(union(grouped[left], grouped[right]));
            }
        }
    }
    for bounds in &mut grouped {
        *bounds = padded(*bounds, CONTEXT, expected.width(), expected.height());
    }
    grouped.sort_by_key(|bounds| {
        (
            std::cmp::Reverse(bounds.area()),
            bounds.y,
            bounds.x,
            bounds.width,
            bounds.height,
        )
    });
    Ok(grouped)
}

fn search(
    source: &NormalizedImage,
    source_bounds: Bounds,
    target: &NormalizedImage,
    global: Offset,
    maximum_local: u32,
) -> Candidates {
    let limit_x =
        i32::try_from(maximum_local.min(source.width().max(target.width()))).unwrap_or(i32::MAX);
    let limit_y =
        i32::try_from(maximum_local.min(source.height().max(target.height()))).unwrap_or(i32::MAX);
    let coarse_step = (maximum_local / 16).max(1);
    let coarse_step = i32::try_from(coarse_step).unwrap_or(i32::MAX);
    let mut candidates = Candidates::new();
    scan_grid(
        source,
        source_bounds,
        target,
        global,
        limit_x,
        limit_y,
        coarse_step,
        &mut candidates,
    );
    let middle_step = (coarse_step / 4).max(1);
    candidates = refine(
        source,
        source_bounds,
        target,
        candidates,
        global,
        limit_x,
        limit_y,
        middle_step,
        coarse_step,
    );
    if middle_step > 1 {
        candidates = refine(
            source,
            source_bounds,
            target,
            candidates,
            global,
            limit_x,
            limit_y,
            1,
            middle_step,
        );
    }
    candidates
}

fn full_resolution_candidates(
    source: &NormalizedImage,
    source_bounds: Bounds,
    target: &NormalizedImage,
    candidates: Candidates,
) -> Candidates {
    let mut result = Candidates::new();
    for candidate in candidates.entries.into_iter().flatten() {
        if let Some(score) = full_patch_score(source, source_bounds, target, candidate.offset) {
            result.consider(Candidate {
                offset: candidate.offset,
                score,
            });
        }
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn scan_grid(
    source: &NormalizedImage,
    source_bounds: Bounds,
    target: &NormalizedImage,
    global: Offset,
    limit_x: i32,
    limit_y: i32,
    step: i32,
    candidates: &mut Candidates,
) {
    let step = usize::try_from(step).unwrap_or(1);
    for delta_y in (-limit_y..=limit_y).step_by(step) {
        for delta_x in (-limit_x..=limit_x).step_by(step) {
            let offset = Offset {
                x: global.x.saturating_add(delta_x),
                y: global.y.saturating_add(delta_y),
            };
            consider_patch(source, source_bounds, target, offset, candidates);
        }
    }
    consider_patch(source, source_bounds, target, global, candidates);
}

#[allow(clippy::too_many_arguments)]
fn refine(
    source: &NormalizedImage,
    source_bounds: Bounds,
    target: &NormalizedImage,
    previous: Candidates,
    global: Offset,
    limit_x: i32,
    limit_y: i32,
    step: i32,
    radius: i32,
) -> Candidates {
    let mut result = Candidates::new();
    for hypothesis in previous.entries.into_iter().flatten() {
        for delta_y in (-radius..=radius).step_by(usize::try_from(step).unwrap_or(1)) {
            for delta_x in (-radius..=radius).step_by(usize::try_from(step).unwrap_or(1)) {
                let local_x = (i64::from(hypothesis.offset.x) - i64::from(global.x)
                    + i64::from(delta_x))
                .clamp(-i64::from(limit_x), i64::from(limit_x));
                let local_y = (i64::from(hypothesis.offset.y) - i64::from(global.y)
                    + i64::from(delta_y))
                .clamp(-i64::from(limit_y), i64::from(limit_y));
                let offset = Offset {
                    x: global
                        .x
                        .saturating_add(i32::try_from(local_x).unwrap_or(i32::MAX)),
                    y: global
                        .y
                        .saturating_add(i32::try_from(local_y).unwrap_or(i32::MAX)),
                };
                consider_patch(source, source_bounds, target, offset, &mut result);
            }
        }
    }
    result
}

fn consider_patch(
    source: &NormalizedImage,
    source_bounds: Bounds,
    target: &NormalizedImage,
    offset: Offset,
    candidates: &mut Candidates,
) {
    if let Some(score) = patch_score(source, source_bounds, target, offset) {
        candidates.consider(Candidate { offset, score });
    }
}

fn patch_score(
    source: &NormalizedImage,
    source_bounds: Bounds,
    target: &NormalizedImage,
    offset: Offset,
) -> Option<f64> {
    let target_bounds = translate_full(source_bounds, offset, target)?;
    Some(proposals::scaled_patch_score(
        source,
        source_bounds,
        target,
        target_bounds,
    ))
}

fn full_patch_score(
    source: &NormalizedImage,
    source_bounds: Bounds,
    target: &NormalizedImage,
    offset: Offset,
) -> Option<f64> {
    let target_bounds = translate_full(source_bounds, offset, target)?;
    let mut total = 0.0_f64;
    for y in 0..source_bounds.height {
        for x in 0..source_bounds.width {
            total += source
                .pixel(source_bounds.x + x, source_bounds.y + y)
                .perceptual_distance(target.pixel(target_bounds.x + x, target_bounds.y + y))
                .min(100.0);
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let area = source_bounds.area().max(1) as f64;
    Some(total / area)
}

fn translate_full(bounds: Bounds, offset: Offset, target: &NormalizedImage) -> Option<Bounds> {
    let x = i64::from(bounds.x) + i64::from(offset.x);
    let y = i64::from(bounds.y) + i64::from(offset.y);
    let x = u32::try_from(x).ok()?;
    let y = u32::try_from(y).ok()?;
    let translated = Bounds {
        x,
        y,
        width: bounds.width,
        height: bounds.height,
    };
    (translated.right() <= target.width() && translated.bottom() <= target.height())
        .then_some(translated)
}

fn distinctive(image: &NormalizedImage, bounds: Bounds) -> bool {
    const GRID: u32 = 8;
    let first = image.pixel(bounds.x, bounds.y);
    let mut variation = 0.0_f64;
    for grid_y in 0..GRID {
        for grid_x in 0..GRID {
            let x = bounds.x
                + u32::try_from(u64::from(bounds.width - 1) * u64::from(grid_x) / 7).unwrap_or(0);
            let y = bounds.y
                + u32::try_from(u64::from(bounds.height - 1) * u64::from(grid_y) / 7).unwrap_or(0);
            variation += first.perceptual_distance(image.pixel(x, y));
        }
    }
    variation / f64::from(GRID * GRID) > 0.5
}

fn candidate_better(left: Candidate, right: Candidate) -> bool {
    candidate_order(left, right).is_lt()
}

fn candidate_order(left: Candidate, right: Candidate) -> std::cmp::Ordering {
    left.score
        .total_cmp(&right.score)
        .then_with(|| magnitude(left.offset).cmp(&magnitude(right.offset)))
        .then_with(|| left.offset.y.cmp(&right.offset.y))
        .then_with(|| left.offset.x.cmp(&right.offset.x))
}

fn magnitude(offset: Offset) -> u64 {
    u64::from(offset.x.unsigned_abs()) + u64::from(offset.y.unsigned_abs())
}

fn distance(left: Offset, right: Offset) -> u64 {
    (i64::from(left.x) - i64::from(right.x)).unsigned_abs()
        + (i64::from(left.y) - i64::from(right.y)).unsigned_abs()
}
