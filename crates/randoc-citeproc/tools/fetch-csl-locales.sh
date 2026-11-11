#!/bin/sh
# Downloads CSL locale files to $RANDOC_ENV/research/csl-locales/ (outside the
# repository), as DIR/csl/locales/locales-TAG.xml, the layout that the loader
# reads. The files are CC BY-SA 3.0 and must not go into the repository.
#
# Usage: fetch-csl-locales.sh [repository|citeproc]
#   repository (default): the CSL locales repository at a pinned commit.
#   citeproc: the 60 locales of the Hackage archive of citeproc 0.14.1.1, the
#             files that the oracle builds in (the repository files differ).
# Then: RANDOC_CSL_DATA=$RANDOC_ENV/research/csl-locales cargo test -p randoc-citeproc -- --ignored
set -eu
source="${1:-repository}"
out="${RANDOC_ENV:?source env.sh first}/research/csl-locales"
work="${TMPDIR:-/tmp}/citeproc"
mkdir -p "$work" "$out/csl/locales"
case "$source" in
  repository)
    commit="8bef426f89f38c0789223c50e53ec2197bc50bb3"
    sha="8064ce8372d6deb7cde1f512c9a670e51d74be81248eef8fa7cf4c18c9e39dbe"
    archive="$work/locales-$commit.tar.gz"
    [ -f "$archive" ] || curl -sSfL -o "$archive" \
      "https://codeload.github.com/citation-style-language/locales/tar.gz/$commit"
    echo "$sha  $archive" | shasum -a 256 -c - >/dev/null
    tar xzf "$archive" -C "$out/csl/locales" --strip-components=1 "locales-$commit/" 
    ;;
  citeproc)
    sha="e5a353f975c32169979b4c9e158d3edb9e57a5f653fab8ff0505a2067e2b1b4b"
    archive="$work/citeproc-0.14.1.1.tar.gz"
    [ -f "$archive" ] || curl -sSfL -o "$archive" \
      "https://hackage.haskell.org/package/citeproc-0.14.1.1/citeproc-0.14.1.1.tar.gz"
    echo "$sha  $archive" | shasum -a 256 -c - >/dev/null
    python3 - "$archive" "$out/csl/locales" <<'PY'
import os, sys, tarfile
archive, out = sys.argv[1], sys.argv[2]
with tarfile.open(archive) as tar:
    for member in tar.getmembers():
        parts = member.name.split("/")
        if member.isfile() and len(parts) == 3 and parts[1] == "locales" and parts[2].endswith(".xml"):
            with open(os.path.join(out, "locales-" + parts[2]), "wb") as handle:
                handle.write(tar.extractfile(member).read())
PY
    ;;
  *) echo "unknown source: $source" >&2; exit 2 ;;
esac
ls "$out/csl/locales" | grep -c '^locales-.*\.xml$'
