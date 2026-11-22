"""Count the reader results and check reader capabilities."""

from collections import Counter

AGGREGATE = "fmt.read"
CAPABILITIES = "caps.readers"
SUITE = "readers"
SOURCES = ("pivot", "harness", "corpus")


def check_exclusions(measure, results):
    known = {}
    for entry in measure.read_json(measure.ROOT / "spec/unstable.json")["units"]:
        if entry["suite"] != SUITE:
            continue
        unit = entry["id"]
        if unit in known or not entry.get("reason") or not entry.get("evidence"):
            raise measure.ToolError(f"{SUITE}: record one reason and evidence for {unit} in spec/unstable.json")
        known[unit] = (entry["status"], entry.get("unstable_parts", []))
    actual = {row["id"]: (row["status"], row.get("unstable_parts", [])) for row in results
              if row["status"] in ("unstable", "oracle_limit")}
    if known != actual:
        changed = next(unit for unit in sorted(known.keys() | actual.keys()) if known.get(unit) != actual.get(unit))
        raise measure.ToolError(f"{SUITE}: record the oracle class of {changed} in spec/unstable.json")


def check_coverage(measure, formats, sources):
    writers = set(measure.inventory_names(measure.ROOT / "spec/inventory/output-formats.json"))
    for fmt in formats:
        group = sources[fmt]
        if fmt == "xlsx" and group["corpus"] < 10:
            raise measure.ToolError("readers: xlsx needs 10 or more open documents")
        if fmt != "xlsx" and fmt not in writers and group["harness"] < 40:
            raise measure.ToolError(f"readers: {fmt} needs 40 or more original inputs")
        if fmt in writers and not 0 < group["pivot"] <= 400:
            raise measure.ToolError(f"readers: {fmt} needs the successful oracle pivot outputs")


def make_runner(measure):
    classes = {"pass": measure.PASS, "fail": measure.FAIL, "no_result": measure.FAIL,
               "unstable": measure.UNSTABLE, "oracle_limit": measure.ORACLE_LIMIT}

    def parse(evidence, _log):
        formats = measure.inventory_names(measure.ROOT / "spec/inventory/input-formats.json")
        report = measure.read_json(evidence / "readers-report.json")
        full = report["summary"]["selected"] is None
        measure.check_sum(SUITE, report["units"], report["summary"]["units"])
        sources = {fmt: Counter(dict.fromkeys(SOURCES, 0)) for fmt in formats}
        units, seen = [], set()
        for row in report["units"]:
            identifier, fmt, status = row["id"], row["format"], row["status"]
            fields = identifier.split("/", 3)
            if fmt not in sources or identifier in seen or len(fields) != 4 or fields[:2] != ["reader", fmt]:
                raise measure.ToolError(f"{SUITE}: invalid or repeated unit {identifier}")
            if fields[2] not in SOURCES or not fields[3]:
                raise measure.ToolError(f"{SUITE}: unknown input source for {identifier}")
            seen.add(identifier)
            sources[fmt][fields[2]] += 1
            units.append(measure.Unit(identifier, classes[status], (AGGREGATE, f"{AGGREGATE}.{fmt}"),
                                      status == "no_result"))
        if sources != report["input_counts"]:
            raise measure.ToolError(f"{SUITE}: the report does not contain each input that it lists")
        keys = (AGGREGATE, *(f"{AGGREGATE}.{fmt}" for fmt in formats if full or sources[fmt].total()))
        if full:
            check_coverage(measure, formats, sources)
            check_exclusions(measure, report["units"])
            counts = measure.Counts.of(units, keys)
            matrix = measure.read_json(evidence / "capabilities.json")["input_formats"]
            for fmt in formats:
                passed, total = counts.rows[f"{AGGREGATE}.{fmt}"]
                groups = matrix[fmt]
                harness_passes = all(groups[name]["passing"] == groups[name]["total"]
                                     for name in ("read", "end_to_end") if name in groups)
                passes = passed == total > 0 and harness_passes
                units.append(measure.Unit(f"capability/reader/{fmt}", measure.PASS if passes else measure.FAIL,
                                          (CAPABILITIES,)))
            keys = (*keys, CAPABILITIES)
        return measure.Counts.of(units, keys)

    return measure.Runner(parse, "--filter", "tools/readers")
