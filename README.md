# Kineprism

`kineprism` compares UI screenshots and reports meaningful visual changes. Rather than
only highlighting changed pixels, it identifies high-confidence moved, resized, added, and removed
regions where possible.

It runs locally and deterministically, with no network connection or external service required.

## Install

Run the packaged CLI through npm:

```console
npx -y @zlepper/kineprism
```

Or install from this checkout:

```console
cargo install --path crates/kineprism-cli
```

## Usage

```console
kineprism expected.png actual.png --output-dir comparison-output
```

`expected.png` is the reference image and `actual.png` is the image being checked. Both inputs
must be PNG files.

### Options

```text
kineprism <EXPECTED> <ACTUAL> --output-dir <PATH>
    [--max-offset <PIXELS>]
    [--color-threshold <DELTA>]
    [--min-region-area <PIXELS>]
    [--region-x <PIXELS> --region-y <PIXELS>
     --region-width <PIXELS> --region-height <PIXELS>]
    [--force]
```

- `--output-dir` is required and is created when needed.
- `--max-offset` limits the translation search; the default is `128` pixels.
- `--color-threshold` controls tolerance for small color differences; the default is `2.3`.
- `--min-region-area` ignores smaller differences; the default is `16` pixels.
- `--region-x`, `--region-y`, `--region-width`, and `--region-height` compare only the specified
  rectangle. All four are required together.
- `--force` replaces the tool's existing output files. Other files in the output directory are
  preserved.

## MCP server

Run a local stdio [Model Context Protocol](https://modelcontextprotocol.io/) server with:

```console
kineprism mcp
```

An npm-based MCP configuration is:

```json
{
  "mcpServers": {
    "kineprism": {
      "command": "npx",
      "args": ["-y", "@zlepper/kineprism", "mcp"]
    }
  }
}
```

## Results

The command writes a JSON report to stdout and to `report.json` in the output directory. It also
creates annotated copies of each input (`expected.png` and `actual.png`) plus `diff.png`, a visual
summary of the findings.

Each finding has a stable ID, type, bounds, and human-readable message. Moved regions also include
their offset and confidence. When the tool cannot make a reliable structural classification, it
reports the affected area as `changed`.

| Exit code | Meaning |
| --- | --- |
| `0` | The images have no meaningful differences. |
| `1` | One or more meaningful differences were found. |
| `2` | The comparison could not be completed. |

Exit code `1` is an expected comparison result and can be used in visual-regression workflows.

## Examples

- [Deterministic UI reflow](examples/deterministic-ui/README.md): three dashboard panels move by
  known offsets.
- [Bounded comparison](examples/bounded-region/README.md): the same change, limited to one region
  of the image.

## Limitations

- PNG inputs only; JPEG, WebP, GIF, SVG, and animation are unsupported.
- Images must use the same scale. Rotation, perspective, and arbitrary transforms are unsupported.
- Movement detection is limited by `--max-offset`.
- Repetitive, textureless, or extensively regenerated images may produce generic `changed`,
  `added`, or `removed` findings instead of a confident movement.
