"""Count the harness units for which the oracle wrote a file."""

METRICS = ("files.units",)


def make_runner(measure):
    def parse(evidence, _log):
        report = measure.read_json(evidence / "files-report.json")
        units = []
        for result in report["units"]:
            written = result["oracle_writes_file"]
            status = result["status"]
            unit_id = result["id"]
            if written is None and status == "skipped":
                units.append(measure.Unit(unit_id, measure.NO_INPUT, METRICS))
            elif written is None:
                raise measure.ToolError(f"{unit_id} needs a recorded oracle file result")
            elif written is True:
                units.append(measure.Unit(unit_id, measure.HARNESS_CLASSES[status], METRICS, status == "no_result"))
            elif written is not False:
                raise measure.ToolError(f"{unit_id} has an invalid oracle file result")
        measure.check_sum("files", report["units"], report["summary"]["units"])
        return measure.Counts.of(units, METRICS)

    return measure.Runner(parse, "--filter", "crates/randoc-harness")
