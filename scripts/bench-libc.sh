#!/usr/bin/env bash
set -euo pipefail

readonly glibc_target="x86_64-unknown-linux-gnu"
readonly musl_target="x86_64-unknown-linux-musl"

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
workspace_dir="$(cd -- "${script_dir}/.." && pwd)"
criterion_dir="${workspace_dir}/target/criterion-libc"

if [[ "$(uname -s)" != "Linux" || "$(uname -m)" != "x86_64" ]]; then
    echo "The libc comparison currently supports only x86-64 Linux hosts." >&2
    exit 2
fi

if ! command -v nix-shell >/dev/null 2>&1; then
    echo "This temporary libc comparison helper requires nix-shell." >&2
    exit 2
fi

cd "${workspace_dir}"
mkdir -p "${criterion_dir}"

glibc_command=(
    env
    "CRITERION_HOME=${criterion_dir}"
    cargo bench
    -p better-image-diff-core
    --bench comparison
    --target "${glibc_target}"
    --
    --save-baseline glibc
    "$@"
)
printf -v glibc_command_quoted '%q ' "${glibc_command[@]}"

musl_command=(
    env
    "CRITERION_HOME=${criterion_dir}"
    CC_x86_64_unknown_linux_musl=musl-gcc
    cargo bench
    -p better-image-diff-core
    --bench comparison
    --target "${musl_target}"
    --
    --baseline glibc
    "$@"
)
printf -v musl_command_quoted '%q ' "${musl_command[@]}"

echo "Rayon threads: ${RAYON_NUM_THREADS:-automatic}"
echo "Running glibc benchmark baseline (${glibc_target})"
nix-shell -p gcc --run "${glibc_command_quoted}"

echo "Running musl benchmark comparison (${musl_target})"
nix-shell -p gcc musl --run "${musl_command_quoted}"

echo "Criterion comparison report: ${criterion_dir}/report/index.html"
