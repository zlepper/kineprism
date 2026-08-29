#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
workspace_dir="$(cd -- "${script_dir}/../.." && pwd)"

cd "${workspace_dir}"

set +e
cargo run --release -p kineprism -- \
    examples/deterministic-ui/expected.png \
    examples/deterministic-ui/actual.png \
    --output-dir examples/bounded-region/output \
    --region-x 5 \
    --region-y 5 \
    --region-width 714 \
    --region-height 1076 \
    --force
status=$?
set -e

if [[ ${status} -ne 1 ]]; then
    echo "Expected comparison exit code 1, got ${status}." >&2
    exit "${status}"
fi

echo "Generated examples/bounded-region/output"
