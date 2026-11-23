"""Read the math bodies from the Pandoc 3.13 writer output."""

import io
import os
import re
import subprocess
import sys
import zipfile
from pathlib import Path


def convert(target, cases, delimiter):
    markdown = "\n\n".join(f"{delimiter}{tex}{delimiter}" for tex in cases)
    result = subprocess.run(
        [os.environ["RANDOC_ORACLE_PANDOC"], "-f", "markdown", "-t", target, "--wrap=none"],
        input=(markdown + "\n").encode(),
        capture_output=True,
        check=True,
    )
    if result.stderr:
        raise ValueError(result.stderr.decode())
    if target == "docx":
        with zipfile.ZipFile(io.BytesIO(result.stdout)) as archive:
            document = archive.read("word/document.xml").decode()
        return re.findall(r"<m:oMath>.*?</m:oMath>|<m:oMath />", document, re.S)

    output = result.stdout.decode()
    if target == "typst":
        inline = delimiter == "$"
        prefix, suffix = ("$", "$") if inline else ("$ ", " $")
        blocks = output.removesuffix("\n").split("\n\n")
        if any(not block.startswith(prefix) or not block.endswith(suffix) for block in blocks):
            raise ValueError(f"Unexpected Typst output: {output!r}")
        return [block[len(prefix):-len(suffix)] for block in blocks]

    if delimiter == "$":
        blocks = re.split(r"^\.(?:LP|PP)\n", output, flags=re.M)[1:]
        if any(not block.startswith("@") or not block.endswith("@\n") for block in blocks):
            raise ValueError(f"Unexpected inline eqn output: {output!r}")
        return [block[1:-2] for block in blocks]
    return [
        body.removesuffix("\n")
        for body in re.findall(r"^\.EQ\n(.*?)^\.EN$", output, re.M | re.S)
        if body != "delim @@\n"
    ]


target = sys.argv[1]
cases = Path(__file__).with_name("wave3.tex").read_text(encoding="utf-8").removesuffix("\n").split("\n")
version = subprocess.check_output([os.environ["RANDOC_ORACLE_PANDOC"], "--version"])
if version.splitlines()[0] != b"pandoc 3.13":
    raise ValueError(f"Expected Pandoc 3.13, got {version.splitlines()[0]!r}")
for delimiter in ("$", "$$"):
    bodies = convert(target, cases, delimiter)
    if len(bodies) != len(cases):
        raise ValueError(f"Expected {len(cases)} math bodies, got {len(bodies)} for {target}")
    for body in bodies:
        sys.stdout.buffer.write(body.encode() + b"\0")
