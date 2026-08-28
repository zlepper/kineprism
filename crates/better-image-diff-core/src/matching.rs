use std::collections::BTreeMap;

use crate::color::NormalizedImage;
use crate::proposals::{self, Proposal};
use crate::{Bounds, CompareError, Offset};

pub(crate) const MATCH_SCORE_LIMIT: f64 = 12.0;
const DISTINCT_SCORE_MARGIN: f64 = 0.2;
const MOVE_IMPROVEMENT: f64 = 1.0;
const MAX_COARSE_CANDIDATES: usize = 16;
const MAX_PROPOSALS_PER_SIDE: usize = 512;
const MAX_TOTAL_CANDIDATE_INSPECTIONS: usize = 65_536;

#[derive(Debug, Clone, Copy)]
pub(crate) struct ProposalPair {
    pub(crate) expected: usize,
    pub(crate) actual: usize,
    pub(crate) score: f64,
    pub(crate) baseline_score: f64,
    pub(crate) ambiguous: bool,
}

impl ProposalPair {
    pub(crate) fn validates_movement(self, offset: Offset, global: Offset) -> bool {
        !self.ambiguous
            && self.score <= MATCH_SCORE_LIMIT
            && offset != global
            && self.baseline_score - self.score >= MOVE_IMPROVEMENT
    }
}

#[derive(Debug, Clone, Copy)]
struct Candidate {
    expected: usize,
    actual: usize,
    score: f64,
}

#[derive(Debug, Clone, Copy)]
struct CoarseCandidate {
    actual: usize,
    score: f64,
}

pub(crate) fn pair_proposals(
    expected: &NormalizedImage,
    expected_proposals: &[Proposal],
    actual: &NormalizedImage,
    actual_proposals: &[Proposal],
    global_offset: Offset,
    max_local_offset: u32,
) -> Result<Vec<ProposalPair>, CompareError> {
    if !within_pairing_budget(expected_proposals.len(), actual_proposals.len()) {
        return Ok(Vec::new());
    }
    let candidates = bounded_candidates(
        expected,
        expected_proposals,
        actual,
        actual_proposals,
        global_offset,
        max_local_offset,
    )?;
    let expected_rankings = rankings(&candidates, expected_proposals.len(), |candidate| {
        candidate.expected
    })?;
    let actual_rankings = rankings(&candidates, actual_proposals.len(), |candidate| {
        candidate.actual
    })?;
    let mut pairs = Vec::new();
    for (expected_index, ranking) in expected_rankings.iter().enumerate() {
        let Some(best_index) = ranking.first().copied() else {
            continue;
        };
        let best = candidates[best_index];
        let reverse_best = actual_rankings[best.actual]
            .first()
            .copied()
            .map(|index| candidates[index].expected);
        if reverse_best != Some(expected_index) {
            continue;
        }
        let runner_up_close = ranking
            .get(1)
            .is_some_and(|index| candidates[*index].score <= best.score + DISTINCT_SCORE_MARGIN);
        let reverse_runner_up_close = actual_rankings[best.actual]
            .get(1)
            .is_some_and(|index| candidates[*index].score <= best.score + DISTINCT_SCORE_MARGIN);
        let expected_bounds = expected_proposals[best.expected].bounds;
        pairs
            .try_reserve(1)
            .map_err(|_error| CompareError::ImageTooLarge)?;
        pairs.push(ProposalPair {
            expected: best.expected,
            actual: best.actual,
            score: best.score,
            baseline_score: globally_aligned_baseline(
                expected,
                expected_bounds,
                actual,
                global_offset,
            ),
            ambiguous: runner_up_close || reverse_runner_up_close,
        });
    }
    append_aligned_fallbacks(
        expected,
        expected_proposals,
        actual,
        actual_proposals,
        global_offset,
        &candidates,
        &mut pairs,
    )?;
    pairs.sort_by_key(|pair| pair.expected);
    Ok(pairs)
}

pub(crate) fn within_pairing_budget(expected_count: usize, actual_count: usize) -> bool {
    expected_count <= MAX_PROPOSALS_PER_SIDE
        && actual_count <= MAX_PROPOSALS_PER_SIDE
        && expected_count
            .checked_mul(actual_count)
            .is_some_and(|total| total <= MAX_TOTAL_CANDIDATE_INSPECTIONS)
}

fn bounded_candidates(
    expected: &NormalizedImage,
    expected_proposals: &[Proposal],
    actual: &NormalizedImage,
    actual_proposals: &[Proposal],
    global_offset: Offset,
    max_local_offset: u32,
) -> Result<Vec<Candidate>, CompareError> {
    let buckets = ProposalBuckets::new(actual_proposals, max_local_offset)?;
    let mut candidates = Vec::new();
    for (expected_index, expected_proposal) in expected_proposals.iter().enumerate() {
        let expected_actual_x = i64::from(expected_proposal.bounds.x) + i64::from(global_offset.x);
        let expected_actual_y = i64::from(expected_proposal.bounds.y) + i64::from(global_offset.y);
        let mut coarse = [None; MAX_COARSE_CANDIDATES];
        for actual_index in buckets.near(expected_actual_x, expected_actual_y) {
            let actual_proposal = actual_proposals[actual_index];
            let absolute_offset = bounds_offset(expected_proposal.bounds, actual_proposal.bounds);
            if !within_local_offset(absolute_offset, global_offset, max_local_offset) {
                continue;
            }
            consider_coarse(
                &mut coarse,
                CoarseCandidate {
                    actual: actual_index,
                    score: coarse_geometry_score(
                        expected_proposal.bounds,
                        actual_proposal.bounds,
                        absolute_offset,
                        global_offset,
                        max_local_offset,
                    ) + proposals::coarse_patch_score(
                        expected,
                        expected_proposal.bounds,
                        actual,
                        actual_proposal.bounds,
                    ),
                },
            );
        }
        for coarse_candidate in coarse.into_iter().flatten() {
            let actual_proposal = actual_proposals[coarse_candidate.actual];
            candidates
                .try_reserve(1)
                .map_err(|_error| CompareError::ImageTooLarge)?;
            candidates.push(Candidate {
                expected: expected_index,
                actual: coarse_candidate.actual,
                score: proposals::scaled_patch_score(
                    expected,
                    expected_proposal.bounds,
                    actual,
                    actual_proposal.bounds,
                ),
            });
        }
    }

    Ok(candidates)
}

fn consider_coarse(
    candidates: &mut [Option<CoarseCandidate>; MAX_COARSE_CANDIDATES],
    candidate: CoarseCandidate,
) {
    if let Some(empty) = candidates.iter().position(Option::is_none) {
        candidates[empty] = Some(candidate);
        return;
    }
    let worst = candidates
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| {
            left.expect("full coarse candidates")
                .score
                .total_cmp(&right.expect("full coarse candidates").score)
        })
        .map(|(index, _)| index)
        .expect("coarse candidate set is non-empty");
    if candidates[worst].is_none_or(|existing| {
        candidate
            .score
            .total_cmp(&existing.score)
            .then_with(|| candidate.actual.cmp(&existing.actual))
            .is_lt()
    }) {
        candidates[worst] = Some(candidate);
    }
}

#[allow(clippy::cast_precision_loss)]
fn coarse_geometry_score(
    expected: Bounds,
    actual: Bounds,
    offset: Offset,
    global: Offset,
    maximum: u32,
) -> f64 {
    let width_difference = expected.width.abs_diff(actual.width);
    let height_difference = expected.height.abs_diff(actual.height);
    let local_x = (i64::from(offset.x) - i64::from(global.x)).unsigned_abs();
    let local_y = (i64::from(offset.y) - i64::from(global.y)).unsigned_abs();
    f64::from(width_difference) / f64::from(expected.width.max(1))
        + f64::from(height_difference) / f64::from(expected.height.max(1))
        + (local_x + local_y) as f64 / f64::from(maximum.max(1)) * 0.01
}

fn append_aligned_fallbacks(
    expected: &NormalizedImage,
    expected_proposals: &[Proposal],
    actual: &NormalizedImage,
    actual_proposals: &[Proposal],
    global: Offset,
    candidates: &[Candidate],
    pairs: &mut Vec<ProposalPair>,
) -> Result<(), CompareError> {
    let mut expected_used = vec![false; expected_proposals.len()];
    let mut actual_used = vec![false; actual_proposals.len()];
    for pair in pairs.iter() {
        expected_used[pair.expected] = true;
        actual_used[pair.actual] = true;
    }
    for (expected_index, is_used) in expected_used.iter().copied().enumerate() {
        if is_used {
            continue;
        }
        let fallback = candidates
            .iter()
            .filter(|candidate| {
                candidate.expected == expected_index
                    && !actual_used[candidate.actual]
                    && bounds_offset(
                        expected_proposals[candidate.expected].bounds,
                        actual_proposals[candidate.actual].bounds,
                    ) == global
            })
            .min_by(|left, right| {
                geometry_distance(
                    expected_proposals[left.expected].bounds,
                    actual_proposals[left.actual].bounds,
                )
                .total_cmp(&geometry_distance(
                    expected_proposals[right.expected].bounds,
                    actual_proposals[right.actual].bounds,
                ))
                .then_with(|| left.actual.cmp(&right.actual))
            });
        let Some(fallback) = fallback else {
            continue;
        };
        actual_used[fallback.actual] = true;
        let expected_bounds = expected_proposals[fallback.expected].bounds;
        pairs
            .try_reserve(1)
            .map_err(|_error| CompareError::ImageTooLarge)?;
        pairs.push(ProposalPair {
            expected: fallback.expected,
            actual: fallback.actual,
            score: fallback.score,
            baseline_score: globally_aligned_baseline(expected, expected_bounds, actual, global),
            ambiguous: true,
        });
    }
    Ok(())
}

fn rankings(
    candidates: &[Candidate],
    count: usize,
    owner: impl Fn(&Candidate) -> usize,
) -> Result<Vec<Vec<usize>>, CompareError> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_error| CompareError::ImageTooLarge)?;
    result.resize_with(count, Vec::new);
    for (index, candidate) in candidates.iter().enumerate() {
        result[owner(candidate)]
            .try_reserve(1)
            .map_err(|_error| CompareError::ImageTooLarge)?;
        result[owner(candidate)].push(index);
    }
    for ranking in &mut result {
        ranking.sort_by(|left, right| {
            candidates[*left]
                .score
                .total_cmp(&candidates[*right].score)
                .then_with(|| candidates[*left].expected.cmp(&candidates[*right].expected))
                .then_with(|| candidates[*left].actual.cmp(&candidates[*right].actual))
        });
    }
    Ok(result)
}

struct ProposalBuckets {
    size: i64,
    cells: BTreeMap<(i64, i64), Vec<usize>>,
}

impl ProposalBuckets {
    fn new(proposals: &[Proposal], maximum_offset: u32) -> Result<Self, CompareError> {
        let size = i64::from(maximum_offset.max(1));
        let mut cells: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
        for (index, proposal) in proposals.iter().enumerate() {
            let key = (
                i64::from(proposal.bounds.x).div_euclid(size),
                i64::from(proposal.bounds.y).div_euclid(size),
            );
            cells
                .entry(key)
                .or_default()
                .try_reserve(1)
                .map_err(|_error| CompareError::ImageTooLarge)?;
            cells
                .get_mut(&key)
                .expect("bucket was inserted")
                .push(index);
        }
        Ok(Self { size, cells })
    }

    fn near(&self, x: i64, y: i64) -> impl Iterator<Item = usize> + '_ {
        let center_x = x.div_euclid(self.size);
        let center_y = y.div_euclid(self.size);
        (-1_i64..=1).flat_map(move |delta_y| {
            (-1_i64..=1).flat_map(move |delta_x| {
                self.cells
                    .get(&(center_x + delta_x, center_y + delta_y))
                    .into_iter()
                    .flatten()
                    .copied()
            })
        })
    }
}

pub(crate) fn pair_offset(
    pair: ProposalPair,
    expected: &[Proposal],
    actual: &[Proposal],
) -> Offset {
    bounds_offset(expected[pair.expected].bounds, actual[pair.actual].bounds)
}

pub(crate) fn within_local_offset(offset: Offset, global: Offset, maximum: u32) -> bool {
    let delta_x = i64::from(offset.x) - i64::from(global.x);
    let delta_y = i64::from(offset.y) - i64::from(global.y);
    delta_x.unsigned_abs() <= u64::from(maximum) && delta_y.unsigned_abs() <= u64::from(maximum)
}

pub(crate) fn geometry_distance(expected: Bounds, actual: Bounds) -> f64 {
    let intersection = expected.intersection(actual).map_or(0, Bounds::area);
    let union = expected
        .area()
        .saturating_add(actual.area())
        .saturating_sub(intersection);
    if union == 0 {
        return 1.0;
    }
    #[allow(clippy::cast_precision_loss)]
    let overlap = intersection as f64 / union as f64;
    1.0 - overlap
}

fn globally_aligned_baseline(
    expected: &NormalizedImage,
    expected_bounds: Bounds,
    actual: &NormalizedImage,
    global: Offset,
) -> f64 {
    let x = i64::from(expected_bounds.x) + i64::from(global.x);
    let y = i64::from(expected_bounds.y) + i64::from(global.y);
    let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
        return 100.0;
    };
    let actual_bounds = Bounds {
        x,
        y,
        width: expected_bounds.width,
        height: expected_bounds.height,
    };
    if actual_bounds.right() > actual.width() || actual_bounds.bottom() > actual.height() {
        return 100.0;
    }
    proposals::scaled_patch_score(expected, expected_bounds, actual, actual_bounds)
}

fn bounds_offset(expected: Bounds, actual: Bounds) -> Offset {
    Offset {
        x: signed_difference(actual.x, expected.x),
        y: signed_difference(actual.y, expected.y),
    }
}

fn signed_difference(left: u32, right: u32) -> i32 {
    i32::try_from(i64::from(left) - i64::from(right)).unwrap_or(if left >= right {
        i32::MAX
    } else {
        i32::MIN
    })
}

#[cfg(test)]
mod tests {
    use image::RgbaImage;

    use super::*;

    #[test]
    fn local_limit_is_relative_to_global_alignment() {
        assert!(within_local_offset(
            Offset { x: 135, y: -20 },
            Offset { x: 125, y: -20 },
            10,
        ));
        assert!(!within_local_offset(
            Offset { x: 136, y: -20 },
            Offset { x: 125, y: -20 },
            10,
        ));
    }

    #[test]
    fn pathologically_dense_candidate_cells_keep_bounded_rankings() {
        let image = NormalizedImage::try_new(&RgbaImage::new(32, 32)).expect("normalize");
        let expected = [Proposal {
            bounds: Bounds {
                x: 1,
                y: 1,
                width: 4,
                height: 4,
            },
        }];
        let actual = vec![expected[0]; 529];

        let pairs = pair_proposals(&image, &expected, &image, &actual, Offset::default(), 16)
            .expect("bounded pairing");

        assert!(pairs.len() <= 1);
    }

    #[test]
    fn pairing_budget_bounds_total_candidate_inspections() {
        assert!(within_pairing_budget(256, 256));
        assert!(!within_pairing_budget(257, 256));
        assert!(!within_pairing_budget(1, 513));
        assert!(!within_pairing_budget(usize::MAX, usize::MAX));
    }
}
