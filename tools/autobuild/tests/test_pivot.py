import hashlib
import json
import re
import sys
import unittest
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tools/pivot"))
import generate as pivot


def measure_tree(value):
    """Count tagged nodes and their nesting depth."""
    stack = [(value, 0)]
    count, deepest = 0, 0
    while stack:
        part, depth = stack.pop()
        if isinstance(part, dict):
            if "t" in part:
                count += 1
                depth += 1
                deepest = max(deepest, depth)
            stack.extend((child, depth) for child in part.values())
        elif isinstance(part, list):
            stack.extend((child, depth) for child in part)
    return count, deepest


class PivotTests(unittest.TestCase):
    def test_fixed_list_hash_and_coverage(self):
        data = pivot.generate(ROOT, lambda _tree: True)
        self.assertEqual(hashlib.sha256(data).hexdigest(),
                         (ROOT / "spec/pivot.sha256").read_text().strip())
        rows = [json.loads(line) for line in data.splitlines()]
        dictionaries = {row["id"]: row["tree"] for row in rows}
        self.assertEqual(len(dictionaries), 400)
        self.assertEqual(pivot.json_bytes(rows[0]["tree"]),
                         (ROOT / "conformance/fixtures/files/sample-cell.json").read_bytes())
        counts = Counter(name.split("/")[0] for name in dictionaries)
        self.assertEqual(counts["highlight"], 194)
        self.assertEqual(counts["math"], 60)
        for language in json.loads((ROOT / "spec/inventory/highlight-languages.json").read_text()):
            code = dictionaries[f"highlight/{language}"]["blocks"][0]
            self.assertEqual(code["t"], "CodeBlock")
            self.assertEqual(code["c"][0][1], [language])
            self.assertGreaterEqual(len(code["c"][1].splitlines()), 15)

        commands = set()
        for name, tree in dictionaries.items():
            if name.startswith("math/commands-"):
                for paragraph in tree["blocks"]:
                    expression = paragraph["c"][0]["c"][1]
                    commands.update(re.findall(r"\\([a-zA-Z]+)", expression))
        names = (ROOT / "crates/randoc-math/tools/katex-names.txt").read_text().splitlines()
        self.assertEqual(commands, {name for name in names if name and not name.startswith("#")})
        malformed = {tree["blocks"][0]["c"][0]["c"][1] for name, tree in dictionaries.items()
                     if name.startswith("math/invalid-")}
        self.assertEqual(len(malformed), 20)

    def test_rejected_random_seed_uses_the_next_seed(self):
        def accept(tree):
            return tree["meta"]["identifier"]["c"] != "urn:randoc:pivot:seed/0001"

        rows = [json.loads(line) for line in pivot.generate(ROOT, accept).splitlines()]
        seeds = [row["id"] for row in rows if row["id"].startswith("seed/")]
        self.assertEqual(seeds[0], "seed/0002")
        self.assertEqual(len(rows), 400)

    def test_generated_random_trees_stay_within_limits(self):
        for seed in range(1, 1001):
            count, depth = measure_tree(pivot.random_tree(seed))
            self.assertLessEqual(count, 200)
            self.assertLessEqual(depth, 6)


if __name__ == "__main__":
    unittest.main()
