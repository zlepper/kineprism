#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source_image="${1:-${script_dir}/../realistic-ui/expected.png}"
expected_image="${script_dir}/expected.png"
actual_image="${script_dir}/actual.png"

if ! command -v magick >/dev/null 2>&1; then
    echo "ImageMagick 7 is required (the 'magick' command was not found)." >&2
    exit 1
fi

if [[ ! -f "${source_image}" ]]; then
    echo "Source image does not exist: ${source_image}" >&2
    exit 1
fi

temporary_dir="$(mktemp -d)"
trap 'rm -rf -- "${temporary_dir}"' EXIT

# Keep the reference image byte-for-byte identical to the source. In particular,
# neither fixture image is resized.
cp -- "${source_image}" "${expected_image}"

# The crop boxes include each panel's shadow and a small margin of background.
# Their content is copied without scaling, rotation, or filtering.
magick "${expected_image}" -crop 368x286+668+120 +repage "${temporary_dir}/middle-kpi.png"
magick "${expected_image}" -crop 720x607+287+426 +repage "${temporary_dir}/revenue-panel.png"
magick "${expected_image}" -crop 398x607+1020+426 +repage "${temporary_dir}/activity-panel.png"

# Clear the three source rectangles using the neutral dashboard canvas color.
# ImageMagick rectangles include both endpoints, hence width/height minus one.
magick "${expected_image}" \
    -fill 'rgb(250,251,252)' -stroke none \
    -draw 'rectangle 668,120 1035,405' \
    -draw 'rectangle 287,426 1006,1032' \
    -draw 'rectangle 1020,426 1417,1032' \
    "${temporary_dir}/cleared.png"

# Reproduce the layout reflow seen in realistic-ui/actual.png:
#   middle KPI:     (0, +62)
#   revenue panel:  (0, +43)
#   activity panel: (0, +43)
magick "${temporary_dir}/cleared.png" \
    "${temporary_dir}/middle-kpi.png" -geometry +668+182 -composite \
    "${temporary_dir}/revenue-panel.png" -geometry +287+469 -composite \
    "${temporary_dir}/activity-panel.png" -geometry +1020+469 -composite \
    -strip +set date:create +set date:modify -define png:exclude-chunks=date,time \
    "${actual_image}"

echo "Generated ${expected_image}"
echo "Generated ${actual_image}"
