# Deterministic UI reflow fixture

This fixture preserves the useful layout change from `realistic-ui` while removing generative
editing from the comparison. `expected.png` is a byte-for-byte copy of
`../realistic-ui/expected.png`; `actual.png` is assembled deterministically with ImageMagick.
Neither image is resized.

The transformation models a vertical reflow:

| Element | Source crop | Offset |
| --- | --- | --- |
| New Customers KPI | `368x286+668+120` | `(0, +62)` |
| Revenue Over Time panel | `720x607+287+426` | `(0, +43)` |
| Recent Activity panel | `398x607+1020+426` | `(0, +43)` |

Each crop contains the complete panel, its shadow, and a small background margin. The script
clears the original rectangles to the neutral dashboard canvas color and composites the crops at
their new positions without scaling or filtering. Volatile PNG timestamp metadata is stripped so
repeated runs produce the same files byte-for-byte.

Regenerate the pair with ImageMagick 7:

```console
bash examples/deterministic-ui/generate.sh
```

Run the comparison:

```console
cargo run --release -p better-image-diff -- \
  examples/deterministic-ui/expected.png \
  examples/deterministic-ui/actual.png \
  --output-dir examples/deterministic-ui/output
```

Exit code `1` is expected because the images intentionally differ. The generated `report.json` and
annotated PNGs are written to the ignored `output/` directory. The expected primary result is three
movement findings; the tiny shadow fringes at their borders are counted in the report's structured
suppression note rather than emitted as independent red `changed` findings.
