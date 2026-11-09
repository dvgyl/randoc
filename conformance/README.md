# Conformance suite

The conformance suite compares Randoc with Pandoc 3.13 (the oracle).

## Case files

A case file is a TOML file in `conformance/cases/SUITE/NAME.toml`. One file
holds many cases. Keep related cases in one file.

```toml
# Defaults for each case in the file.
from = "markdown"          # the input format, with optional extension changes
to = ["html", "native"]    # the output formats
args = ["--wrap=none"]     # more options for each command

[[case]]
id = "emphasis"
input = '''
*text*
'''
```

Fields of a case:

| Field | Meaning |
| --- | --- |
| `id` | The case name. It is unique in the file and has no `#`. |
| `input` | The standard input, as text. |
| `input_file` | A file in `conformance/fixtures/` that is the standard input. Use it for binary input. |
| `from`, `to`, `args` | Replace the defaults of the file. |
| `cmd` | A full argument list. The case then has one unit and uses no `from` or `to`. |
| `files` | More files for the working directory: a table from file name to text. |
| `covers` | More capability tags, for example `["ext:smart", "opt:toc"]`. |

## Units

The harness makes these units from a case with `from` and `to`:

| Unit | Command | Passes when |
| --- | --- | --- |
| `CASE#read` | `pandoc -f FROM -t json` | The document tree is equal to the tree from the oracle. |
| `CASE#write:TO` | `pandoc -f json -t TO`, with the tree from the oracle as input | The output is equal. |
| `CASE#e2e:TO` | `pandoc -f FROM -t TO` | The output is equal. |

A case with `cmd` gives one unit, `CASE#cmd`.

The read unit tests a reader without a writer. The write unit tests a writer
without a reader. A unit also needs the same exit code and the same standard
error output as the oracle.

Text output must have the same bytes. A ZIP format (DOCX, ODT, EPUB, PPTX) must
have the same entries with the same content. The harness ignores the order,
the time stamps and the compression of the entries. The report counts
byte-identical and content-identical results separately.

The full identifier of a unit is `SUITE/NAME/CASE#UNIT`.

## Commands

Run these from the repository root. `record` and `verify-oracle` need the
oracle: set `RANDOC_ORACLE_PANDOC` to the path of the Pandoc 3.13 program.

```sh
# Run the oracle for each new unit and store the results in conformance/expected/.
cargo run -q -p randoc-harness -- record

# Compare Randoc with the stored results. Fail when a unit of the baseline does not pass.
cargo build -p randoc-cli
cargo run -q -p randoc-harness -- run --baseline spec/baseline.json

# Show one unit: the command, the input, the expected result and the Randoc result.
cargo run -q -p randoc-harness -- show 'json/roundtrip/numbers#write:json'

# Run only the units whose identifier contains a text.
cargo run -q -p randoc-harness -- run --filter markdown/
```

## Rules

- The stored results in `conformance/expected/` come from the oracle only.
  Do not edit them by hand. If a result from the oracle looks wrong, record it
  in `docs/ISSUES.md`.
- `spec/baseline.json` lists the units that pass on `main`. A change must not
  make one of them fail. Only the coordinator updates the baseline, after a merge.
- The harness runs each program with a fixed environment and a fixed clock
  (`SOURCE_DATE_EPOCH=1700000000`), under the program name `pandoc`.
