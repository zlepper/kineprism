//! Recognizes differences confined to same-style, same-position text lines.
//!
//! Each candidate finding is reduced to the pixels that actually differ. Those are grouped into
//! lines, and every line is grown to the full run of glyphs around it in both images. A line is
//! text when, on each side, it is two-tone (background, one ink color and the ramp between them)
//! on an isolated flat background, holds several glyph-sized ink components and no drawn shapes
//! such as checkboxes, and when both sides share colors, stroke width, glyph height and placement
//! while their glyph shapes differ. A finding is replaced only when all of its differing pixels
//! fall inside such lines.

use std::collections::BTreeMap;

use crate::antialias;
use crate::color::{NormalizedImage, NormalizedPixel, RAMP_TOLERANCE};
use crate::local_geometry::{contains, horizontal_gap, padded_axes, union};
use crate::mask::{Component, Mask};
use crate::{
    Bounds, CompareError, CompareOptions, Difference, DifferenceKind, Offset, SuppressionSummary,
};

const MIN_TEXT_HEIGHT: u32 = 6;
const MAX_TEXT_HEIGHT: u32 = 64;
const MIN_GLYPHS: usize = 2;
/// Horizontal search distance around a seed, enough for several words of the tallest text.
const LINE_SEARCH_RADIUS: u32 = MAX_TEXT_HEIGHT * 4;
const MAX_CANDIDATE_COMPONENTS: usize = 256;
const MIN_INK_DISTANCE: f64 = 4.0;
const SOLID_INK_SHARE: f64 = 0.8;
const MIN_BACKGROUND_SHARE: f64 = 0.5;
const MIN_RING_SHARE: f64 = 0.9;
const MIN_TWO_TONE_SHARE: f64 = 0.9;
const MIN_INK_DENSITY: f64 = 0.05;
const MAX_INK_DENSITY: f64 = 0.75;
const MAX_STROKE_DIFFERENCE: f64 = 0.5;
const EDGE_TOLERANCE: u32 = 1;
/// Share of a shape's bounding-box edge that is ink for it to be a drawn outline, not a glyph.
const MIN_OUTLINE_SHARE: f64 = 0.9;
/// Share of a squarish shape's bounding box that is ink for it to be a drawn block, not a glyph.
const MIN_BLOCK_FILL: f64 = 0.85;
/// Largest share of differing pixels between two glyphs that still counts as the same glyph.
const MAX_GLYPH_MISMATCH: f64 = 0.05;

struct DiffBox {
    bounds: Bounds,
    candidate: usize,
}

struct Candidates {
    boxes: Vec<DiffBox>,
    /// Candidates whose differences are too fragmented to be text.
    fragmented: Vec<bool>,
}

struct Line {
    bounds: Bounds,
    boxes: Vec<usize>,
}

struct TextSide {
    bounds: Bounds,
    background: NormalizedPixel,
    foreground: NormalizedPixel,
    two_tone: f64,
    stroke: f64,
    /// Glyph shapes in reading order.
    shapes: Vec<Shape>,
}

/// A glyph's ink, thresholded halfway between background and foreground so that antialiasing
/// differences do not change it.
struct Shape {
    width: u32,
    height: u32,
    ink: Vec<bool>,
}

#[derive(Clone, Copy)]
struct TextLine {
    expected: Bounds,
    actual: Bounds,
    /// Both sides' text in expected coordinates.
    covered: Bounds,
    confidence: f64,
}

/// Replaces findings explained entirely by text changes with `text_changed` findings, or records
/// them as suppressed when text differences are ignored. Returns the suppressed areas in expected
/// coordinates.
pub(crate) fn classify(
    expected: &NormalizedImage,
    actual: &NormalizedImage,
    alignment: Offset,
    options: &CompareOptions,
    differences: &mut Vec<Difference>,
    suppression: &mut SuppressionSummary,
) -> Result<Vec<Bounds>, CompareError> {
    let Candidates { boxes, fragmented } =
        diff_boxes(expected, actual, alignment, options, differences)?;
    let lines = group_lines(&boxes);
    let evaluated: Vec<Option<TextLine>> = lines
        .iter()
        .map(|line| text_line(expected, actual, line.bounds, alignment, options))
        .collect();

    let mut explained: Vec<bool> = fragmented.iter().map(|fragmented| !fragmented).collect();
    let mut has_boxes = vec![false; differences.len()];
    for (line, result) in lines.iter().zip(&evaluated) {
        for &index in &line.boxes {
            let candidate = boxes[index].candidate;
            has_boxes[candidate] = true;
            explained[candidate] &= result.is_some();
        }
    }
    for (explained, has_boxes) in explained.iter_mut().zip(&has_boxes) {
        *explained &= *has_boxes;
    }
    // A line shared with an unexplained finding stays part of that finding, so the other
    // findings on it cannot be explained either.
    let mut settled = false;
    while !settled {
        settled = true;
        for line in &lines {
            let members = line.boxes.iter().map(|&index| boxes[index].candidate);
            if members.clone().all(|candidate| explained[candidate]) {
                continue;
            }
            for candidate in members {
                settled &= !explained[candidate];
                explained[candidate] = false;
            }
        }
    }

    let clean = lines.iter().zip(evaluated).filter_map(|(line, result)| {
        let first = line.boxes.first().map(|&index| boxes[index].candidate)?;
        result.filter(|_| explained[first])
    });
    let text_lines = merge_overlapping(clean);

    // Findings without differing pixels inside recognized text are glyphs paired with the wrong
    // neighbor, so they belong to the text change.
    for (candidate, difference) in differences.iter().enumerate() {
        if has_boxes[candidate] || fragmented[candidate] {
            continue;
        }
        explained[candidate] = candidate_region(difference, alignment, expected, actual)
            .is_some_and(|region| text_lines.iter().any(|text| contains(text.covered, region)));
    }

    let mut index = 0;
    differences.retain(|_| {
        let keep = !explained[index];
        index += 1;
        keep
    });
    let mut suppressed = Vec::new();
    for text in text_lines {
        if options.ignore_text {
            let pixels = differing_pixels(expected, actual, text.covered, alignment, options);
            suppression.record_text(pixels);
            suppressed.push(text.covered);
        } else {
            differences.push(text_difference(text));
        }
    }
    Ok(suppressed)
}

fn text_difference(text: TextLine) -> Difference {
    Difference {
        id: String::new(),
        kind: DifferenceKind::TextChanged,
        expected_bounds: Some(text.expected),
        actual_bounds: Some(text.actual),
        offset: None,
        confidence: text.confidence,
        message: format!(
            "Text in a {}x{} px line changed while its style and position stayed the same.",
            text.actual.width, text.actual.height
        ),
    }
}

/// Merges text lines whose areas overlap, which happens when several seeds grow into the same
/// run of glyphs.
fn merge_overlapping(lines: impl Iterator<Item = TextLine>) -> Vec<TextLine> {
    let mut merged: Vec<TextLine> = Vec::new();
    for mut line in lines {
        while let Some(index) = merged
            .iter()
            .position(|other| other.covered.intersection(line.covered).is_some())
        {
            let other = merged.swap_remove(index);
            line = TextLine {
                expected: union(line.expected, other.expected),
                actual: union(line.actual, other.actual),
                covered: union(line.covered, other.covered),
                confidence: line.confidence.min(other.confidence),
            };
        }
        merged.push(line);
    }
    merged.sort_by_key(|line| (line.covered.y, line.covered.x));
    merged
}

fn differing_pixels(
    expected: &NormalizedImage,
    actual: &NormalizedImage,
    region: Bounds,
    alignment: Offset,
    options: &CompareOptions,
) -> u64 {
    let mut count = 0;
    for y in region.y..region.bottom() {
        for x in region.x..region.right() {
            if let Some(mapped) = alignment.translate_point((x, y), actual.width(), actual.height())
                && antialias::differs(expected, (x, y), actual, mapped, options)
            {
                count += 1;
            }
        }
    }
    count
}

/// Splits candidate findings into connected boxes of differing pixels in expected coordinates.
/// Candidates with too many boxes are marked as fragmented and never explained.
fn diff_boxes(
    expected: &NormalizedImage,
    actual: &NormalizedImage,
    alignment: Offset,
    options: &CompareOptions,
    differences: &[Difference],
) -> Result<Candidates, CompareError> {
    let mut boxes = Vec::new();
    let mut fragmented = vec![false; differences.len()];
    for (candidate, difference) in differences.iter().enumerate() {
        let Some(region) = candidate_region(difference, alignment, expected, actual) else {
            continue;
        };
        let mut mask = Mask::try_new(region.width, region.height)?;
        for y in 0..region.height {
            for x in 0..region.width {
                let point = (region.x + x, region.y + y);
                if let Some(mapped) =
                    alignment.translate_point(point, actual.width(), actual.height())
                    && antialias::differs(expected, point, actual, mapped, options)
                {
                    mask.set(x, y, true);
                }
            }
        }
        let components = mask.components(1)?;
        if components.len() > MAX_CANDIDATE_COMPONENTS {
            fragmented[candidate] = true;
            continue;
        }
        boxes.extend(components.into_iter().map(|component| DiffBox {
            bounds: Bounds {
                x: region.x + component.bounds.x,
                y: region.y + component.bounds.y,
                ..component.bounds
            },
            candidate,
        }));
    }
    Ok(Candidates { boxes, fragmented })
}

/// The expected-coordinate area a finding covers on either side, limited to the aligned overlap.
/// Movements and canvas changes are never text changes.
fn candidate_region(
    difference: &Difference,
    alignment: Offset,
    expected: &NormalizedImage,
    actual: &NormalizedImage,
) -> Option<Bounds> {
    let eligible = match difference.kind {
        DifferenceKind::Changed
        | DifferenceKind::Resized
        | DifferenceKind::Added
        | DifferenceKind::Removed => true,
        DifferenceKind::Moved | DifferenceKind::CanvasSize | DifferenceKind::TextChanged => false,
    };
    if !eligible {
        return None;
    }
    let reverse = alignment.reversed();
    let from_actual = difference
        .actual_bounds
        .and_then(|bounds| bounds.translated_clipped(reverse, expected.width(), expected.height()));
    let region = match (difference.expected_bounds, from_actual) {
        (Some(left), Some(right)) => union(left, right),
        (Some(bounds), None) | (None, Some(bounds)) => bounds,
        (None, None) => return None,
    };
    region
        .translated_clipped(alignment, actual.width(), actual.height())?
        .translated_clipped(reverse, expected.width(), expected.height())
}

/// Merges boxes that share a text line: substantial vertical overlap and a gap no wider than
/// the taller box.
fn group_lines(boxes: &[DiffBox]) -> Vec<Line> {
    let mut lines: Vec<Line> = boxes
        .iter()
        .enumerate()
        .map(|(index, diff_box)| Line {
            bounds: diff_box.bounds,
            boxes: vec![index],
        })
        .collect();
    let mut merged = true;
    while merged {
        merged = false;
        let mut left = 0;
        while left < lines.len() {
            let mut right = left + 1;
            while right < lines.len() {
                if same_line(lines[left].bounds, lines[right].bounds) {
                    let absorbed = lines.swap_remove(right);
                    lines[left].bounds = union(lines[left].bounds, absorbed.bounds);
                    lines[left].boxes.extend(absorbed.boxes);
                    merged = true;
                } else {
                    right += 1;
                }
            }
            left += 1;
        }
    }
    lines.sort_by_key(|line| (line.bounds.y, line.bounds.x));
    lines
}

fn same_line(left: Bounds, right: Bounds) -> bool {
    let overlap = left
        .bottom()
        .min(right.bottom())
        .saturating_sub(left.y.max(right.y));
    let shorter = left.height.min(right.height);
    overlap > 0
        && overlap.saturating_mul(2) >= shorter
        && horizontal_gap(left, right) <= left.height.max(right.height)
}

fn text_line(
    expected: &NormalizedImage,
    actual: &NormalizedImage,
    seed: Bounds,
    alignment: Offset,
    options: &CompareOptions,
) -> Option<TextLine> {
    let actual_seed = seed.translated_clipped(alignment, actual.width(), actual.height())?;
    let expected_side = text_side(expected, seed, options)?;
    let actual_side = text_side(actual, actual_seed, options)?;
    let mapped =
        expected_side
            .bounds
            .translated_clipped(alignment, actual.width(), actual.height())?;
    // A run clipped by the alignment cannot be compared as a whole.
    let clipped =
        (mapped.width, mapped.height) != (expected_side.bounds.width, expected_side.bounds.height);
    if clipped
        || !same_style(&expected_side, &actual_side, options)
        || !same_placement(mapped, actual_side.bounds)
        || same_shapes(&expected_side.shapes, &actual_side.shapes)
    {
        return None;
    }
    let covered = union(
        expected_side.bounds,
        actual_side.bounds.translated_clipped(
            alignment.reversed(),
            expected.width(),
            expected.height(),
        )?,
    );
    if !contains(covered, seed) {
        return None;
    }
    Some(TextLine {
        expected: expected_side.bounds,
        actual: actual_side.bounds,
        covered,
        confidence: expected_side
            .two_tone
            .min(actual_side.two_tone)
            .clamp(0.5, 0.95),
    })
}

/// Same colors, stroke width and glyph height, so the text was neither restyled nor resized.
fn same_style(expected: &TextSide, actual: &TextSide, options: &CompareOptions) -> bool {
    expected.background.perceptual_distance(actual.background) <= options.color_threshold
        && expected.foreground.perceptual_distance(actual.foreground) <= options.color_threshold
        && (expected.stroke - actual.stroke).abs() <= MAX_STROKE_DIFFERENCE
        && typical_height(&expected.shapes).abs_diff(typical_height(&actual.shapes))
            <= EDGE_TOLERANCE
}

/// Whether both runs hold the same glyphs in the same order, meaning the text moved, was spaced
/// differently or changed color rather than changed content.
fn same_shapes(expected: &[Shape], actual: &[Shape]) -> bool {
    expected.len() == actual.len()
        && expected
            .iter()
            .zip(actual)
            .all(|(left, right)| same_shape(left, right))
}

fn same_shape(left: &Shape, right: &Shape) -> bool {
    if left.width.abs_diff(right.width) > EDGE_TOLERANCE
        || left.height.abs_diff(right.height) > EDGE_TOLERANCE
    {
        return false;
    }
    let ink = |shape: &Shape, x: u32, y: u32| {
        x < shape.width && y < shape.height && shape.ink[(y * shape.width + x) as usize]
    };
    let mut mismatched = 0_u64;
    let mut total = 0_u64;
    for y in 0..left.height.max(right.height) {
        for x in 0..left.width.max(right.width) {
            let (left, right) = (ink(left, x, y), ink(right, x, y));
            mismatched += u64::from(left != right);
            total += u64::from(left || right);
        }
    }
    share(mismatched, total) <= MAX_GLYPH_MISMATCH
}

/// The most common glyph height, preferring the taller on ties.
fn typical_height(shapes: &[Shape]) -> u32 {
    let mut counts: BTreeMap<u32, usize> = BTreeMap::new();
    for shape in shapes {
        *counts.entry(shape.height).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by_key(|&(height, count)| (count, height))
        .map_or(0, |(height, _)| height)
}

/// Whether an ink component continues a run of glyphs: on the same line, of comparable height,
/// close by, and not a frame enclosing the line.
fn joins_line(line: Bounds, component: Bounds) -> bool {
    component.height.saturating_mul(2) <= line.height.saturating_mul(3)
        && !contains(component, line)
        && same_line(line, component)
}

fn joins_seed(seed: Bounds, component: Bounds) -> bool {
    !contains(component, seed) && same_line(seed, component)
}

/// A shared top or bottom edge with the other edge nearby, and a shared left edge, right edge,
/// or center.
fn same_placement(expected: Bounds, actual: Bounds) -> bool {
    let close = |left: u32, right: u32, tolerance: u32| left.abs_diff(right) <= tolerance;
    let vertical_slack = expected.height.min(actual.height) / 4;
    close(expected.y, actual.y, vertical_slack)
        && close(expected.bottom(), actual.bottom(), vertical_slack)
        && (close(expected.y, actual.y, EDGE_TOLERANCE)
            || close(expected.bottom(), actual.bottom(), EDGE_TOLERANCE))
        && (close(expected.x, actual.x, EDGE_TOLERANCE)
            || close(expected.right(), actual.right(), EDGE_TOLERANCE)
            || close(
                expected.x + expected.right(),
                actual.x + actual.right(),
                EDGE_TOLERANCE * 2,
            ))
}

/// Grows `seed` to the run of glyphs on its line within one image and checks it looks like text.
fn text_side(image: &NormalizedImage, seed: Bounds, options: &CompareOptions) -> Option<TextSide> {
    if seed.height > MAX_TEXT_HEIGHT {
        return None;
    }
    let line_height = seed.height.max(MIN_TEXT_HEIGHT);
    let local = padded_axes(
        seed,
        line_height,
        line_height,
        image.width(),
        image.height(),
    );
    let (background, background_count) = dominant(image, local, |_| true)?;
    if share(background_count, local.area()) < MIN_BACKGROUND_SHARE {
        return None;
    }
    let ink_distance = ink_distance(options);
    let is_ink = |pixel: NormalizedPixel| pixel.perceptual_distance(background) > ink_distance;
    let window = padded_axes(
        seed,
        LINE_SEARCH_RADIUS,
        MAX_TEXT_HEIGHT / 2,
        image.width(),
        image.height(),
    );
    let mut glyphs = grow_glyphs(image, seed, window, is_ink)?;
    let line = glyphs.iter().copied().reduce(union)?;
    if glyphs.len() < MIN_GLYPHS || line.height < MIN_TEXT_HEIGHT || line.height > MAX_TEXT_HEIGHT {
        return None;
    }

    let ring = padded_axes(
        line,
        EDGE_TOLERANCE + 1,
        EDGE_TOLERANCE + 1,
        image.width(),
        image.height(),
    );
    let ring_pixels = ring.area() - line.area();
    let ring_background = count_pixels(image, ring, |x, y, pixel| {
        !contains_point(line, x, y) && !is_ink(pixel)
    });
    if ring_pixels == 0 || share(ring_background, ring_pixels) < MIN_RING_SHARE {
        return None;
    }

    let ink = count_pixels(image, line, |_, _, pixel| is_ink(pixel));
    if !(MIN_INK_DENSITY..=MAX_INK_DENSITY).contains(&share(ink, line.area())) {
        return None;
    }
    let foreground = solid_color(image, line, background)?;
    if glyphs.iter().any(|glyph| {
        solid_color(image, *glyph, background)
            .is_none_or(|color| color.perceptual_distance(foreground) > ink_distance)
    }) {
        return None;
    }
    let two_tone = share(
        count_pixels(image, line, |_, _, pixel| {
            is_ink(pixel) && pixel.ramp_distance(background, foreground) <= RAMP_TOLERANCE
        }),
        ink,
    );
    if two_tone < MIN_TWO_TONE_SHARE {
        return None;
    }
    let strongest = max_distance(image, line, background);
    let stroke = stroke_width(image, line, |pixel| {
        pixel.perceptual_distance(background) >= strongest * SOLID_INK_SHARE
    });
    glyphs.sort_by_key(|glyph| (glyph.x, glyph.y));
    let shapes = glyphs
        .iter()
        .map(|glyph| shape(image, *glyph, background, foreground))
        .collect();
    Some(TextSide {
        bounds: line,
        background,
        foreground,
        two_tone,
        stroke,
        shapes,
    })
}

/// Distance from the background beyond which a pixel is ink, also used as the color tolerance
/// between glyphs of one line.
fn ink_distance(options: &CompareOptions) -> f64 {
    options.color_threshold.max(MIN_INK_DISTANCE)
}

/// Collects the ink components forming the run of glyphs around `seed`. Returns `None` when a
/// drawn shape, such as a checkbox or an icon frame, sits on the line.
fn grow_glyphs(
    image: &NormalizedImage,
    seed: Bounds,
    window: Bounds,
    is_ink: impl Fn(NormalizedPixel) -> bool,
) -> Option<Vec<Bounds>> {
    let mut mask = Mask::try_new(window.width, window.height).ok()?;
    for y in 0..window.height {
        for x in 0..window.width {
            if is_ink(image.pixel(window.x + x, window.y + y)) {
                mask.set(x, y, true);
            }
        }
    }
    // Anything reaching the window edge is a structure larger than a line of text.
    let (outlines, components): (Vec<Component>, Vec<Component>) = mask
        .components(1)
        .ok()?
        .into_iter()
        .map(|component| Component {
            bounds: Bounds {
                x: window.x + component.bounds.x,
                y: window.y + component.bounds.y,
                ..component.bounds
            },
            ..component
        })
        .filter(|component| !touches_border(component.bounds, window, image))
        .partition(|component| outline(image, component.bounds, &is_ink));
    let mut included = vec![false; components.len()];
    let mut glyphs: Vec<&Component> = Vec::new();
    let mut line: Option<Bounds> = None;
    let mut grew = true;
    while grew {
        grew = false;
        for (index, component) in components.iter().enumerate() {
            let bounds = component.bounds;
            let joins = match line {
                Some(line) => joins_line(line, bounds),
                // A seed covering only background here, such as a glyph added on the other side,
                // says nothing about glyph height yet.
                None => bounds.height <= MAX_TEXT_HEIGHT && joins_seed(seed, bounds),
            };
            if included[index] || !(bounds.intersection(seed).is_some() || joins) {
                continue;
            }
            included[index] = true;
            glyphs.push(component);
            line = Some(line.map_or(bounds, |line| union(line, bounds)));
            grew = true;
        }
    }
    let line = line?;
    // An outline around the whole line is its container, such as a card or chip.
    let on_line = |outline: Bounds| {
        !contains(outline, line)
            && (outline.intersection(line).is_some() || same_line(line, outline))
    };
    if outlines.iter().any(|outline| on_line(outline.bounds))
        || glyphs.iter().any(|glyph| block(glyph, line))
    {
        return None;
    }
    Some(glyphs.into_iter().map(|glyph| glyph.bounds).collect())
}

/// A closed rectangular outline, such as an empty checkbox or icon frame, is drawn rather than a
/// glyph.
fn outline(
    image: &NormalizedImage,
    bounds: Bounds,
    is_ink: impl Fn(NormalizedPixel) -> bool,
) -> bool {
    if bounds.width < MIN_TEXT_HEIGHT || bounds.height < MIN_TEXT_HEIGHT {
        return false;
    }
    let edge = count_pixels(image, bounds, |x, y, pixel| {
        (x == bounds.x || y == bounds.y || x + 1 == bounds.right() || y + 1 == bounds.bottom())
            && is_ink(pixel)
    });
    let perimeter = 2 * (u64::from(bounds.width) + u64::from(bounds.height)) - 4;
    share(edge, perimeter) >= MIN_OUTLINE_SHARE
}

/// A filled, nearly square shape about as tall as the line, such as a filled checkbox, is drawn
/// rather than a glyph; punctuation is shorter or narrower.
fn block(component: &Component, line: Bounds) -> bool {
    let bounds = component.bounds;
    let squarish = bounds.width.min(bounds.height) * 4 >= bounds.width.max(bounds.height) * 3;
    squarish
        && bounds.height.saturating_mul(3) >= line.height.saturating_mul(2)
        && share(u64::from(component.area), bounds.area()) >= MIN_BLOCK_FILL
}

/// The glyph's ink, cropped to the pixels closer to the foreground than to the background.
fn shape(
    image: &NormalizedImage,
    glyph: Bounds,
    background: NormalizedPixel,
    foreground: NormalizedPixel,
) -> Shape {
    let midpoint = background.perceptual_distance(foreground) / 2.0;
    let is_ink = |x: u32, y: u32| image.pixel(x, y).perceptual_distance(background) > midpoint;
    let points: Vec<(u32, u32)> = (glyph.y..glyph.bottom())
        .flat_map(|y| (glyph.x..glyph.right()).map(move |x| (x, y)))
        .filter(|&(x, y)| is_ink(x, y))
        .collect();
    let (Some(left), Some(top)) = (
        points.iter().map(|point| point.0).min(),
        points.iter().map(|point| point.1).min(),
    ) else {
        return Shape {
            width: 0,
            height: 0,
            ink: Vec::new(),
        };
    };
    let right = points.iter().map(|point| point.0).max().unwrap_or(left);
    let bottom = points.iter().map(|point| point.1).max().unwrap_or(top);
    let (width, height) = (right - left + 1, bottom - top + 1);
    let mut ink = vec![false; (width * height) as usize];
    for (x, y) in points {
        ink[((y - top) * width + x - left) as usize] = true;
    }
    Shape { width, height, ink }
}

/// The most common color among pixels close to the strongest ink. Antialiasing fringes can
/// outnumber solid ink in small text, so weaker ink is left out.
fn solid_color(
    image: &NormalizedImage,
    bounds: Bounds,
    background: NormalizedPixel,
) -> Option<NormalizedPixel> {
    let strongest = max_distance(image, bounds, background);
    dominant(image, bounds, |pixel| {
        pixel.perceptual_distance(background) >= strongest * SOLID_INK_SHARE
    })
    .map(|(color, _)| color)
}

/// Mean stroke width of solid ink: for each solid pixel, the shorter of its horizontal and
/// vertical solid runs.
fn stroke_width(
    image: &NormalizedImage,
    bounds: Bounds,
    is_solid: impl Fn(NormalizedPixel) -> bool,
) -> f64 {
    let width = bounds.width as usize;
    let height = bounds.height as usize;
    let solid: Vec<bool> = (0..bounds.height)
        .flat_map(|y| (0..bounds.width).map(move |x| (x, y)))
        .map(|(x, y)| is_solid(image.pixel(bounds.x + x, bounds.y + y)))
        .collect();
    let mut horizontal = vec![0_u32; solid.len()];
    let mut vertical = vec![0_u32; solid.len()];
    for y in 0..height {
        fill_runs(&solid, &mut horizontal, (0..width).map(|x| y * width + x));
    }
    for x in 0..width {
        fill_runs(&solid, &mut vertical, (0..height).map(|y| y * width + x));
    }
    let widths: Vec<u32> = (0..solid.len())
        .filter(|&index| solid[index])
        .map(|index| horizontal[index].min(vertical[index]))
        .collect();
    share(
        widths.iter().map(|width| u64::from(*width)).sum(),
        widths.len() as u64,
    )
}

/// Writes, for each solid index along a line, the length of the solid run containing it.
fn fill_runs(solid: &[bool], runs: &mut [u32], line: impl Iterator<Item = usize>) {
    let mut run: Vec<usize> = Vec::new();
    for index in line.chain(std::iter::once(usize::MAX)) {
        if index != usize::MAX && solid[index] {
            run.push(index);
            continue;
        }
        let length = u32::try_from(run.len()).unwrap_or(u32::MAX);
        for member in run.drain(..) {
            runs[member] = length;
        }
    }
}

fn dominant(
    image: &NormalizedImage,
    bounds: Bounds,
    include: impl Fn(NormalizedPixel) -> bool,
) -> Option<(NormalizedPixel, u64)> {
    let mut counts: BTreeMap<[u32; 4], (NormalizedPixel, u64)> = BTreeMap::new();
    for y in bounds.y..bounds.bottom() {
        for x in bounds.x..bounds.right() {
            let pixel = image.pixel(x, y);
            if include(pixel) {
                counts.entry(pixel.key()).or_insert((pixel, 0)).1 += 1;
            }
        }
    }
    counts
        .into_values()
        .max_by(|left, right| left.1.cmp(&right.1))
}

fn max_distance(image: &NormalizedImage, bounds: Bounds, reference: NormalizedPixel) -> f64 {
    let mut strongest = 0.0_f64;
    for y in bounds.y..bounds.bottom() {
        for x in bounds.x..bounds.right() {
            strongest = strongest.max(image.pixel(x, y).perceptual_distance(reference));
        }
    }
    strongest
}

fn count_pixels(
    image: &NormalizedImage,
    bounds: Bounds,
    predicate: impl Fn(u32, u32, NormalizedPixel) -> bool,
) -> u64 {
    let mut count = 0;
    for y in bounds.y..bounds.bottom() {
        for x in bounds.x..bounds.right() {
            if predicate(x, y, image.pixel(x, y)) {
                count += 1;
            }
        }
    }
    count
}

#[allow(
    clippy::cast_precision_loss,
    reason = "pixel counts are far below 2^52"
)]
fn share(count: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        count as f64 / total as f64
    }
}

/// Whether `bounds` reaches the edge of the search window, except where that is the image edge.
fn touches_border(bounds: Bounds, window: Bounds, image: &NormalizedImage) -> bool {
    (bounds.x == window.x && window.x > 0)
        || (bounds.y == window.y && window.y > 0)
        || (bounds.right() == window.right() && window.right() < image.width())
        || (bounds.bottom() == window.bottom() && window.bottom() < image.height())
}

fn contains_point(bounds: Bounds, x: u32, y: u32) -> bool {
    x >= bounds.x && y >= bounds.y && x < bounds.right() && y < bounds.bottom()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(x: u32, y: u32, width: u32, height: u32) -> Bounds {
        Bounds {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn neighboring_glyph_boxes_share_a_line() {
        assert!(same_line(bounds(0, 10, 10, 14), bounds(12, 12, 10, 12)));
    }

    #[test]
    fn stacked_or_distant_boxes_do_not_share_a_line() {
        assert!(!same_line(bounds(0, 10, 10, 14), bounds(0, 30, 10, 14)));
        assert!(!same_line(bounds(0, 10, 10, 14), bounds(40, 10, 10, 14)));
    }

    #[test]
    fn placement_accepts_left_right_or_center_anchoring_but_not_shifts() {
        let label = bounds(20, 10, 40, 14);
        assert!(same_placement(label, bounds(20, 10, 52, 14)));
        assert!(same_placement(label, bounds(8, 10, 52, 14)));
        assert!(same_placement(label, bounds(14, 10, 52, 14)));
        assert!(!same_placement(label, bounds(25, 10, 40, 14)));
        assert!(!same_placement(label, bounds(20, 10, 40, 28)));
        assert!(!same_placement(label, bounds(20, 12, 40, 14)));
        assert!(!same_placement(label, bounds(22, 10, 40, 14)));
        assert!(same_placement(label, bounds(20, 10, 44, 16)));
    }
}
