#!/bin/sh
# Parses the CSL styles of the test suite of citeproc 0.14.1.1 and its 61
# locales with Randoc and with the oracle, and prints the counts.
#
# The archive goes to $TMPDIR/citeproc/ (outside the repository), with a
# SHA-256 check. No file of the archive goes into the repository; the tool
# writes only counts and case names to tests/csl-corpus-status.txt.
#
# Usage: crates/randoc-citeproc/tools/csl-corpus.sh   (from the worktree root)
# Environment: RANDOC_ORACLE_PANDOC (from env.sh), TMPDIR.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
crate="$(dirname "$here")"
url="https://hackage.haskell.org/package/citeproc-0.14.1.1/citeproc-0.14.1.1.tar.gz"
sha="e5a353f975c32169979b4c9e158d3edb9e57a5f653fab8ff0505a2067e2b1b4b"
work="${TMPDIR:-/tmp}/citeproc"
archive="$work/citeproc-0.14.1.1.tar.gz"
mkdir -p "$work"
if [ ! -f "$archive" ]; then
  curl -sSfL -o "$archive" "$url"
fi
echo "$sha  $archive" | shasum -a 256 -c - >/dev/null

python3 - "$archive" "$work" <<'PY'
import os, re, sys, tarfile
archive, work = sys.argv[1], sys.argv[2]
styles = os.path.join(work, "styles")
locales = os.path.join(work, "locales")
os.makedirs(styles, exist_ok=True)
os.makedirs(locales, exist_ok=True)
style_list, locale_list = [], []
pattern = re.compile(r">>=+\s*CSL\s*=+>>[ \t]*\r?\n(.*?)\r?\n?<<=+\s*CSL\s*=+<<", re.S)
with tarfile.open(archive) as tar:
    for member in tar.getmembers():
        name = member.name.split("/", 1)[-1]
        if not member.isfile():
            continue
        if re.match(r"test/(csl|extra|overrides)/[^/]+\.txt$", name):
            text = tar.extractfile(member).read().decode("utf-8", "replace")
            found = pattern.search(text)
            if not found:
                print(f"no CSL section: {name}", file=sys.stderr)
                continue
            case = name[len("test/"):-len(".txt")].replace("/", "__")
            path = os.path.join(styles, case + ".csl")
            with open(path, "w", encoding="utf-8") as handle:
                handle.write(found.group(1))
            style_list.append(path)
        elif re.match(r"locales/[^/]+\.xml$", name):
            path = os.path.join(locales, os.path.basename(name))
            with open(path, "wb") as handle:
                handle.write(tar.extractfile(member).read())
            locale_list.append(path)
for list_name, paths in (("styles.list", style_list), ("locales.list", locale_list)):
    with open(os.path.join(work, list_name), "w") as handle:
        handle.write("\n".join(sorted(paths)) + "\n")
PY

cargo build -q -p randoc-citeproc --example csl_parse
target_dir="${CARGO_TARGET_DIR:-target}"
"$target_dir/debug/examples/csl_parse" style "$work/styles.list" > "$work/randoc-styles.tsv"
"$target_dir/debug/examples/csl_parse" locale "$work/locales.list" > "$work/randoc-locales.tsv"
perl -e 'alarm 600; exec @ARGV' -- "$RANDOC_ORACLE_PANDOC" lua "$here/oracle-parse.lua" "$work/styles.list" > "$work/oracle-styles.tsv"

python3 - "$work" "$crate/tests/csl-corpus-status.txt" <<'PY'
import os, sys
work, status = sys.argv[1], sys.argv[2]
def load(name):
    out = {}
    with open(os.path.join(work, name), encoding="utf-8") as handle:
        for line in handle:
            path, _, result = line.rstrip("\n").partition("\t")
            out[os.path.basename(path).rsplit(".", 1)[0]] = result
    return out
randoc, oracle, locales = load("randoc-styles.tsv"), load("oracle-styles.tsv"), load("randoc-locales.tsv")
accepted = [c for c in oracle if oracle[c] == "ok"]
rejected = [c for c in oracle if oracle[c] != "ok"]
locale_fail = sorted(c for c in locales if locales[c] != "ok")
accepted_fail = sorted(c for c in accepted if randoc.get(c) != "ok")
rejected_equal = [c for c in rejected if randoc.get(c) == oracle[c]]
rejected_diff = sorted(c for c in rejected if randoc.get(c) != oracle[c])
lines = [
    "# Counts of tools/csl-corpus.sh (citeproc 0.14.1.1 test suite and locales). No case text.",
    f"locales {len(locales)} parse {len(locales) - len(locale_fail)}",
    f"styles {len(oracle)} oracle-accepted {len(accepted)} oracle-rejected {len(rejected)}",
    f"accepted-and-parsed {len(accepted) - len(accepted_fail)}",
    f"rejected-with-equal-text {len(rejected_equal)}",
]
lines += [f"locale-fails {c}" for c in locale_fail]
lines += [f"accepted-but-randoc-fails {c}" for c in accepted_fail]
lines += [f"rejected-with-other-text {c}" for c in rejected_diff]
with open(status, "w") as handle:
    handle.write("\n".join(lines) + "\n")
print("\n".join(lines[1:5 + 1]))
for c in accepted_fail + rejected_diff:
    print(f"{c}\n  oracle: {oracle[c][:300]}\n  randoc: {randoc.get(c, '')[:300]}")
PY
