# Specification: compatible with Pandoc 3.13

This file defines "100% compatible with Pandoc 3.13" for the branch
`gpt/build`, and each scoreboard metric. Paths are relative to the clone
`/Volumes/Extreme Pro/Randoc/gpt/randoc`. `$RANDOC_GPT` is
`/Volumes/Extreme Pro/Randoc/gpt`.

## 0. Terms

- **Oracle**: the program of section 1. Its behaviour defines "correct".
- **Randoc**: the program of this repository, run with the name `pandoc`.
- **Result**: what one run of a program gives (3.2).
- **Unit**: one comparison of a Randoc result with the oracle result.
- **Metric**: PASS/TOTAL with a key. PASS: passing units. TOTAL: 3.11. A
  row of `plan/scoreboard/*.tsv` holds one metric of one run.
- **Stable**, **unstable**, **oracle-limit** unit: 3.10.
- **Normalization**: a fixed text change that a tool applies to both
  results before it compares them (3.12).
- **Pivot set P**: 400 document trees (4.2).
- **Own-written**: made by the team or a team tool, never copied from Pandoc
  tests, the Pandoc manual or other copyleft text.
- **Data pack**: `$RANDOC_GPT/datapack`, the data files that may not enter
  the repository (`plan/ARCHITECTURE.md`, 4.7).
- **Held-out**: data that only the evaluator reads (section 6).
- **Lead model**, **worker**, **reviewer**: roles of `AGENTS.md`.
  **Evaluator**: the held-out program outside the build sandbox.

## 1. The oracle

| Field | Value |
| --- | --- |
| Program | `/Volumes/Extreme Pro/Randoc/env/oracle/pandoc-3.13-arm64/bin/pandoc` (`$RANDOC_ORACLE_PANDOC`) |
| Version | `pandoc 3.13`, `Features: +server +lua`, Lua 5.4.8, document API 1.23.1.2 |
| SHA-256 | `8f17378a70e46eed7b63b5433fff6f5a0802f92659aa5782418548e0ca4a8e44` |
| Release file | `pandoc-3.13-arm64-macOS.zip`, SHA-256 `f436d18bebbfb3ddd326597686dacab9fac5dc23bd060495020f35a28a43a7c3` |
| Source tree | `/Volumes/Extreme Pro/Randoc/env/pandoc-src`, tag 3.13, commit `30b135468435dc4fbccfc2694508fa8e3e9ddc33` |
| Platform | macOS 27.0.1 (Darwin 27.0.0), Apple silicon, arm64 (Mach-O 64-bit) |
| Heap limit | `GHCRTS=-M3g` (GHC run-time option: largest heap 3 GB) |

1. Each tool checks the SHA-256 before a run. On a mismatch it exits 2 and
   writes no row. Nobody replaces or updates the oracle.
2. Each oracle run gets `GHCRTS=-M3g`. Exit 251 with `Heap exhausted` is
   not an oracle result (3.9).
3. Randoc never calls the oracle. No Randoc crate starts it or a program
   named `pandoc`, at build or run time, or reads an oracle result. Only
   `crates/randoc-harness`, `tools/` and tests run it. A gate step checks
   this. A source file made once from oracle output (for example
   `crates/randoc-core/src/extensions_data.rs`) is permitted when section 8
   permits its content.

## 2. Drop-in compatible

A user puts Randoc on `PATH` with the name `pandoc`, and each property below
is equal to the oracle's under section 3. The list keeps the full scope of
`docs/ORIGINAL_SPEC.md`. The right column names the metrics that measure it.

| # | Property | Metrics |
| --- | --- | --- |
| 1 | Program names `pandoc`, `pandoc-lua` (`lua` mode), `pandoc-server` and `pandoc-server.cgi` (`server` mode) | `harness.cmd`, `suite.pandoc.lua`, `server.units` |
| 2 | Command line: the 109 options, short forms, aliases, argument forms, prefixes of long options, repeats, errors | `caps.options`, `harness.cmd`, `casegen.options-cli` |
| 3 | Defaults: each value that applies when the user gives none | `caps.*`, `fmt.*` |
| 4 | The 52 readers | `fmt.read.F`, `caps.readers`, `corpus.*`, `suite.pandoc*` |
| 5 | The 77 writers | `fmt.write.F`, `caps.writers`, `zip.units`, `pdf.units` |
| 6 | The 1,719 (direction, format, extension) pairs | `caps.extensions`, `casegen.ext-matrix` |
| 7 | Output bytes; ZIP content; PDF engine input and content | all |
| 8 | Exit codes: 0 and the 31 codes of the manual's table | `errors.units`, all |
| 9 | Standard error: warnings, errors, `--verbose`, `--quiet`, `--fail-if-warnings`, `--trace` | all |
| 10 | Written files: `-o`, `--extract-media`, `--log`, each other file | `files.units`, `errors.units` |
| 11 | Input and resources: standard input, URLs, `--resource-path`, `--embed-resources`, `data:` URIs, fetch options (local HTTP server) | `files.units`, `caps.options` |
| 12 | JSON document tree and `native` | `ast.roundtrip`, `fmt.*` |
| 13 | JSON filters: interpreters, arguments, environment, data folder, errors, order | `caps.options` (`filter`) |
| 14 | Lua filters, the `pandoc` module and its globals, custom readers and writers | `lua.*`, `custom.*`, `suite.pandoc.lua` |
| 15 | The `lua` mode and its options `-e`, `-l`, `-i`, `-v`, `-E`, `-W`, `-`, `--` | `suite.pandoc.lua`, `harness.cmd` (`conformance/cases/cli/lua-mode.toml`) |
| 16 | The `server` mode, also as CGI | `server.units` |
| 17 | Templates, `-D`, partials, variables | `templates.units` |
| 18 | Defaults files | `defaults.units` |
| 19 | User data folder: `--data-dir`, the default folders, each data file kind | `caps.options` (`data-dir`) |
| 20 | Reference documents | `zip.units` |
| 21 | Citations, CSL styles, bibliography formats, `--natbib`, `--biblatex` | `citeproc.units`, `caps.options` |
| 22 | Highlighting: 194 languages, 8 styles, theme and syntax files | `fmt.write.F` (P part 2), `caps.options` |
| 23 | Math: TeX to MathML, Word equations, `eqn`, Typst, text; math in readers | `fmt.*` (P part 3), `caps.options` |
| 24 | PDF through the 15 engines that the oracle accepts | `pdf.units` |
| 25 | `--version` (rule 3.8) | `harness.cmd` |
| 26 | Rust library, also as WebAssembly | `lib.units`, `wasm.units` |
| 27 | No crash, hang or unbounded memory | `safety.units` |
| 28 | Not slower and not larger than the oracle | `perf.*` |
| 29 | Quarto and R Markdown | `downstream.*` |

The oracle's platform defines platform-dependent behaviour.

## 3. Comparison rules

### 3.1 Run environment

Both programs get the same environment. The tool clears it and sets only
`PATH=/usr/bin:/bin:/usr/sbin:/sbin` (the PDF stand-in folder first for
`pdf.units`), `HOME=<work>/.home`, `LANG` and `LC_ALL` `en_US.UTF-8`,
`TZ=UTC`, `SOURCE_DATE_EPOCH=1700000000`, `GHCRTS=-M3g` and
`RANDOC_DATA_DIR=$RANDOC_GPT/datapack`. Each program ignores the variable of
the other. A variable that a unit adds goes to both. Argument 0 is `pandoc`
or the name that the unit tests. Each run starts in a new empty work folder
with the unit's input files. The harness lacks `GHCRTS` and
`RANDOC_DATA_DIR` today (`crates/randoc-harness/src/exec.rs:137-146`).

### 3.2 Parts of a result

1. Exit code (an integer).
2. Standard output (bytes).
3. Standard error (bytes). The order between the two pipes is not compared.
4. Written files: each file or folder that the run creates, changes or
   removes in the work folder (`.home` included): path and bytes.
5. For `server.units`: HTTP status, `Content-Type`, body bytes.

A unit passes when each part is equal under 3.3 to 3.8.

### 3.3 Text: bytes

Each output that 3.4 to 3.6 do not name is compared by bytes, with no change
of line ends, white space, encoding or order.

### 3.4 JSON value equality

Used only by: harness `read` units (`crates/randoc-harness/src/compare.rs:87-93`),
the `readers` tests of Pandoc's suite (`tools/pandoc-testsuite/suite.py:117`),
the corpus target `json` (`tools/corpus/run.py:203`), the CommonMark
specification tool, the evaluator, casegen and `fmt.read.F`. Why: these units
measure the tree that a reader makes. `ast.roundtrip` measures the JSON
writer by bytes on each oracle tree of these units, and an equal tree with a
byte-equal writer gives equal bytes.

Rule: both outputs parse as JSON with equal values. Key order does not
count; array order counts; `1` is not `1.0`; invalid JSON or a duplicate key
is not equal. Evidence field `value_only`: units equal by value and not by
bytes. No other unit uses this rule.

### 3.5 ZIP content

For the output of `docx`, `odt`, `epub`, `epub2`, `epub3`, `pptx`,
`chunkedhtml`, also as a written file. Both archives are valid, with no
duplicate name, at most 512 MB and 100,000 entries. Equal: the same set of
entry names and the same bytes in each entry, after N1 only (no XML
canonicalization). When the oracle's first entry is `mimetype` with no
compression, so is Randoc's.

### 3.6 PDF

1. Engine input: a recording stand-in replaces each engine. Equal: engine
   name, arguments, number of calls, bytes of each file in the engine's
   working folder, standard input, and the PDF file that the program writes.
2. Content: for each engine installed on the machine, equal PDF bytes after
   N6.

### 3.7 Standard error and exit codes

Standard error: bytes after N2 and N3 only. Exit codes: equal integers.

### 3.8 The `--version` rule

With the name `pandoc`, the output of `--version` and `-v` equals the
oracle's with one line after line 1: `Randoc VERSION (compatible with Pandoc
3.13)` (VERSION: the `randoc` crate version). This is the one required
difference (`docs/ORIGINAL_SPEC.md`). All other lines, and `pandoc lua -v`
and `pandoc server --version`, are byte-equal.

### 3.9 Limits and no result

- Each run: 60 s CPU time, 600 s wall time, 3,000 MB for its process group,
  for both programs. A tool may give a unit larger limits, for both.
- The oracle gives no result (exit 251 with `Heap exhausted`, a limit, or a
  signal): an oracle-limit unit.
- Randoc gives no result: the unit fails. The tool runs it once more alone
  after the main run; the second result decides. Evidence field
  `no_result`. (Today the tools use wall limits of 20 s to 60 s; M1-01.)

### 3.10 Stable units

When a tool records the oracle result of a unit:

1. Run the oracle two times (R1, R2), each in a new work folder with another
   absolute path. R2 starts at least 1 s after R1.
2. R1 equals R2 under the unit's pass rule: stable. Store R1.
3. Else apply the rows of `spec/normalizations.toml` whose scope covers the
   unit to R1 and R2. Equal now: stable under those rows. Store R1 and the
   row ids; each comparison with Randoc applies the same rows to both.
4. Else: unstable. Write the unit id, the parts that differ and the reason
   in `spec/unstable.json`.
5. R1 or R2 with no result (3.9): oracle-limit; record it in the same file.
6. Once in each milestone, run the oracle again for each stored result (as
   `randoc-harness verify-oracle --all`). A difference makes the unit
   unstable.

Only oracle runs decide the class, never a Randoc result. A unit whose input
depends on time or chance gets a corrected input, not an exclusion.

### 3.11 How excluded units count

- Each unit of an input set is in one class: pass, fail (no result
  included), unstable, oracle-limit, or no input (a harness write unit whose
  oracle read unit exits with a code that is not 0). If the classes do not
  add up to the input set, the tool exits 2 and writes no row.
- PASS = pass. TOTAL = pass + fail. The other classes are never in PASS or
  TOTAL.
- They are reported apart, never silently dropped: evidence fields
  `unstable`, `oracle_limit`, `no_input`; `ops measure` prints their counts
  with each row; `spec/unstable.json` keeps them with reasons.
- A move into an excluded class lowers TOTAL: a tool correction (3.13).
- For an unstable unit the tool compares the parts where R1 and R2 agree.
  Evidence field `unstable_parts_ok`: unstable units with all such parts
  equal.

### 3.12 Allowed normalizations (closed list)

- **N1 ZIP container**: entry order, header times and dates, extra fields,
  compression, version fields, flags, attributes, comments, folder entries
  (`docs/ORIGINAL_SPEC.md`: "ignore timestamps and file order").
- **N2 Work folder**: its absolute path (also the `/private` form) becomes
  `<WORK>`.
- **N3 Temporary names**: the random part of a temporary name that the
  program makes becomes `<TMP>`; the row names the fixed part.
- **N4 Clock time**: a date or time that the program takes from the clock,
  not from `SOURCE_DATE_EPOCH` or the input, becomes `<TIME>` (for example
  time stamps inside ZIP entries); the row names pattern and scope.
- **N5 Random identifiers**: a UUID or other random token that the program
  makes becomes `<ID1>`, `<ID2>`, ... in order of first appearance; the row
  names the scope (for example EPUB with no `identifier`).
- **N6 PDF metadata**: in a real-engine PDF, each byte of the values of
  `/CreationDate`, `/ModDate`, trailer `/ID`, `xmp:CreateDate`,
  `xmp:ModifyDate`, `xmp:MetadataDate`, `xmpMM:DocumentID`,
  `xmpMM:InstanceID` becomes `0`.
- **Suite rules**: `suite.pandoc.command` and `.old` compare with Pandoc's
  expected text under the rules of Pandoc's own test runner (carriage
  returns removed; FB2 through `fb2_normalize`,
  `tools/pandoc-testsuite/suite.py:229`). The oracle passes 2,231 of 2,231.

N2 to N6 apply only through rows of `spec/normalizations.toml` (id, kind,
scope, pattern, replacement, log entry). N1 applies to each ZIP comparison.

### 3.13 New normalizations and tool corrections

- A new row is of kind N2 to N6, applies to both programs, and is valid
  only when 3.10 shows the replaced text differing between two oracle runs
  of a unit in its scope. Before first use: a `decision` entry in
  `plan/REVIEW_LOG.md` (rule, oracle evidence, units changed). Only the
  owner adds a new kind.
- A tool correction corrects how a tool forms or counts units (duplicate,
  no input, unstable, wrong count). If it lowers a TOTAL, it needs a
  `decision` entry (old and new TOTAL, reason, removed units) and a review
  round with verdict `approve` in `plan/REVIEWS.tsv`. The gate refuses a
  lower TOTAL without both.

## 4. Shared definitions

### 4.1 Scoreboard rules

- A metric whose measurement does not exist yet has the baseline 0/0. Its
  first row with TOTAL above 0 is the gain of the item that builds it.
- A metric with TOTAL 0 is not at TOTAL/TOTAL.
- Only a full measurement (`ops measure`, no `--quick`, no filter, clean
  tree) counts for a milestone gate and the final goal.

### 4.2 Pivot set P

400 JSON trees from an own generator (`tools/pivot/`, M1-04) with fixed
seeds. The repository stores the generator and the SHA-256 of the
list (`spec/pivot.sha256`). Parts, in order:

1. Forms (about 75): a small tree for each constructor of Block (14),
   Inline (20), MetaValue (6), and each value of ListNumberStyle,
   ListNumberDelim, Alignment, QuoteType, MathType, CitationMode; 8 table
   forms (bodies, row heads, row and column spans, head and foot, short
   caption, ColWidth forms, empty); 4 attribute forms.
2. Highlighting (194): a code block for each language of
   `spec/inventory/highlight-languages.json`, with an own sample of 15 lines
   or more.
3. Math (60): 10 own TeX expressions each, inline and display, that use
   each command of the KaTeX function list, the environments `matrix`,
   `pmatrix`, `bmatrix`, `cases`, `aligned`, `array`, `split`, and 20
   expressions that a converter rejects.
4. Random trees from seeds 1, 2, 3, ...: depth 6 or less, 200 nodes or
   less, text from a fixed character table (ASCII, accented, CJK, emoji,
   combining marks, markup characters), until P has 400 trees.

A tree enters P only when the oracle's `-f json -t json` exits 0; else the
next seed. A change of P is a tool correction.

### 4.3 Sensitive probe

A probe of a feature is sensitive when the oracle result changes with the
feature (on and off, or value A and B). Only sensitive probes test it.

### 4.4 External expected results

An oracle result that embeds a data pack file or other copyleft text
(section 8) stays in the oracle cache `$RANDOC_GPT/cache/SUITE/` (key: the
SHA-256 of oracle hash, arguments, input, environment). The repository holds
the unit id and the result's SHA-256 only. A tool remakes a missing entry
with 3.10. Generated units (casegen, P, stress) use the same cache.

## 5. Metrics

Defaults: the pass rule is section 3; TOTAL is the input set minus the
excluded classes (3.11); each tool is in `tools/autobuild/metrics.toml`.
"Tool: M1-nn": item M1-nn builds it; until then no measurement exists, which
is its weak point.

### `harness.units`
Unit: one harness unit: `CASE#read` (`-f FROM -t json`, by 3.4),
`#write:TO` (`-f json -t TO` on the stored oracle tree), `#e2e:TO`, `#cmd`.
Inputs: `conformance/cases/` (131 files, 12 suites). Tool: existing
(`randoc-harness run`). Weak points: no written-file check; 0 ZIP writer
units; no PDF engine on `PATH`; no oracle heap limit; wall limits fail
correct code under load; no unstable class; three generated suites hold
68.5% of passing units.

### `harness.read`
As `harness.units`, `read` units only.

### `harness.write`
As `harness.units`, `write` units only.

### `harness.e2e`
As `harness.units`, `e2e` units only.

### `harness.cmd`
As `harness.units`, `cmd` units only.

### `suite.pandoc`
Unit: one supported test of Pandoc's suite: `command/FILE.md#N`, `old/...`,
`readers/...`, from `env/pandoc-src/test` at run time. Tool: existing
(`tools/pandoc-testsuite/run.py`). Pass: command tests: the text (stderr lines
with `2> `, stdout, `=> CODE` when not 0) equals the expected text (suite
rules, 3.12); old tests: exit 0 and stdout equals the expected file (suite
rules); readers tests: Randoc's `-f R -t json` equals the oracle's reading
of the expected file (3.4). If the oracle fails a test, its result becomes
the reference (`decision` entry), and the test stays. TOTAL: 2,231 today
(command 1,884, old 174, readers 173). Weak points: `--write-baseline` runs
before the `--baseline` check (`run.py:255-259`); no heap limit.

### `suite.pandoc.command`
As `suite.pandoc`, group `command` only.

### `suite.pandoc.old`
As `suite.pandoc`, group `old` only.

### `suite.pandoc.readers`
As `suite.pandoc`, groups `readers/*` only.

### `suite.pandoc.inline`
Unit: one test of the Haskell-only groups (`docs/PANDOC_TESTSUITE.md`,
second table, about 1,939). The tool reads the modules at run time and
extracts each test's input (string or file), format and options, mapped to
command-line options. A builder expression becomes a JSON tree through an
own evaluator of the `pandoc-types` builders (BSD-3-Clause). A test of an
internal function gets a command with the same observable effect (image
size through `--dpi -t latex`, media through `--extract-media`). Command:
the mapped command, both programs. TOTAL: extracted inputs. Evidence field
`not_extracted`: each test that the tool cannot map, with the reason. Tool:
M1-17.

### `suite.pandoc.lua`
Unit: one test case: each `test_case` of
`pandoc-lua-engine/test/lua/module/*.lua`, run by `pandoc lua` with an own
stand-in of the `tasty` Lua module (one output line per case: path, name,
pass or error text); and each filter of `pandoc-lua-engine/test/lua/*.lua`
with its input from `Tests/Lua.hs` (extracted as in `suite.pandoc.inline`).
TOTAL: test cases. Tool: M1-16.

### `corpus.units`
Unit: (document, target) for `json` (3.4), `html`, `markdown`. Inputs:
`tools/corpus/manifest.toml` (230 documents, 33 formats) in
`$RANDOC_CORPUS`. Command: `-f FORMAT -t TARGET FILE`. A document that the
oracle rejects stays in. Tool: existing (`tools/corpus/run.py`). Weak
points: no baseline check (`run.py:415`); cache in `env/corpus`; no heap
limit; 19 input formats have no document. Held-out: `holdout.corpus`.

### `corpus.json`
As `corpus.units`, target `json` only.

### `corpus.html`
As `corpus.units`, target `html` only.

### `corpus.markdown`
As `corpus.units`, target `markdown` only.

### `casegen.visible`
Unit: one generated case of `tools/casegen/casegen.py --seed S --count 2`
for S = 1 to 1000 (2,000 cases; input file and JSON argument list). Pass:
section 3, `-t json` by 3.4 as the evaluator does. Tool: M1-26. Held-out:
`holdout.generated`.

### `casegen.ext-matrix`
As `casegen.visible`, family `ext-matrix` only.

### `casegen.json-roundtrip`
As `casegen.visible`, family `json-roundtrip` only.

### `casegen.options-cli`
As `casegen.visible`, family `options-cli` only.

### `holdout.corpus`
Unit: (held-out document, target) for `json`, `html`, `markdown`, `latex`,
`rst`, `plain`. Pass: equal exit code, stdout and stderr; JSON by 3.4. PASS
and TOTAL: the newest evaluator results file (`ops holdout results`). Tool:
existing, outside the build. Weak point: the evaluator drops unstable units
when it records oracle results; its results must give their count, else the
lead writes a BLOCKED ON OWNER entry.

### `holdout.generated`
As `holdout.corpus`, for the held-out generated cases (about 2,000, secret
seeds, pinned generator).

### `caps.readers`
Unit: an input format F of `spec/inventory/input-formats.json`. Pass:
`fmt.read.F` is at TOTAL/TOTAL and each harness unit with `from` F passes.
TOTAL: 52. Tool: existing matrix, new rule in M1-05. Weak point of today's
rule: one unit can make a format "passing" (`html5` and `markdown_github`).

### `caps.writers`
Unit: an output format F of `spec/inventory/output-formats.json`. Pass:
`fmt.write.F` is at TOTAL/TOTAL and each harness unit with `to` F passes;
`pdf` passes when `pdf.units` is at TOTAL/TOTAL. TOTAL: 77. Tool: as
`caps.readers` (M1-04).

### `caps.extensions`
Unit: a (direction, format F, extension e) pair of the field `supported` of
`spec/inventory/extensions.json` (771 reader, 948 writer pairs). Inputs: own
probes for each extension. Command: reader pair `-f F+e -t json`, `-f F-e`,
`-f F`; writer pair `-f json -t F+e`, `-t F-e`, `-t F`. Pass: a sensitive
probe exists and each sensitive probe passes in the three settings. A pair
with 5 or more probes and none sensitive passes only with a `decision`
entry that names it and with each probe passing. TOTAL: 1,719. Tool: M1-06.
Weak point today: the matrix counts names over all formats
(`crates/randoc-harness/src/cases.rs:114-125`).

### `caps.options`
Unit: an option of `spec/inventory/options.json` (first long name; short
forms and aliases map to it). Inputs: a value set from the argument type
and documented values (named values, number limits, missing file, folder,
empty value). Also: `filter`: an own filter for each interpreter extension,
no extension, missing, exit 1, invalid JSON, `DATADIR/filters`; `data-dir`:
each data file kind, and no option with each default folder;
`print-default-data-file`: each name the oracle knows; highlighting: the 8
styles, a theme file, an own syntax file; math: P part 3; fetch: a local
HTTP server. Bases: markdown to `html`, `latex`, `docx`, `markdown`; json
to `native`; with needed companion options. A unit (option, value, base) is
kept when sensitive or when the oracle exit code is not 0. Pass: a kept unit
exists, each kept unit passes, each short form and alias has a passing unit.
TOTAL: 109; no option is "not applicable". Tool: M1-07. Weak point today:
20 options "pass" on 1 to 3 units.

### `fmt.write.F`
One key for each of the 76 output formats other than `pdf`. Unit: (t, F)
for t in P. Command: `-f json -t F` with t as input. TOTAL: 400. Tool:
M1-04.

### `fmt.read.F`
One key for each of the 52 input formats. Unit: one input of F. Inputs:
(a) if F is also an output format, the oracle's `-f json -t F` output of
each t in P where the oracle exits 0; (b) the distinct inputs of harness
cases with `from` F (any extension suffix); (c) the visible corpus
documents of F. A reader with no writer (`xlsx`, `bits`, `endnotexml`,
`ris`, `creole`, `tikiwiki`, `twiki`, `vimwiki`, `pod`, `mdoc`, `csv`,
`tsv`) gets 40 or more own inputs (`xlsx`: 10 or more open documents).
Command: `-f F -t json`. Pass: equal exit code and stderr, output by 3.4.
Tool: M1-05.

### `fmt.write`
PASS and TOTAL: the sums over all `fmt.write.F` rows of the same run.

### `fmt.read`
PASS and TOTAL: the sums over all `fmt.read.F` rows of the same run.

### `ast.roundtrip`
Unit: a tree t of T. T: P, plus each distinct (by SHA-256) oracle JSON
output of the units that 3.4 names, plus 10 own cases of a wrong
`pandoc-api-version` (missing, older, newer, wrong type, wrong length).
Command: `-f json -t json`; `-f json -t native`; `-f native -t json` on the
oracle's native output. Pass: the three results are equal by bytes. Tool:
M1-08.

### `lua.api`
Unit: a name of the `pandoc` Lua module, or a filter global (`FORMAT`,
`PANDOC_STATE`, ...). Inputs: the 468 names of
`crates/randoc-lua/MODULES.md`, plus names that an own enumeration finds in
the oracle only (TOTAL grows). Own probes for each name print each result
through one pure-Lua printer (sorted keys, numbers as `%.17g`). Command:
`-L probe.lua` on an empty document, and `pandoc lua probe.lua`. Pass:
equal kind of the name, and each probe passes in both modes. TOTAL: 468 or
more. Tool: M1-12.

### `lua.filters`
Unit: (filter, tree). Inputs: own filters for element type (14 Block, 20
Inline, `Meta`, `Pandoc`, `Blocks`, `Inlines`) x return (nil, same element
changed, new element, list of 2, empty list) x traversal (`typewise`,
`topdown`) x form (global functions, `return {filter}`, list of 2): 1,140
filters, each on the first 3 trees of P with the element. Command:
`-f json -t native -L FILTER`. TOTAL: 3,420. Tool: M1-13.

### `custom.readers`
Unit: (reader, input). Inputs: 10 own readers (`Reader`,
`ByteStringReader`, `lpeg` grammar, `pandoc.read` inside, `Extensions` with
`+e` and `-e`, an error), 20 own inputs each; `creole.lua` run from
`env/pandoc-src/data` at run time with 30 own inputs. Command:
`-f READER.lua -t json` (bytes). TOTAL: 230. Tool: M1-14.

### `custom.writers`
Unit: (writer, t in P). Inputs: 10 own writers (new form `Writer`, classic
form, `Template`, `Extensions`, `pandoc.layout`,
`pandoc.scaffolding.Writer`, `ByteStringWriter`, an error). Command:
`-f json -t WRITER.lua` (`-s` with `Template`). TOTAL: 4,000. Tool: M1-15.

### `templates.units`
Unit: (format, variable set, document), or a partial case. Inputs: formats
F where the oracle's `-D F` exits 0; the sorted variable names of that
template, read at run time (`$x$`, `${x}`, `$if(x)$`, `$for(x)$`; names
only stored); 15 sets: 1 none; 2 each name `v-NAME` by `-V`; 3 `true`, 4 a
list of 2 strings, 5 a map, by metadata file; 6 to 10: names with index k,
k mod 5 = set - 6, get ``<&>"'\ é 漢`` by `-V`; 11 to 15: k mod 5 = set -
11 get two Markdown paragraphs by metadata file; documents: empty and P's
first tree; 20 own templates with partials, to `html`, `latex`, `markdown`.
Command: `-s -t F`. TOTAL: formats x 30 + 60. Tool: M1-09.

### `citeproc.units`
Unit: (style, bibliography, document, output). Inputs: 11 styles: the
built-in default, and `apa`, `chicago-author-date`,
`chicago-fullnote-bibliography`, `ieee`, `modern-language-association`,
`harvard-cite-them-right`, `nature`, `vancouver`,
`american-medical-association`, `american-sociological-association` from
the CSL styles repository at a pinned commit, in `$RANDOC_GPT/data/csl/`,
never committed; 5 files with the same 30 own references (CSL JSON,
BibTeX, BibLaTeX, CSL YAML, RIS); 15 own documents (one citation, several,
prefix and suffix, locators, suppressed author, in-text, `nocite`,
`nocite: '@*'`, `link-citations`, `suppress-bibliography`,
`reference-section-title`, notes, `lang: de-DE`, `lang: fr-FR`,
`--citation-abbreviations`); outputs `html`, `plain`, `latex`, `markdown`.
Command: `--citeproc [--csl S] --bibliography B -t O DOC`. TOTAL: 3,300.
Tool: M1-18.

### `defaults.units`
Unit: a key of K, or one of 8 feature keys. K: each long option name and
each key name of the manual's section on defaults files (read at run time,
names only) that the oracle accepts with no "unknown key" result. Feature
keys: nested `defaults`, `${.}`, `${USERDATA}`, `${HOME}`,
`DATADIR/defaults` with and without `.yaml`, several `-d` in order, options
before and after `-d`. Pass: 3 or more units use the key (two valid values,
one wrong type), and each unit that uses it passes, the harness defaults
cases included. TOTAL: |K| + 8. Tool: M1-10. Weak point today: 1,463
passing defaults units and no number for each key.

### `errors.units`
Unit: a trigger case or a log unit. Inputs: (a) for each of the 31 codes of
the manual's exit-code table (numbers only stored), own triggers that the
oracle confirms; (b) each corpus document with `--verbose --log=log.json
-t html`; (c) each t in P with `--verbose --log=log.json` to `html`,
`latex`, `markdown`. Pass: section 3 with the log file. Evidence: the codes
with no trigger. Tool: M1-11.

### `files.units`
Unit: a harness unit where the oracle writes a file. Inputs: each such
case, and own cases: `-o` for the 76 writers other than `pdf` on the first
tree of P; `--extract-media` for `docx`, `odt`, `epub`, `pptx` (input made
by the oracle from an own document with PNG, JPEG, SVG images), `ipynb`,
`html` and `markdown` with `data:` URIs; `--log`; `-o` into a missing
folder; `--epub-cover-image`. Pass: section 3 with written files. TOTAL: 60
or more. Tool: M1-03.

### `zip.units`
Unit: (W, t in P) for W in `docx`, `odt`, `epub`, `epub2`, `epub3`, `pptx`;
and for `docx`, `odt`, `pptx` the first 20 trees of P with `--reference-doc`
(an own change of the default reference document, made at run time). Pass:
section 3 with 3.5. TOTAL: 2,460. Tool: M1-19, with 3.10 for each entry and
4.4.

### `pdf.units`
Unit: (route, document, variant). Routes (21): the 15 engines that the
oracle accepts (`pdflatex`, `lualatex`, `xelatex`, `latexmk`, `tectonic`,
`pdflatex-dev`, `lualatex-dev` with `-t latex`; `context`; `weasyprint`,
`wkhtmltopdf`, `pagedjs-cli`, `prince` with `-t html`; `groff`, `pdfroff`
with `-t ms`; `typst`), and `-o out.pdf` with no engine for `latex`,
`beamer`, `context`, `html`, `ms`, `typst`. Documents (5): empty, the first
tree of P, an image, math, `--toc`. Variants (6): none; `--pdf-engine-opt`
twice; `-V geometry:margin=1in -V fontsize=12pt`; `--resource-path` with an
image; engine missing; engine exits 1 with a log. An own stand-in
(`tools/pdf-shim/`) under each engine name records each call (3.6) and
writes a fixed PDF. Plus the 5 documents with each installed real engine
(today only `tectonic`). TOTAL: 630 + 5 x installed engines. Tool: M1-20.

### `perf.startup`
Unit: `--version`; `-f F -t json` on empty input for the 52 readers;
`-f json -t W` on an empty document for the 76 writers other than `pdf`.
Command: paired runs, CPU time (user + system) from `wait4`. Pass: equal
outputs, and the sequential sign test of `tools/bench/run.py` (alpha 0.01,
at most 480 pairs or 30 minutes of oracle time) gives an upper bound of the
median ratio Randoc / oracle of 1.0 or less; inconclusive is not a pass.
TOTAL: 129. Tool: M1-21. Weak point of `tools/bench`: it compares with
`main`, not with the oracle.

### `perf.throughput`
Unit: a corpus document whose corpus units all pass in the same run.
Command: `-f FORMAT -t html FILE`, paired. Pass: as `perf.startup`. TOTAL:
those documents; this TOTAL follows `corpus.units` and is the one TOTAL
that can fall without a tool correction. Rule for `perf.*`: release build;
run only at a 1-minute load average of 4.0 or less (else wait 30 minutes,
then print `deferred: load` and write no row); the evidence holds load,
processor and system version. Tool: M1-21.

### `perf.memory`
As `perf.throughput`. Pass: equal outputs, and the median peak memory
(`ru_maxrss`, 7 runs or more) of Randoc is at or below the oracle's.

### `downstream.quarto`
Unit: one recorded oracle call of Quarto. Inputs: a pinned Quarto (version
and SHA-256 in `spec/downstream.toml`); 12 or more open projects in
`$RANDOC_GPT/downstream/quarto/` (HTML, PDF with `keep-tex`, DOCX,
revealjs, website, book, citations, Lua filter extension, cross references,
callouts, code, math; no executed cells). Quarto calls a recording stand-in
(`QUARTO_PANDOC`; a probe confirms it) that stores arguments, input and
files, then runs the oracle. Command: each call replayed with both
programs. Evidence field `renders_equal`: projects whose whole render with
Randoc equals the oracle render. Tool: M1-25. Quarto is not installed; if
the install needs the owner: row 0/TOTAL and a BLOCKED ON OWNER entry.

### `downstream.rmarkdown`
As `downstream.quarto` with a pinned R and `rmarkdown`, 10 or more projects
(`html_document`, `pdf_document` with `keep_tex`, `word_document`,
`ioslides_presentation`, `slidy_presentation`, `beamer_presentation`,
`github_document`, `md_document`, citations, a Lua filter) and
`RSTUDIO_PANDOC` (the stand-in's folder). R is not installed.

### `server.units`
Unit: one request of Q: for each harness `e2e` unit that the server's JSON
fields can express, `POST /` with `Accept: application/json` and with
`text/plain`; a `POST /batch` of 10; `GET /version`; own error requests
(invalid JSON, unknown format or field, wrong type, `--timeout 1` with a
slow input); all again through `pandoc-server.cgi`; the options `--port`,
`--timeout`, `--help`, `--version`. Command: `pandoc server --port N` on
127.0.0.1 for each program. Pass: 3.2 part 5. Tool: M1-23.

### `lib.units`
Unit: (t in P, W in the 76 writers other than `pdf`). Command: an own
program on `randoc::pipeline::run` with `Context::pure()`. Pass: bytes (or
error and exit code) equal to the Randoc program's. TOTAL: 30,400. Tool:
M1-24.

### `wasm.units`
As `lib.units` through the `wasm32-wasip1` build in a pinned WASI runtime.

### `safety.units`
Unit: one stress input, for each reader and writer that Randoc has (no exit
4): depth 1,000, 10,000, 100,000; 100,000 flat list items; a 10 MB line;
100,000 repeated openers; for archives an entry above 1 GB unpacked,
100,000 entries, names with `..` or absolute paths; for writers the same
depths as JSON. Command: `limit-run 2000 60` around Randoc. Pass: Randoc
ends in the limits with no signal or panic and gives the oracle result, or
the depth error of `plan/ARCHITECTURE.md` section 16 when the oracle has no
result in the limits (3.9). TOTAL: 100 or more. Tool: M1-22.

### `gate.steps`
Unit: one step in the build sandbox: each step of `tools/autobuild/gate.py`
(format, lints, docs, tests, harness no-regression, licence scan,
measurement no-regression); for each tool that runs the oracle, a stand-in
oracle shows `GHCRTS=-M3g`; for each baseline check, a stand-in Randoc that
breaks one passing unit makes it fail; two harness runs of one commit with
`--jobs 8` give the same verdicts (`tools/harness-verdicts/compare.py`); the
oracle SHA-256 check; the "never calls the oracle" check. Tool: M1-01,
M1-02. Weak points today: `crates/randoc-citeproc/tests/time.rs:15` runs
`ps` (refused); `init.sh` stops at the hook copy.

## 6. Held-out rules

1. The held-out store is `/Volumes/Extreme Pro/Randoc/holdout/`.
2. The build never reads it: no lead model, worker, reviewer, adviser or
   tool that they start lists, copies, hashes or infers it. The build
   sandbox denies each read.
3. Only the evaluator reads it, outside the build sandbox.
4. The evaluator reports totals only: for each set (`corpus`, `generated`)
   and each target or family, PASS, TOTAL and unstable count; the commit,
   the program's SHA-256, the time and the duration. Never a unit name,
   input, output, seed, case hash or difference.
5. One request each 6 hours (`ops holdout request`). An earlier request gets
   `deferred` and the next allowed time.
6. `ops holdout results` writes the rows `holdout.corpus` and
   `holdout.generated` with the measured commit.
7. The held-out numbers count for the final goal. A milestone gate needs a
   held-out result of the milestone commit.
8. A held-out number is never the only evidence of a change. If a visible
   fraction rises and the held-out one does not, write a `weak-spot` entry
   and change the approach.

## 7. The final goal

Randoc is done when all of these hold on one commit, in one full
measurement:

1. Each key of section 5 has a row at TOTAL/TOTAL with TOTAL above 0: each
   `fmt.write.F` and `fmt.read.F`, `caps.readers` 52/52, `caps.writers`
   77/77, `caps.extensions` 1,719/1,719, `caps.options` 109/109.
2. Each property of section 2 is measured.
3. `holdout.corpus` and `holdout.generated` at TOTAL/TOTAL for a program
   built from that commit.
4. Pandoc's own test suite passes: `suite.pandoc`, `suite.pandoc.inline`
   and `suite.pandoc.lua` at TOTAL/TOTAL, and `not_extracted` is empty or
   each test in it has a `decision` entry that shows with an oracle command
   that no command line shows its effect.
5. Quarto and R Markdown give the same results: both `downstream.*` keys at
   TOTAL/TOTAL and `renders_equal` equal to the project count.
6. `value_only` is empty in each metric; `unstable_parts_ok` equals the
   unstable count; each excluded unit has its reason in
   `spec/unstable.json`.
7. Each of the 31 exit codes has a confirmed trigger, or a `decision` entry
   that shows why no command line gives it.
8. The licence scan of the whole tree passes (section 8).

The goal can become stronger, never weaker; only the owner can lower it. A
TOTAL never shrinks, except through a recorded tool correction (3.13);
`perf.throughput` and `perf.memory` follow `corpus.units`. Each change of
this file makes a rule stronger or more exact and has a `decision` entry.

## 8. Licence rules

Pandoc is GPL-2.0-or-later. Randoc is `MIT OR Apache-2.0`. Never in the
repository, in any commit of a pushed branch:

1. Pandoc source code, or code ported from it, `texmath`, the GPL
   `skylighting` package, `xml-light` or the GPL files of
   `pandoc-lua-marshal`. Do not read the Pandoc source folders.
2. Pandoc test files (inputs and expected results).
3. Text or examples of the Pandoc manual, and examples of the CommonMark or
   GFM specification.
4. Pandoc data files with no permissive option: reference documents,
   translations, `epub.css`, `creole.lua`, `init.lua`, the abbreviation and
   DocBook entity lists, the completion script, `default.csl`, CSL styles
   and locales, copyleft or ungranted syntax definitions
   (`docs/DATA_FILES_LICENCES.md`). They go to the data pack.
5. Stored results that embed copyleft text: `--completion`,
   `--bash-completion`, `--print-default-data-file` of a non-template,
   `-D` of a template that `crates/randoc-data` lacks, `-s` for `fodt`,
   `odt`, `docx`, `pptx`, `epub`, output with a translation or CSL file, a
   script that the oracle writes into HTML (test it by hash). Use 4.4.
6. Corpus documents, their oracle results, held-out data, and files of
   `/Volumes/Extreme Pro/Randoc/env/store`.
7. A dependency whose licence is not MIT, Apache-2.0, BSD-2-Clause,
   BSD-3-Clause, ISC, Zlib, Unicode-3.0, CC0-1.0 or Unlicense.

Permitted with attribution: the Pandoc templates (BSD-3-Clause option) and
dzslides (WTFPL), also in stored results; permissive syntax definitions;
the libraries of `THIRD_PARTY_NOTICES.md`; interface facts as program output
(error texts, `--help` lines, format lists); a byte sequence that the
program must write, as a literal with an origin comment. Procedure: check
the licence of each file at the exact release; add rows to
`docs/PRIOR_ART.md`, `docs/ADR.md` and `THIRD_PARTY_NOTICES.md`, the
licence text to `third_party/licences/`, and a crate `NOTICE.md`. Prefer a
pinned dependency to a copy.

Pandoc's tests run from `env/pandoc-src/test` and
`env/pandoc-src/pandoc-lua-engine/test` at run time. A tool may read a test
file at run time. The repository holds only numbers and the test
identifiers that the tool makes. Never commit a test file, a part of one or
its expected output.

Licence scan: `python3 -I tools/autobuild/licence_scan.py scan` before each
push and in each gate (8-word runs against a hashed copyleft index). Replace
copied text and scan again. A correct use (for example an error text that
the program prints) gets a row in `tools/autobuild/licence-allow.tsv` and a
`decision` entry. The scan misses copies under 8 words, binary files, files
above 5 MB and minified text; each new such file gets a `decision` entry
with origin and licence. At each milestone gate, scan the whole tree
(`--base` at the first commit). Never try to remove old GPL commits of
`main` (that needs a forced push).
