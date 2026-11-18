#!/usr/bin/env python3
"""Make the fixed set of document trees for the format suites."""

import argparse
import hashlib
import json
import os
import random
import subprocess
from pathlib import Path

from samples import fixed_trees, node


ROOT = Path(__file__).resolve().parents[2]
SIZE = 400
API_VERSION = [1, 23, 1, 2]
CHARACTERS = 'abcXYZ019éø漢字🙂\u0301*_[#]<>&' + '"\\'


def random_tree(seed):
    draw = random.Random(seed).random

    def text():
        return "".join(CHARACTERS[int(draw() * len(CHARACTERS))] for _ in range(2 + int(draw() * 8)))

    def inline(depth):
        kind = int(draw() * 5) if depth else 0
        if kind == 0:
            return node("Str", text())
        if kind == 1:
            return node("Code", [["", [], []], text()])
        label = ("Emph", "Strong", "Strikeout")[kind - 2]
        return node(label, [inline(depth - 1), inline(depth - 1)])

    def block(depth):
        if depth and draw() < 0.4:
            return node("BlockQuote", [block(depth - 1), block(depth - 1)])
        return node("Para", [inline(depth), node("Space"), inline(depth)])

    return {"meta": {}, "blocks": [block(4) for _ in range(1 + int(draw() * 2))]}


def document(name, value):
    metadata = value["meta"]
    if name != "forms/sample-cell":
        metadata = {
            "identifier": node("MetaString", f"urn:randoc:pivot:{name}"),
            "date": node("MetaString", "2023-11-14"),
            **metadata,
        }
    return {"pandoc-api-version": API_VERSION, "meta": metadata, "blocks": value["blocks"]}


def json_bytes(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-8")


def oracle_accepts(oracle, value):
    command = ["limit-run", "3000", "600", "/bin/sh", "-c", 'ulimit -t 60 && exec "$@"',
               "pivot", oracle, "-f", "json", "-t", "json"]
    result = subprocess.run(command, input=json_bytes(value), stdout=subprocess.DEVNULL,
                            stderr=subprocess.PIPE)
    if result.returncode < 0 or result.returncode >= 125:
        raise RuntimeError(f"oracle check stopped with exit code {result.returncode}")
    return result.returncode == 0


def generate(root, accepts):
    trees = []
    for name, value in fixed_trees(root):
        value = document(name, value)
        if not accepts(value):
            raise ValueError(f"Pandoc rejected {name}")
        trees.append({"id": name, "tree": value})

    seed = 0
    while len(trees) < SIZE:
        seed += 1
        name = f"seed/{seed:04d}"
        value = document(name, random_tree(seed))
        if accepts(value):
            trees.append({"id": name, "tree": value})
        if seed == 1_000 and len(trees) < SIZE:
            raise ValueError("Pandoc did not accept enough seeded trees")
    if len(trees) != SIZE or len({entry["id"] for entry in trees}) != SIZE:
        raise ValueError("the pivot set must have 400 different identifiers")
    return b"".join(json_bytes(entry) + b"\n" for entry in trees)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--oracle", default=os.environ.get("RANDOC_ORACLE_PANDOC"))
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--print-sha256", action="store_true")
    args = parser.parse_args()
    if not args.oracle:
        parser.error("set RANDOC_ORACLE_PANDOC or use --oracle")
    data = generate(ROOT, lambda value: oracle_accepts(args.oracle, value))
    digest = hashlib.sha256(data).hexdigest()
    if args.print_sha256:
        print(digest)
    else:
        expected = (ROOT / "spec/pivot.sha256").read_text(encoding="ascii").strip()
        if digest != expected:
            parser.error(f"the generated SHA-256 is {digest}. Expected {expected}")
    with args.out.open("xb") as output:
        output.write(data)


if __name__ == "__main__":
    main()
