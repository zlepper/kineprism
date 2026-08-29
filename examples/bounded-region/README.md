# Bounded left-half comparison

This example reuses the deterministic UI reflow fixture while restricting comparison to the left
half of the 1448×1086 screenshots. The mask is inset five pixels from every edge of that half:

```text
x=5, y=5, width=714, height=1076
```

The selected rectangle therefore covers `[5, 719) × [5, 1081)` in full-screen coordinates. The
tool analyzes only those pixels, but the JSON findings and all three artifacts retain the original
1448×1086 coordinate system and dimensions.

Run the example from the workspace root:

```console
bash examples/bounded-region/run.sh
```

The equivalent direct CLI invocation is:

```console
cargo run --release -p better-image-diff -- \
  examples/deterministic-ui/expected.png \
  examples/deterministic-ui/actual.png \
  --output-dir examples/bounded-region/output \
  --region-x 5 \
  --region-y 5 \
  --region-width 714 \
  --region-height 1076 \
  --force
```

Exit code `1` is expected. The selected area contains the left-hand portions of two moved panels:

- the visible portion of the New Customers KPI moves 62 pixels down;
- the visible portion of the Revenue Over Time panel moves 43 pixels down.

The Recent Activity panel on the right is outside the mask and does not appear in the report. The
three full-size output images show the two-pixel dashed cyan mask boundary; `diff.png` is white
outside it. `report.json` preserves the global finding coordinates and records the selected region
under `settings.region`.

## Output

The generated artifacts are committed so the example can be inspected without running the CLI.
See the machine-readable [`report.json`](output/report.json) for exact metrics and findings.

### Annotated expected image

![Bounded comparison annotated expected image](output/expected.png)

### Annotated actual image

![Bounded comparison annotated actual image](output/actual.png)

### Structural diff

![Bounded comparison structural diff](output/diff.png)
