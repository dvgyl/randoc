#!/usr/bin/env python3
"""Writes the benchmark inputs in benchmarks/inputs/.

The script repeats one Markdown section 1, 10 and 100 times. The oracle
converts each Markdown file to the JSON document tree. Run it again after a
change to SECTION or to the oracle version:

    python3 tools/bench/make_inputs.py --oracle "$RANDOC_ORACLE_PANDOC"
"""

import argparse
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
INPUTS = ROOT / "benchmarks" / "inputs"
SCALES = (1, 10, 100)

# {n} makes heading texts, note labels and reference labels unique in each copy.
SECTION = """\
# Part {n}: the pump station

The station moves water from the *lower tank* to the **upper tank**. Read the
[operating limits][limits-{n}] before you start a pump. Each pump has a
`status` word and a counter.[^count-{n}]

## Start procedure {n}

1. Open the inlet valve.
2. Set the selector to `AUTO`.
   - Make sure that the green lamp comes on.
   - If the red lamp comes on, stop and read the fault list.
3. Push the start button.

## Equipment list {n}

| Item | Part number | Quantity | Note |
|:-----|------------:|:--------:|------|
| Pump | P-100-{n} | 2 | Duty and standby |
| Valve | V-220 | 6 | Ball type |
| Sensor | S-31 | 4 | Level, 4 to 20 mA |

Term {n}
:   A word that the station manual uses with one meaning.

> Do not run a pump with the outlet valve closed. The pump becomes hot and
> the seal can fail.

```python
def flow(level, area):
    \"\"\"Return the flow in cubic metres for each hour.\"\"\"
    return level * area * 3600
```

See <https://example.com/station/{n}> for the full manual, and the
[parts catalogue](https://example.com/parts "Parts") for spare parts.

[limits-{n}]: https://example.com/limits/{n} "Operating limits"
[^count-{n}]: The counter shows the number of starts since the last service.

"""


def markdown(scale):
    """Returns the Markdown document with SECTION repeated `scale` times."""
    return "".join(SECTION.format(n=n) for n in range(1, scale + 1))


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--oracle", required=True, help="path of Pandoc 3.13")
    args = parser.parse_args()
    INPUTS.mkdir(parents=True, exist_ok=True)
    for scale in SCALES:
        text = markdown(scale)
        md_path = INPUTS / f"doc-x{scale}.md"
        md_path.write_text(text, encoding="utf-8")
        tree = subprocess.run(
            [args.oracle, "-f", "markdown", "-t", "json"],
            input=text.encode("utf-8"),
            capture_output=True,
            check=False,
        )
        if tree.returncode != 0 or tree.stderr:
            sys.stderr.write(tree.stderr.decode("utf-8", "replace"))
            sys.exit(f"make_inputs: the oracle failed for {md_path.name}")
        (INPUTS / f"doc-x{scale}.json").write_bytes(tree.stdout)
        print(f"wrote doc-x{scale}.md and doc-x{scale}.json")


if __name__ == "__main__":
    main()
