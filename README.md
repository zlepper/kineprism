# better-image-diff

`better-image-diff` is a local, deterministic structural image comparison tool for UI
screenshots. It is designed for coding agents and visual-regression workflows where a small
layout mistake should be described as geometry, not as a wall of changed pixels.

If a complete card is rendered five pixels too far to the right, a literal pixel diff marks both
the old and new card footprints. This tool instead tries to produce one `moved` finding with
`{"x":5,"y":0}`, matching bounds, a confidence value, and an annotated arrow. When there is not
enough evidence for a structural explanation, it deliberately falls back to `changed`.

The project is a Rust workspace with two packages:

- `better-image-diff-core` contains the path-independent comparison, metrics, report types, and
  in-memory renderer for reuse by other Rust tools.
- `better-image-diff` is a thin CLI for PNG decoding, argument handling, JSON output, and safe
  artifact commits.

It runs without a network connection, native computer-vision library, vision model, or external
service.

## Install and run

Install the CLI from this checkout:

```console
cargo install --path crates/better-image-diff-cli
```

Or run it directly from the workspace:

```console
cargo run --release -p better-image-diff -- \
  expected.png actual.png --output-dir comparison-output
```

The general form is:

```text
better-image-diff <EXPECTED> <ACTUAL> --output-dir <PATH>
    [--max-offset <PIXELS>]
    [--color-threshold <DELTA>]
    [--min-region-area <PIXELS>]
    [--force]
```

Options:

- `--output-dir` is required. Missing parent directories are created.
- `--max-offset` defaults to `128` pixels on each axis and bounds global and local translation
  searches.
- `--color-threshold` defaults to `2.3`. Pixels at or below this perceptual distance are treated
  as equivalent.
- `--min-region-area` defaults to `16`. Smaller connected residual regions are ignored.
- `--force` replaces only `expected.png`, `actual.png`, and `diff.png` in the output directory.
  Unrelated files are preserved.

On a completed comparison, stdout is exactly one pretty-printed JSON document followed by a
newline. Diagnostics and failures go to stderr.

| Exit code | Meaning |
| --- | --- |
| `0` | Comparison completed with no meaningful findings. |
| `1` | Comparison completed with one or more meaningful findings. |
| `2` | Arguments, inputs, processing, or artifact output failed. |

Exit code `1` is an expected visual-regression result, not a tool failure.

## JSON report

The report is stable and agent-oriented. It contains source dimensions and paths, effective
settings, global alignment, three metric scopes, deterministic findings and summary counts, and
artifact paths. Fields that do not apply to a finding are omitted rather than set to `null`.

An abbreviated movement looks like this:

```json
{
  "schema_version": 1,
  "equivalent": false,
  "alignment": {
    "offset": { "x": 0, "y": 0 },
    "confidence": 1.0
  },
  "summary": {
    "total": 1,
    "moved": 1,
    "resized": 0,
    "added": 0,
    "removed": 0,
    "changed": 0,
    "canvas_size": 0
  },
  "differences": [
    {
      "id": "D1",
      "kind": "moved",
      "expected_bounds": { "x": 100, "y": 40, "width": 300, "height": 120 },
      "actual_bounds": { "x": 105, "y": 40, "width": 300, "height": 120 },
      "offset": { "x": 5, "y": 0 },
      "confidence": 0.98,
      "message": "D1: Region appears 5 px right of its expected position."
    }
  ]
}
```

Offsets always mean `actual_position - expected_position`. Positive `x` is right, and positive
`y` is down. IDs are assigned after deterministic sorting, with `canvas_size` first and remaining
findings ordered top-to-bottom and left-to-right.

Consumers should act on explicit, high-confidence `moved` findings directly. A generic `changed`
finding means the visual difference is real but a narrower geometric explanation was not
trustworthy; inspect its bounds and diagnostic mask rather than guessing semantics.

## Similarity metrics

Metrics describe similarity but do not decide equivalence or the exit code. Final filtered
findings are the source of truth. Every report contains the same metrics over three coordinate
mappings:

- `raw` pairs pixels at identical coordinates in the canvas overlap.
- `global_aligned` applies the detected whole-image translation before pairing overlap pixels.
- `structural_aligned` also applies finalized, validated local movement correspondences. Each
  expected and actual pixel is consumed at most once. Added, removed, resized, and changed content
  is not silently warped away.

Each scope reports:

| Field | Range and interpretation |
| --- | --- |
| `compared_pixels` | Exact number of valid pixel pairs in the scope. |
| `expected_coverage` | Compared pairs divided by expected canvas pixels, from `0` to `1`. |
| `actual_coverage` | Compared pairs divided by actual canvas pixels, from `0` to `1`. |
| `mae` | Mean absolute error over linear, alpha-premultiplied RGBA, from `0` to `1`; lower is better. |
| `rmse` | Root mean squared error over the same four channels, from `0` to `1`; lower is better. |
| `psnr_db` | Peak signal-to-noise ratio with peak `1`; higher is better. Perfect equality is JSON `null` because its mathematical value is positive infinity. |
| `ssim` | Mean windowed structural similarity across premultiplied RGBA, from `-1` to `1`; higher is better. |
| `changed_pixel_ratio` | Fraction of pairs above the configured perceptual threshold, from `0` to `1`; lower is better. |

Coverage must be read alongside overlap-only scores: a high SSIM over a small crop does not imply
that the complete canvases match. A `null` score with `compared_pixels == 0` means no pairs were
available; `psnr_db: null` with nonzero pairs and zero error means a perfect scope.

MAE and RMSE use linear, alpha-premultiplied RGB and normalized alpha with equal channel weight.
Hidden RGB under fully transparent alpha therefore has no effect, while alpha changes remain
visible. SSIM uses an 11×11 Gaussian window (`sigma = 1.5`, `K1 = 0.01`, `K2 = 0.03`, `L = 1`),
sampled every eight pixels and adapted to the available area for smaller images.

## Artifacts

The CLI commits three PNGs only after all three have rendered and encoded successfully:

- `expected.png` overlays expected-side evidence on the target.
- `actual.png` overlays actual-side evidence on the implementation.
- `diff.png` is a white diagnostic canvas with dashed expected bounds, solid actual bounds,
  movement arrows, stable IDs, signed offsets, source canvas boundaries, and meaningful changed
  residual shapes.

Finding colors are blue for `moved`, purple for `resized`, green for `added`, orange for `removed`,
red for `changed`, and neutral gray for `canvas_size`. A stable ID and color connect the JSON record
to all applicable images.

Without `--force`, any existing artifact target aborts the operation. With `--force`, prior
artifacts are backed up inside an atomically reserved transaction directory and restored if the
commit cannot complete. The CLI refuses to overwrite either input, including a path alias.

## Use the core library

Add `better-image-diff-core` and an `image` version compatible with this workspace to another Rust
package. The core needs no PNG codec when callers already have `RgbaImage` values.

```rust
use better_image_diff_core::{CompareOptions, compare, render_artifacts};
use image::{Rgba, RgbaImage};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let expected = RgbaImage::from_pixel(320, 200, Rgba([248, 249, 252, 255]));
    let actual = expected.clone();
    let options = CompareOptions {
        max_offset: 64,
        ..CompareOptions::default()
    };

    let comparison = compare(&expected, &actual, &options)?;
    println!("MAE: {:?}", comparison.metrics.raw.mae);
    println!("SSIM: {:?}", comparison.metrics.structural_aligned.ssim);
    for difference in &comparison.differences {
        println!("{}: {}", difference.id, difference.message);
    }

    if !comparison.equivalent {
        let artifacts = render_artifacts(&expected, &actual, &comparison)?;
        println!("diagnostic dimensions: {:?}", artifacts.diff.dimensions());
    }
    Ok(())
}
```

`compare` and `render_artifacts` operate entirely in memory. `Comparison` contains no filesystem
paths or process state, while `RenderedArtifacts` returns three `RgbaImage` buffers for the caller
to store or display as appropriate.

## Perceptual and structural behavior

PNG pixels are decoded to RGBA8. Comparison converts sRGB to linear color and a Lab-like
perceptual representation, premultiplies visible color by alpha, and retains an alpha penalty.
The color threshold suppresses small antialiasing and rasterization differences. The minimum
region area then removes isolated connected noise.

The matcher uses deterministic image pyramids, bounded coarse-to-fine global alignment, connected
residual proposals, local forward/reverse translation validation, and conservative residual
classification. Repeated or textureless content is intentionally treated as ambiguous. This
keeps the report useful to an agent: a specific geometric claim should mean more than a coincidental
patch match.

## Realistic validation fixture

[`examples/realistic-ui`](examples/realistic-ui) contains an AI-generated SaaS dashboard pair used
for release-mode end-to-end validation. It includes real-looking typography, shadows, charts,
cards, transparency, and broad layout edits that are harder than the focused synthetic acceptance
fixtures. Run it with:

```console
cargo run --release -p better-image-diff -- \
  examples/realistic-ui/expected.png \
  examples/realistic-ui/actual.png \
  --output-dir examples/realistic-ui/output
```

The command is expected to exit `1`. Generated output is ignored by Git and can be safely
regenerated. The CLI integration suite also takes a crop of the generated target, moves its
complete “New Customers” card down by exactly 12 pixels, and asserts both a `moved` finding with
offset `(0, 12)` and improved structural-aligned MAE. Focused synthetic tests remain the
authoritative regression for the exact five-pixel, one-movement behavior.

## Limitations

- Inputs are PNG only. JPEG, WebP, GIF, SVG, and animation are unsupported.
- Images must use the same scale. Rotation, perspective, arbitrary affine transforms, and
  automatic scaling are not handled.
- Translation search is bounded by `--max-offset`.
- Classification uses conservative visual heuristics, not DOM knowledge, OCR, or UI semantics.
- Resizing is described but not geometrically warped for structural metrics.
- Highly repetitive, textureless, or extensively regenerated screenshots may yield generic
  changed/add/remove findings rather than confident movements.

## Development

The supported Rust version is 1.85 or newer. The final validation matrix is:

```console
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --release
```

The ignored full-HD smoke test can be run separately in release mode:

```console
cargo test --release -p better-image-diff-core --test performance -- --ignored
```

### Benchmarks

Criterion benchmarks exercise `better_image_diff_core::compare` on deterministic 1920×1080
application-like screenshots. They cover identical images, one card moved five pixels, and a
dashboard with many moved and appearance-changed elements. PNG decoding and artifact rendering are
excluded so the measurements isolate structural comparison.

```console
cargo bench -p better-image-diff-core --bench comparison
```

Criterion prints statistical timing estimates and throughput in pixels per second. It stores
detailed reports under `target/criterion/`. Each scenario validates its expected comparison result
before measurement so an algorithm regression cannot silently produce a faster but invalid sample.
