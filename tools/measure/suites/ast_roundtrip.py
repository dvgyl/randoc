"""Count the JSON and native round trips."""

import re

METRIC = ("ast.roundtrip",)
GROUPS = {"pivot", "version", "harness", "corpus", "reader", "pandoc", "commonmark", "casegen"}


def make_runner(measure):
    classes = {"pass": measure.PASS, "fail": measure.FAIL, "no_result": measure.FAIL,
               "unstable": measure.UNSTABLE, "oracle_limit": measure.ORACLE_LIMIT}

    def parse(evidence, _log):
        report = measure.read_json(evidence / "ast-report.json")
        units = []
        seen = set()
        groups = dict.fromkeys(GROUPS, 0)
        for row in report["units"]:
            key, status = row["id"], row["status"]
            if key in seen or re.fullmatch("[0-9a-f]{64}", key) is None:
                raise measure.ToolError(f"ast: invalid or repeated tree hash {key}")
            seen.add(key)
            for source in row["sources"]:
                groups[source.partition("/")[0]] += 1
            units.append(measure.Unit(key, classes[status], METRIC, status == "no_result"))
        measure.check_sum("ast", units, report["summary"]["units"])
        if report["summary"]["filter"] is None:
            if groups["pivot"] != 400 or groups["version"] != 10 or not all(groups.values()):
                raise measure.ToolError(f"ast: incomplete tree sources ({groups})")
            known = {entry["id"]: entry["status"] for entry in
                     measure.read_json(measure.ROOT / "spec/unstable.json")["units"]
                     if entry["suite"] == "ast" and entry.get("reason") and entry.get("evidence")}
            actual = {row["id"]: row["status"] for row in report["units"]
                      if row["status"] in ("oracle_limit", "unstable")}
            if actual != known:
                changed = next(key for key in sorted(actual.keys() | known.keys())
                               if actual.get(key) != known.get(key))
                raise measure.ToolError(f"ast: record the oracle class of {changed} in spec/unstable.json")
        return measure.Counts.of(units, METRIC)

    return measure.Runner(parse, "--filter", "tools/ast")
