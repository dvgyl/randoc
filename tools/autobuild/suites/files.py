"""Run and count the harness units for which the oracle wrote a file."""

import argparse
import os
from pathlib import Path
import subprocess
import time

METRICS = ("files.units",)
REPORTS = ("files-report.json", "files-media-report.json")
MEDIA_FORMATS = ("docx", "odt", "epub", "pptx")


def make_runner(measure):
    def parse(evidence, _log):
        units = []
        seen = set()
        for name in REPORTS:
            report = measure.read_json(evidence / name)
            measure.check_sum("files", report["units"], report["summary"]["units"])
            for result in report["units"]:
                written = result["oracle_writes_file"]
                status = result["status"]
                unit_id = result["id"]
                if unit_id in seen:
                    raise measure.ToolError(f"{unit_id} occurs more than once in the file reports")
                seen.add(unit_id)
                if written is None and status == "skipped":
                    units.append(measure.Unit(unit_id, measure.NO_INPUT, METRICS))
                elif written is None:
                    raise measure.ToolError(f"{unit_id} needs a recorded oracle file result")
                elif written is True:
                    units.append(measure.Unit(unit_id, measure.HARNESS_CLASSES[status], METRICS, status == "no_result"))
                elif written is not False:
                    raise measure.ToolError(f"{unit_id} has an invalid oracle file result")
        return measure.Counts.of(units, METRICS)

    return measure.Runner(parse, "--filter", "tools/autobuild")


def prepare_media(root, oracle, source):
    conformance = root / "conformance"
    fixtures = conformance / "fixtures"
    cases = conformance / "cases" / "files"
    fixtures.mkdir(parents=True)
    cases.mkdir(parents=True)
    env = dict(os.environ, SOURCE_DATE_EPOCH="1700000000", GHCRTS="-M3g", TZ="UTC",
               LC_ALL="en_US.UTF-8", HOME=str(root / ".home"))
    create = ["limit-run", "3000", "600", "/bin/sh", "-c", 'ulimit -t 60 && exec "$@"',
              "files", oracle, "--from", "markdown"]
    media_cases = ['capture_files = {}\n']
    for fmt in MEDIA_FORMATS:
        name = f"images.{fmt}"
        subprocess.run(create + ["--to", fmt, "--output", str(fixtures / name)],
                       input=source, cwd=root, env=env, check=True)
        media_cases.append(f'''
[[case]]
id = "extract-{fmt}"
cmd = ["-f", "{fmt}", "-t", "plain", "--extract-media", "media"]
input_file = "{name}"
''')
    (cases / "media.toml").write_text("".join(media_cases), encoding="utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--harness", required=True)
    parser.add_argument("--oracle", required=True)
    parser.add_argument("--randoc", required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--filter")
    args = parser.parse_args()
    evidence = args.evidence.resolve()
    root = evidence / "media-inputs"
    source = Path("conformance/fixtures/files/media-sample.md").read_bytes()
    prepare_media(root, str(Path(args.oracle).resolve()), source)

    harness = [str(Path(args.harness).resolve())]
    media_options = ["--root", str(root), "--oracle", args.oracle, "--files", "--jobs", "2"]
    subprocess.run(harness + ["record"] + media_options, check=True)
    time.sleep(1)
    subprocess.run(harness + ["verify-oracle", "--all"] + media_options, check=True)
    selection = ["--filter", args.filter] if args.filter is not None else []
    run = harness + ["run", "--files", "--randoc", args.randoc] + selection
    subprocess.run(run + ["--report", str(evidence / REPORTS[0]), "--jobs", "4"], check=True)
    subprocess.run(run + ["--report", str(evidence / REPORTS[1]), "--root", str(root), "--jobs", "2"], check=True)


if __name__ == "__main__":
    main()
