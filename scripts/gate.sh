#!/bin/sh
# Runs the mechanical checks of the merge gate: format, lints, documentation,
# tests and the conformance harness against the baseline.
# The gate also needs mutation tests, coverage, fuzzing, benchmarks and two
# reviews. See docs/CONTRIBUTING.md.
set -eu
cd "$(dirname "$0")/.."

echo "gate: format"
cargo fmt --all -- --check
echo "gate: lints"
cargo clippy --workspace --all-targets --quiet -- -D warnings
echo "gate: documentation"
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --quiet
echo "gate: tests"
cargo test --workspace --quiet
echo "gate: conformance harness"
cargo build --quiet -p randoc-cli -p randoc-harness
target_dir="${CARGO_TARGET_DIR:-target}"
"$target_dir/debug/randoc-harness" run --baseline spec/baseline.json
echo "gate: all mechanical checks passed"
