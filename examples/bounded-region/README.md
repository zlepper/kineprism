# Bounded comparison

This example compares only the left half of the deterministic UI reflow. The selected region is
`x=5`, `y=5`, `width=714`, and `height=1076`; the right-hand Recent Activity panel is ignored.

```console
better-image-diff examples/deterministic-ui/expected.png \
  examples/deterministic-ui/actual.png \
  --output-dir examples/bounded-region/output \
  --region-x 5 --region-y 5 --region-width 714 --region-height 1076 --force
```

Exit code `1` is expected. The full findings are available in
[`report.json`](output/report.json).

## Annotated images

![Annotated expected image](output/expected.png)

![Annotated actual image](output/actual.png)

![Structural diff](output/diff.png)
