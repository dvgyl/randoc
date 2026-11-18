#!/usr/bin/env python3
"""Licence scan against a hashed index of copyleft reference text.

The build sandbox cannot read the Pandoc source folders (clean-room rule).
So the owner side builds a one-way index once, outside the sandbox:

    licence_scan.py build-index            (reads the copyleft folders)

and the build scans its own work against that index:

    licence_scan.py scan [--base REV]      each commit of REV..HEAD that is not on origin/main
    licence_scan.py scan --all             the whole tree at HEAD

A shingle is a run of 8 words (lower case, split on white space). The index
holds only 8-byte BLAKE2b hashes of shingles, one sorted file for each part,
so it does not hold or reveal the reference text.

The commit scan counts, for each file that a commit adds or changes, the
shingles that the file has and none of the commit's parents had. So text that
one commit adds and a later commit removes is still found, as the push
publishes both commits. The Git guard runs the same scan on each outgoing
commit before a push (its protected copy of this file).

`scan --all` counts all shingles of each file at HEAD. A hit whose shingles
the same file on origin/main has already is `inherited`: it is listed, but it
does not fail the scan (main's history is audited apart, with the owner).

A row of tools/autobuild/licence-allow.tsv (path, part, sha256, reason)
allows the hits of one exact file content: the SHA-256 of the file. A change
of the file makes the row stale, so the changed file needs a new review.

Exit codes of `scan`: 0 no hit to fix; 1 a hit to fix; 2 a tool error.
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
# The suffixes of the CSL data files (styles, locales, their JSON forms).
DATA_SUFFIXES = {".csl", ".xml", ".json"}
MIN_HITS = 3
MAX_BYTES = 5 * 1024 * 1024
SNIFF_BYTES = 8192
ENV = Path("/Volumes/Extreme Pro/Randoc/env")
INDEX = ENV / "licence-index"
ALLOW_FILE = "tools/autobuild/licence-allow.tsv"


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


def decode(raw: bytes | None) -> str | None:
    if raw is None or len(raw) > MAX_BYTES or b"\0" in raw[:SNIFF_BYTES]:
        return None
    return raw.decode("utf-8", errors="replace")


def shingle_hashes(text: str) -> set[int]:
    words = text.lower().split()
    out = set()
    for i in range(len(words) - N + 1):
        digest = hashlib.blake2b("\x1f".join(words[i : i + N]).encode(), digest_size=8).digest()
        out.add(int.from_bytes(digest, "big"))
    return out


def write_index(folder: Path, name: str, hashes: set[int]) -> int:
    folder.mkdir(parents=True, exist_ok=True)
    values = array("Q", sorted(hashes))
    tmp = folder / f"{name}.bin.new"
    with open(tmp, "wb") as handle:
        values.tofile(handle)
    tmp.replace(folder / f"{name}.bin")
    return len(values)


def build_index(src: Path, folder: Path) -> None:
    for name, roots, skip in parts(src):
        hashes: set[int] = set()
        count = 0
        for path in files_under(roots, skip):
            # The CSL folders also hold tool scripts; only the CSL data counts.
            if name == "csl-data" and path.suffix not in DATA_SUFFIXES:
                continue
            text = decode(path.read_bytes())
            if text is not None:
                hashes |= shingle_hashes(text)
                count += 1
        print(f"{name}: {count} files, {write_index(folder, name, hashes)} shingles")


def load_index(folder: Path = INDEX) -> dict[str, array]:
    out = {}
    for path in sorted(folder.glob("*.bin")):
        if path.name.startswith("._"):
            continue
        values = array("Q")
        with open(path, "rb") as handle:
            values.frombytes(handle.read())
        out[path.stem] = values
    if not out:
        raise SystemExit(f"licence_scan: no index in {folder}; the owner side must run build-index")
    return out


def contains(values: array, item: int) -> bool:
    i = bisect.bisect_left(values, item)
    return i < len(values) and values[i] == item


def git(*args: str) -> str:
    return subprocess.run(["git", *args], capture_output=True, text=True, check=True).stdout


def show(commit: str, path: str) -> str:
    return subprocess.run(["git", "show", f"{commit}:{path}"], capture_output=True, text=True).stdout


def rev(name: str) -> str | None:
    out = subprocess.run(["git", "rev-parse", "-q", "--verify", f"{name}^{{commit}}"],
                         capture_output=True, text=True).stdout.strip()
    return out or None


class Blobs:
    """File contents at a commit, through one `git cat-file --batch` process."""

    def __init__(self) -> None:
        self.process = subprocess.Popen(["git", "cat-file", "--batch"], stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE)

    def get(self, commit: str | None, path: str) -> bytes | None:
        if commit is None or "\n" in path:
            return None
        self.process.stdin.write(f"{commit}:{path}\n".encode())
        self.process.stdin.flush()
        header = self.process.stdout.readline().split()
        if len(header) != 3 or header[1] != b"blob":
            return None
        data = self.process.stdout.read(int(header[2]) + 1)
        return data[:-1]

    def close(self) -> None:
        self.process.stdin.close()
        self.process.wait()


def allowed(text: str) -> set[tuple[str, str, str]]:
    """The (path, part, sha256) triples of the allow list rows that give a reason."""
    rows = [l.split("\t") for l in text.splitlines() if l.strip() and not l.startswith("#")]
    return {(r[0], r[1], r[2]) for r in rows[1:] if len(r) >= 4 and r[3].strip()}


def part_hits(hashes: set[int], index: dict[str, array]) -> dict[str, set[int]]:
    return {part: {h for h in hashes if contains(values, h)} for part, values in index.items()}


def commit_hits(commits: list[str], index: dict[str, array], allow: set[tuple[str, str, str]],
                min_hits: int = MIN_HITS) -> list[str]:
    """One line for each file and part where a commit adds at least `min_hits` indexed shingles."""
    blobs = Blobs()
    found = []
    try:
        for sha in commits:
            parents = git("rev-list", "--parents", "-n", "1", sha).split()[1:]
            names: set[str] = set()
            for parent in parents or [None]:
                trees = [parent, sha] if parent else ["--root", sha]
                out = git("diff-tree", "-r", "-z", "--no-commit-id", "--no-renames", "--name-only",
                          "--diff-filter=AM", *trees)
                names |= {n for n in out.split("\0") if n}
            for name in sorted(names):
                if Path(name).name.startswith("._"):
                    continue
                raw = blobs.get(sha, name)
                text = decode(raw)
                if text is None:
                    continue
                mine = shingle_hashes(text)
                for parent in parents:
                    mine -= shingle_hashes(decode(blobs.get(parent, name)) or "")
                digest = hashlib.sha256(raw).hexdigest()
                for part, hits in part_hits(mine, index).items():
                    if len(hits) >= min_hits and (name, part, digest) not in allow:
                        found.append(f"{sha[:12]}\t{name}\t{part}\t{len(hits)} shingles\tsha256={digest}")
    finally:
        blobs.close()
    return found


def scan_commits(base: str | None, index: dict[str, array], min_hits: int) -> int:
    main = rev("refs/remotes/origin/main")
    if main is None:
        print("licence_scan: refs/remotes/origin/main is missing; run `git fetch origin main`", file=sys.stderr)
        return 2
    base = base or git("merge-base", "HEAD", main).strip()
    commits = git("rev-list", "--reverse", "HEAD", "--not", base, main).split()
    hits = commit_hits(commits, index, allowed(show("HEAD", ALLOW_FILE)),
                       min_hits)
    for hit in hits:
        print(f"HIT\t{hit}")
    print(f"licence_scan: {len(commits)} commits after {base[:12]}, {len(hits)} file and part hits to fix")
    return 1 if hits else 0


def scan_tree(index: dict[str, array], min_hits: int) -> int:
    main = rev("refs/remotes/origin/main")
    allow = allowed(show("HEAD", ALLOW_FILE))
    names = [n for n in git("ls-tree", "-r", "-z", "--name-only", "HEAD").split("\0") if n]
    blobs = Blobs()
    bad = inherited = 0
    try:
        for name in names:
            if Path(name).name.startswith("._"):
                continue
            raw = blobs.get("HEAD", name)
            text = decode(raw)
            if text is None:
                continue
            found = {part: hits for part, hits in part_hits(shingle_hashes(text), index).items()
                     if len(hits) >= min_hits}
            if not found:
                continue
            on_main = shingle_hashes(decode(blobs.get(main, name)) or "")
            digest = hashlib.sha256(raw).hexdigest()
            for part, hits in found.items():
                if (name, part, digest) in allow:
                    state = "allowed"
                elif len(hits - on_main) < min_hits:
                    state, inherited = "inherited", inherited + 1
                else:
                    state, bad = "HIT", bad + 1
                print(f"{state}\t{name}\t{part}\t{len(hits)} shingles\tsha256={digest}")
    finally:
        blobs.close()
    print(f"licence_scan: {len(names)} files at HEAD, {bad} file and part hits to fix, {inherited} inherited from main")
    return 1 if bad else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build-index")
    b.add_argument("--src", default=str(ENV / "pandoc-src"))
    b.add_argument("--index", default=str(INDEX))
    s = sub.add_parser("scan")
    s.add_argument("--base", help="scan the commits after REV (default: the merge base with origin/main)")
    s.add_argument("--all", action="store_true", help="scan the whole tree at HEAD")
    s.add_argument("--min-hits", type=int, default=MIN_HITS)
    s.add_argument("--index", default=str(INDEX))
    args = parser.parse_args()
    try:
        if args.cmd == "build-index":
            build_index(Path(args.src), Path(args.index))
            return 0
        index = load_index(Path(args.index))
        if args.all:
            return scan_tree(index, args.min_hits)
        return scan_commits(args.base, index, args.min_hits)
    except subprocess.CalledProcessError as error:
        print(f"licence_scan: {error}: {error.stderr}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
