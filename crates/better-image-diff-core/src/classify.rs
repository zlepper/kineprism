use image::RgbaImage;

use crate::color::NormalizedImage;
use crate::local::{self, LocalMovement};
use crate::mapping::MovementMapping;
use crate::matching::{
    MATCH_SCORE_LIMIT, ProposalPair, geometry_distance, pair_offset, pair_proposals,
    within_pairing_budget,
};
use crate::movement;
use crate::proposals::{self, Proposal};
use crate::residual;
use crate::{Alignment, Bounds, CompareError, CompareOptions, Difference, DifferenceKind, Offset};

pub(crate) struct StructuralAnalysis {
    pub(crate) alignment: Alignment,
    pub(crate) differences: Vec<Difference>,
    pub(crate) movements: Vec<MovementMapping>,
}

#[derive(Default)]
struct ClassificationOutput {
    differences: Vec<Difference>,
    movements: Vec<MovementMapping>,
    handled_expected: Vec<Bounds>,
    handled_actual: Vec<Bounds>,
}

struct PairClassificationContext<'a> {
    expected: &'a [Proposal],
    actual: &'a [Proposal],
    expected_image: &'a NormalizedImage,
    actual_image: &'a NormalizedImage,
    alignment: Alignment,
    options: &'a CompareOptions,
}

pub(crate) fn analyze(
    expected_image: &RgbaImage,
    expected: &NormalizedImage,
    actual_image: &RgbaImage,
    actual: &NormalizedImage,
    options: &CompareOptions,
    mut alignment: Alignment,
) -> Result<StructuralAnalysis, CompareError> {
    let (mut expected_proposals, mut actual_proposals, pairing_budget_available) =
        extract_proposals(
            expected_image,
            expected,
            actual_image,
            actual,
            options,
            &mut alignment,
        )?;
    let mut pairs = pair_proposals(
        expected,
        &expected_proposals,
        actual,
        &actual_proposals,
        alignment.offset,
        options.max_offset,
    )?;
    if alignment_is_unsupported(
        alignment,
        expected,
        &expected_proposals,
        &actual_proposals,
        &pairs,
    ) {
        alignment = Alignment {
            offset: Offset::default(),
            confidence: 0.0,
        };
        pairs = pair_proposals(
            expected,
            &expected_proposals,
            actual,
            &actual_proposals,
            alignment.offset,
            options.max_offset,
        )?;
    }
    let detected_movements = if pairing_budget_available
        && (expected_proposals.is_empty() || actual_proposals.is_empty())
    {
        local::detect(expected, actual, alignment.offset, options)?
    } else {
        Vec::new()
    };
    if !detected_movements.is_empty() {
        remove_moved_proposals(
            &mut expected_proposals,
            &mut actual_proposals,
            &detected_movements,
        );
        pairs = pair_proposals(
            expected,
            &expected_proposals,
            actual,
            &actual_proposals,
            alignment.offset,
            options.max_offset,
        )?;
    }
    let mut output = ClassificationOutput::default();
    append_global_movement(
        &pairs,
        &expected_proposals,
        &actual_proposals,
        expected,
        actual,
        alignment,
        &mut output,
    );
    append_detected_movements(&detected_movements, &mut output);
    let pair_context = PairClassificationContext {
        expected: &expected_proposals,
        actual: &actual_proposals,
        expected_image: expected,
        actual_image: actual,
        alignment,
        options,
    };
    append_paired_differences(&pairs, &pair_context, &mut output);
    movement::coalesce(&mut output.differences, &mut output.movements)?;
    append_unpaired_differences(&pairs, &expected_proposals, &actual_proposals, &mut output);

    residual::append_unhandled(
        expected,
        actual,
        alignment.offset,
        options,
        &output.handled_expected,
        &output.handled_actual,
        &mut output.differences,
    )?;

    Ok(StructuralAnalysis {
        alignment,
        differences: output.differences,
        movements: output.movements,
    })
}

fn extract_proposals(
    expected_image: &RgbaImage,
    expected: &NormalizedImage,
    actual_image: &RgbaImage,
    actual: &NormalizedImage,
    options: &CompareOptions,
    alignment: &mut Alignment,
) -> Result<(Vec<Proposal>, Vec<Proposal>, bool), CompareError> {
    let mut expected_proposals = proposals::extract(
        expected_image,
        expected,
        options.color_threshold,
        options.min_region_area,
    )?;
    let mut actual_proposals = proposals::extract(
        actual_image,
        actual,
        options.color_threshold,
        options.min_region_area,
    )?;
    let budget_available = within_pairing_budget(expected_proposals.len(), actual_proposals.len());
    if !budget_available {
        expected_proposals.clear();
        actual_proposals.clear();
        *alignment = Alignment {
            offset: Offset::default(),
            confidence: 0.0,
        };
    }
    Ok((expected_proposals, actual_proposals, budget_available))
}

fn remove_moved_proposals(
    expected: &mut Vec<Proposal>,
    actual: &mut Vec<Proposal>,
    movements: &[LocalMovement],
) {
    expected.retain(|proposal| {
        !movements.iter().any(|movement| {
            movement
                .expected_bounds
                .intersection(proposal.bounds)
                .is_some()
        })
    });
    actual.retain(|proposal| {
        !movements.iter().any(|movement| {
            movement
                .actual_bounds
                .intersection(proposal.bounds)
                .is_some()
        })
    });
}

fn append_detected_movements(movements: &[LocalMovement], output: &mut ClassificationOutput) {
    for movement in movements {
        output.handled_expected.push(movement.expected_bounds);
        output.handled_actual.push(movement.actual_bounds);
        output.differences.push(movement_difference(
            movement.expected_bounds,
            movement.actual_bounds,
            movement.offset,
            movement.confidence,
        ));
        output.movements.push(MovementMapping {
            bounds: movement.expected_bounds,
            offset: movement.offset,
            confidence: movement.confidence,
            order: output.differences.len() - 1,
        });
    }
}

fn proposals_are_repetitive(image: &NormalizedImage, proposals: &[Proposal]) -> bool {
    let Some(first) = proposals.first() else {
        return false;
    };
    proposals.iter().skip(1).all(|proposal| {
        proposals::scaled_patch_score(image, first.bounds, image, proposal.bounds) <= 0.2
    })
}

fn alignment_is_unsupported(
    alignment: Alignment,
    expected_image: &NormalizedImage,
    expected: &[Proposal],
    actual: &[Proposal],
    pairs: &[ProposalPair],
) -> bool {
    if alignment.offset == Offset::default() || expected.is_empty() {
        return false;
    }
    let supporting = pairs
        .iter()
        .filter(|pair| {
            pair.score <= MATCH_SCORE_LIMIT
                && pair_offset(**pair, expected, actual) == alignment.offset
        })
        .count();
    let broad_support = supporting.saturating_mul(4) >= expected.len().saturating_mul(3);
    let has_distinct_support = pairs.iter().any(|pair| {
        !pair.ambiguous
            && pair.score <= MATCH_SCORE_LIMIT
            && pair_offset(*pair, expected, actual) == alignment.offset
    });
    !broad_support || (proposals_are_repetitive(expected_image, expected) && !has_distinct_support)
}

fn append_global_movement(
    pairs: &[ProposalPair],
    expected: &[Proposal],
    actual: &[Proposal],
    expected_image: &NormalizedImage,
    actual_image: &NormalizedImage,
    alignment: Alignment,
    output: &mut ClassificationOutput,
) {
    if alignment.offset == Offset::default() {
        return;
    }
    let supporting: Vec<_> = pairs
        .iter()
        .filter(|pair| {
            !pair.ambiguous
                && pair.score <= MATCH_SCORE_LIMIT
                && pair_offset(**pair, expected, actual) == alignment.offset
        })
        .collect();
    if let (Some(expected_bounds), Some(actual_bounds)) = (
        union_bounds(supporting.iter().map(|pair| expected[pair.expected].bounds)),
        union_bounds(supporting.iter().map(|pair| actual[pair.actual].bounds)),
    ) {
        output.differences.push(movement_difference(
            expected_bounds,
            actual_bounds,
            alignment.offset,
            alignment.confidence,
        ));
        return;
    }
    let expected_canvas = Bounds {
        x: 0,
        y: 0,
        width: expected_image.width(),
        height: expected_image.height(),
    };
    if let Some(actual_bounds) = expected_canvas.translated_clipped(
        alignment.offset,
        actual_image.width(),
        actual_image.height(),
    ) {
        let reverse = Offset {
            x: alignment.offset.x.saturating_neg(),
            y: alignment.offset.y.saturating_neg(),
        };
        let expected_bounds = actual_bounds
            .translated_clipped(reverse, expected_image.width(), expected_image.height())
            .unwrap_or(expected_canvas);
        output.differences.push(movement_difference(
            expected_bounds,
            actual_bounds,
            alignment.offset,
            alignment.confidence,
        ));
    }
}

fn append_paired_differences(
    pairs: &[ProposalPair],
    context: &PairClassificationContext<'_>,
    output: &mut ClassificationOutput,
) {
    for pair in pairs {
        let expected_proposal = context.expected[pair.expected];
        let actual_proposal = context.actual[pair.actual];
        output.handled_expected.push(expected_proposal.bounds);
        output.handled_actual.push(actual_proposal.bounds);
        let offset = pair_offset(*pair, context.expected, context.actual);
        let same_size = expected_proposal.bounds.width == actual_proposal.bounds.width
            && expected_proposal.bounds.height == actual_proposal.bounds.height;
        let good_match = !pair.ambiguous && pair.score <= MATCH_SCORE_LIMIT;

        let has_residual = same_size
            && residual::significant_pair(
                context.expected_image,
                expected_proposal.bounds,
                context.actual_image,
                actual_proposal.bounds,
                context.options.color_threshold,
                context.options.min_region_area,
            );

        if good_match && same_size && offset == context.alignment.offset && !has_residual {
            continue;
        }
        if pair.validates_movement(offset, context.alignment.offset) && same_size && !has_residual {
            let confidence = match_confidence(pair.score);
            output.differences.push(movement_difference(
                expected_proposal.bounds,
                actual_proposal.bounds,
                offset,
                confidence,
            ));
            output.movements.push(MovementMapping {
                bounds: expected_proposal.bounds,
                offset,
                confidence,
                order: output.differences.len() - 1,
            });
        } else if good_match && !same_size {
            output.differences.push(resized_difference(
                expected_proposal.bounds,
                actual_proposal.bounds,
                offset,
                pair.score,
            ));
        } else {
            output.differences.push(changed_difference(
                expected_proposal.bounds,
                actual_proposal.bounds,
            ));
        }
    }
}

fn append_unpaired_differences(
    pairs: &[ProposalPair],
    expected: &[Proposal],
    actual: &[Proposal],
    output: &mut ClassificationOutput,
) {
    let mut paired_expected = vec![false; expected.len()];
    let mut paired_actual = vec![false; actual.len()];
    for pair in pairs {
        paired_expected[pair.expected] = true;
        paired_actual[pair.actual] = true;
    }
    for (index, proposal) in expected.iter().enumerate() {
        if !paired_expected[index] {
            output.handled_expected.push(proposal.bounds);
            output.differences.push(Difference {
                id: String::new(),
                kind: DifferenceKind::Removed,
                expected_bounds: Some(proposal.bounds),
                actual_bounds: None,
                offset: None,
                confidence: 0.8,
                message: "Expected region is missing from the actual image.".to_owned(),
            });
        }
    }
    for (index, proposal) in actual.iter().enumerate() {
        if !paired_actual[index] {
            output.handled_actual.push(proposal.bounds);
            output.differences.push(Difference {
                id: String::new(),
                kind: DifferenceKind::Added,
                expected_bounds: None,
                actual_bounds: Some(proposal.bounds),
                offset: None,
                confidence: 0.8,
                message: "Actual image contains an additional region.".to_owned(),
            });
        }
    }
}

fn resized_difference(expected: Bounds, actual: Bounds, offset: Offset, score: f64) -> Difference {
    Difference {
        id: String::new(),
        kind: DifferenceKind::Resized,
        expected_bounds: Some(expected),
        actual_bounds: Some(actual),
        offset: Some(offset),
        confidence: match_confidence(score),
        message: format!(
            "Region resized from {}x{} to {}x{}.",
            expected.width, expected.height, actual.width, actual.height
        ),
    }
}

fn changed_difference(expected: Bounds, actual: Bounds) -> Difference {
    Difference {
        id: String::new(),
        kind: DifferenceKind::Changed,
        expected_bounds: Some(expected),
        actual_bounds: Some(actual),
        offset: None,
        confidence: geometry_confidence(expected, actual),
        message: "Corresponding region has changed appearance.".to_owned(),
    }
}

fn movement_difference(
    expected_bounds: Bounds,
    actual_bounds: Bounds,
    offset: Offset,
    confidence: f64,
) -> Difference {
    Difference {
        id: String::new(),
        kind: DifferenceKind::Moved,
        expected_bounds: Some(expected_bounds),
        actual_bounds: Some(actual_bounds),
        offset: Some(offset),
        confidence: confidence.clamp(0.0, 1.0),
        message: format!(
            "Region appears {} instead of at its expected position.",
            describe_offset(offset)
        ),
    }
}

fn geometry_confidence(expected: Bounds, actual: Bounds) -> f64 {
    (1.0 - geometry_distance(expected, actual)).clamp(0.5, 0.95)
}

fn match_confidence(score: f64) -> f64 {
    (1.0 - score / MATCH_SCORE_LIMIT).clamp(0.5, 1.0)
}

fn union_bounds(bounds: impl Iterator<Item = Bounds>) -> Option<Bounds> {
    bounds.reduce(|left, right| {
        let x = left.x.min(right.x);
        let y = left.y.min(right.y);
        let right_edge = left.right().max(right.right());
        let bottom = left.bottom().max(right.bottom());
        Bounds {
            x,
            y,
            width: right_edge - x,
            height: bottom - y,
        }
    })
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
