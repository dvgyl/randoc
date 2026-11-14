#!/usr/bin/env python3
"""Makes the replay data of the session 5 oracle matrices.

Usage: matrix_replay.py OUT.json

For each case of the matrices in `differ.S5_MATRICES`, the oracle reads the
input once (`pandoc lua oracle.lua`, one process for all cases), and the
program `target/debug/randoc` reads it too. Each record is {"id", "from",
"input", "doc", "known"}: "doc" is the oracle tree, and "known" is true when
the program differs from it when the file is made. The test
`matrix_replay_matches_the_oracle` of the crate reads OUT.json from
crates/randoc-markdown/tests/matrix_replay.json.
"""
import json
import os
import sys
import tempfile

import differ


def main():
    cases = [case for make in differ.S5_MATRICES.values() for case in make()]
    with tempfile.TemporaryDirectory() as tmp:
        cases_path = os.path.join(tmp, "cases.json")
        with open(cases_path, "w", encoding="utf-8") as f:
            json.dump(cases, f)
        oracle = differ.run_oracle(cases_path, os.path.join(tmp, "oracle.json"))
    records = []
    for case in cases:
        want = oracle[case["id"]]
        got = differ.run_randoc(case)
        known = differ.first_diff(want, got, []) is not None
        records.append({**case, "doc": want, "known": known})
    with open(sys.argv[1], "w", encoding="utf-8") as f:
        f.write("[\n" + ",\n".join(json.dumps(r, ensure_ascii=False) for r in records) + "\n]\n")
    print(f"{len(records)} cases, {sum(r['known'] for r in records)} known differences")


if __name__ == "__main__":
    main()
