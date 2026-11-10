#!/usr/bin/env python3
"""Run a probe set through Pandoc 3.13 (the oracle) and other converters.

Usage: python3 -I probe.py INPUT_DIR REPO_DIR OUT_TSV

The program reads these environment variables:
  RANDOC_ORACLE_PANDOC  the oracle program
  PROBE_WRITERS         optional: a comma-separated list of output formats.
                        If set, the program converts only the Markdown
                        documents, to each of these formats.
  PROBE_TOOLS           NAME=PATH pairs, separated by ":", for example
                        "carta=/x/carta:ferrodoc=/y/ferrodoc:rescribe=/z/rescribe"

INPUT_DIR has two folders: "md" (Markdown documents) and "html" (HTML
documents). REPO_DIR is the Randoc repository. The program also uses the
cases in REPO_DIR/conformance/cases/{json,cli}/*.toml.

The program writes one row for each tool and unit. A unit is one input and
one conversion. The columns are: tool, unit, status, seconds, detail.
Status values:
  identical    stdout and exit code are the same as the oracle
  ast-equal    JSON output is different in bytes but equal as a JSON value,
               when the "pandoc-api-version" field is not compared
  ws-only      the outputs are equal after whitespace is removed
  differ       the output is different
  exit-differ  the exit code is different and the oracle succeeded or failed
  unsupported  the tool refused the format (a non-zero exit without a panic)
  crash        the tool panicked or a signal stopped it
  timeout      the tool did not finish in TIMEOUT seconds
"""

import json
import os
import subprocess
import sys
import time
import tomllib

TIMEOUT = 60

MD_CONVERSIONS = [
    ("markdown", "html", False),
    ("markdown", "json", False),
    ("markdown", "latex", False),
    ("markdown", "html", True),
    ("commonmark", "html", False),
    ("gfm", "html", False),
]
HTML_CONVERSIONS = [
    ("html", "markdown", False),
    ("html", "json", False),
]
JSON_WRITERS = ["json", "html", "markdown", "native", "latex"]


def argv_for(tool, path, frm, to, standalone):
    """Return the command for one conversion, or None if the tool has no CLI for it."""
    if tool == "rescribe":
        if standalone:
            return None
        return [TOOLS[tool], "convert", path, "--from", frm, "--to", to]
    cmd = [ORACLE if tool == "pandoc" else TOOLS[tool], "-f", frm, "-t", to]
    if standalone:
        cmd.append("-s")
    if path is not None:
        cmd.append(path)
    return cmd


def run(cmd, stdin_bytes, cwd):
    start = time.perf_counter()
    try:
        proc = subprocess.run(
            cmd,
            input=stdin_bytes,
            capture_output=True,
            timeout=TIMEOUT,
            cwd=cwd,
        )
    except subprocess.TimeoutExpired:
        return None, time.perf_counter() - start
    return proc, time.perf_counter() - start


def squash(data):
    return b"".join(data.split())


def classify(oracle, proc, to):
    if proc is None:
        return "timeout", ""
    err = proc.stderr.decode("utf-8", "replace")
    if proc.returncode < 0 or "panicked" in err or "RUST_BACKTRACE" in err:
        return "crash", err.strip().splitlines()[0][:120] if err.strip() else f"signal {-proc.returncode}"
    if proc.returncode != oracle.returncode:
        if oracle.returncode == 0:
            first = err.strip().splitlines()[0][:120] if err.strip() else ""
            return "unsupported", first
        return "exit-differ", f"oracle {oracle.returncode} tool {proc.returncode}"
    if proc.stdout == oracle.stdout:
        same_err = "stderr-same" if proc.stderr == oracle.stderr else "stderr-differs"
        return "identical", same_err
    if to == "json":
        try:
            mine, theirs = json.loads(proc.stdout), json.loads(oracle.stdout)
            if mine == theirs:
                return "ast-equal", ""
            # Tools that target an older Pandoc write an older API version.
            if isinstance(mine, dict) and isinstance(theirs, dict):
                version = mine.pop("pandoc-api-version", None)
                theirs.pop("pandoc-api-version", None)
                if mine == theirs:
                    return "ast-equal", f"api version {version}"
        except ValueError:
            pass
    if squash(proc.stdout) == squash(oracle.stdout):
        return "ws-only", ""
    return "differ", f"bytes oracle {len(oracle.stdout)} tool {len(proc.stdout)}"


def units(input_dir, repo):
    writers = os.environ.get("PROBE_WRITERS")
    if writers:
        # Writer mode: Markdown input to each named writer, and no other unit.
        md = [("markdown", w, False) for w in writers.split(",")]
        plan = (("md", md),)
    else:
        plan = (("md", MD_CONVERSIONS), ("html", HTML_CONVERSIONS))
    for sub, conversions in plan:
        folder = os.path.join(input_dir, sub)
        for name in sorted(os.listdir(folder)):
            if name.startswith("."):
                continue
            path = os.path.join(folder, name)
            for frm, to, standalone in conversions:
                tag = "-s" if standalone else ""
                yield f"{sub}/{name}#{frm}>{to}{tag}", path, None, frm, to, standalone, None
    if writers:
        return
    for suite in ("json", "cli"):
        folder = os.path.join(repo, "conformance", "cases", suite)
        for name in sorted(os.listdir(folder)):
            if name.startswith(".") or not name.endswith(".toml"):
                continue
            with open(os.path.join(folder, name), "rb") as handle:
                data = tomllib.load(handle)
            for case in data.get("case", []):
                stdin = case.get("input", "").encode()
                unit = f"conformance/{suite}/{name}/{case['id']}"
                if "cmd" in case:
                    yield unit + "#cmd", None, stdin, None, None, False, case
                else:
                    for to in JSON_WRITERS:
                        yield f"{unit}#json>{to}", None, stdin, "json", to, False, None


def main():
    input_dir, repo, out_tsv = sys.argv[1:4]
    work = os.path.join(os.environ.get("TMPDIR", "/tmp"), "probe-work")
    os.makedirs(work, exist_ok=True)
    rows = []
    for unit, path, stdin, frm, to, standalone, case in units(input_dir, repo):
        for name, text in (case or {}).get("files", {}).items():
            with open(os.path.join(work, name), "w") as handle:
                handle.write(text)
        if case is not None:
            oracle_cmd = [ORACLE] + case["cmd"]
        else:
            oracle_cmd = argv_for("pandoc", path, frm, to, standalone)
        oracle, oracle_secs = run(oracle_cmd, stdin, work)
        if oracle is None:
            rows.append(("pandoc", unit, "timeout", f"{oracle_secs:.3f}", "unit skipped"))
            continue
        rows.append(("pandoc", unit, f"exit {oracle.returncode}", f"{oracle_secs:.3f}", ""))
        for tool in TOOLS:
            if case is not None:
                cmd = None if tool == "rescribe" else [TOOLS[tool]] + case["cmd"]
            elif path is None and tool == "rescribe":
                cmd = [TOOLS[tool], "convert", "-", "--from", frm, "--to", to]
            else:
                cmd = argv_for(tool, path, frm, to, standalone)
            if cmd is None:
                rows.append((tool, unit, "no-cli", "", "the tool has no matching command line"))
                continue
            proc, secs = run(cmd, stdin, work)
            status, detail = classify(oracle, proc, to)
            rows.append((tool, unit, status, f"{secs:.3f}", detail.replace("\t", " ")))
    with open(out_tsv, "w") as handle:
        handle.write("tool\tunit\tstatus\tseconds\tdetail\n")
        for row in rows:
            handle.write("\t".join(row) + "\n")
    summary = {}
    for tool, unit, status, _, _ in rows:
        if tool == "pandoc":
            continue
        group = unit.split("#", 1)[1] if unit.startswith(("md/", "html/")) else unit.split("/")[1]
        summary.setdefault((tool, group), {}).setdefault(status, 0)
        summary[(tool, group)][status] += 1
    for (tool, group), counts in sorted(summary.items()):
        total = sum(counts.values())
        parts = " ".join(f"{k}={v}" for k, v in sorted(counts.items()))
        print(f"{tool:10} {group:28} n={total:3} {parts}")


ORACLE = os.environ["RANDOC_ORACLE_PANDOC"]
TOOLS = dict(pair.split("=", 1) for pair in os.environ.get("PROBE_TOOLS", "").split(":") if pair)

if __name__ == "__main__":
    main()
