# Better Image Diff: Implementation Plan

## 1. Objective

Build `better-image-diff`, a deterministic Rust command-line tool for comparing two UI screenshots structurally.

The primary use case is an AI coding agent comparing an implemented UI screenshot with a target design. A conventional pixel-by-pixel diff turns a small positioning error into a large region of changed pixels. This tool must instead recognize common geometric relationships, such as a component being translated by five pixels, and express those relationships directly in both machine-readable and visual output.

Version one will:

- Accept two same-scale PNG images representing the expected and actual UI.
- Compare images even when their canvas dimensions differ.
- Detect global and local translations up to a configurable distance.
- Conservatively classify remaining differences as resized, added, removed, or changed.
- Ignore small perceptual and rasterization differences by default.
- Emit a structured JSON report intended for automated consumption.
- Generate three annotated PNG artifacts and persist the JSON report for later inspection.
- Run entirely locally without a vision model, network access, native OpenCV installation, or external service.

The implementation must favor trustworthy findings over aggressive classification. When the evidence for a specific explanation is weak, report a generic `changed` region rather than inventing a movement or semantic interpretation.

## 2. Scope and Non-Goals

### In scope

- Lossless PNG decoding and PNG artifact generation.
- Opaque, translucent, and transparent pixels.
- Images with different canvas dimensions, provided their content remains at the same scale.
- Whole-image and local translations.
- Basic detection of resized, added, removed, and appearance-changed regions.
- Perceptual color tolerance and minimum-region filtering.
- Stable JSON intended for use by AI agents and scripts.
- Deterministic output ordering and rendering.
- Linux, macOS, and Windows support through portable Rust code.

### Out of scope for version one

- JPEG, WebP, GIF, SVG, or animated input.
- Automatic scaling, rotation, perspective correction, or arbitrary affine transforms.
- DOM-aware or semantic labels such as `button`, `card`, or `heading`.
- OCR or text-content comparison.
- Neural models, external APIs, or network calls.
- Pixel-perfect emulation of Applitools or another commercial visual-testing product.
- Configuration files or ignore-region files.
- Batch comparison, directory traversal, or a long-running service. These can be built later on top of the reusable core crate.

## 3. Project Structure

Create a Rust 2024 workspace containing a reusable library crate and a thin CLI crate:

```text
better-image-diff/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── goal.md
└── crates/
    ├── better-image-diff-core/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── alignment.rs
    │       ├── classify.rs
    │       ├── color.rs
    │       ├── compare.rs
    │       ├── error.rs
    │       ├── geometry.rs
    │       ├── matching.rs
    │       ├── metrics.rs
    │       ├── render.rs
    │       └── report.rs
    └── better-image-diff-cli/
        ├── Cargo.toml
        ├── src/
        │   └── main.rs
        └── tests/
            └── cli.rs
```

The library package and crate are named `better-image-diff-core` and `better_image_diff_core`. The CLI package and executable are named `better-image-diff`. The workspace root is virtual and uses resolver version 2.

All image-analysis, matching, classification, metric calculation, report-domain types, and annotation rendering belong to the core crate. The CLI crate is limited to:

- Parsing process arguments.
- Reading and decoding input files.
- Checking and writing output paths atomically.
- Adding caller-provided paths to the JSON envelope.
- Printing JSON and diagnostics.
- Mapping comparison outcomes and failures to process exit codes.

The exact core module split may be collapsed where a module would otherwise be trivial, but these concerns must remain logically separated:

- Normalized pixel representation and input-independent image validation.
- Global alignment.
- Local proposal generation and translation matching.
- Residual classification.
- Similarity metric calculation.
- JSON report types and serialization.
- Artifact rendering.

Declare shared versions under `[workspace.dependencies]` and use only the approved dependency set:

- Core: `image`, with default features disabled, for `RgbaImage` and in-memory buffers. The core must not require a codec feature.
- Core: `serde`, using derive support, for reusable report types.
- CLI: `clap`, using derive support, for argument parsing.
- CLI: `image`, using the same workspace version, with default features disabled and PNG support enabled for decoding and encoding.
- CLI: `serde` and `serde_json` for the CLI report envelope and stdout serialization.
- CLI: a path dependency on `better-image-diff-core`.

Do not add an error-handling, drawing, computer-vision, test-helper, or temporary-directory crate. Implement the small amount of required support with the standard library.

Commit `Cargo.lock` so builds and tests use resolved dependency versions reproducibly.

### Core library API

The core crate exposes a documented reusable API independent of filesystem paths and process behavior:

```rust
pub fn compare(
    expected: &image::RgbaImage,
    actual: &image::RgbaImage,
    options: &CompareOptions,
) -> Result<Comparison, CompareError>;

pub fn render_artifacts(
    expected: &image::RgbaImage,
    actual: &image::RgbaImage,
    comparison: &Comparison,
) -> Result<RenderedArtifacts, RenderError>;
```

The public supporting types include `CompareOptions`, `Comparison`, `ComparisonSummary`, `SuppressionSummary`, `Alignment`, `Difference`, `DifferenceKind`, `Bounds`, `Offset`, `SimilarityMetrics`, `MetricSet`, `RenderedArtifacts`, `CompareError`, and `RenderError`.

- `Comparison` contains dimensions, settings, alignment, metrics, sorted differences, summary counts, and equivalence state. It contains no filesystem paths.
- `RenderedArtifacts` contains three in-memory `RgbaImage` buffers named `expected`, `actual`, and `diff`.
- Public report-domain types derive `Debug`, `Clone`, `PartialEq`, and `Serialize` where their contents allow it.
- `CompareOptions::default()` exactly matches the CLI defaults.
- Public fields that are part of the serialized report remain directly readable. Construction of invariant-bearing types uses validated constructors where appropriate.
- Every public item has rustdoc explaining coordinate systems, offset direction, ranges, and failure behavior.
- The CLI JSON envelope embeds the serialized core `Comparison` data and adds input and artifact paths; the CLI must not duplicate the comparison algorithms or recompute summary values.

## 4. CLI Contract

The executable name is `better-image-diff`.

```text
better-image-diff <EXPECTED.png> <ACTUAL.png> --output-dir <PATH>
    [--max-offset <PIXELS>]
    [--color-threshold <DELTA>]
    [--min-region-area <PIXELS>]
    [--force]
```

### Positional arguments

- `EXPECTED.png`: the target or reference design.
- `ACTUAL.png`: the implementation being evaluated.

The argument names and all messages must consistently preserve this direction. An offset of `{ "x": 5, "y": -2 }` means the actual region is five pixels to the right and two pixels above its expected position.

### Options

- `--output-dir <PATH>` is required. The program creates the directory and missing parents if necessary.
- `--max-offset <PIXELS>` defaults to `128`. It must be a non-negative integer. It controls both global and local translation searches.
- `--color-threshold <DELTA>` defaults to `2.3`. It must be finite and non-negative. It controls the perceptual pixel-distance threshold.
- `--min-region-area <PIXELS>` defaults to `16`. It must be a positive integer and represents the minimum significant connected-region area.
- `--force` permits replacement of the four known artifact files. It must not delete or modify unrelated files in the output directory.
- Standard `--help` and `--version` behavior comes from `clap`.

### Standard streams

- On a completed comparison, stdout contains exactly one JSON document and a trailing newline.
- Human-readable diagnostics and processing errors go to stderr.
- Progress messages must not be printed to stdout.
- A failed comparison does not emit a success-shaped JSON report.

### Exit codes

- `0`: comparison completed and no meaningful differences remain after configured tolerances.
- `1`: comparison completed and at least one meaningful difference was found.
- `2`: invalid arguments, unreadable or invalid input, processing failure, artifact collision, or artifact-writing failure.

`clap` validation failures must also resolve to exit code `2`, overriding its defaults if necessary to keep the contract consistent.

## 5. Artifact Contract

Write these files inside `--output-dir`:

- `report.json`: the exact pretty-printed JSON document emitted on stdout, including its trailing newline.
- `expected.png`: the expected screenshot with expected-side regions marked.
- `actual.png`: the actual screenshot with actual-side regions marked.
- `diff.png`: a white-background diagnostic image containing only comparison annotations.

If any target file already exists and `--force` is absent, fail before performing any replacement. With `--force`, replace only these exact four files.

Render all image buffers and serialize the report fully before committing output files. Write each artifact to a transaction directory, finish all encodes and serialization, and then rename the completed files into place. Clean up temporary files on a handled error. This minimizes partially written artifacts without recursively modifying the output directory.

### Annotation conventions

Use the same finding ID and category color on all applicable artifacts:

- `moved`: blue.
- `resized`: purple.
- `added`: green.
- `removed`: orange.
- `changed`: red.
- `canvas_size`: neutral gray.

On `expected.png`:

- Mark an expected bounding box for moved, resized, removed, and changed regions.
- Do not fabricate an expected box for a purely added region.

On `actual.png`:

- Mark an actual bounding box for moved, resized, added, and changed regions.
- Do not fabricate an actual box for a purely removed region.

On `diff.png`:

- Use a white background.
- Size the canvas to `max(expected.width, actual.width)` by `max(expected.height, actual.height)`.
- Draw expected bounds with dashed outlines and actual bounds with solid outlines.
- Draw an arrow from the expected region center to the actual region center for movement findings.
- Label findings with their stable ID. Movement labels also show signed `dx` and `dy` values.
- Indicate both source canvas boundaries when their dimensions differ.
- Draw meaningful residual changed pixels or a translucent mask inside residual bounds so the image conveys shape rather than boxes alone.

Implement the small set of required lines, rectangles, arrowheads, dashed strokes, alpha blending, and labels directly. Embed a compact bitmap font supporting the ASCII characters needed for finding IDs and signed integer offsets instead of adding a drawing or font dependency.

Rendering must clamp coordinates safely at image edges and must never panic due to zero-sized, one-pixel, or partially clipped regions.

## 6. JSON Report

The top-level report has this conceptual shape:

```json
{
  "schema_version": 1,
  "equivalent": false,
  "expected": {
    "path": "expected.png",
    "width": 1440,
    "height": 900
  },
  "actual": {
    "path": "actual.png",
    "width": 1440,
    "height": 900
  },
  "settings": {
    "max_offset": 128,
    "color_threshold": 2.3,
    "min_region_area": 16
  },
  "alignment": {
    "offset": { "x": 0, "y": 0 },
    "confidence": 1.0
  },
  "metrics": {
    "raw": {
      "compared_pixels": 1296000,
      "expected_coverage": 1.0,
      "actual_coverage": 1.0,
      "mae": 0.0124,
      "rmse": 0.0481,
      "psnr_db": 26.36,
      "ssim": 0.9712,
      "changed_pixel_ratio": 0.034
    },
    "global_aligned": {
      "compared_pixels": 1296000,
      "expected_coverage": 1.0,
      "actual_coverage": 1.0,
      "mae": 0.0124,
      "rmse": 0.0481,
      "psnr_db": 26.36,
      "ssim": 0.9712,
      "changed_pixel_ratio": 0.034
    },
    "structural_aligned": {
      "compared_pixels": 1296000,
      "expected_coverage": 1.0,
      "actual_coverage": 1.0,
      "mae": 0.0011,
      "rmse": 0.0063,
      "psnr_db": 44.01,
      "ssim": 0.9989,
      "changed_pixel_ratio": 0.002
    }
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
  "suppression": {
    "movement_border_regions": 0,
    "movement_border_pixels": 0
  },
  "differences": [],
  "artifacts": {
    "expected": "output/expected.png",
    "actual": "output/actual.png",
    "diff": "output/diff.png"
  }
}
```

Paths must be serialized consistently as provided or derived by the CLI; do not attempt filesystem canonicalization, which can fail or make results environment-dependent.

Each difference record contains:

```json
{
  "id": "D1",
  "kind": "moved",
  "expected_bounds": { "x": 100, "y": 40, "width": 300, "height": 120 },
  "actual_bounds": { "x": 105, "y": 40, "width": 300, "height": 120 },
  "offset": { "x": 5, "y": 0 },
  "confidence": 0.98,
  "message": "Region D1 appears 5 px right of its expected position."
}
```

Fields that do not apply are omitted rather than serialized as `null`:

- `added` has only `actual_bounds`.
- `removed` has only `expected_bounds`.
- `canvas_size` describes expected and actual dimensions and does not require region bounds.
- `offset` applies to `moved` and may be included for `resized` when the centers also moved.
- `confidence` is a finite value clamped to `[0.0, 1.0]`.

Order differences deterministically:

1. `canvas_size`, if present.
2. Remaining findings ordered by the topmost applicable bound, then leftmost bound.
3. Break ties by kind and geometry.

Assign IDs after sorting so the same comparison and settings always produce the same IDs and JSON ordering.

`equivalent` is true exactly when `differences` is empty. Summary counts must be derived from the final difference collection rather than maintained independently.

## 7. Similarity Metrics

The core library computes and reports three metric sets so consumers can distinguish literal image similarity from similarity after structural correspondence:

- `raw`: compare pixels at the same `(x, y)` coordinates in the overlapping canvas without applying any detected movement.
- `global_aligned`: compare the overlap after applying the detected global translation, but before applying local region matches.
- `structural_aligned`: start with global alignment and additionally pair pixels inside validated moved regions with their detected actual positions. Do not warp resized regions or suppress added, removed, or changed regions.

Every set is a `MetricSet` containing:

- `compared_pixels`: number of pixel pairs included in the metric calculation.
- `expected_coverage`: compared expected pixels divided by the full expected pixel count.
- `actual_coverage`: compared actual pixels divided by the full actual pixel count.
- `mae`: mean absolute error over normalized visible RGBA channels, in `[0, 1]`; lower is better.
- `rmse`: root mean squared error over the same channels, in `[0, 1]`; lower is better.
- `psnr_db`: peak signal-to-noise ratio in decibels using a normalized peak value of `1`; higher is better. Serialize this as `null` when MSE is zero because the mathematical value is positive infinity and JSON has no infinity value.
- `ssim`: mean structural similarity in `[-1, 1]`; higher is better. Calculate windowed SSIM over normalized premultiplied RGBA channels and average the four channel scores.
- `changed_pixel_ratio`: fraction of compared pixel pairs whose perceptual distance exceeds `color_threshold`; lower is better.

Metric rules:

- MAE and RMSE operate on linear, alpha-premultiplied RGB plus normalized alpha, averaging all four channels equally. Stored RGB beneath fully transparent alpha therefore has no effect, while alpha differences remain visible.
- SSIM uses the standard constants `K1 = 0.01`, `K2 = 0.03`, and dynamic range `L = 1.0`, with an 11x11 Gaussian window. For images narrower or shorter than the window, use one window covering the available pixels. Document this exact variant because SSIM implementations can otherwise differ.
- Raw coverage is the same-coordinate intersection of both canvases. Aligned coverage uses only valid mapped pixel pairs; pixels outside either canvas are not synthesized with a background color.
- Structural alignment consumes each expected and actual pixel at most once. If validated moved regions overlap, resolve ownership deterministically by higher confidence, then finding order.
- Coverage accompanies every metric set because overlap-only scores can look deceptively strong when canvas sizes differ or content is cropped.
- Metric values must be finite except `psnr_db`, which uses JSON `null` for a perfect match. Empty comparisons return `compared_pixels = 0`, zero coverage, and omit score calculations from the Rust value with `Option`; the JSON representation uses `null` for unavailable numeric scores.
- Metrics are descriptive and never determine `equivalent` or the process exit code by themselves. Final filtered findings remain the source of truth for equivalence.
- Accumulate sums using numerically stable `f64` arithmetic, then serialize deterministic finite values without display-only rounding.

The public `SimilarityMetrics` type contains `raw`, `global_aligned`, and `structural_aligned` fields. The public `MetricSet` documents all ranges and the exact comparison scope.

## 8. Pixel Normalization and Perceptual Difference

Decode both PNGs to RGBA8. Invalid PNGs and unsupported formats produce a clear stderr error and exit code `2`.

Normalize pixels as follows:

- Treat stored RGB values under alpha `0` as irrelevant.
- Convert sRGB channels to linear RGB.
- Convert linear RGB to a perceptual Lab-like representation using in-project conversion functions.
- Retain alpha as a separate normalized channel.
- Compute perceptual distance from visible premultiplied color plus an alpha penalty so transparency changes remain detectable.

A pixel is locally equivalent when its distance is less than or equal to `--color-threshold`.

The same normalized representation should feed alignment and local matching. Edge descriptors should be derived from perceptual luminance rather than encoded sRGB values.

All conversion functions require focused unit tests for black, white, primary colors, transparent pixels, symmetry, and finite output.

## 9. Structural Comparison Pipeline

### Stage 1: Decode and validate the library boundary

- The CLI parses arguments, verifies both input paths can be decoded as PNG, converts them to `RgbaImage`, and constructs `CompareOptions`.
- The core validates `CompareOptions`, dimensions, and image invariants independently so non-CLI callers receive the same safety guarantees.
- Ensure dimensions and all area/index calculations fit safely in platform memory and integer types.
- The CLI refuses artifact collisions before invoking expensive core matching.

### Stage 2: Build image pyramids

- Build successively reduced representations until the smallest useful level or a small fixed minimum dimension is reached.
- Each pyramid level stores perceptual luminance/color summaries and gradient or edge strength.
- Downsampling must be deterministic and must handle odd dimensions explicitly.
- Use coarse levels to eliminate implausible offsets before scoring candidates at higher resolution.

### Stage 3: Estimate global translation

- Search integer translations within `[-max_offset, +max_offset]` for both axes.
- Score candidates over their overlapping area using normalized perceptual and edge error.
- Normalize scores by compared area so different overlap sizes remain comparable.
- Require overlap with a meaningful portion of the smaller image; when one image is contained in another, the smaller content can still provide full overlap.
- Refine the best coarse candidate at each finer pyramid level.
- Calculate confidence from absolute match quality and separation between the best and second-best sufficiently distinct candidates.
- A non-zero global translation is a real reported movement, not an alignment silently removed from the result.
- Use the translation internally to pair corresponding content before local residual analysis.
- If global alignment is ambiguous, retain offset `(0, 0)` with low confidence and continue conservatively.

### Stage 4: Build the residual mask

- Compare pixels in aligned overlapping coordinates using the configured perceptual threshold.
- Track unmatched expected and actual canvas areas separately.
- Produce expected-side and actual-side residual masks.
- Filter isolated regions smaller than `min_region_area`.
- Apply only small, deterministic mask-closing/grouping operations needed to join fragments of the same visual feature. Do not use large dilation that would combine unrelated neighboring components.

### Stage 5: Generate local region proposals

- Find connected residual components with eight-neighbor connectivity.
- Group nearby components when their padded structural descriptors and spatial relationship indicate a shared object, such as the outline and text inside one shifted box.
- Include a small context margin around proposals so solid shapes and boundaries provide matching evidence.
- Clamp padded bounds to their source image.
- Exclude proposals that contain too little texture, edge information, or color variation to support translation matching.

### Stage 6: Match local translations

For each expected-side proposal:

- Search the actual image within `max_offset` of its position after accounting for the estimated global correspondence.
- Use the pyramid for coarse candidate discovery and full-resolution perceptual/edge scores for final validation.
- Compare the best translated candidate with:

  - The proposal at its unshifted position.
  - The second-best spatially distinct candidate.
  - A reverse match from actual back to expected.

- Classify as `moved` only when:

  - The translated match is materially better than the same-position match.
  - It has a sufficient margin over the second-best candidate.
  - The reverse match returns to the original region within a small coordinate tolerance.
  - The candidate contains enough visual information to be distinctive.

- Derive confidence from these checks and clamp it to `[0, 1]`.
- Merge adjacent proposals with nearly identical displacement vectors when the combined-region validation also succeeds.
- Mark both expected and actual evidence as consumed so one visual feature does not generate duplicate move and residual findings.

The result for a simple box shifted five pixels should be one movement record whose bounds cover the structurally changed box, not separate records for every changed edge or text glyph.

### Stage 7: Classify remaining differences

Use unmatched proposal pairs and residual masks:

- `resized`: expected and actual regions have clearly correlated structural/color content, compatible centers or anchors, and materially different width or height. Do not classify arbitrary content replacement as a resize.
- `added`: distinctive actual content exists where the corresponding expected area is locally background-like, with no credible expected match.
- `removed`: distinctive expected content exists where the corresponding actual area is locally background-like, with no credible actual match.
- `changed`: both sides contain content but no reliable translation/resize explanation exists, or evidence for a narrower category is ambiguous.
- `canvas_size`: emit exactly one record whenever dimensions differ, independent of other findings.

Background likeness should be estimated from nearby border colors and low edge density, not from an assumption that backgrounds are white.

After classification, remove insignificant components again and coalesce immediately adjacent residual components only when they have the same class and compatible evidence.

Defer low-value residual components bordering validated local movements so agents can address the
larger structural cause first:

- Apply this only to residual `changed` components after primary structural classification.
- Require the residual bounds to fit wholly within the border halo of exactly one movement.
- Scale the halo as `ceil(sqrt(movement_area) / 128)`, clamped to 1–8 pixels.
- Scale the maximum suppressed connected area as `movement_area / 512`, clamped to 16–1024 pixels.
- Never suppress primary `moved`, `resized`, `added`, or `removed` findings, large residuals,
  out-of-halo residuals, or residuals ambiguous between movements.
- Return a structured suppression summary containing region and pixel counts plus guidance to
  compare again after correcting the movements. Suppression affects finding priority and
  annotations, not similarity metrics.

### Stage 8: Calculate similarity metrics

- Calculate `raw` metrics directly from the same-coordinate overlap.
- Calculate `global_aligned` metrics using the finalized global alignment.
- Calculate `structural_aligned` metrics using the global alignment plus only the finalized, validated movement correspondences.
- Calculate coverage from the exact valid pixel pairs used in each set.
- Keep this work in the core crate and reuse normalized pixel buffers rather than decoding or converting pixels again.

### Stage 9: Report and render

- Sort findings and assign IDs.
- Derive summary counts and `equivalent`.
- Return a path-independent `Comparison`, including metrics, from the core crate.
- Render all three in-memory artifacts from the finalized `Comparison` through the core renderer.
- Let the CLI add input/artifact paths, serialize the JSON envelope once, atomically commit the three images and `report.json`, and then write those same JSON bytes to stdout.
- Let the CLI return the exit code determined by the core `equivalent` value.

## 10. Geometry and Safety Rules

- Store image dimensions and bounds in unsigned integer types compatible with `image`; use signed wide integers for offsets and intermediate coordinate arithmetic.
- Convert between signed and unsigned coordinates only after explicit bounds checks.
- Use checked multiplication for image areas and buffer sizes.
- Define rectangles as half-open ranges: `[x, x + width)` and `[y, y + height)`.
- Reject or omit zero-area region findings.
- Use a single documented convention for offset direction: `actual_position - expected_position`.
- Never index an image through unchecked offset arithmetic.
- Avoid panics for malformed input, extreme dimensions, empty overlap, all-transparent images, or images containing no distinctive content.

## 11. Error Handling

Define public core `CompareError` and `RenderError` enums plus a private CLI error enum using the standard library. Core errors must not mention filesystem paths or process concepts. CLI errors add path and I/O context. Errors must retain useful context without exposing sensitive or irrelevant environment data.

Cover at least:

- Missing or invalid CLI arguments.
- Numeric option outside its allowed range.
- Input and output path collision where an artifact would overwrite an input.
- Input open/read failure.
- Invalid or unsupported image data.
- Excessive or overflowing image dimensions.
- Output artifact already exists without `--force`.
- Output directory creation failure.
- Artifact encoding, temporary write, or rename failure.
- JSON serialization or stdout write failure.

Error messages identify which input or artifact failed. Do not emit debug dumps or backtraces by default.

## 12. Testing Strategy

Tests must exercise behavior with real in-memory/generated images. Do not mock image decoding, filesystem operations, or the comparison engine. Use only the standard library for temporary test directories, with collision-resistant names incorporating the process ID and an atomic counter. Ensure cleanup through a small test-only guard type.

### Unit tests

- Perceptual conversion produces finite and stable values for representative colors.
- Transparent RGB differences are ignored when alpha is zero.
- Alpha differences remain detectable.
- Rectangle intersection, translation, clipping, and center calculations follow the documented conventions.
- Connected-component extraction uses eight-neighbor connectivity and deterministic ordering.
- Pyramid downsampling handles odd, narrow, and one-pixel dimensions.
- Offset scoring is normalized by overlap and uses the documented direction.
- Confidence decreases for ambiguous repeated matches.
- Rendering primitives safely clip at all image edges.
- Bitmap labels render deterministically.
- JSON omits non-applicable optional fields and derives summary counts correctly.
- MAE and RMSE match hand-calculated normalized pixel examples.
- PSNR is `None`/JSON `null` for a perfect match and matches a known finite example otherwise.
- Windowed SSIM is `1.0` for identical content, remains in range, and decreases for a known structural change.
- Changed-pixel ratio uses the configured perceptual threshold.
- Raw, global-aligned, and structural-aligned scopes pair the expected coordinates on translated fixtures.
- Metric coverage reflects cropping and differing canvas dimensions.

### Structural comparison scenarios

Generate simple UI-like fixtures using rectangles, borders, text-like stripe patterns, and contrasting backgrounds:

1. Identical images produce no findings, three valid PNGs, a matching `report.json`, and exit `0`.
2. A bordered content box shifted right by 5 px produces one `moved` finding with offset `(5, 0)` and no giant added/removed pair.
3. A box shifted left and upward verifies signed offsets and edge clipping.
4. Two components shifted independently produce two stable IDs and correct vectors.
5. A whole layout shifted on an equal canvas is reported as global movement.
6. A smaller canvas containing the same-scale content can be aligned; the size mismatch remains reported.
7. Added canvas space, cropped content, and non-overlapping bands remain visible in the residual output.
8. A movement exactly at `max_offset` is detected.
9. The same movement beyond a smaller configured limit falls back to residual classification.
10. A resized box with preserved internal structure is classified as `resized`.
11. A newly introduced component is `added`.
12. A deleted component is `removed`.
13. A color or content replacement at the same bounds is `changed`.
14. Below-threshold color/rasterization noise is ignored.
15. The same noise is reported when `color_threshold` is reduced appropriately.
16. Components smaller than `min_region_area` are ignored, and become visible when the option is lowered.
17. A repetitive grid does not receive a high-confidence invented translation.
18. A solid, textureless region without distinctive boundaries falls back conservatively.
19. Partially transparent content is compared correctly.
20. Fully transparent images do not expose hidden RGB channel differences.
21. A globally shifted layout improves global-aligned metrics relative to raw metrics.
22. A locally shifted box improves structural-aligned metrics relative to global-aligned metrics without removing the movement finding.
23. Added, removed, and resized regions remain reflected in coverage or error instead of being silently corrected by structural metrics.
24. Small residual fringes around a large movement are summarized as suppressed, while larger,
    distant, and movement-ambiguous residuals remain visible.

### Core API integration tests

- A consumer can construct two `RgbaImage` values, call `compare`, inspect findings and all metric sets, and render artifacts without filesystem access.
- `CompareOptions::default()` matches documented CLI defaults.
- Invalid library options or excessive dimensions return typed errors rather than panicking.
- The core comparison and rendering results contain no caller paths or other CLI-only state.
- Serialization of public core report types is stable and embeds correctly in the CLI JSON envelope.
- Movement-border suppression counts and guidance are available through the public comparison result.

### CLI integration tests

- Valid invocation emits parseable JSON and only JSON on stdout.
- `report.json` is byte-equivalent to stdout and participates in collision, rollback, and `--force` behavior.
- Exit codes are exactly `0`, `1`, and `2` for equivalent, different, and failed comparisons.
- Artifact files exist, decode as PNGs, and have the expected dimensions.
- Expected and actual annotations contain the same IDs/colors as the diff artifact.
- Existing artifacts fail without `--force` and are replaced with it.
- `--force` preserves unrelated output-directory files.
- Missing input, corrupt PNG, non-PNG content, and unwritable output locations produce exit `2` and useful stderr messages.
- An output artifact path colliding with an input is rejected even with `--force`.
- Repeated identical invocations produce byte-equivalent JSON apart from caller-provided paths and deterministic image artifacts.

## 13. Performance and Determinism

- Avoid exhaustive full-resolution evaluation of every possible offset. Use coarse-to-fine candidate refinement and restrict expensive local scoring to residual proposals.
- Reuse computed normalized pixels, edge maps, pyramids, and integral summaries where useful.
- Reuse normalized buffers and alignment maps for metrics; do not perform another image decode or structural search.
- Avoid copying full images for each candidate offset.
- Keep finding generation single-threaded in version one unless profiling demonstrates a need; deterministic behavior is more important than premature parallelism.
- Do not include timestamps, random identifiers, absolute canonical paths, or nondeterministic map iteration in JSON or artifacts.
- Add a release-mode smoke benchmark or ignored timing test using a generated 1920x1080 UI screenshot and the default 128 px search radius. It should guard against obviously quadratic full-resolution search behavior without imposing a machine-specific hard timing assertion in the normal test suite.

## 14. Documentation

The README must include:

- The agent-oriented motivation and an example of a five-pixel component shift.
- CLI installation with `cargo install --path crates/better-image-diff-cli` and execution with `cargo run --release -p better-image-diff --`.
- Core library usage from another Rust package, including `CompareOptions`, `compare`, metric inspection, and optional artifact rendering.
- Full CLI usage and option defaults.
- Exit-code semantics.
- A JSON example with the offset-direction convention.
- Definitions, ranges, comparison scopes, and interpretation guidance for MAE, RMSE, PSNR, SSIM, changed-pixel ratio, and coverage.
- A description of `report.json`, the three output images, and annotation colors.
- The exact scale-aware movement-border suppression rule and the distinction between prioritization and equivalence.
- Explanation of perceptual tolerance and minimum-region filtering.
- Clear limitations: PNG only, same scale, bounded translation, conservative heuristics, and no UI semantics.
- Guidance that consumers should trust explicit movement findings and inspect generic `changed` regions when confidence is insufficient.

## 15. Implementation Sequence

Complete changes in atomic, internally consistent stages, validating after each stage is complete:

1. Scaffold the workspace, public core types/API, thin CLI, split error handling, report envelope, and basic PNG I/O.
2. Implement normalized color, geometry, masks, connected components, dependency-free metrics, and their unit tests in the core crate.
3. Implement pyramids and global translation estimation with synthetic tests.
4. Implement local proposals, bidirectional matching, confidence, movement merging, and the shifted-box acceptance scenario.
5. Implement residual classification for resized, added, removed, and changed regions.
6. Implement all three metric scopes using finalized alignment and movement maps.
7. Implement deterministic sorting, IDs, core serialization, the CLI JSON envelope, exit codes, and integration tests.
8. Implement all three core renderers and CLI artifact-safe writing.
9. Complete core API tests, edge-case tests, performance smoke coverage, README documentation, and final validation.

Run validation only when each interdependent stage is consistent. The final validation set is:

```text
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --release
```

Do not add dependencies in response to implementation difficulty without explicit human approval.

## 16. Acceptance Criteria

The first version is complete when:

- The approved core library API and CLI, JSON, exit-code, and artifact contracts are implemented and documented.
- Other Rust tools can compare in-memory images, inspect findings and metrics, and render annotations without depending on the CLI or filesystem behavior.
- The canonical synthetic case—a UI box shifted five pixels—produces one clear movement finding and corresponding annotations rather than a giant raw residual diff.
- Different canvas dimensions are accepted, aligned at the same content scale, and explicitly reported.
- Ambiguous matches fall back to `changed` instead of receiving misleading structural labels.
- Default tolerances suppress small perceptual noise while remaining configurable.
- All three images correlate findings through deterministic IDs and colors.
- JSON and the core API report raw, globally aligned, and structurally aligned MAE, RMSE, PSNR, SSIM, changed-pixel ratio, and coverage using the documented formulas.
- The complete validation command set passes without warnings.
- The implementation uses only the approved Rust dependencies and performs no network or external-service calls at runtime.
