"""Read the report of the server measurement."""

from collections import Counter

METRICS = ("server.units",)
REPORT = "server-report.json"
FIXED = {
    "batch", "version", "version/text", "option/timeout", "error/invalid-json",
    "error/unknown-format", "error/unknown-field", "error/wrong-type",
}
MODES = ("http", "cgi")


def check_coverage(measure, ids):
    fixed = {f"{mode}/{name}" for mode in MODES for name in FIXED}
    fixed.update(("cli/--help", "cli/--version"))
    missing = fixed - ids
    if missing:
        raise measure.ToolError(f"server: missing required unit {min(missing)}")
    conversions = ids - fixed
    if not conversions:
        raise measure.ToolError("server: the full report has no conversion requests")
    for unit in conversions:
        mode, _, request = unit.partition("/")
        base, _, accept = request.rpartition("/")
        if mode not in MODES or not base.startswith("e2e/") or accept not in ("text", "json"):
            raise measure.ToolError(f"server: invalid conversion unit {unit}")
        for peer in (f"{peer_mode}/{base}/{kind}" for peer_mode in MODES for kind in ("text", "json")):
            if peer not in conversions:
                raise measure.ToolError(f"server: missing conversion unit {peer}")


def check_exclusions(measure, excluded):
    entries = measure.read_json(measure.ROOT / "spec/unstable.json")["units"]
    recorded = {}
    for entry in entries:
        if entry.get("suite") != "server":
            continue
        key = entry["id"]
        if key in recorded or not entry.get("reason") or not entry.get("evidence"):
            raise measure.ToolError(f"server: {key} needs one exclusion record with evidence")
        recorded[key] = (entry["status"], entry.get("unstable_parts", []))
    changed = sorted(key for key in recorded.keys() | excluded.keys()
                     if recorded.get(key) != excluded.get(key))
    if changed:
        raise measure.ToolError(f"server: record the oracle class of {changed[0]} "
                                f"and {len(changed) - 1} other units in spec/unstable.json")


def make_runner(measure):
    classes = {"pass": measure.PASS, "fail": measure.FAIL, "no_result": measure.FAIL,
               "oracle_limit": measure.ORACLE_LIMIT, "unstable": measure.UNSTABLE}

    def parse(evidence, _log):
        report = measure.read_json(evidence / REPORT)
        ids = set()
        excluded = {}
        counts = Counter()
        units = []
        for row in report["units"]:
            key, status = row["id"], row["status"]
            if key in ids:
                raise measure.ToolError(f"server: repeated unit {key}")
            if status in ("oracle_limit", "unstable"):
                excluded[key] = (status, row.get("different", []))
            ids.add(key)
            counts[status] += 1
            units.append(measure.Unit(key, classes[status], METRICS, status == "no_result"))
        if ids != set(report["planned"]) or counts != report["counts"]:
            raise measure.ToolError("server: the units do not match the planned units and counts")
        if report["selected"] is None:
            check_coverage(measure, ids)
            check_exclusions(measure, excluded)
        return measure.Counts.of(units, METRICS)

    return measure.Runner(parse, "--filter", "tools/server")
