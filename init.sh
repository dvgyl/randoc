#!/bin/sh
# Builds Randoc, runs the tests and runs the conformance harness.
# Also installs the commit-msg hook that removes AI attribution lines.
#
# Usage: ./init.sh [--quick]
#   --quick   install the hook and build only
set -eu
cd "$(dirname "$0")"

# Optional machine settings, for example a CARGO_HOME on a different drive.
if [ -n "${RANDOC_ENV_FILE:-}" ] && [ -f "$RANDOC_ENV_FILE" ]; then
  . "$RANDOC_ENV_FILE"
elif [ -f .local/env.sh ]; then
  . ./.local/env.sh
fi

hooks_dir="$(git rev-parse --git-common-dir)/hooks"
mkdir -p "$hooks_dir"
cp scripts/hooks/commit-msg "$hooks_dir/commit-msg"
chmod +x "$hooks_dir/commit-msg"
echo "init: installed the commit-msg hook in $hooks_dir"

cargo build --workspace --all-targets
if [ "${1:-}" = "--quick" ]; then
  exit 0
fi

cargo test --workspace
cargo run --quiet -p randoc-harness -- run --baseline spec/baseline.json
