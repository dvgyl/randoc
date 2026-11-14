#!/usr/bin/env python3
"""Records the level of each log message that Randoc can emit.

Usage: level_matrix.py PANDOC OUT.json

Each probe provokes one message kind (`target`) in a new folder with its
files. PANDOC runs with `--log=log.json`. The data file keeps the arguments,
the files, the standard input and the pairs (type, verbosity) of the log.
`tests/levels.rs` runs Randoc on each probe and compares the level of each
message type that both logs hold. The probe must make Randoc emit `target`.
"""

import json
import os
import subprocess
import sys
import tempfile

MD = ["-f", "markdown", "-t", "json"]
HTML_IN = ["-f", "html", "-t", "json"]
LATEX_IN = ["-f", "latex", "-t", "json"]

PROBES = [
    ("SkippedContent", HTML_IN, "<p>a</p><script>b</script>\n", {}),
    ("SkippedContent", HTML_IN, "<p>a<object>b</object></p>\n", {}),
    ("DuplicateLinkReference", MD, "[k]: one\n[k]: two\n\n[k]\n", {}),
    ("DuplicateNoteReference", MD, "x[^n]\n\n[^n]: one\n\n[^n]: two\n", {}),
    ("NoteDefinedButNotUsed", MD, "x\n\n[^u]: unused\n", {}),
    ("DuplicateIdentifier", MD, "# One {#h}\n\n# Two {#h}\n", {}),
    ("ReferenceNotFound", HTML_IN, '<p>x<a href="#gone" role="doc-noteref">9</a></p>\n', {}),
    ("CouldNotLoadIncludeFile", LATEX_IN, "\\input{absentfile}\n", {}),
    ("MacroAlreadyDefined", LATEX_IN, "\\newcommand{\\q}{1}\\newcommand{\\q}{2}\n", {}),
    ("CouldNotConvertTexMath", ["-f", "markdown", "-t", "html"], "$x^$\n", {}),
    ("LoadedResource", ["-f", "markdown", "-t", "html", "-s", "-M", "title=t", "-H", "head.html"],
     "x\n", {"head.html": "<meta>\n"}),
    ("NoTitleElement", ["-f", "markdown", "-t", "html", "-s"], "x\n", {}),
    ("NoTitleElement", ["-f", "markdown", "-t", "html5", "-s"], "", {}),
    ("Deprecated", ["-f", "markdown_github", "-t", "json"], "x\n", {}),
    ("Deprecated", ["-f", "json", "-t", "markdown_github"],
     '{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[]}', {}),
    ("CouldNotDeduceFormat", ["-t", "json", "in.qqq"], None, {"in.qqq": "x\n"}),
    ("RunningFilter", ["-f", "markdown", "-t", "json", "--verbose", "--filter", "same.sh"], "x\n",
     {"same.sh": "cat\n"}),
    ("FilterCompleted", ["-f", "markdown", "-t", "json", "--verbose", "--filter", "same.sh"], "x\n",
     {"same.sh": "cat\n"}),
    ("DuplicateAttribute", ["-f", "markdown", "-t", "html"], "[a]{k=1 k=2}\n", {}),
    ("NotUtf8Encoded", ["-f", "markdown", "-t", "json", "bad.md"], None, {"bad.md": b"a\xffb\n"}),
    ("UnclosedDiv", MD, "::: box\ntext\n", {}),
]


def run(pandoc, target, args, stdin, files):
    with tempfile.TemporaryDirectory(prefix="cli-s5-level-") as root:
        for name, text in files.items():
            with open(os.path.join(root, name), "wb") as handle:
                handle.write(text if isinstance(text, bytes) else text.encode())
        full = args + ["--log=log.json", "-o", "out"]
        result = subprocess.run([pandoc] + full, cwd=root, input=(stdin or "").encode(),
                                capture_output=True, timeout=60, check=False)
        try:
            with open(os.path.join(root, "log.json"), encoding="utf-8") as handle:
                log = json.load(handle)
        except FileNotFoundError:
            sys.exit(f"no log for {target} {args}: {result.stderr.decode()}")
    return {
        "target": target,
        "args": args,
        "stdin": stdin,
        "files": {name: (text.hex() if isinstance(text, bytes) else text.encode().hex())
                  for name, text in files.items()},
        "oracle": [[record["type"], record["verbosity"]] for record in log],
    }


def main():
    pandoc, out = sys.argv[1], sys.argv[2]
    records = [run(pandoc, *probe) for probe in PROBES]
    for record in records:
        types = {kind.lower() for kind, _ in record["oracle"]}
        if record["target"].lower() not in types:
            print("the oracle does not emit", record["target"], record["args"], file=sys.stderr)
    with open(out, "w", encoding="utf-8") as handle:
        json.dump(records, handle, indent=1)
        handle.write("\n")


if __name__ == "__main__":
    main()
