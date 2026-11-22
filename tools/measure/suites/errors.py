"""Read the exit code and conversion log report."""

METRICS = ("errors.units",)
REPORT = "errors-report.json"
TARGETS = ("html", "latex", "markdown")
CODES = {
    str(code) for code in (1, 3, 4, 5, 6, 21, 22, 23, 24, 25, 31, 43, 44, 47, 61, 62,
                            63, 64, 66, 67, 83, 84, 89, 91, 92, 93, 94, 95, 97, 98, 99)
}


def check_coverage(measure, report, ids):
    codes = report["exit_codes"]
    if set(codes) != CODES:
        raise measure.ToolError("errors: the full report needs evidence for all 31 exit codes")
    confirmed = set()
    for code, row in codes.items():
        if row.get("confirmed"):
            if not any(probe["name"] == row["confirmed"] and len(probe["runs"]) == 2
                       and all(isinstance(run, dict) and run["exit"] == int(code) for run in probe["runs"])
                       for probe in row["probes"]):
                raise measure.ToolError(f"errors: exit code {code} needs two oracle confirmations")
            confirmed.add(f"exit/{code}")
        elif not row.get("reason"):
            raise measure.ToolError(f"errors: exit code {code} needs a reason")
    corpus = {i for i in ids if i.startswith("log/corpus/")}
    if len(corpus) != 230:
        raise measure.ToolError("errors: the full report needs 230 corpus logs")
    logs = []
    for target in TARGETS:
        prefix = f"log/pivot/{target}/"
        logs.append({i.removeprefix(prefix) for i in ids if i.startswith(prefix)})
    if len(logs[0]) != 400 or any(group != logs[0] for group in logs[1:]):
        raise measure.ToolError("errors: the full report needs the same 400 trees for each target")
    if len(ids) != len(confirmed) + 1430 or {i for i in ids if i.startswith("exit/")} != confirmed:
        raise measure.ToolError("errors: the full report has missing or unexpected units")


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
        units = []
        ids = set()
        for row in report["units"]:
            identifier, status = row["id"], row["status"]
            if identifier in ids:
                raise measure.ToolError(f"errors: repeated unit {identifier}")
            if status in ("unstable", "oracle_limit") and not row.get("reason"):
                raise measure.ToolError(f"errors: {identifier} needs oracle evidence")
            ids.add(identifier)
            units.append(measure.Unit(identifier, classes[status], METRICS, status == "no_result"))
        measure.check_sum("errors", units, report["summary"]["units"])
        if report["summary"]["selected"] is None:
            check_coverage(measure, report, ids)
        return measure.Counts.of(units, METRICS)

    return measure.Runner(parse, "--filter", "tools/errors")
