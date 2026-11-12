#!/usr/bin/env python3
"""Records the result of Pandoc for each defaults file and metadata file text.

Usage: yaml_texts.py [--limit N]

The tool collects each defaults file and each metadata file that a command
line names in these sources: the case files `conformance/cases/cli/*.toml`,
the resource matrix `tests/data/resources.json` (made by
`resource_matrix.py`) and the generated command lines of `cli_diff.py`. It
keeps the other files of the source case, so that a nested defaults file or
an include file is there too.

Each distinct text runs once through one fixed command line, which shows the
accept decision, the error text and the metadata:

    -f json --defaults=NAME -t native -s -o -
    -f json --metadata-file=NAME -t native -s -o -

The standard input is an empty JSON document. A defaults file with the key
`filters` is left out, because a filter starts a process. These records
are left out too: a result that names the home folder of the run (it
changes with each run), and the keys `citeproc` and `syntax-definition(s)`,
whose results wait for subsystems (rows I-CLI-02 and I-CLI-07).

The result goes to `tests/data/yaml-texts.json`, in the record format of
`tests/resources.rs`, with the log file `log.json` that a defaults file can name. At most 2 Pandoc processes run at a time, each with a
time limit of 60 seconds. The case folders are under `$TMPDIR/cli-s4b/yaml/`.
"""

import argparse
import concurrent.futures
import importlib.util
import json
import os
import re
import shutil
import subprocess
import sys
import tomllib

CRATE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
WT = os.path.dirname(os.path.dirname(CRATE))
ORACLE = os.environ["RANDOC_ORACLE_PANDOC"]
BASE = os.path.join(os.environ["TMPDIR"], "cli-s4b", "yaml")
OUT = os.path.join(CRATE, "tests", "data", "yaml-texts.json")
DOC = '{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[]}'
LIMIT_SECONDS = 60


def named(args):
    """Returns (kind, name) for each defaults file and metadata file of args."""
    found = []
    tokens = list(args)
    for index, token in enumerate(tokens):
        following = tokens[index + 1] if index + 1 < len(tokens) else None
        if token in ("-d", "--defaults") and following is not None:
            found.append(("defaults", following))
        elif token.startswith("--defaults="):
            found.append(("defaults", token.split("=", 1)[1]))
        elif token.startswith("-d") and not token.startswith("--") and len(token) > 2:
            found.append(("defaults", token[2:]))
        elif token == "--metadata-file" and following is not None:
            found.append(("metadata", following))
        elif token.startswith("--metadata-file="):
            found.append(("metadata", token.split("=", 1)[1]))
    return found


def from_toml():
    for path in sorted(os.listdir(os.path.join(WT, "conformance", "cases", "cli"))):
        if not path.endswith(".toml") or path.startswith("._"):
            continue
        with open(os.path.join(WT, "conformance", "cases", "cli", path), "rb") as handle:
            data = tomllib.load(handle)
        for case in data.get("case", []):
            args = case.get("cmd") or case.get("args") or data.get("args") or []
            files = {k: v.encode() for k, v in case.get("files", {}).items()}
            yield args, files, case.get("env", {})


def from_resources():
    with open(os.path.join(CRATE, "tests", "data", "resources.json")) as handle:
        for record in json.load(handle):
            files = {k: bytes.fromhex(v) for k, v in record["files"].items()}
            yield record["args"], files, record.get("env", {})


def from_cli_diff():
    spec = importlib.util.spec_from_file_location("cli_diff", os.path.join(CRATE, "tools", "cli_diff.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    for case in module.generate():
        yield case["args"], {k: v.encode() for k, v in case["files"].items()}, {}


def collect():
    seen, items = set(), []
    for source in (from_toml(), from_resources(), from_cli_diff()):
        for args, files, env in source:
            for kind, name in named(args):
                if kind == "defaults" and has_filters(files):
                    continue
                if not any(path.endswith((name, name + ".yaml")) for path in files):
                    continue
                key = json.dumps([kind, name, sorted((k, v.hex()) for k, v in files.items()), env])
                if key not in seen:
                    seen.add(key)
                    items.append((kind, name, files, env))
    return items


def has_filters(files):
    return any(re.search(rb"(^|[\s{,\"])filters\"?\s*:", content) for content in files.values())


def keep(record):
    if "{home}" in record["stdout"] + record["stderr"]:
        return False
    texts = b"".join(bytes.fromhex(v) for v in record["files"].values())
    return not re.search(rb"citeproc|syntax-definition", texts)


def run(index, kind, name, files, env):
    root = os.path.join(BASE, str(index))
    home = os.path.join(BASE, "home")
    os.makedirs(os.path.join(home, "share"), exist_ok=True)
    os.makedirs(root)
    try:
        for path, content in files.items():
            full = os.path.join(root, path)
            os.makedirs(os.path.dirname(full), exist_ok=True)
            if os.path.basename(path) != "keep":
                with open(full, "wb") as handle:
                    handle.write(content)
        flag = "--defaults=" if kind == "defaults" else "--metadata-file="
        args = ["-f", "json", flag + name, "-t", "native", "-s", "-o", "-"]
        environment = {"PATH": os.environ["PATH"], "HOME": home, "XDG_DATA_HOME": os.path.join(home, "share"),
                       "GHCRTS": os.environ.get("GHCRTS", "-M3g")}
        environment.update({k: v.replace("{root}", root) for k, v in env.items()})
        try:
            done = subprocess.run([ORACLE] + args, cwd=root, env=environment, input=DOC.encode(),
                                  capture_output=True, timeout=LIMIT_SECONDS)
        except subprocess.TimeoutExpired:
            return None
        fix = lambda data: data.decode("utf-8", "replace").replace(root, "{root}").replace(home, "{home}")
        record = {"args": args, "files": {k: v.hex() for k, v in files.items()},
                  "exit": done.returncode, "stdout": fix(done.stdout), "stderr": fix(done.stderr)}
        log = os.path.join(root, "log.json")
        if os.path.exists(log):
            with open(log, "rb") as handle:
                record["log"] = fix(handle.read())
        if env:
            record["env"] = env
        return record
    finally:
        shutil.rmtree(root, ignore_errors=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--limit", type=int, default=0)
    options = parser.parse_args()
    items = collect()
    if options.limit:
        items = items[:options.limit]
    print(f"{len(items)} texts", file=sys.stderr)
    os.makedirs(BASE, exist_ok=True)
    with concurrent.futures.ThreadPoolExecutor(2) as pool:
        records = list(pool.map(lambda pair: run(pair[0], *pair[1]), enumerate(items)))
    stopped = sum(record is None for record in records)
    records = [record for record in records if record is not None and keep(record)]
    with open(OUT, "w") as handle:
        handle.write("[\n" + ",\n".join(json.dumps(r, sort_keys=True) for r in records) + "\n]\n")
    print(f"{len(records)} records, {stopped} stopped at the time limit", file=sys.stderr)


if __name__ == "__main__":
    main()
