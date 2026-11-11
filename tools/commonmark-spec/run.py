#!/usr/bin/env python3
"""Runs the examples of the CommonMark specification with Randoc and with the oracle.

The specification has the licence CC-BY-SA-4.0, so its text does not go into
the repository. The program downloads `spec.txt` of CommonMark 0.31.2 from a
pinned commit, checks its SHA-256 hash, and keeps it outside the repository
($RANDOC_ENV/commonmark-spec/ by default). It runs `-f commonmark -t json` with
both programs on each example and writes only numbers and example numbers to
spec/commonmark-spec.json.

Usage: python3 tools/commonmark-spec/run.py [--spec-dir DIR] [--randoc BIN]
       [--oracle BIN] [--output FILE] [--timeout SECONDS] [--jobs N]
"""

import argparse
import concurrent.futures
import dataclasses
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import urllib.parse
import urllib.request
from pathlib import Path

SPEC_VERSION = "0.31.2"
SPEC_COMMIT = "9103e341a973013013bb1a80e13567007c5cef6f"
SPEC_URL = f"https://raw.githubusercontent.com/commonmark/commonmark-spec/{SPEC_COMMIT}/spec.txt"
SPEC_SHA256 = "257c41ad946f7a1414a499aca402a1aa8fdac3678532266611348c1cf54f4b80"
SPEC_FILE = "spec.txt"
FENCE = "`" * 32
MAX_DOWNLOAD_BYTES = 4_000_000
ARGS = ["-f", "commonmark", "-t", "json"]


@dataclasses.dataclass(frozen=True)
class Outcome:
    """The result of one program run."""

    exit: int
    stdout: bytes
    stderr: bytes
    timed_out: bool = False


def sha256_of(data):
    """Returns the SHA-256 hash of bytes as hexadecimal text."""
    return hashlib.sha256(data).hexdigest()


def check_hash(data, expected=SPEC_SHA256):
    """Fails with ValueError when the hash of `data` is not `expected`."""
    found = sha256_of(data)
    if found != expected:
        raise ValueError(f"SHA-256 is {found}, expected {expected}")


class HttpsOnlyRedirects(urllib.request.HTTPRedirectHandler):
    """Follows a redirect only to an https URL."""

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if urllib.parse.urlsplit(newurl).scheme != "https":
            raise ValueError(f"redirect to a non-https URL: {newurl}")
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def download(url=SPEC_URL, timeout=120):
    """Downloads `url` and returns its bytes. Stops after MAX_DOWNLOAD_BYTES."""
    opener = urllib.request.build_opener(HttpsOnlyRedirects)
    with opener.open(url, timeout=timeout) as response:
        data = response.read(MAX_DOWNLOAD_BYTES + 1)
    if len(data) > MAX_DOWNLOAD_BYTES:
        raise ValueError(f"more than {MAX_DOWNLOAD_BYTES} bytes")
    return data


def fetch(spec_dir, getter=download):
    """Returns the specification text. Downloads it when the file is missing or has a wrong hash."""
    path = Path(spec_dir) / SPEC_FILE
    if path.is_file():
        data = path.read_bytes()
        if sha256_of(data) == SPEC_SHA256:
            return data.decode("utf-8")
    data = getter()
    check_hash(data)
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as file:
        file.write(data)
    os.replace(file.name, path)
    return data.decode("utf-8")


def extract_examples(text):
    """Returns the examples as a list of (number, input) pairs.

    An example starts with a line of 32 backticks and the word `example`,
    has the input, a line with one `.`, the expected HTML, and a line of 32
    backticks. The character U+2192 stands for a tab in the input.
    """
    examples = []
    lines = text.split("\n")
    index = 0
    while index < len(lines):
        if lines[index].startswith(FENCE + " example"):
            body = []
            index += 1
            while index < len(lines) and lines[index] != ".":
                body.append(lines[index])
                index += 1
            while index < len(lines) and lines[index] != FENCE:
                index += 1
            source = "".join(line + "\n" for line in body).replace("→", "\t")
            examples.append((len(examples) + 1, source))
        index += 1
    return examples


def environment():
    """Returns the fixed environment of each run, as the conformance harness uses it."""
    return {
        "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
        "HOME": "/var/empty",
        "LANG": "en_US.UTF-8",
        "LC_ALL": "en_US.UTF-8",
        "TZ": "UTC",
        "SOURCE_DATE_EPOCH": "1700000000",
    }


def execute(binary, source, timeout):
    """Runs `binary` under the program name `pandoc` with the input `source`."""
    try:
        done = subprocess.run(
            ["pandoc", *ARGS], executable=str(binary), input=source.encode("utf-8"),
            capture_output=True, env=environment(), timeout=timeout)
    except subprocess.TimeoutExpired:
        return Outcome(-1, b"", b"", timed_out=True)
    except OSError as error:
        return Outcome(-1, b"", f"{binary}: {error}\n".encode())
    return Outcome(done.returncode, done.stdout, done.stderr)


def same_result(oracle, program):
    """Returns True when both runs have the same exit code, standard error and document tree."""
    if oracle.timed_out or program.timed_out:
        return False
    if oracle.exit != program.exit or oracle.stderr != program.stderr:
        return False
    try:
        return json.loads(oracle.stdout) == json.loads(program.stdout)
    except ValueError:
        return oracle.stdout == program.stdout


def compare(examples, oracle_bin, randoc_bin, timeout, jobs, run=execute):
    """Runs both programs on each example. Returns a summary without example text."""
    def one(example):
        number, source = example
        oracle = run(oracle_bin, source, timeout)
        program = run(randoc_bin, source, timeout)
        return number, oracle, program

    failing, timeouts, oracle_problems = [], [], []
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, jobs)) as pool:
        for number, oracle, program in pool.map(one, examples):
            if oracle.timed_out or oracle.exit < 0:
                oracle_problems.append(number)
            elif program.timed_out:
                timeouts.append(number)
            elif not same_result(oracle, program):
                failing.append(number)
    bad = len(failing) + len(timeouts) + len(oracle_problems)
    return {
        "examples": len(examples),
        "pass": len(examples) - bad,
        "fail": len(failing),
        "timeouts": len(timeouts),
        "oracle_problems": len(oracle_problems),
        "failing_examples": sorted(failing),
        "timeout_examples": sorted(timeouts),
        "oracle_problem_examples": sorted(oracle_problems),
    }


def file_hash(path):
    """Returns the SHA-256 hash of a file, or an empty text when it cannot be read."""
    try:
        return sha256_of(Path(path).read_bytes())
    except OSError:
        return ""


def main(argv=None):
    root = Path(__file__).resolve().parents[2]
    env_dir = os.environ.get("RANDOC_ENV", str(root.parent / "env"))
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--spec-dir", default=str(Path(env_dir) / "commonmark-spec"))
    parser.add_argument("--randoc", default=str(root / "target" / "debug" / "randoc"))
    parser.add_argument("--oracle", default=os.environ.get("RANDOC_ORACLE_PANDOC", "pandoc"))
    parser.add_argument("--output", default=str(root / "spec" / "commonmark-spec.json"))
    parser.add_argument("--timeout", type=float, default=20.0)
    parser.add_argument("--jobs", type=int, default=4)
    args = parser.parse_args(argv)

    text = fetch(args.spec_dir)
    examples = extract_examples(text)
    summary = compare(examples, args.oracle, args.randoc, args.timeout, args.jobs)
    report = {
        "suite": f"CommonMark specification {SPEC_VERSION}",
        "spec_url": SPEC_URL,
        "spec_sha256": SPEC_SHA256,
        "command": ARGS,
        "program_sha256": file_hash(args.randoc),
        "oracle_sha256": file_hash(args.oracle),
        "timeout_seconds": args.timeout,
        **summary,
    }
    Path(args.output).write_text(json.dumps(report, indent=1) + "\n", encoding="utf-8")
    print(f"commonmark-spec: {report['examples']} examples, {report['pass']} pass, "
          f"{report['fail']} fail, {report['timeouts']} timeouts, "
          f"{report['oracle_problems']} oracle problems")
    return 0 if report["examples"] > 0 else 1


if __name__ == "__main__":
    sys.exit(main())
