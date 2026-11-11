#!/usr/bin/env python3
"""Runs Pandoc and Randoc on generated command lines and compares the results.

Usage: cli_diff.py [--minutes N] [--limit N] [--list]

The generator makes command lines from `spec/inventory/options.json`: each
option with valid values, invalid values and no value, pairs of options, the
same option twice, and each defaults key in `d.yaml`. Each case has a small
`json` or `native` input.

The tool copies `target/debug/randoc` to `$TMPDIR/cli-s3b/diff/randoc-bin` and
runs that copy. Each program runs in its own new case folder under
`$TMPDIR/cli-s3b/diff/`, with `HOME` and `XDG_DATA_HOME` inside that folder.
At most one Pandoc process and two Randoc processes run at a time.

The comparison uses the exit code, standard output, standard error and the
files that the program writes in the case folder. It normalises only the
case folder path (`{root}`), the `pandoc: ` prefix and the GHC backtrace
(I-CLI-06), and the second line of `--version` (D-001).

Results go to `results.jsonl` (one line for each case; a new run skips the
cases that the file has) and the signature table to `signatures.txt`.
"""

import argparse
import concurrent.futures
import json
import os
import random
import re
import shutil
import subprocess
import sys
import threading
import time
from collections import defaultdict

WT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
DIFF = os.path.join(os.environ["TMPDIR"], "cli-s3b", "diff")
ORACLE = os.environ["RANDOC_ORACLE_PANDOC"]
RANDOC = os.path.join(DIFF, "randoc-bin")
LIMIT_SECONDS = 30

JSON_DOC = '{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[]}'
NATIVE_DOC = '[ Para [ Str "a" , Space , Str "b" ] ]\n'
BASES = {
    "json": (["-f", "json", "-t", "json", "in.json"], {"in.json": JSON_DOC}),
    "native": (["-f", "native", "-t", "native", "in.native"], {"in.native": NATIVE_DOC}),
}

FILES = {
    "f.txt": "text\n",
    "meta.yaml": "k: v\nlist: [a, b]\n",
    "d2.yaml": "toc: true\n",
    "tpl.txt": "$body$\n",
    "filt.lua": "return {}\n",
    "filt.sh": "#!/bin/sh\ncat\n",
    "bib.json": "[]\n",
    "s.csl": "<style/>\n",
    "s.css": "p {}\n",
    "img.png": "not a png\n",
    "syn.xml": "<language/>\n",
    "dd/keep": "",
    "r1/keep": "",
}

# Options that write outside the case folder or use the network.
EXCLUDED = {"pdf-engine", "request-header", "no-check-certificate"}

BOOL = (["", "true", "false"], ["maybe"])
NUM = lambda *v: (list(v), ["x", "-1"])
ENUM = lambda *v: (list(v), ["bogus"])
FILE = lambda *v: (list(v), ["nosuch.txt"])

# Option name: (valid values, invalid values). "" means the option without a value.
VALUES = {
    "from": (["json", "native", "markdown", "html"], ["bogus", "json+bogus"]),
    "to": (["json", "native", "plain", "html"], ["bogus", "json+bogus"]),
    "output": (["out.json", "-"], ["nosuchdir/out.json"]),
    "data-dir": (["dd"], ["nosuchdir"]),
    "metadata": (["k=v", "k", "k:v"], ["=v"]),
    "metadata-file": FILE("meta.yaml"),
    "defaults": (["d2.yaml", "d2"], ["nosuch.yaml"]),
    "template": FILE("tpl.txt"),
    "variable": (["k=v", "k"], ["=v"]),
    "variable-json": (['k:[1,2]', 'k:{"a":1}', "k:true"], ["k:[1", "k"]),
    "wrap": ENUM("auto", "none", "preserve"),
    "toc-depth": NUM("2", "6"),
    "number-offset": (["1,2", "0"], ["x", "1,,2"]),
    "top-level-division": ENUM("section", "chapter", "part", "default"),
    "extract-media": (["media"], ["f.txt"]),
    "resource-path": (["r1:.", "."], ["nosuchdir"]),
    "include-in-header": FILE("f.txt"),
    "include-before-body": FILE("f.txt"),
    "include-after-body": FILE("f.txt"),
    "no-highlight": ([""], []),
    "highlight-style": (["tango", "pygments"], ["bogus"]),
    "syntax-definition": FILE("syn.xml"),
    "syntax-highlighting": (["none", "default", "idiomatic", "tango"], ["bogus"]),
    "dpi": NUM("72"),
    "eol": ENUM("crlf", "lf", "native"),
    "columns": NUM("40"),
    "tab-stop": NUM("2"),
    "pdf-engine-opt": (["-x"], []),
    "reference-doc": FILE("f.txt"),
    "abbreviations": FILE("f.txt"),
    "typst-input": (["k=v"], ["k"]),
    "indented-code-classes": (["haskell"], []),
    "default-image-extension": (["png"], []),
    "filter": (["filt.sh"], ["nosuchfilter"]),
    "lua-filter": (["filt.lua"], ["nosuch.lua"]),
    "shift-heading-level-by": (["1", "-1"], ["x"]),
    "base-header-level": (["2"], ["x", "0"]),
    "track-changes": ENUM("accept", "reject", "all"),
    "reference-location": ENUM("block", "section", "document"),
    "figure-caption-position": ENUM("above", "below"),
    "table-caption-position": ENUM("above", "below"),
    "markdown-headings": ENUM("setext", "atx"),
    "slide-level": NUM("2"),
    "email-obfuscation": ENUM("none", "javascript", "references"),
    "id-prefix": (["p"], []),
    "title-prefix": (["T"], []),
    "css": (["s.css"], []),
    "epub-subdirectory": (["EPUB"], []),
    "epub-cover-image": FILE("img.png"),
    "epub-metadata": FILE("f.txt"),
    "epub-embed-font": FILE("f.txt"),
    "split-level": NUM("2"),
    "chunk-template": (["%n.html"], []),
    "epub-chapter-level": NUM("2"),
    "ipynb-output": ENUM("all", "none", "best"),
    "citeproc": ([""], []),
    "bibliography": FILE("bib.json"),
    "csl": FILE("s.csl"),
    "citation-abbreviations": FILE("f.txt"),
    "natbib": ([""], []),
    "biblatex": ([""], []),
    "math-method": ENUM("mathml", "katex", "plain", "webtex", "gladtex", "mathjax"),
    "mathml": ([""], []),
    "webtex": (["", "u/"], []),
    "mathjax": (["", "u/"], []),
    "katex": (["", "u/"], []),
    "gladtex": ([""], []),
    "verbose": ([""], []),
    "quiet": ([""], []),
    "log": (["log.json"], ["nosuchdir/log.json"]),
    "completion": (["bash", "zsh", "fish"], ["bogus"]),
    "bash-completion": ([""], []),
    "list-input-formats": ([""], []),
    "list-output-formats": ([""], []),
    "list-extensions": (["", "markdown", "json"], ["bogus"]),
    "list-highlight-languages": ([""], []),
    "list-highlight-styles": ([""], []),
    "print-default-template": (["html", "json", "plain"], ["bogus"]),
    "print-default-data-file": (["abbreviations", "templates/default.html", "epub.css"], ["nosuch"]),
    "print-highlight-style": (["tango"], ["bogus"]),
    "version": ([""], []),
    "help": ([""], []),
}

# Defaults key: (option of VALUES, YAML text for a valid value).
DEFAULTS_KEYS = {
    "reader": ("from", "json"), "writer": ("to", "json"), "output-file": ("output", "out.json"),
    "input-file": (None, "in.json"), "input-files": (None, "[in.json]"),
    "data-dir": ("data-dir", "dd"), "metadata": ("metadata", "{k: v}"),
    "metadata-file": ("metadata-file", "meta.yaml"), "metadata-files": ("metadata-file", "[meta.yaml]"),
    "variables": ("variable", "{k: v}"), "template": ("template", "tpl.txt"),
    "toc": ("toc", "true"), "table-of-contents": ("toc", "true"),
    "lof": ("lof", "true"), "list-of-figures": ("lof", "true"),
    "lot": ("lot", "true"), "list-of-tables": ("lot", "true"),
    "syntax-definitions": ("syntax-definition", "[syn.xml]"),
    "pdf-engine-opts": ("pdf-engine-opt", "[-x]"), "filters": ("filter", "[filt.sh]"),
    "identifier-prefix": ("id-prefix", "p"), "epub-fonts": ("epub-embed-font", "[f.txt]"),
    "cite-method": (None, "natbib"), "html-math-method": (None, "{method: mathml}"),
    "verbosity": (None, "INFO"), "log-file": ("log", "log.json"), "defaults": ("defaults", "d2.yaml"),
}


def load_options():
    with open(os.path.join(WT, "spec", "inventory", "options.json")) as handle:
        options = json.load(handle)
    return [o for o in options if o["long"][0] not in EXCLUDED]


def values_of(option):
    name = option["long"][0]
    if name in VALUES:
        return VALUES[name]
    if option["argument"] == "true|false":
        return BOOL
    raise SystemExit(f"cli_diff.py: no values for --{name}")


def files_for(args):
    """The case files that the arguments name."""
    tokens = set(re.split(r"[ =:,\[\]]", " ".join(args)))
    return {name: content for name, content in FILES.items()
            if name.split("/")[0] in tokens or os.path.splitext(name)[0] in tokens}


def with_value(flag, value, form):
    if value == "":
        return [flag]
    if form == "eq":
        return [flag + "=" + value]
    if form == "sep":
        return [flag, value]
    return [flag + value]


def case(cid, opt, args, base="json", extra=None):
    base_args, base_files = BASES[base]
    full = args + base_args
    files = dict(base_files)
    files.update(files_for(full))
    files.update(extra or {})
    return {"id": cid, "opt": opt, "args": full, "files": files}


def single_cases(options):
    cases = []
    for option in options:
        name = option["long"][0]
        valid, invalid = values_of(option)
        long_flag = "--" + name
        for base in BASES:
            for value in valid:
                cases.append(case(f"one/{name}/valid/{base}/{value}", name,
                                  with_value(long_flag, value, "eq"), base))
        value = next((v for v in valid if v), "")
        if value and not option["optional_argument"]:
            cases.append(case(f"one/{name}/sep", name, with_value(long_flag, value, "sep")))
        for alias in option["long"][1:]:
            cases.append(case(f"one/{name}/alias/{alias}", name, with_value("--" + alias, value, "eq")))
        for short in option["short"]:
            flag = "-" + short
            if value:
                cases.append(case(f"one/{name}/short-sep/{short}", name,
                                  [flag] if option["optional_argument"] else [flag, value]))
                cases.append(case(f"one/{name}/short-join/{short}", name, [flag + value]))
            else:
                cases.append(case(f"one/{name}/short/{short}", name, [flag]))
        for value in invalid:
            for base in BASES:
                cases.append(case(f"one/{name}/invalid/{base}/{value}", name,
                                  with_value(long_flag, value, "eq"), base))
        cases.append(case(f"one/{name}/none", name, [long_flag]))
        last = case(f"one/{name}/none-last", name, [])
        last["args"].append(long_flag)
        cases.append(last)
        if option["argument"]:
            cases.append(case(f"one/{name}/empty", name, [long_flag + "="]))
        cases.append(case(f"one/{name}/twice", name,
                          with_value(long_flag, value, "eq") + with_value(long_flag, valid[-1] if valid else "", "eq")))
    return cases


def pair_cases(options, per_option):
    rng = random.Random(13)
    cases = []
    for option in options:
        partners = rng.sample([o for o in options if o is not option], per_option)
        for partner in partners:
            a, b = option["long"][0], partner["long"][0]
            args = []
            for opt in (option, partner):
                valid, invalid = values_of(opt)
                pool = valid + invalid
                args += with_value("--" + opt["long"][0], rng.choice(pool) if pool else "", "eq")
            base = rng.choice(list(BASES))
            cases.append(case(f"pair/{a}/{b}/{base}", f"{a}+{b}", args, base))
    return cases


def defaults_cases(options):
    keys = {o["long"][0]: (o["long"][0], None) for o in options}
    keys.update(DEFAULTS_KEYS)
    info = {"completion", "bash-completion", "list-input-formats", "list-output-formats", "list-extensions",
            "list-highlight-languages", "list-highlight-styles", "print-default-template",
            "print-default-data-file", "print-highlight-style", "version", "help", "mathml", "webtex",
            "mathjax", "katex", "gladtex", "natbib", "biblatex", "no-highlight", "quiet", "verbose",
            "base-header-level", "id-prefix", "log", "syntax-definition", "filter", "lua-filter",
            "epub-embed-font", "include-in-header", "include-before-body", "include-after-body",
            "variable", "variable-json", "typst-input", "pdf-engine-opt"}
    cases = []
    for key, (opt, text) in sorted(keys.items()):
        if key in info and key not in DEFAULTS_KEYS:
            continue
        valid, invalid = VALUES.get(opt, BOOL) if opt else ([], [])
        if text is None:
            text = next((v for v in valid if v), "true")
        forms = {"valid": text, "empty": "", "null": "null", "list": f"[{text}, {text}]",
                 "invalid": invalid[0] if invalid else "{a: b}", "map": "{a: b}"}
        for form, value in forms.items():
            body = f"{key}: {value}\n"
            for base in BASES:
                c = case(f"defaults/{key}/{form}/{base}", "d:" + key, ["-d", "d.yaml"], base,
                         {"d.yaml": body})
                c["files"].update(files_for([value]))
                cases.append(c)
        if opt:
            flag = with_value("--" + opt, next((v for v in valid if v), ""), "eq")
            for order in ("before", "after"):
                args = (["-d", "d.yaml"] + flag) if order == "before" else (flag + ["-d", "d.yaml"])
                c = case(f"defaults/{key}/with-option/{order}", "d:" + key, args, "json",
                         {"d.yaml": f"{key}: {text}\n"})
                c["files"].update(files_for([text]))
                cases.append(c)
    return cases


def generate():
    options = load_options()
    cases = single_cases(options) + pair_cases(options, 12) + defaults_cases(options)
    seen, unique = set(), []
    for c in cases:
        key = json.dumps([c["args"], sorted(c["files"].items())])
        if key not in seen:
            seen.add(key)
            unique.append(c)
    ids = defaultdict(int)
    for c in unique:
        ids[c["id"]] += 1
        if ids[c["id"]] > 1:
            c["id"] += f"#{ids[c['id']]}"
    return unique


ORACLE_LOCK = threading.Semaphore(1)
RANDOC_SLOTS = threading.Semaphore(2)
COUNTER = iter(range(10**9))
COUNTER_LOCK = threading.Lock()


def new_folder(tag):
    with COUNTER_LOCK:
        number = next(COUNTER)
    path = os.path.join(DIFF, f"c{os.getpid()}-{number}-{tag}")
    os.makedirs(path)
    return path


def remove_folder(path):
    path = os.path.abspath(path)
    if not path.startswith(DIFF + os.sep):
        raise SystemExit(f"cli_diff.py: refuse to delete {path}")
    shutil.rmtree(path, ignore_errors=True)


def snapshot(root, skip):
    found = {}
    for folder, dirs, names in os.walk(root):
        for name in names:
            if name.startswith("._"):
                continue
            full = os.path.join(folder, name)
            rel = os.path.relpath(full, root)
            if rel in skip:
                continue
            try:
                with open(full, "rb") as handle:
                    found[rel] = handle.read(65536)
            except OSError as error:
                found[rel] = repr(error).encode()
    return found


def run_one(program, c, slot):
    root = new_folder("o" if program == ORACLE else "r")
    try:
        for name, content in c["files"].items():
            path = os.path.join(root, name)
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, "w") as handle:
                handle.write(content)
            if name.endswith(".sh"):
                os.chmod(path, 0o755)
        env = dict(os.environ)
        env["HOME"] = os.path.join(root, "home")
        env["XDG_DATA_HOME"] = os.path.join(root, "xdg")
        os.makedirs(env["HOME"])
        os.makedirs(env["XDG_DATA_HOME"])
        with slot:
            try:
                done = subprocess.run([program] + c["args"], cwd=root, env=env, stdin=subprocess.DEVNULL,
                                      capture_output=True, timeout=LIMIT_SECONDS)
                exit_code, out, err = done.returncode, done.stdout, done.stderr
            except subprocess.TimeoutExpired as expired:
                exit_code, out, err = "timeout", expired.stdout or b"", expired.stderr or b""
        written = snapshot(root, set(c["files"]))
        return normalise(root, c, exit_code, out, err, written)
    finally:
        remove_folder(root)


def normalise(root, c, exit_code, out, err, written):
    roots = sorted({root, os.path.realpath(root)}, key=len, reverse=True)

    def fix(data):
        for r in roots:
            data = data.replace(r.encode(), b"{root}")
        return data

    out, err = fix(out), fix(err)
    err = err.split(b"HasCallStack backtrace:")[0]
    err = re.sub(rb"(?m)^pandoc: ", b"", err)
    if "--version" in c["args"] or "-v" in c["args"]:
        lines = out.split(b"\n")
        if len(lines) > 1:
            lines[1] = b"{D-001}"
        out = b"\n".join(lines)
    files = {k: fix(v).decode("utf-8", "replace") for k, v in sorted(written.items())}
    return {"exit": exit_code, "out": out.decode("utf-8", "replace"), "err": err.decode("utf-8", "replace"),
            "files": files}


def compare(c):
    ref = run_one(ORACLE, c, ORACLE_LOCK)
    got = run_one(RANDOC, c, RANDOC_SLOTS)
    diff = [k for k in ("exit", "out", "err", "files") if ref[k] != got[k]]
    return {"id": c["id"], "opt": c["opt"], "args": c["args"], "files": c["files"], "equal": not diff,
            "diff": diff, "ref": ref, "randoc": got}


def first_line(text, case_row):
    lines = [l for l in text.split("\n") if l.strip()]
    errors = [l for l in lines if not l.startswith(("[WARNING]", "[INFO]", " "))]
    line = (errors or lines or ["(empty)"])[0]
    for value in sorted({a.split("=", 1)[-1] for a in case_row["args"] if len(a) > 1}, key=len, reverse=True):
        if len(value) > 1 and not value.startswith("-"):
            line = line.replace(value, "<V>")
    line = re.sub(r"'[^']*'|\"[^\"]*\"|`[^`]*`", "<Q>", line)
    line = re.sub(r"\d+", "N", line)
    return line[:160]


def cause(row):
    return (",".join(row["diff"]), f"{row['ref']['exit']}/{row['randoc']['exit']}",
            first_line(row["ref"]["err"], row), first_line(row["randoc"]["err"], row))


def size(row):
    return len(" ".join(row["args"])) + sum(len(v) for v in row["files"].values())


def write_signatures(rows, path):
    by_cause = defaultdict(list)
    for row in rows:
        if not row["equal"]:
            by_cause[cause(row)].append(row)
    equal = sum(1 for row in rows if row["equal"])
    lines = [f"cases run: {len(rows)}  equal: {equal}  different: {len(rows) - equal}  signatures: {len(by_cause)}", ""]
    for key, members in sorted(by_cause.items(), key=lambda item: -len(item[1])):
        smallest = min(members, key=size)
        opts = sorted({m["opt"] for m in members})
        lines.append(f"## {len(members)} cases  diff={key[0]}  exit pandoc/randoc={key[1]}")
        lines.append(f"   pandoc err: {key[2]}")
        lines.append(f"   randoc err: {key[3]}")
        lines.append(f"   options ({len(opts)}): {' '.join(opts[:25])}{' ...' if len(opts) > 25 else ''}")
        lines.append(f"   smallest: {smallest['id']}: pandoc {' '.join(smallest['args'])}")
        lines.append(f"   files: {json.dumps(smallest['files'])[:300]}")
        if "out" in key[0]:
            lines.append(f"   pandoc out: {smallest['ref']['out'][:160]!r}")
            lines.append(f"   randoc out: {smallest['randoc']['out'][:160]!r}")
        if "files" in key[0]:
            lines.append(f"   pandoc files: {json.dumps(smallest['ref']['files'])[:200]}")
            lines.append(f"   randoc files: {json.dumps(smallest['randoc']['files'])[:200]}")
        lines.append("")
    with open(path, "w") as handle:
        handle.write("\n".join(lines))
    return equal, len(by_cause)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--minutes", type=float, default=70)
    parser.add_argument("--limit", type=int, default=0)
    parser.add_argument("--list", action="store_true")
    options = parser.parse_args()
    cases = generate()
    if options.list:
        for c in cases:
            print(c["id"], json.dumps(c["args"]))
        print(f"{len(cases)} cases", file=sys.stderr)
        return
    os.makedirs(DIFF, exist_ok=True)
    shutil.copy2(os.path.join(WT, "target", "debug", "randoc"), RANDOC)
    results = os.path.join(DIFF, "results.jsonl")
    rows = []
    if os.path.exists(results):
        with open(results) as handle:
            rows = [json.loads(line) for line in handle if line.strip()]
    done = {row["id"] for row in rows}
    todo = [c for c in cases if c["id"] not in done]
    if options.limit:
        todo = todo[:options.limit]
    print(f"{len(cases)} cases generated, {len(done)} done before, {len(todo)} to run", file=sys.stderr)
    stop_at = time.time() + options.minutes * 60
    with open(results, "a") as out, concurrent.futures.ThreadPoolExecutor(3) as pool:
        pending = iter(todo)
        running = set()
        while True:
            while len(running) < 6 and time.time() < stop_at:
                c = next(pending, None)
                if c is None:
                    break
                running.add(pool.submit(compare, c))
            if not running:
                break
            finished, running = concurrent.futures.wait(running, return_when=concurrent.futures.FIRST_COMPLETED)
            for future in finished:
                row = future.result()
                rows.append(row)
                out.write(json.dumps(row) + "\n")
                out.flush()
            if len(rows) % 100 < len(finished):
                print(f"{len(rows)} run", file=sys.stderr)
    equal, signatures = write_signatures(rows, os.path.join(DIFF, "signatures.txt"))
    print(f"{len(rows)} of {len(cases)} cases run, {equal} equal, {signatures} signatures", file=sys.stderr)


if __name__ == "__main__":
    main()
