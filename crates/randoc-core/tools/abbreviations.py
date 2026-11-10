#!/usr/bin/env python3
"""Finds the default abbreviations of Pandoc 3.13 with the oracle only.

Usage: abbreviations.py [--words] [--max-inner N]

With the `smart` extension, the Markdown reader joins a known abbreviation
and the next word with a non-breaking space: `q Mr. w` gives `Str "Mr.\\160w"`.
This script makes candidates mechanically, asks the oracle about each one and
keeps the candidates that the oracle joins. It uses no other knowledge.

Candidate sources, each in lower case and with a capital first letter:
  (a) each string of 1 to 4 ASCII letters, followed by a period;
  (b) each string of 2 to N ASCII letters (N is --max-inner, default 4) with
      one period inside and one period at the end, for example `x.y.`;
  (c) with --words: each word of 2 to 8 letters of /usr/share/dict/words, followed
      by a period. On macOS that file is `web2` of FreeBSD: Webster's Second
      International Dictionary (1934). Its README says that the copyright has
      lapsed.

The script writes:
  - crates/randoc-core/src/shared/abbreviation_table.rs: the kept entries;
  - crates/randoc-core/tests/abbreviations_oracle.json: a sample of 300 kept
    and 300 refused candidates with the result of the oracle, for the replay
    test. The sample comes from a random generator with a fixed seed.
RANDOC_ORACLE_PANDOC is the path of Pandoc 3.13.
"""
import itertools
import json
import os
import random
import string
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
TABLE = os.path.join(HERE, "..", "src", "shared", "abbreviation_table.rs")
SAMPLE = os.path.join(HERE, "..", "tests", "abbreviations_oracle.json")
PANDOC = os.environ["RANDOC_ORACLE_PANDOC"]
BATCH = 100000
TIME_LIMIT = 600


def both_cases(stem):
    return [stem.lower(), stem[:1].upper() + stem[1:].lower()]


def candidates(max_inner, words):
    out = set()
    letters = string.ascii_lowercase
    for length in range(1, 5):
        for chars in itertools.product(letters, repeat=length):
            out.update(c + "." for c in both_cases("".join(chars)))
    for length in range(2, max_inner + 1):
        for chars in itertools.product(letters, repeat=length):
            stem = "".join(chars)
            for cut in range(1, length):
                out.update(c + "." for c in both_cases(stem[:cut] + "." + stem[cut:]))
    if words:
        for line in open("/usr/share/dict/words", encoding="utf-8"):
            word = line.strip()
            if 2 <= len(word) <= 8 and word.isascii() and word.isalpha():
                out.update(c + "." for c in both_cases(word))
    return sorted(out)


def joined(batch):
    source = "".join("q %s w\n\n" % candidate for candidate in batch)
    result = subprocess.run(
        [PANDOC, "-f", "markdown", "-t", "json"],
        input=source.encode(),
        capture_output=True,
        check=True,
        timeout=TIME_LIMIT,
    )
    blocks = json.loads(result.stdout)["blocks"]
    if len(blocks) != len(batch):
        raise SystemExit("the oracle gave %d blocks for %d candidates" % (len(blocks), len(batch)))
    out = []
    for candidate, block in zip(batch, blocks):
        texts = [inline.get("c") for inline in block["c"] if inline["t"] == "Str"]
        out.append(candidate + " w" in texts)
    return out


def main():
    words = "--words" in sys.argv
    max_inner = 4
    if "--max-inner" in sys.argv:
        max_inner = int(sys.argv[sys.argv.index("--max-inner") + 1])
    start = time.time()
    pool = candidates(max_inner, words)
    kept, refused, runs = [], [], 0
    for index in range(0, len(pool), BATCH):
        batch = pool[index : index + BATCH]
        for candidate, ok in zip(batch, joined(batch)):
            (kept if ok else refused).append(candidate)
        runs += 1
        print("run %d: %d candidates, %d kept, %.0f s" % (runs, index + len(batch), len(kept), time.time() - start), flush=True)
    with open(TABLE, "w", encoding="utf-8") as out:
        out.write("//! The default abbreviations of the `smart` extension.\n")
        out.write("//! `tools/abbreviations.py` writes this file from the results of the oracle. Do not edit it.\n\n")
        out.write("/// The kept candidates, sorted by bytes.\n")
        out.write("pub(super) static ABBREVIATIONS: [&str; %d] = [\n" % len(kept))
        for entry in sorted(kept, key=str.encode):
            out.write('    "%s",\n' % entry)
        out.write("];\n")
    rng = random.Random(20261110)
    sample = [[c, True] for c in rng.sample(kept, min(300, len(kept)))]
    sample += [[c, False] for c in rng.sample(refused, 300)]
    with open(SAMPLE, "w", encoding="utf-8") as out:
        json.dump(sample, out, separators=(",", ":"))
        out.write("\n")
    print("candidates %d, oracle runs %d, kept %d, %.0f s" % (len(pool), runs, len(kept), time.time() - start))


if __name__ == "__main__":
    main()
