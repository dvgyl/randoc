#!/bin/sh
# Compare default_template with `pandoc -D` for each output format of the inventory.
# Run it from any directory after `. env/env.sh`. The oracle needs RANDOC_ORACLE_PANDOC.
set -u
root=$(cd "$(dirname "$0")/../../.." && pwd)
cd "$root" || exit 2
tmp=$(mktemp -d) || exit 2
trap 'rm -rf "$tmp"' EXIT
cargo build -q -p randoc-data --example default_template || exit 2
formats=$(python3 -c 'import json,sys; print("\n".join(json.load(open("spec/inventory/output-formats.json"))))
' ; printf '%s\n' markdown+smart HTML nosuchformat)
fail=0
count=0
for f in $formats; do
  count=$((count + 1))
  "$RANDOC_ORACLE_PANDOC" -D "$f" >"$tmp/want" 2>/dev/null
  want=$?
  cargo run -q -p randoc-data --example default_template -- "$f" >"$tmp/got" 2>/dev/null
  got=$?
  if [ "$want" -eq 0 ] && [ "$got" -eq 0 ] && cmp -s "$tmp/want" "$tmp/got"; then
    echo "same  $f"
  elif [ "$want" -ne 0 ] && [ "$got" -ne 0 ]; then
    echo "none  $f (oracle exit $want)"
  else
    echo "DIFF  $f (oracle exit $want, randoc exit $got)"
    fail=1
  fi
done
echo "$count formats checked, status $fail"
exit "$fail"
