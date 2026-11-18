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
| `capture_files` | Capture work-folder changes. Use `{}` for byte comparison or `{ zip = ["result.docx"] }` to compare the entries of named ZIP outputs. A file can set this field as a default for its cases. |
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

Text output must have the same bytes. A ZIP format (DOCX, ODT, EPUB, PPTX,
chunked HTML) must
have the same entries with the same content. The harness ignores the order,
the time stamps and the compression of the entries. The report counts
byte-identical and content-identical results separately.

The full identifier of a unit is `SUITE/NAME/CASE#UNIT`.

For a case with `capture_files`, the harness starts each run in a separate
folder. It compares the files and directories that the run adds, changes,
or removes. This includes `.home`. The stored result uses SHA-256 for file
contents. The harness records a symbolic link as a link and hashes its target
name. When the oracle writes a ZIP with `mimetype` as its first entry and
leaves that entry uncompressed, Randoc must do the same.

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

# Run only the units that capture work-folder changes.
cargo run -q -p randoc-harness -- run --files
```

The `files` measurement suite also checks media extraction from DOCX, ODT,
EPUB and PPTX. It uses the oracle to make those inputs from
`conformance/fixtures/files/media-sample.md` and saves them in the evidence
folder for the run. Use `tools/autobuild/ops measure --suite files --filter
files/media/` to run those cases and inspect their reports.

## Rules

- The stored results in `conformance/expected/` come from the oracle only.
  Do not edit them by hand. If a result from the oracle looks wrong, record it
  in `docs/ISSUES.md`.
- `spec/baseline.json` lists the units that pass on `main`. A change must not
  make one of them fail. Only the coordinator updates the baseline, after a merge.
- The harness runs each program with a fixed environment and a fixed clock
  (`SOURCE_DATE_EPOCH=1700000000`), under the program name `pandoc`.
- A stored result belongs to one command line, one standard input and one set
  of files. When you change a case, its stored results are out of date. `run`
  reports such a unit as "not recorded", which is a failure. Run `record` again.
- A run that the harness stops (time limit) or cannot start is never a result.
  `record` refuses it, and `run` reports the unit as "no result".
- `run` without `--baseline` exits with 0 also when units fail. Use
  `--baseline spec/baseline.json` to get a failure exit code for a regression.
- `record` and `verify-oracle` start at most 2 oracle processes at the same
  time, because of the memory of the machine. `record` runs the oracle only
  for a unit with no current stored result, and writes only the expected
  files that change.
- `verify-oracle` keeps the digest of each stored result that the oracle
  confirmed in `target/harness/verified-oracle.json`. It runs the oracle again
  only for a unit whose digest changed: a different oracle version, input or
  stored result. `verify-oracle --all` runs the oracle for each unit.
- Each command of the harness uses one scratch folder for each worker thread
  under `$TMPDIR` and removes the scratch folder at its end, also after an
  error.
