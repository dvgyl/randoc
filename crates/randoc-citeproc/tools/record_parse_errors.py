#!/usr/bin/env python3
"""Records the oracle result of each case of tests/replay/parse-errors.txt.

Each line of the replay file is one case with four fields separated by tabs:
NAME, EXIT, STYLE, RESULT. STYLE and RESULT use the escapes \\n, \\t, \\r and
\\\\. The tool runs `pandoc --citeproc --csl STYLE` for each case, one process
at a time with a time limit of 60 seconds, and writes EXIT and RESULT again.
RESULT is the standard error without the final newline, or `ok` for exit 0.

Usage: record_parse_errors.py [REPLAY_FILE]
Environment: RANDOC_ORACLE_PANDOC, TMPDIR.
"""
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT = os.path.join(HERE, "..", "tests", "replay", "parse-errors.txt")
REFS = '[{"id":"a","type":"book","title":"T","author":[{"family":"Doe","given":"J"}],"issued":{"date-parts":[[2001]]}}]'


def unescape(field):
    out, i = [], 0
    while i < len(field):
        c = field[i]
        if c == "\\" and i + 1 < len(field):
            out.append({"n": "\n", "t": "\t", "r": "\r", "\\": "\\"}[field[i + 1]])
            i += 2
        else:
            out.append(c)
            i += 1
    return "".join(out)


def escape(text):
    return (text.replace("\\", "\\\\").replace("\n", "\\n")
            .replace("\t", "\\t").replace("\r", "\\r"))


def run(oracle, workdir, style):
    path = os.path.join(workdir, "case.csl")
    with open(path, "w", encoding="utf-8", newline="") as handle:
        handle.write(style)
    cmd = ["perl", "-e", "alarm 60; exec @ARGV", "--", oracle, "-f", "markdown",
           "-t", "plain", "--citeproc", "--csl", path,
           "--bibliography", os.path.join(workdir, "refs.json")]
    done = subprocess.run(cmd, input=b"[@a]\n", capture_output=True, check=False)
    if done.returncode in (142, -14):
        return "timeout", "the oracle did not end"
    stderr = done.stderr.decode("utf-8", "replace").rstrip("\n").replace(path, "STYLE")
    # The parser accepts a style that prints nothing; that warning comes from evaluation.
    evaluation = "[WARNING] Citeproc: Citation with no printed form"
    if done.returncode == 0 and all(l.startswith(evaluation) for l in stderr.splitlines()):
        return "0", "ok"
    return str(done.returncode), stderr


def main():
    replay = sys.argv[1] if len(sys.argv) > 1 else DEFAULT
    oracle = os.environ["RANDOC_ORACLE_PANDOC"]
    workdir = os.path.join(os.environ.get("TMPDIR", "/tmp"), "citeproc-record")
    os.makedirs(workdir, exist_ok=True)
    with open(os.path.join(workdir, "refs.json"), "w", encoding="utf-8") as handle:
        handle.write(REFS)
    lines = []
    with open(replay, encoding="utf-8") as handle:
        for line in handle:
            line = line.rstrip("\n")
            if not line or line.startswith("#"):
                lines.append(line)
                continue
            name, _, style, _ = line.split("\t")
            code, result = run(oracle, workdir, unescape(style))
            lines.append("\t".join([name, code, style, escape(result)]))
            print(name, code, result.split("\n")[0][:100], file=sys.stderr)
    with open(replay, "w", encoding="utf-8") as handle:
        handle.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
