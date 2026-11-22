"""Compare Lua name probes in an empty-document filter and in standalone Lua."""

import argparse
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time
from typing import NamedTuple


ROOT = Path(__file__).resolve().parents[3]
METRICS = ("lua.api",)
REPORT = "lua-report.json"
NAME = r"[A-Za-z_][A-Za-z_0-9]*(?:\.[A-Za-z_][A-Za-z_0-9]*)*"
ROW = b"lua-api\t"
EFFECT = b"lua-api-effect "
FRAME = re.compile(rb"\x1eLuaProbe\t(begin|end)\t(" + NAME.encode("ascii") + rb")\x1f")
REPL = "pandoc.cli.repl"
WRITER_DIRECTORY = "writer-probe/"
ZIP_WRITERS = ("chunkedhtml", "docx", "epub", "epub2", "epub3", "odt", "pptx")
FILE_EFFECTS = {
    "copied.txt": "pandoc.system.copy",
    "made": "pandoc.system.make_directory",
    "media": "pandoc.mediabag.write",
    "remove.txt": "pandoc.system.remove",
    "remove-dir": "pandoc.system.remove_directory",
    "rename.txt": "pandoc.system.rename",
    "renamed.txt": "pandoc.system.rename",
    "written.txt": "pandoc.system.write_file",
}
MODES = {
    "filter": ["-f", "markdown", "-t", "plain", "-L", "probe.lua"],
    "standalone": ["lua", "probe.lua"],
}
LAUNCH = '''
ulimit -t 60 && ulimit -f 32768 && PERL_BADLANG=0 exec /usr/bin/perl -e '
    my $program = shift @ARGV;
    delete @ENV{qw(PWD OLDPWD SHLVL _ PERL_BADLANG __CF_USER_TEXT_ENCODING)};
    exec {$program} "pandoc", @ARGV;
    kill "KILL", $$;
' -- "$@"
kill -KILL "$$"
'''


class Run(NamedTuple):
    rows: dict
    effects: dict
    common: tuple
    no_result: bool


def make_runner(measure):
    def parse(evidence, _log):
        report = measure.read_json(evidence / REPORT)
        results = report["units"]
        names = report["names"]
        if sorted(row["id"] for row in results) != sorted(set(names)) or len(names) != len(set(names)):
            raise measure.ToolError("lua: expected one result for each selected name")
        classes = {"pass": measure.PASS, "fail": measure.FAIL, "no_result": measure.FAIL}
        units = [measure.Unit(row["id"], classes[row["status"]], METRICS,
                              row["status"] == "no_result") for row in results]
        return measure.Counts.of(units, METRICS)

    return measure.Runner(parse, "--filter", "tools/measure")


def inventory():
    path = ROOT / "crates" / "randoc-lua" / "MODULES.md"
    names = re.findall(rf"^\| `({NAME})` \| (?:equal|different|absent) \|$",
                       path.read_text(encoding="utf-8"), re.MULTILINE)
    if len(set(names)) != len(names) or len(names) < 468:
        raise ValueError("MODULES.md must list at least 468 distinct oracle names")
    return set(names)


def output_segments(data):
    owner = None
    offset = 0
    for marker in FRAME.finditer(data):
        yield owner, data[offset:marker.start()]
        action, name = marker.groups()
        name = name.decode("ascii")
        if action == b"begin" and owner is None:
            owner = name
        elif action == b"end" and owner == name:
            owner = None
        else:
            raise ValueError(f"invalid output frame for Lua probe {name}")
        offset = marker.end()
    yield owner, data[offset:]


def read_rows(stdout, stderr):
    rows, effects, other = {}, {}, []
    for pipe, data in (("stdout", stdout), ("stderr", stderr)):
        remainder = []
        for owner, output in output_segments(data):
            if owner is not None:
                if output:
                    effects.setdefault(owner, []).append((pipe, output))
                continue
            for line in output.splitlines(keepends=True):
                if pipe == "stderr" and line.startswith(ROW):
                    _, name, result = line.rstrip(b"\n").split(b"\t", 2)
                    name = name.decode("ascii")
                    if not re.fullmatch(NAME, name) or name in rows:
                        raise ValueError(f"invalid or repeated Lua probe name {name!r}")
                    rows[name] = result.decode("ascii")
                elif EFFECT in line:
                    name = line.split(EFFECT, 1)[1].split(b" ", 1)[0].decode("ascii")
                    effects.setdefault(name, []).append((pipe, line))
                else:
                    remainder.append(line)
        other.append(b"".join(remainder))
    return rows, effects, other


def load_snapshot():
    source = ROOT / "crates/randoc-harness/src/snapshot.py"
    spec = importlib.util.spec_from_file_location("lua_api_snapshot", source)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.snapshot


def run(binary, mode, parent, names, snapshot, *, discover=False):
    parent.mkdir(parents=True)
    work = parent / "work"
    work.mkdir()
    (work / ".home").mkdir()
    (work / ".tmp").mkdir()
    (work / WRITER_DIRECTORY).mkdir()
    (work / "listed").mkdir()
    (work / "listed" / "tile.txt").write_bytes(b"copper")
    (work / "sample.txt").write_bytes(b"A copper button.\n")
    for name in ("remove.txt", "rename.txt", "timed.txt"):
        (work / name).write_bytes(b"A copper button.\n")
    for name in ("sample.txt", "timed.txt"):
        os.utime(work / name, (1700000000, 1700000000))
    (work / "remove-dir").mkdir()
    (work / "inner.lua").write_text(
        "return {{Str = function(s) s.text = s.text .. '!' return s end}}\n", encoding="utf-8")
    (work / "identity").write_text("#!/bin/sh\nexec /bin/cat\n", encoding="utf-8")
    (work / "identity").chmod(0o755)
    script = Path(__file__).with_name("lua_probe.lua").read_text(encoding="utf-8")
    prefix = "local names = {" + ", ".join(json.dumps(name) for name in sorted(names)) + "}\n"
    prefix += "local discover = " + str(discover).lower() + "\n"
    prefix += "local standalone = " + str(mode == "standalone").lower() + "\n"
    (work / "probe.lua").write_text(prefix + script, encoding="utf-8")
    before = snapshot(work, set())
    # Standalone Pandoc needs a writable folder for a temporary file at startup.
    env = {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "HOME": str(work / ".home"),
           "TMPDIR": str(work / ".tmp"),
           "LANG": "en_US.UTF-8", "LC_ALL": "en_US.UTF-8", "TZ": "UTC",
           "SOURCE_DATE_EPOCH": "1700000000", "GHCRTS": "-M3g",
           "RANDOC_DATA_DIR": os.environ.get("RANDOC_DATA_DIR")
           or str(Path(os.environ.get("RANDOC_ENV", ROOT.parent / "env")) / "datapack")}
    launch = [sys.executable, str(ROOT / "tools/measure/bin/limit-run"),
              "3000", "600", "/bin/sh", "-c", LAUNCH,
              "lua-probe", str(binary), *MODES[mode]]
    out_path, err_path = parent / "stdout", parent / "stderr"
    with out_path.open("wb") as out, err_path.open("wb") as err:
        code = subprocess.run(launch, cwd=work, env=env, input=b"", stdout=out, stderr=err).returncode
    (parent / "exit").write_text(f"{code}\n", encoding="ascii")
    stderr = err_path.read_bytes()
    if code == 125 or b"limit-run: cannot read process memory" in stderr:
        raise ValueError(f"{mode}: the process limit tool failed. See {parent / 'stderr'}")
    if code < 0 or 128 <= code < 192 or (code == 251 and b"Heap exhausted" in stderr):
        return Run({}, {}, (code, b"", stderr, {}), True)
    try:
        archives = {(WRITER_DIRECTORY + fmt).encode("ascii") for fmt in ZIP_WRITERS}
        after = snapshot(work, archives)
    except (OSError, ValueError) as error:
        (parent / "capture-error").write_text(f"{error}\n", encoding="utf-8")
        return Run({}, {}, (code, b"", stderr, {}), True)
    rows, effects, (stdout, stderr) = read_rows(out_path.read_bytes(), stderr)
    changes = {}
    for path in sorted(before.keys() | after.keys()):
        if before.get(path) == after.get(path):
            continue
        state = after.get(path)
        owner = FILE_EFFECTS.get(path.partition("/")[0])
        if path.startswith(WRITER_DIRECTORY):
            owner = "pandoc.writers." + path.removeprefix(WRITER_DIRECTORY)
            if state and "zip" in state:
                state = {"kind": "zip", **state["zip"]}
        if owner:
            effects.setdefault(owner, []).append((path, state))
        else:
            changes[path] = state
    return Run(rows, effects, (code, stdout, stderr, changes), False)


def require_oracle(result, names, label):
    rows, effects, common, no_result = result
    if no_result or common[0] or names - rows.keys() or effects.keys() - rows.keys():
        raise ValueError(f"{label}: incomplete oracle probes. See the raw results in the evidence folder")


def compare_mode(reference, candidate, name):
    rows, effects, common, no_result = candidate
    expected_rows, expected_effects, expected_common, _ = reference
    if no_result:
        return "no_result"
    if common != expected_common or rows.get(name) != expected_rows[name]:
        return "fail"
    return "pass" if effects.get(name) == expected_effects.get(name) else "fail"


def compare_name(references, candidates, name):
    modes = {mode: compare_mode(references[mode], candidates[mode], name) for mode in MODES}
    if "no_result" in modes.values():
        status = "no_result"
    elif "fail" in modes.values():
        status = "fail"
    else:
        status = "pass"
    return {"id": name, "status": status, "modes": modes}


def measure_probes(oracle, randoc, evidence, names, snapshot, *, discover=False):
    references, candidates = {}, {}
    for mode in MODES:
        discover_filter = discover and mode == "filter"
        first = run(oracle, mode, evidence / f"oracle-{mode}-1", names, snapshot,
                    discover=discover_filter)
        require_oracle(first, names, f"{mode} run 1")
        time.sleep(1)
        second = run(oracle, mode, evidence / f"oracle-{mode}-2", names, snapshot,
                     discover=discover_filter)
        require_oracle(second, names, f"{mode} run 2")
        require_stable(first, second, mode)
        references[mode] = first
        if discover_filter:
            names = set(first.rows)

        candidate = run(randoc, mode, evidence / f"randoc-{mode}-1", names, snapshot)
        if candidate.no_result:
            candidate = run(randoc, mode, evidence / f"randoc-{mode}-2", names, snapshot)
        candidates[mode] = candidate
    return [compare_name(references, candidates, name) for name in sorted(names)]


def measure_api(oracle, randoc, evidence, selection):
    names = inventory()
    snapshot = load_snapshot()
    oracle = oracle.resolve(strict=True)
    randoc = randoc.resolve()
    units = measure_probes(oracle, randoc, evidence, names - {REPL}, snapshot, discover=True)
    # The interactive banner can reach stdout after the Lua probe ends.
    units.extend(measure_probes(oracle, randoc, evidence / "repl", {REPL}, snapshot))
    selected = sorted((unit for unit in units if selection is None or selection in unit["id"]),
                      key=lambda unit: unit["id"])
    return {"names": [unit["id"] for unit in selected], "units": selected}


def require_stable(first, second, mode):
    if first == second:
        return
    names = first.rows.keys() | second.rows.keys() | first.effects.keys() | second.effects.keys()
    changed = sorted(name for name in names if first.rows.get(name) != second.rows.get(name)
                     or first.effects.get(name) != second.effects.get(name))
    raise ValueError(f"{mode}: the oracle runs differ. Probe differences: {changed}. "
                     f"Process results differ: {first.common != second.common}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--oracle", type=Path, required=True)
    parser.add_argument("--randoc", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--filter")
    args = parser.parse_args()
    try:
        report = measure_api(args.oracle, args.randoc, args.evidence.resolve(), args.filter)
    except (OSError, ValueError) as error:
        parser.exit(2, f"lua: {error}\n")
    (args.evidence / REPORT).write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
