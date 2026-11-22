"""Count the steps in one full gate report."""

METRICS = ("gate.steps",)


def make_runner(measure):
    def parse(evidence, _log):
        from gate import STEPS

        report = measure.read_json(evidence / "gate-report.json")
        steps = report["steps"]
        names = [step["name"] for step in steps]
        expected = [step.name for step in STEPS]
        if names == expected + ["programs"] and steps[-1]["status"] == "fail":
            expected.append("programs")
        statuses = [step["status"] for step in steps]
        if names != expected or any(status not in ("pass", "fail", "not-run") for status in statuses):
            raise measure.ToolError("gate: invalid step list in gate-report.json")
        diagnostic = report.get("filter") is not None
        passed = "fail" not in statuses if diagnostic else all(status == "pass" for status in statuses)
        if report["quick"] != diagnostic or report["result"] != ("pass" if passed else "fail"):
            raise measure.ToolError("gate: the report has an invalid result")
        units = [measure.Unit(name, measure.PASS if status == "pass" else measure.FAIL, METRICS)
                 for name, status in zip(names, statuses)]
        return measure.Counts.of(units, METRICS)

    return measure.Runner(parse, "--filter", "tools/measure", exits=(0, 1))
