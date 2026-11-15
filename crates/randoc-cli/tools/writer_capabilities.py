#!/usr/bin/env python3
"""Records standalone output for each registered writer.

Usage: writer_capabilities.py PANDOC OUT.json

Each writer runs with no option, -s, --template, and both options. Each
command reads a titled document and an empty document. The default
templates of these writers are present in randoc-data. No other default
template output is stored. Each oracle process has memory and time limits.
"""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


FORMATS = [
    "beamer", "commonmark", "commonmark_x", "csljson", "gfm", "html",
    "html4", "html5", "json", "latex", "markdown", "markdown_github",
    "markdown_mmd", "markdown_phpextra", "markdown_strict", "markua", "native", "plain",
]
REQUESTS = [
    ("none", []),
    ("standalone", ["-s"]),
    ("template", ["--template=t"]),
    ("both", ["-s", "--template=t"]),
]
DOCUMENTS = [
    ("titled", {
        "pandoc-api-version": [1, 23, 1, 2],
        "meta": {"title": {"t": "MetaInlines", "c": [{"t": "Str", "c": "Title"}]}},
        "blocks": [{"t": "Para", "c": [{"t": "Str", "c": "Body"}]}],
    }),
    ("empty", {"pandoc-api-version": [1, 23, 1, 2], "meta": {}, "blocks": []}),
]
TEMPLATE = "marker:$title$|$body$"


def main():
    oracle, destination = sys.argv[1:]
    with tempfile.TemporaryDirectory(prefix="randoc-writers-") as folder:
        records = record(oracle, Path(folder))
    Path(destination).write_text(json.dumps({
        "template": TEMPLATE, "records": records,
    }, indent=1) + "\n", encoding="utf-8")
    print(f"Recorded {len(records)} commands for {len(FORMATS)} writers.", flush=True)


def record(oracle, root):
    (root / "t").write_text(TEMPLATE, encoding="utf-8")
    home = root / "home"
    home.mkdir(exist_ok=True)
    environment = dict(os.environ, HOME=str(home), XDG_DATA_HOME=str(home / "share"))
    records = []
    for writer in FORMATS:
        for request, options in REQUESTS:
            for name, document in DOCUMENTS:
                stdin = json.dumps(document, separators=(",", ":"))
                args = ["-f", "json", "-t", writer] + options
                result = subprocess.run(
                    ["limit-run", "1500", "60", oracle] + args,
                    input=stdin.encode(), cwd=root, env=environment,
                    capture_output=True, timeout=65, check=False,
                )
                if result.returncode != 0:
                    sys.exit(f"oracle did not complete {args}: {result.stderr!r}")
                records.append({
                    "writer": writer, "request": request, "document": name,
                    "args": args, "stdin": stdin,
                    "exit": result.returncode,
                    "stdout": result.stdout.decode("utf-8"),
                    "stderr": result.stderr.decode("utf-8"),
                })
        print(f"Recorded {writer}: {len(records)} commands.", flush=True)
    return records


if __name__ == "__main__":
    main()
