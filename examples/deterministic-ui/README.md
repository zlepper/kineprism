# Deterministic UI reflow

This example moves three dashboard panels without scaling: the New Customers card moves 62 pixels
down, and the Revenue Over Time and Recent Activity panels each move 43 pixels down.

```console
kineprism examples/deterministic-ui/expected.png \
  examples/deterministic-ui/actual.png \
  --output-dir examples/deterministic-ui/output --force
```

Exit code `1` is expected. The full findings are available in
[`report.json`](output/report.json).

## Annotated images

![Annotated expected image](output/expected.png)

![Annotated actual image](output/actual.png)

![Structural diff](output/diff.png)
