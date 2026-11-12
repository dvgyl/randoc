#!/usr/bin/env python3
"""Records the oracle results of the replay cases of randoc-lua.

Usage (from the root of the worktree, after `. env.sh`):

    python3 -I crates/randoc-lua/tools/record-replay.py [SET]

SET is `cases` (the default), `walk`, `modules` or `system`. The script reads
`crates/randoc-lua/tests/replay/SET.txt`. For each filter
and each of its documents it runs

    pandoc -f json -t json --lua-filter FILTER.lua DOCUMENT.json

with Pandoc 3.13 (`$RANDOC_ORACLE_PANDOC`) in a temporary folder, and it
writes the exit code and the output to
`crates/randoc-lua/tests/replay/expected.txt` (for `cases`) or
`SET-expected.txt`. The output is the standard
output for the exit code 0 and the standard error for each other code.

The format of `cases.txt`: a line `@@@ doc NAME` starts a document in the
JSON form, and a line `@@@ filter NAME DOC DOC ...` starts a Lua filter
that runs on the named documents. The format of `expected.txt`: a line
`@@@ FILTER DOC EXIT` starts the output of one run.

At most 2 oracle processes run at one time, each with a limit of 60 s.
"""

import concurrent.futures
import os
import shutil
import subprocess
import sys
import tempfile

HEADER = "@@@ "
HERE = os.path.dirname(os.path.abspath(__file__))
REPLAY = os.path.join(HERE, "..", "tests", "replay")


def read_cases(path):
    docs, filters = {}, []
    current = None
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            if line.startswith(HEADER):
                words = line[len(HEADER):].split()
                if words[0] == "doc" and len(words) == 2:
                    current = []
                    docs[words[1]] = current
                elif words[0] == "filter" and len(words) >= 4:
                    current = []
                    filters.append((words[1], words[2:], current))
                else:
                    sys.exit(f"bad header: {line.rstrip()}")
            elif current is None:
                if line.strip() and not line.startswith("#"):
                    sys.exit(f"text before the first header: {line.rstrip()}")
            else:
                current.append(line)
    return (
        {name: "".join(lines) for name, lines in docs.items()},
        [(name, names, "".join(lines)) for name, names, lines in filters],
    )


def run_one(pandoc, folder, filter_name, doc_name):
    command = [
        pandoc, "-f", "json", "-t", "json",
        "--lua-filter", filter_name + ".lua", doc_name + ".json",
    ]
    try:
        done = subprocess.run(command, cwd=folder, capture_output=True, timeout=60)
    except subprocess.TimeoutExpired:
        sys.exit(f"the oracle did not end in 60 s: {filter_name} {doc_name}")
    if done.returncode == 251 and b"Heap exhausted" in done.stderr:
        sys.exit(f"the oracle reached its heap limit: {filter_name} {doc_name}")
    output = done.stdout if done.returncode == 0 else done.stderr
    text = output.decode("utf-8")
    if not text.endswith("\n"):
        sys.exit(f"output with no final newline: {filter_name} {doc_name}")
    if any(line.startswith(HEADER) for line in text.split("\n")):
        sys.exit(f"output with a header line: {filter_name} {doc_name}")
    return f"{HEADER}{filter_name} {doc_name} {done.returncode}\n{text}"


def main():
    pandoc = os.environ.get("RANDOC_ORACLE_PANDOC")
    if not pandoc or "GHCRTS" not in os.environ:
        sys.exit("source env.sh first: RANDOC_ORACLE_PANDOC and GHCRTS must be set")
    name = sys.argv[1] if len(sys.argv) > 1 else "cases"
    if name not in ("cases", "walk", "modules", "system"):
        sys.exit(f"unknown set: {name}")
    docs, filters = read_cases(os.path.join(REPLAY, name + ".txt"))
    expected = "expected.txt" if name == "cases" else name + "-expected.txt"
    folder = tempfile.mkdtemp(prefix="lua-replay-")
    try:
        for doc_name, text in docs.items():
            with open(os.path.join(folder, doc_name + ".json"), "w", encoding="utf-8") as handle:
                handle.write(text)
        jobs = []
        for filter_name, doc_names, source in filters:
            with open(os.path.join(folder, filter_name + ".lua"), "w", encoding="utf-8") as handle:
                handle.write(source)
            for doc_name in doc_names:
                if doc_name not in docs:
                    sys.exit(f"filter {filter_name}: no document {doc_name}")
                jobs.append((filter_name, doc_name))
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            results = list(pool.map(lambda job: run_one(pandoc, folder, *job), jobs))
    finally:
        shutil.rmtree(folder, ignore_errors=True)
    with open(os.path.join(REPLAY, expected), "w", encoding="utf-8") as handle:
        handle.write("".join(results))
    failures = sum(1 for result in results if not result.split("\n", 1)[0].endswith(" 0"))
    print(f"{len(filters)} filters, {len(results)} runs, {failures} runs with an error exit")


if __name__ == "__main__":
    main()
