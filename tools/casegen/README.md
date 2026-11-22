# Case generator

`casegen.py` makes conformance cases for Randoc from a seed. The same seed
always gives the same cases, byte for byte. Each case is one run of
`pandoc ARGS` with an input file. Compare Randoc with the Pandoc 3.13 oracle
on each case: the exit code, the standard output and the standard error must
be equal.

The tool uses Python 3.11 or later and only the standard library.

## Seeds

The visible seeds are 1 to 1000. Use only these seeds. Do not tune Randoc to
the cases of one seed: the cases of each seed must pass.

## Usage

Run these commands from the root of the repository, after `. env/env.sh`.

```sh
# 250 cases into a new or empty folder.
python3 tools/casegen/casegen.py --seed 17 --count 250 --out /path/to/cases

# The same cases in one tar archive (better on the exFAT drive, where each
# file uses at least 1 MB).
python3 tools/casegen/casegen.py --seed 17 --count 250 --out /path/to/cases-17.tar \
  --limit-run "$RANDOC_ENV/bin/limit-run"

# Only some families (json-roundtrip needs the oracle, the others do not).
python3 tools/casegen/casegen.py --seed 17 --count 100 --out /path/to/x --families ext-matrix,options-cli

# The unit tests.
python3 -m unittest discover -s tools/casegen
```

Options:

| Option | Meaning |
| --- | --- |
| `--seed N` | The seed, 0 to 2^64 - 1. |
| `--count K` | The number of cases. The first K cases of a seed do not depend on K. |
| `--out PATH` | A new or empty folder, or a file name that ends in `.tar`. |
| `--families LIST` | A comma-separated subset of `ext-matrix`, `json-roundtrip`, `options-cli`. Default: all. |
| `--extensions PATH` | The extension inventory. Default: `spec/inventory/extensions.json`. |
| `--oracle PATH` | Pandoc 3.13, for `json-roundtrip`. Default: `$RANDOC_ORACLE_PANDOC`. |
| `--limit-run PATH` | Run each oracle call as `limit-run 3000 60 ORACLE ...`. |

## Case files

For case `0007`, the tool writes the input file (for example `0007.md`) and
`0007.case.json`:

```json
{
 "args": ["-f", "markdown+mark-smart", "-t", "rst", "--columns=40", "0007.md"],
 "family": "ext-matrix",
 "generator": "casegen/1",
 "id": "0007",
 "input": "0007.md"
}
```

Run each case in the folder of its input file, under the program name
`pandoc`, with no standard input and with the fixed environment of the
conformance harness (`PATH=/usr/bin:/bin:/usr/sbin:/sbin`, `HOME=/var/empty`,
`LANG=en_US.UTF-8`, `LC_ALL=en_US.UTF-8`, `TZ=UTC`,
`SOURCE_DATE_EPOCH=1700000000`). Compare the `json` output by JSON
equality and all other outputs by bytes, as `tools/corpus/run.py` does.

`casegen.read_cases(PATH)` reads the cases of a folder or a tar archive.

## Families

| Family | What a case tests |
| --- | --- |
| `ext-matrix` | A synthesized document in `markdown`, `commonmark`, `commonmark_x`, `gfm`, `html`, `latex`, `rst` or `org`; 0 to 3 extension toggles that the reader supports; one text writer, sometimes with 1 or 2 writer extension toggles; 0 to 4 options from a safe list (`--wrap`, `--columns`, `--toc`, `-s`, `--number-sections`, `--shift-heading-level-by`, `--top-level-division`, `--markdown-headings`, `--reference-links`, `--tab-stop`, `--preserve-tabs`, `--ascii`, `--id-prefix`, `--section-divs`, `--eol` and others). |
| `json-roundtrip` | One case reads a synthesized document to `json`. Three more cases give the oracle's JSON of that document to three writers (`-f json`). Field `source` names the reading case. When the oracle fails on the document, only the reading case is written. |
| `options-cli` | Errors, warnings and exit codes: unknown formats, extension names that do not exist or that the format does not support, bad option values, unknown options, missing option arguments, missing files, binary readers on text input, deprecated names, repeated options, `--list-*` options. |

An extension toggle flips the default state of an extension that the format
supports, as `pandoc --list-extensions=FORMAT` lists it (stored in
`spec/inventory/extensions.json`).

## Rules for changes

- `bank.py` holds the snippet bank. Write each snippet yourself. Never copy
  text from the Pandoc test suite, the Pandoc manual or other GPL sources.
- The random source is SplitMix64 (in `casegen.py`), not the `random`
  module, so the cases do not change with the Python version.
- Each change of the output changes the visible cases. Then raise
  `GENERATOR` in `casegen.py` and update the hash in
  `test_golden_output_of_a_visible_seed`.
