# The Pandoc test suite

`tools/pandoc-testsuite/run.py` runs the test suite of Pandoc 3.13 against a
program that has the Pandoc command line. The program can be the oracle or
Randoc.

## Licence boundary

Pandoc and its tests are GPL. The tool reads the test files from a Pandoc
source tree when it runs. The repository holds no Pandoc test file, no test
input, no expected output and no table that copies them. The results name the
tests by identifier only.

## Usage

Run each command from the root of the worktree.

```sh
# All supported tests against Randoc.
cargo build -p randoc-cli
python3 tools/pandoc-testsuite/run.py --pandoc target/debug/randoc \
  --suite "$RANDOC_PANDOC_SRC/test" --out "$TMPDIR/randoc.json"

# A part of the suite, with the difference for each failure.
python3 tools/pandoc-testsuite/run.py --pandoc target/debug/randoc \
  --suite "$RANDOC_PANDOC_SRC/test" --filter command/emoji --show-failures

# Fail when a test of the baseline does not pass.
python3 tools/pandoc-testsuite/run.py --pandoc target/debug/randoc \
  --suite "$RANDOC_PANDOC_SRC/test" --baseline spec/pandoc-testsuite-baseline.json

# The unit tests of the tool.
python3 -B -m unittest discover -s tools/pandoc-testsuite
```

Other options:

- `--label NAME` stores the results under the key `NAME` of the `--out` file
  and keeps the other keys. `spec/pandoc-testsuite.json` has the keys
  `oracle` and `randoc`.
- `--write-baseline FILE` writes the identifiers of the passing tests.
- `--timeout SECONDS` sets the time limit for one test (default 60).
- `--jobs N` sets the number of threads (default 4).
- `--tables` prints the tables of this document.

The tool exits with 1 when a test of the baseline does not pass, and with 0
in all other cases.

## How the tool runs a test

The tool makes a temporary directory with a link `pandoc` to the program. It
runs each test in the `test/` directory of the source tree, as the Pandoc test
driver does. The environment is fixed:

| Variable | Value | Source |
|---|---|---|
| `PATH` | the link directory, then `/usr/bin:/bin:/usr/sbin:/sbin` | `Tests/Helpers.hs` puts the test program first |
| `TMP` | `.` | `Tests/Helpers.hs` |
| `HOME` | `./` | `Tests/Helpers.hs` |
| `LANG`, `LC_ALL` | `en_US.UTF-8` | the conformance harness |
| `TZ` | `UTC` | the conformance harness |

`SOURCE_DATE_EPOCH` is not set. With this variable, Pandoc adds a PDF trailer
ID to LaTeX output, and 10 command and golden tests then fail with the oracle.

The tool has three kinds of test:

1. **Command tests** (group `command`). Each fenced code block of
   `test/command/*.md` is one test. The identifier is `command/FILE#N`, where
   `N` counts the code blocks of the file from 1. The tool splits the block as
   `Tests/Command.hs` does: the `%` line (with `\` continuation lines) is a
   shell command, the lines up to `^D` are the input, and the lines after
   `^D` up to a `.` line are the expected text. `/bin/sh` runs the command.
   The actual text is each line of standard error with the prefix `2> `, then
   standard output, then `=> CODE` when the exit code is not 0.
   The tool finds the code blocks like the Pandoc Markdown reader: it skips
   HTML comments and keeps tabs. A check found the same blocks as the oracle
   (`pandoc --preserve-tabs -t json`) in all 1127 files.
2. **Golden file tests** (group `old`). The tool reads the `tests` list of
   `Tests/Old.hs` with a small evaluator for the Haskell expressions in it
   (`testGroup`, lists, `$`, `++`, `<.>`, `</>`, `map` with a lambda,
   `mconcat`) and the helper functions of that file (`test'`,
   `writerTests'`, `extWriterTests'`, `lhsWriterTests'`, `lhsReaderTest'`,
   `s5WriterTest'`, `fb2WriterTest'`). The identifier is the path of group
   names and the test name, for example `old/markdown/reader/basic`. A
   repeated name gets `#2`, `#3` and so on. The program must exit with 0, and
   its standard output must be equal to the expected file. The FB2 tests use
   the normalization of `fb2WriterTest`.
3. **Reader tests for file formats** (groups `readers/...`). The tool reads
   the `testCompare` entries of `Tests/Readers/Docx.hs`, `Pptx.hs` and
   `Xlsx.hs`, the name lists of `Tests/Readers/ODT.hs`, the `fb2Test` entries
   of `FB2.hs` and the `rtfTest` list of `RTF.hs`. The Haskell tests compare
   documents, not text. So for DOCX, PPTX, XLSX and ODT the tool converts the
   input and the expected file to Pandoc JSON with the same program and
   compares the two JSON values. A `def{readerExtensions = ...}` record sets
   the exact set of extensions: the tool asks the program for the defaults
   with `--list-extensions` and turns the others off. `readerTrackChanges`
   becomes `--track-changes`, and `readerStandalone=True` becomes
   `--standalone`. The FB2 and RTF tests compare the text of
   `pandoc -t native -s` with the expected file.

## Results

The numbers come from a run on 2026-11-09 with the oracle (Pandoc 3.13,
arm64) and the source tree of tag 3.13.

The suite has 2231 tests that the tool supports and an estimated 1917 tests
that it cannot run.

| Program | Run | Pass | Fail | Excluded |
|---|---:|---:|---:|---:|
| Oracle | 2229 | 2229 | 0 | 2 |
| Randoc | 2229 | 0 | 2229 | 2 |

Randoc passes no test yet, because its program reads only JSON (see I-002 in
`docs/ISSUES.md`). `spec/pandoc-testsuite.json` has the totals and the passing
identifiers for each program. `spec/pandoc-testsuite-baseline.json` has the
identifiers that pass with Randoc.

### Supported tests

| Group | Tests | Run | Pass | Excluded |
|---|---:|---:|---:|---:|
| `command` | 1884 | 1884 | 1884 | 0 |
| `old` | 174 | 172 | 172 | 2 |
| `readers/docx` | 105 | 105 | 105 | 0 |
| `readers/fb2` | 6 | 6 | 6 | 0 |
| `readers/odt` | 44 | 44 | 44 | 0 |
| `readers/pptx` | 1 | 1 | 1 | 0 |
| `readers/rtf` | 16 | 16 | 16 | 0 |
| `readers/xlsx` | 1 | 1 | 1 | 0 |

| Group | Tests | Reason |
|---|---:|---|
| `hs/ImageSize` | 62 | the tests call internal Haskell functions |
| `hs/MediaBag` | 4 | the tests call internal Haskell functions |
| `hs/Readers/Creole` | 51 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Readers/DokuWiki` | 75 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Readers/EPUB` | 6 | the tests inspect the media bag of the reader, which the command line does not show |
| `hs/Readers/HTML` | 30 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Readers/JATS` | 13 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Readers/Jira` | 37 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Readers/LaTeX` | 90 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Readers/Man` | 30 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Readers/Markdown` | 112 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Readers/Mdoc` | 103 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Readers/Muse` | 240 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Readers/Org/Block` | 25 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Readers/Org/Block/CodeBlock` | 16 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Readers/Org/Block/Figure` | 7 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Readers/Org/Block/Header` | 27 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Readers/Org/Block/List` | 34 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Readers/Org/Block/Table` | 17 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Readers/Org/Directive` | 42 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Readers/Org/Inline` | 75 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Readers/Org/Inline/Citation` | 12 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Readers/Org/Inline/Note` | 7 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Readers/Org/Inline/Smart` | 8 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Readers/Org/Meta` | 38 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Readers/Pod` | 37 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Readers/RST` | 40 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Readers/Txt2Tags` | 58 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Shared` | 15 | the tests call internal Haskell functions |
| `hs/Writers/AnnotatedTable` | 7 | QuickCheck properties of an internal Haskell module |
| `hs/Writers/AsciiDoc` | 12 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Writers/BBCode` | 75 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Writers/ConTeXt` | 10 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Writers/DocBook` | 28 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Writers/Docx` | 50 | the tests compare the XML parts of the output archive with a Haskell normalization |
| `hs/Writers/FB2` | 3 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Writers/HTML` | 19 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Writers/JATS` | 23 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Writers/Jira` | 22 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Writers/LaTeX` | 32 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Writers/Markdown` | 28 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Writers/Markua` | 6 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Writers/Ms` | 2 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Writers/Muse` | 155 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Writers/Native` | 2 | QuickCheck properties on random Haskell values |
| `hs/Writers/ODT` | 1 | the tests compare the XML parts of the output archive with a Haskell normalization |
| `hs/Writers/Org` | 8 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Writers/Plain` | 1 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Writers/Powerpoint` | 46 | the tests compare the XML parts of the output archive with a Haskell normalization |
| `hs/Writers/RST` | 21 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Writers/TEI` | 7 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/Writers/Txt2Tags` | 34 | the tests give Haskell values (Pandoc builders) that only Haskell can evaluate |
| `hs/XML` | 1 | the tests call internal Haskell functions |
| `readers/docx` | 11 | media bag and warning-list tests inspect reader state that the command line does not show |
| `readers/odt` | 2 | media bag tests inspect reader state that the command line does not show |

The second table gives an estimate for each group that the tool cannot run.
For a Haskell module, the tool counts each `=?>` pair and each line with a
call of a test constructor (`testCase`, `testProperty`, `goldenTest` and
others). The rows `readers/docx` and `readers/odt` count the media bag and
warning tests of modules that the tool runs in part.

### Excluded tests

`tools/pandoc-testsuite/exclusions.json` lists the tests that the tool does
not run, each with a reason.

| Test | Reason |
|---|---|
| `old/fb2/meta` | The expected file ends with an empty line. Pandoc 3.13 does not write it, and the rest of its output is equal. The test fails with the oracle. |
| `old/fb2/titles` | The same reason as `old/fb2/meta`. |

## Limits

- The Haskell reader and writer tests (`Tests/Readers/*.hs`,
  `Tests/Writers/*.hs`) give the input or the expected result as Pandoc
  builder expressions, for example `para (emph "x")`. Only Haskell can
  evaluate them. No test of these modules is a plain pair of texts.
- The archive writer tests (DOCX, ODT, PPTX) compare the XML parts of the
  output with a normalization in Haskell. The tool does not run them.
- The command tests run in the source tree. None of them writes a file
  there. After a full run, no file in the tree had changed.
