#!/bin/sh
# Runs the mechanical checks of the merge gate: format, lints, documentation,
# tests and the conformance harness against the baseline.
# The gate also needs mutation tests, coverage, fuzzing, benchmarks and two
# reviews. See docs/CONTRIBUTING.md.
#
# Usage: scripts/gate.sh                 the checks
#        scripts/gate.sh --update-spec   the checks, and when they pass, write
#                                        spec/baseline.json and spec/capabilities.json
#
# The conformance suite runs one time. Without an option it runs inside
# `cargo test` (crates/randoc-cli/tests/conformance.rs). With --update-spec
# the harness program runs it, because only that program writes the
# baseline and the matrix.
set -eu
cd "$(dirname "$0")/.."

update_spec=false
case "${1:-}" in
  "") ;;
  --update-spec) update_spec=true ;;
  *) echo "usage: scripts/gate.sh [--update-spec]" >&2; exit 2 ;;
esac

step() {
  echo "gate: $1 ($(date -u +%H:%M:%S) UTC)"
}

step "format"
cargo fmt --all -- --check
step "lints"
cargo clippy --workspace --all-targets --quiet -- -D warnings
step "documentation"
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --quiet
target_dir="${CARGO_TARGET_DIR:-target}"
# The conformance test also checks the local baseline. The harness program does not.
if $update_spec && [ ! -e target/harness/local-baseline.json ]; then
  step "tests without the conformance suite"
  cargo test --workspace --quiet -- --skip each_unit_of_the_baseline_passes
  step "conformance harness, baseline and matrix"
  # `cargo test` built target/debug/randoc. The time limit is the limit of the conformance test.
  cargo build --quiet -p randoc-harness
  "$target_dir/debug/randoc-harness" run --timeout 20 --baseline spec/baseline.json \
    --update-baseline --matrix spec/capabilities.json
else
  step "tests and the conformance suite"
  cargo test --workspace --quiet
  if $update_spec; then
    step "conformance harness, baseline and matrix"
    cargo build --quiet -p randoc-harness
    "$target_dir/debug/randoc-harness" run --timeout 20 --baseline spec/baseline.json \
      --update-baseline --matrix spec/capabilities.json
  fi
fi
step "all mechanical checks passed"
