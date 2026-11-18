"""Count writer pivot results and check writer capabilities."""

AGGREGATE = "fmt.write"
CAPABILITIES = "caps.writers"
PIVOT_SIZE = 400


def make_runner(measure):
    classes = {"pass": measure.PASS, "fail": measure.FAIL, "no_result": measure.FAIL,
               "unstable": measure.UNSTABLE, "oracle_limit": measure.ORACLE_LIMIT}

    def parse(evidence, _log):
        inventory = measure.ROOT / "spec/inventory/output-formats.json"
        formats = measure.inventory_names(inventory)
        writers = {fmt: 0 for fmt in formats if fmt != "pdf"}
        report = measure.read_json(evidence / "writers-report.json")
        full = report["summary"]["selected"] is None
        measure.check_sum("writers", report["units"], report["summary"]["units"])
        seen = set()
        units = []
        for item in report["units"]:
            identifier, fmt, status = item["id"], item["format"], item["status"]
            if fmt not in writers or identifier in seen or not identifier.startswith(f"writer/{fmt}/"):
                raise measure.ToolError(f"writers: invalid or repeated unit {identifier}")
            seen.add(identifier)
            writers[fmt] += 1
            units.append(measure.Unit(identifier, classes[status], (AGGREGATE, f"{AGGREGATE}.{fmt}"),
                                      status == "no_result"))
        if full and any(count != PIVOT_SIZE for count in writers.values()):
            raise measure.ToolError("writers: the full report needs 400 trees per writer")
        keys = (AGGREGATE, *(f"{AGGREGATE}.{fmt}" for fmt in writers if full or writers[fmt] > 0))
        if full:
            counts = measure.Counts.of(units, keys)
            matrix = measure.read_json(evidence / "capabilities.json")["output_formats"]
            pdf_path = evidence / "pdf.json"
            pdf_rows = measure.read_json(pdf_path)["rows"] if pdf_path.is_file() else []
            pdf_passes = any(row["metric"] == "pdf.units" and row["pass"] == row["total"] > 0
                             for row in pdf_rows)
            for fmt in formats:
                if fmt == "pdf":
                    passed = pdf_passes
                else:
                    p, total = counts.rows[f"{AGGREGATE}.{fmt}"]
                    groups = matrix[fmt]
                    harness_passes = all(groups[name]["passing"] == groups[name]["total"]
                                         for name in ("write", "end_to_end") if name in groups)
                    passed = p == total > 0 and harness_passes
                units.append(measure.Unit(f"capability/writer/{fmt}", measure.PASS if passed else measure.FAIL,
                                          (CAPABILITIES,)))
            keys = (*keys, CAPABILITIES)
        return measure.Counts.of(units, keys)

    return measure.Runner(parse, "--filter", "tools/pivot")
