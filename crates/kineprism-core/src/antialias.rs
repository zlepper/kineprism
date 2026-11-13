//! Antialiased-edge detection adapted from pixelmatch (ISC License, Copyright (c) Mapbox), which
//! follows "Anti-aliased Pixel and Intensity Slope Detector" by V. Vysniauskas (2009).
//!
//! Unlike pixelmatch, a pixel is only tolerated when it is antialiased in both images, so ink
//! that becomes solid, such as a bolder stroke or a new line, still counts as a difference. Its
//! color must also blend its neighbors', so an edge that changes hue is not tolerated.

use crate::color::{NormalizedImage, RAMP_TOLERANCE};
use crate::{CompareOptions, Offset};

type Point = (u32, u32);

/// How two corresponding pixels relate under the configured options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PixelMatch {
    /// Within the color threshold.
    Same,
    /// Beyond the color threshold, but antialiased in both images while antialiasing is ignored.
    Antialiased,
    /// A meaningful difference.
    Different,
}

pub(crate) fn compare(
    expected: &NormalizedImage,
    (expected_x, expected_y): Point,
    actual: &NormalizedImage,
    (actual_x, actual_y): Point,
    options: &CompareOptions,
) -> PixelMatch {
    if expected
        .pixel(expected_x, expected_y)
        .perceptual_distance(actual.pixel(actual_x, actual_y))
        <= options.color_threshold
    {
        PixelMatch::Same
    } else if options.ignore_antialiasing
        && antialiased(
            expected,
            (expected_x, expected_y),
            actual,
            (actual_x, actual_y),
        )
        && antialiased(
            actual,
            (actual_x, actual_y),
            expected,
            (expected_x, expected_y),
        )
    {
        PixelMatch::Antialiased
    } else {
        PixelMatch::Different
    }
}

/// Whether two corresponding pixels differ meaningfully under the configured options.
pub(crate) fn differs(
    expected: &NormalizedImage,
    expected_point: Point,
    actual: &NormalizedImage,
    actual_point: Point,
    options: &CompareOptions,
) -> bool {
    compare(expected, expected_point, actual, actual_point, options) == PixelMatch::Different
}

/// A pixel is antialiased when its color blends a darker and a brighter neighbor, and one of
/// those neighbors sits inside a flat area in both images.
fn antialiased(
    image: &NormalizedImage,
    point: Point,
    other: &NormalizedImage,
    other_point: Point,
) -> bool {
    let center = image.pixel(point.0, point.1).lightness();
    let mut identical = u32::from(on_edge(image, point));
    let mut darkest: Option<(Point, f64)> = None;
    let mut brightest: Option<(Point, f64)> = None;
    for neighbor in neighbors(image, point) {
        let delta = image.pixel(neighbor.0, neighbor.1).lightness() - center;
        if delta.abs() <= f64::EPSILON {
            identical += 1;
            if identical > 2 {
                return false;
            }
        } else if delta < darkest.map_or(0.0, |(_, value)| value) {
            darkest = Some((neighbor, delta));
        } else if delta > brightest.map_or(0.0, |(_, value)| value) {
            brightest = Some((neighbor, delta));
        }
    }
    let (Some((darkest, _)), Some((brightest, _))) = (darkest, brightest) else {
        return false;
    };
    let pixel = |(x, y): Point| image.pixel(x, y);
    if pixel(point).ramp_distance(pixel(darkest), pixel(brightest)) > RAMP_TOLERANCE {
        return false;
    }
    [darkest, brightest].into_iter().any(|neighbor| {
        let step = Offset {
            x: step(point.0, neighbor.0),
            y: step(point.1, neighbor.1),
        };
        has_many_siblings(image, neighbor)
            && step
                .translate_point(other_point, other.width(), other.height())
                .is_some_and(|mapped| has_many_siblings(other, mapped))
    })
}

fn has_many_siblings(image: &NormalizedImage, point: Point) -> bool {
    let color = image.pixel(point.0, point.1);
    let mut identical = u32::from(on_edge(image, point));
    for neighbor in neighbors(image, point) {
        if image.pixel(neighbor.0, neighbor.1) == color {
            identical += 1;
            if identical > 2 {
                return true;
            }
        }
    }
    false
}

/// The signed distance between neighboring coordinates, which is at most one.
fn step(from: u32, to: u32) -> i32 {
    match to.cmp(&from) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

fn on_edge(image: &NormalizedImage, (x, y): Point) -> bool {
    x == 0 || y == 0 || x + 1 == image.width() || y + 1 == image.height()
}

fn neighbors(image: &NormalizedImage, (x, y): Point) -> impl Iterator<Item = Point> {
    let right = (x + 1).min(image.width() - 1);
    let bottom = (y + 1).min(image.height() - 1);
    (y.saturating_sub(1)..=bottom).flat_map(move |neighbor_y| {
        (x.saturating_sub(1)..=right)
            .map(move |neighbor_x| (neighbor_x, neighbor_y))
            .filter(move |neighbor| *neighbor != (x, y))
    })
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};

    use super::*;

    const LIGHT: Rgba<u8> = Rgba([255, 255, 255, 255]);
    const DARK: Rgba<u8> = Rgba([0, 0, 0, 255]);

    /// A 9x9 light canvas with a dark three-pixel-wide vertical stroke at columns 3..6 and the
    /// given colors in the edge column 6.
    fn stroke(edge: Rgba<u8>) -> NormalizedImage {
        let mut image = RgbaImage::from_pixel(9, 9, LIGHT);
        for y in 0..9 {
            for x in 3..6 {
                image.put_pixel(x, y, DARK);
            }
            image.put_pixel(6, y, edge);
        }
        NormalizedImage::try_new(&image).expect("normalized")
    }

    fn options() -> CompareOptions {
        CompareOptions {
            ignore_antialiasing: true,
            ..CompareOptions::default()
        }
    }

    #[test]
    fn differently_blended_stroke_edges_are_antialiased() {
        let soft = stroke(Rgba([160, 160, 160, 255]));
        let hard = stroke(Rgba([90, 90, 90, 255]));

        assert_eq!(
            compare(&soft, (6, 4), &hard, (6, 4), &options()),
            PixelMatch::Antialiased
        );
        assert_eq!(
            compare(&soft, (6, 4), &hard, (6, 4), &CompareOptions::default()),
            PixelMatch::Different
        );
        assert_eq!(
            compare(&soft, (6, 4), &soft, (6, 4), &options()),
            PixelMatch::Same
        );
    }

    #[test]
    fn a_fringe_that_becomes_solid_ink_is_a_difference() {
        let soft = stroke(Rgba([160, 160, 160, 255]));
        let bold = stroke(DARK);

        assert!(differs(&soft, (6, 4), &bold, (6, 4), &options()));
    }

    #[test]
    fn a_new_solid_line_is_not_antialiased() {
        let without = stroke(LIGHT);
        let with = stroke(DARK);

        assert!(differs(&without, (6, 4), &with, (6, 4), &options()));
    }

    #[test]
    fn an_edge_that_changes_hue_is_not_antialiased() {
        let gray = stroke(Rgba([128, 128, 128, 255]));
        let red = stroke(Rgba([200, 40, 40, 255]));

        assert!(differs(&gray, (6, 4), &red, (6, 4), &options()));
    }

    #[test]
    fn an_isolated_dot_on_a_flat_area_is_not_antialiased() {
        let flat = NormalizedImage::try_new(&RgbaImage::from_pixel(5, 5, LIGHT)).expect("flat");
        let mut dotted = RgbaImage::from_pixel(5, 5, LIGHT);
        dotted.put_pixel(2, 2, Rgba([128, 128, 128, 255]));
        let dotted = NormalizedImage::try_new(&dotted).expect("dotted");

        assert!(differs(&flat, (2, 2), &dotted, (2, 2), &options()));
    }
}
