"""Validate and count extension pairs."""

from typing import NamedTuple


METRIC = "caps.extensions"
SUITE = "extension_caps"


class Pair(NamedTuple):
    direction: str
    format: str
    extension: str

    @property
    def id(self):
        return "/".join(self)

    def commands(self):
        for toggle in ("+", "-", ""):
            spec = self.format + (toggle + self.extension if toggle else "")
            yield ("-f", spec, "-t", "json") if self.direction == "reader" else (
                "-f", "json", "-t", spec)


def inventory(root, read_json):
    folder = root / "spec/inventory"
    extensions = read_json(folder / "extensions.json")["formats"]
    result = []
    for direction, file, required in (("reader", "input-formats.json", 771),
                                      ("writer", "output-formats.json", 948)):
        pairs = [Pair(direction, fmt, ext) for fmt in read_json(folder / file)
                 for ext in extensions[fmt]["supported"]]
        if len(pairs) != required or len(set(pairs)) != required:
            raise ValueError(f"expected {required} distinct {direction} extension pairs")
        result.extend(pairs)
    return result


def make_runner(measure):
    def parse(evidence, _log):
        report = measure.read_json(evidence / "extension-caps-report.json")
        selected = report["selected"]
        try:
            pairs = inventory(measure.ROOT, measure.read_json)
        except ValueError as error:
            raise measure.ToolError(f"{SUITE}: {error}") from error
        expected = {pair.id for pair in pairs if selected is None or selected in pair.id}
        units, seen, inert = [], set(), set()
        for row in report["units"]:
            identifier, status = row["id"], row["status"]
            if identifier not in expected or identifier in seen:
                raise measure.ToolError(f"{SUITE}: unexpected or repeated pair {identifier}")
            seen.add(identifier)
            if status not in {"pass", "fail", "no_result"}:
                raise measure.ToolError(f"{SUITE}: invalid status for {identifier}")
            method = row["method"]
            if method == "inert":
                if status != "pass" or row["inputs_checked"] <= 0:
                    raise measure.ToolError(f"{SUITE}: incomplete inert check for {identifier}")
                inert.add(identifier)
            if status == "pass" and method != "inert":
                if method != "sensitive" or len(row["oracle_exits"]) != 3 or 0 not in row["oracle_exits"]:
                    raise measure.ToolError(f"{SUITE}: no successful sensitive probe for {identifier}")
            units.append(measure.Unit(identifier, measure.PASS if status == "pass" else measure.FAIL,
                                      (METRIC,), status == "no_result"))
        if seen != expected or sorted(inert) != report["inert"]:
            raise measure.ToolError(f"{SUITE}: incomplete pair coverage or inconsistent inert list")
        return measure.Counts.of(units, (METRIC,))

    return measure.Runner(parse, "--filter", "tools/extension-caps")
