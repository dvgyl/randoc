"""Count custom writer results on the pinned document trees."""

METRIC = "custom.writers"
REPORT = "custom-writers-report.json"
WRITERS = {"binary", "blocks", "classic", "extensions", "layout", "options", "reject",
           "scaffold", "template", "walk"}
EXCLUDED = {"unstable", "oracle_limit"}


def make_runner(measure):
    classes = {
        "pass": measure.PASS,
        "fail": measure.FAIL,
        "no_result": measure.FAIL,
        "unstable": measure.UNSTABLE,
        "oracle_limit": measure.ORACLE_LIMIT,
    }

    def parse(evidence, _log):
        report = measure.read_json(evidence / REPORT)
        measure.check_sum("custom", report["units"], report["summary"]["units"])
        selected = report["summary"]["selected"]
        trees = {writer: set() for writer in WRITERS}
        exclusions = {}
        units = []
        for row in report["units"]:
            identifier, status = row["id"], row["status"]
            kind, _, rest = identifier.partition("/")
            writer, _, tree = rest.partition("/")
            if (kind != "writer" or writer not in trees or not tree or tree in trees[writer]
                    or (selected is not None and selected not in identifier)):
                raise measure.ToolError(f"custom: invalid or repeated writer unit {identifier}")
            trees[writer].add(tree)
            if status in EXCLUDED:
                if not row.get("reason"):
                    raise measure.ToolError(f"custom: {identifier} needs oracle evidence")
                exclusions[identifier] = (status, sorted(row.get("unstable_parts", [])))
            units.append(measure.Unit(identifier, classes[status], (METRIC,), status == "no_result"))
        if selected is None:
            pivot = trees["blocks"]
            if len(pivot) != 400 or any(group != pivot for group in trees.values()):
                raise measure.ToolError("custom: the report needs the same 400 trees for all ten writers")
            known = {}
            for entry in measure.read_json(measure.ROOT / "spec/unstable.json")["units"]:
                if entry["suite"] == "custom":
                    identifier = entry["id"]
                    if identifier in known or not entry.get("reason") or not entry.get("evidence"):
                        raise measure.ToolError(f"custom: record one reason and evidence for {identifier}")
                    known[identifier] = (entry["status"], sorted(entry.get("unstable_parts", [])))
            for identifier in sorted(exclusions.keys() | known.keys()):
                if exclusions.get(identifier) != known.get(identifier):
                    raise measure.ToolError(f"custom: record the oracle class of {identifier} in spec/unstable.json")
        return measure.Counts.of(units, (METRIC,))

    return measure.Runner(parse, "--filter", "tools/custom-writers")
