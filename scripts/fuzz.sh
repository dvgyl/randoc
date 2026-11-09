#!/bin/sh
# Builds one fuzz target of a crate and runs it for a fixed time.
#
# Usage: scripts/fuzz.sh CRATE_DIR TARGET [SECONDS]
# Example: scripts/fuzz.sh crates/randoc-ast json_read 900
#
# The corpus stays in memory. The files in CRATE_DIR/fuzz/seeds/TARGET/ are
# the start inputs. A crash input is saved in CRATE_DIR/fuzz/artifacts/TARGET/.
# The script needs the nightly toolchain and cargo-fuzz.
set -eu
crate_dir="$1"
target="$2"
seconds="${3:-900}"
cd "$crate_dir"

cargo +nightly fuzz build "$target"
triple="$(rustc +nightly -vV | sed -n 's/^host: //p')"
binary="fuzz/target/$triple/release/$target"
mkdir -p "fuzz/artifacts/$target"

seeds=""
if [ -d "fuzz/seeds/$target" ]; then
  seeds="$(find "fuzz/seeds/$target" -type f ! -name '._*' | sort | paste -sd, -)"
fi

set -- "-max_total_time=$seconds" "-artifact_prefix=fuzz/artifacts/$target/" \
  -rss_limit_mb=4096 -timeout=25 -print_final_stats=1
if [ -n "$seeds" ]; then
  set -- "$@" "-seed_inputs=$seeds"
fi
exec "$binary" "$@"
