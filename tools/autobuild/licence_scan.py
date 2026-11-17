#!/usr/bin/env python3
"""Licence scan against a hashed index of copyleft reference text.

The build sandbox cannot read the Pandoc source folders (clean-room rule).
So the owner side builds a one-way index once, outside the sandbox:

    licence_scan.py build-index            (reads the copyleft folders)

and the build scans its own changes against that index:

    licence_scan.py scan [--base REV] [--min-hits N]

A shingle is a run of 8 words (lower case, split on white space). The index
holds only 8-byte BLAKE2b hashes of shingles, one sorted file for each part,
so it does not hold or reveal the reference text.

Exit codes of `scan`: 0 no file at or above the threshold (or each such file
is listed in tools/autobuild/licence-allow.tsv); 1 a hit to fix; 2 a tool error.
"""

from __future__ import annotations

import argparse
import bisect
import hashlib
import os
import subprocess
import sys
from array import array
from pathlib import Path

N = 8
MAX_BYTES = 5 * 1024 * 1024
ENV = Path("/Volumes/Extreme Pro/Randoc/env")
INDEX = ENV / "licence-index"
ALLOW = Path(__file__).resolve().parent / "licence-allow.tsv"


def parts(src: Path) -> list[tuple[str, list[Path], set[Path]]]:
    skip = {src / "data" / "templates", src / "data" / "dzslides"}
    out = [
        ("pandoc-manual", [src / "MANUAL.txt"], set()),
        ("pandoc-tests", [src / "test", src / "pandoc-lua-engine" / "test"], set()),
        (
            "pandoc-source",
            [src / "src", src / "doc", src / "pandoc-lua-engine" / "src", src / "pandoc-cli",
             src / "pandoc-server", src / "citeproc", src / "xml-light"],
            set(),
        ),
        ("pandoc-data", [src / "data"], skip),
        ("commonmark-spec", [ENV / "commonmark-spec"], set()),
        (
            "csl-data",
            [ENV / "research" / "csl-locales", ENV / "research" / "citeproc" / "citeproc-0.14.1.1" / "locales",
             ENV / "tmp" / "citeproc" / "locales", ENV / "tmp" / "citeproc" / "styles"],
            set(),
        ),
    ]
    marshal = [
        Path(root) / f
        for root, _dirs, files in os.walk(ENV / "research")
        if "pandoc-lua-marshal" in root
        for f in files
        if f in ("Walk.hs", "SpliceList.hs", "Topdown.hs")
    ]
    if marshal:
        out.append(("pandoc-lua-marshal-gpl", marshal, set()))
    return out


def files_under(roots: list[Path], skip: set[Path]):
    for root in roots:
        if root.is_file():
            yield root
            continue
        for base, dirs, names in os.walk(root):
            dirs[:] = sorted(d for d in dirs if Path(base, d) not in skip and d != ".git")
            for name in sorted(names):
                if not name.startswith("._"):
                    yield Path(base, name)


def decode(raw: bytes) -> str | None:
    if len(raw) > MAX_BYTES or b"\0" in raw[:8192]:
        return None
    return raw.decode("utf-8", errors="replace")


def shingle_hashes(text: str) -> set[int]:
    words = text.lower().split()
    out = set()
    for i in range(len(words) - N + 1):
        digest = hashlib.blake2b("\x1f".join(words[i : i + N]).encode(), digest_size=8).digest()
        out.add(int.from_bytes(digest, "big"))
    return out


def build_index(src: Path) -> None:
    INDEX.mkdir(parents=True, exist_ok=True)
    for name, roots, skip in parts(src):
        hashes: set[int] = set()
        count = 0
        for path in files_under(roots, skip):
            text = decode(path.read_bytes())
            if text is not None:
                hashes |= shingle_hashes(text)
                count += 1
        values = array("Q", sorted(hashes))
        tmp = INDEX / f"{name}.bin.new"
        with open(tmp, "wb") as handle:
            values.tofile(handle)
        tmp.replace(INDEX / f"{name}.bin")
        print(f"{name}: {count} files, {len(values)} shingles")


def load_index() -> dict[str, array]:
    out = {}
    for path in sorted(INDEX.glob("*.bin")):
        if path.name.startswith("._"):
            continue
        values = array("Q")
        with open(path, "rb") as handle:
            values.frombytes(handle.read())
        out[path.stem] = values
    if not out:
        raise SystemExit(f"licence_scan: no index in {INDEX}; the owner side must run build-index")
    return out


def contains(values: array, item: int) -> bool:
    i = bisect.bisect_left(values, item)
    return i < len(values) and values[i] == item


def git(*args: str) -> str:
    return subprocess.run(["git", *args], capture_output=True, text=True, check=True).stdout


def allowed() -> set[tuple[str, str]]:
    if not ALLOW.exists():
        return set()
    rows = [l.split("\t") for l in ALLOW.read_text().splitlines() if l.strip() and not l.startswith("#")]
    return {(r[0], r[1]) for r in rows[1:] if len(r) >= 3 and r[2].strip()}


def scan(base: str | None, min_hits: int) -> int:
    index = load_index()
    if base is None:
        base = git("merge-base", "HEAD", "refs/remotes/origin/main").strip()
    names = [n for n in git("diff", "--name-only", "--diff-filter=AMR", f"{base}...HEAD").splitlines() if n]
    ok_pairs = allowed()
    bad = 0
    for name in names:
        if Path(name).name.startswith("._"):
            continue
        raw = subprocess.run(["git", "show", f"HEAD:{name}"], capture_output=True, check=False).stdout
        text = decode(raw)
        if text is None:
            continue
        old_raw = subprocess.run(["git", "show", f"{base}:{name}"], capture_output=True, check=False).stdout
        old_text = decode(old_raw) or ""
        # Only shingles that the branch adds; main's text was judged before.
        mine = shingle_hashes(text) - shingle_hashes(old_text)
        for part, values in index.items():
            hits = sum(1 for h in mine if contains(values, h))
            if hits >= min_hits:
                state = "allowed" if (name, part) in ok_pairs else "HIT"
                print(f"{state}\t{name}\t{part}\t{hits} shingles")
                if state == "HIT":
                    bad += 1
    print(f"licence_scan: {len(names)} changed files since {base[:12]}, {bad} file and part pairs to fix")
    return 1 if bad else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build-index")
    b.add_argument("--src", default=str(ENV / "pandoc-src"))
    s = sub.add_parser("scan")
    s.add_argument("--base")
    s.add_argument("--min-hits", type=int, default=3)
    args = parser.parse_args()
    try:
        if args.cmd == "build-index":
            build_index(Path(args.src))
            return 0
        return scan(args.base, args.min_hits)
    except subprocess.CalledProcessError as error:
        print(f"licence_scan: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
