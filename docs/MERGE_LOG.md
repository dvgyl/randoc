# Merge log

One entry for each merge into `main`, with the evidence for each gate.
Only reviews and checks that happened are recorded here.

## Foundation (commits 3c0079d to this commit)

Content: workspace, document model with JSON, shared contracts, the JSON
format, the first program, the conformance harness, the inventory.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The first baseline: 28 passing units of 36. |
| 2. Measured progress | From no code to 28 passing units. The 8 failing units are JSON error texts (I-001). |
| 3. Tests | Line coverage 80.4% for the workspace (`randoc-cov --workspace`). Below 85%: `randoc-ast/src/types.rs` 49% and `randoc-cli/src/lib.rs` 57% (both are replaced by the workstreams json-codec and cli), `randoc-core/src/logging.rs` 54% and `error.rs` 75% (message texts that no code produces yet; each text gets a test from the oracle when a workstream first produces it), `randoc-harness/src/main.rs` 66% (the usage text and the print functions). Mutation tests: a sample of 1 in 6 of 577 mutants ran after this entry; see the next entry for the result. |
| 4. No panics | Fuzz target `json_read`: 901 seconds, 3,714,707 runs, no crash. |
| 5. Two reviews | Codex (GPT-6.1 Sol, fast tier): REQUEST CHANGES, 13 findings (3 high, 10 medium). Opus 5.5 with a new context: REQUEST CHANGES, 14 findings (3 high, 6 medium, 5 low). All harness findings are corrected in commit 7b4bd0f and this commit, each with a test. The JSON findings are in the workstream json-codec. The findings about the first program are in the workstream cli. Both reviewers must approve the corrected parts before the gate is complete for the foundation. |
| 6. No slowdowns | Not applicable: there is no earlier version. |
| 7. Clean build | `scripts/gate.sh` passes. CI passes on commit 7b4bd0f. |

Gate 5 is not complete for the foundation. The foundation is on `main`
because each workstream needs it as its base. This exception is recorded in
`PROGRESS.md`.

### Foundation: mutation tests

A sample of 1 in 6 of the 577 mutants in `randoc-ast`, `randoc-core`,
`randoc`, `randoc-cli` and `randoc-harness` ran on commit 7b4bd0f:
97 mutants, 60 caught, 35 missed, 1 timeout, 1 not viable.

| Missed mutants | Count | Action |
| --- | --- | --- |
| `Format` equality in `randoc-ast/src/types.rs` | 13 | New tests in `randoc-ast/src/type_tests.rs`, with the behaviour checked against the oracle. |
| `walk::query` in `randoc-ast/src/walk.rs` | 1 | New test in the same file. |
| The first program in `randoc-cli/src/lib.rs` and `main.rs` | 21 | Unit tests cannot see the exit code and the output of the program. The conformance suite now runs as a test of `randoc-cli` (`tests/conformance.rs`), so `cargo test` and the mutation tool judge the program by its conformance units. The command-line workstream replaces this file. |

The mutation tool did not run again after these changes. The next sample runs
when the workstreams json-codec and cli merge, because they replace most of
the code with missed mutants.

## Workstream testsuite (branch `ws/testsuite`, head 4fefb11)

Content: `tools/pandoc-testsuite` runs Pandoc's own test suite, from the Pandoc
source tree outside this repository, against a program. Results are in
`spec/pandoc-testsuite.json`.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | Harness after the merge: 28 units pass, 0 regressions. |
| 2. Measured progress | The work adds a measure and no product code. Pandoc test suite: the tool runs 2231 tests and cannot run about 1939 (they need Haskell). The oracle passes 2231 of 2231. Randoc passes 0 of 2231, because `main` has only the JSON format. |
| 3. Tests | 53 unit tests of the tool pass on `main`. Mutation tests and coverage are for Rust code and did not run. |
| 4. No panics | Not applicable: the change has no Rust parser. |
| 5. Two reviews | Codex (GPT-6.1 Sol, fast tier): round 1 REQUEST CHANGES (2 high, 1 medium, 1 low), round 2 REQUEST CHANGES (2 medium), round 3 APPROVE at 4fefb11. Opus 5.5 with a new context: round 1 REQUEST CHANGES (1 high, 2 medium, 3 low), round 2 APPROVE at a3a3fec with 2 low findings, round 3 APPROVE at 4fefb11. Each finding is corrected with a test. The reports are in the coordinator store (`reviews/testsuite-*.md`). |
| 6. No slowdowns | Not applicable: no product code changed. |
| 7. Clean build | `scripts/gate.sh` passes on the merged tree. |

Check by the coordinator on the merged tree: the unit tests pass, the oracle
passes 174 of 174 tests of the group `old`, and Randoc passes 0 of 174.

## Workstream bench (branch `ws/bench`, head b35473d, merged as one commit)

Content: `tools/bench` measures the CPU time of a program for startup and
conversion, compares two programs in paired rounds, and keeps raw data in
`benchmarks/`. `docs/BENCHMARKS.md` gives the method and the gate procedure.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | Harness after the merge: 28 units pass, 0 regressions. |
| 2. Measured progress | The work adds a measure and no product code. It gives gate 6 its tool. |
| 3. Tests | 30 unit tests of the tool pass on `main`. Mutation tests and coverage are for Rust code and did not run. |
| 4. No panics | Not applicable: the change has no Rust parser. |
| 5. Two reviews | Codex (GPT-6.1 Sol, fast tier): round 1 REQUEST CHANGES (4 medium, 1 low), round 2 REQUEST CHANGES (2 medium), round 3 REQUEST CHANGES (1 medium), round 4 APPROVE at b35473d with 1 low finding. Opus 5.5 with a new context: round 1 REQUEST CHANGES (1 high, 2 medium, 4 low), round 2 APPROVE at 9b80ef5 with 4 low findings, final check APPROVE at b35473d with 1 low finding. The two open low findings are L-001 and L-002 in `docs/ISSUES.md`. |
| 6. No slowdowns | Not applicable: no product code changed. |
| 7. Clean build | `scripts/gate.sh` passes on the merged tree. |

The branch is merged as one squashed commit, because two of its older commits
held a local home path in a result file. The files on `main` hold no user name
and no local path (checked with a search).

Limit of the tool under the present machine load: a comparison of a program
with itself often ends as "inconclusive" (exit code 3). Gate 6 then has the
result "not decided", and the merge log must say so.

## Workstream research (branch `ws/research`, head e768696)

Content: `docs/COMPETITIVE_ANALYSIS.md`, `docs/DATA_FILES_LICENCES.md` and the
probe data in `docs/research-data/`. No product code.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | Harness after the merge: 28 units pass, 0 regressions. |
| 2. Measured progress | Documents only. They give the tested state of three other projects and the licence state of each Pandoc data file. |
| 3. Tests | Not applicable: no code. Each number in the documents comes from a recorded command. |
| 4. No panics | Not applicable. |
| 5. Two reviews | Codex (GPT-6.1 Sol, fast tier): round 1 REQUEST CHANGES (1 high, 3 medium, 3 low), round 2 APPROVE at e768696 with 1 low finding. Opus 5.5 with a new context: round 1 REQUEST CHANGES (1 medium, 8 low), round 2 APPROVE at e768696 with 1 low finding. The two open low findings are L-003 and L-004 in `docs/ISSUES.md`. |
| 6. No slowdowns | Not applicable. |
| 7. Clean build | `scripts/gate.sh` passes on the merged tree. |

Not checked by a reviewer: the competitor programs could not run again in the
reviews, because the worker deleted its clones to free disk space. The
reviewers checked the numbers against the stored result rows and the public
repositories.

## Workstream corpus (branch `ws/corpus`, head a0fab04)

Content: `tools/corpus/` (the manifest of 230 real documents in 33 formats
with a pinned URL, a licence and a SHA-256 hash for each one, the download
tool and the comparison tool), `docs/CORPUS.md` and `spec/corpus.json`. The
documents are not in the repository. No product code.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | Harness after the merge: 28 units pass, 0 regressions. |
| 2. Measured progress | The corpus run is a new measurement: 690 units (230 documents, each to `json`, `html` and `markdown`). The oracle against itself gives 690 equal results. Randoc passes 0 of 690 (issue I-CORPUS-01), because `main` has only the JSON format. |
| 3. Tests | 72 unit tests of the tools pass on the merged tree (`python3 -B -m unittest discover -s tools/corpus`). Mutation tests and Rust coverage do not apply to Python tools. |
| 4. No panics | Not applicable: no parser of Randoc changed. The tools limit the download size, the download time and the run time of each program, and they stop a process group that does not end. |
| 5. Two reviews | Six rounds. Codex (GPT-6.1 Sol, fast tier): rounds 1 to 4 REQUEST CHANGES, round 5 APPROVE, round 6 APPROVE at 206f10a with no open finding. Opus 5.5 with a new context: round 1 REQUEST CHANGES (2 high, 2 medium), rounds 2 to 4 APPROVE, round 5 REQUEST CHANGES (1 medium: a download that the server cut was accepted), round 6 APPROVE at 206f10a with 1 low finding, which is L-005 in `docs/ISSUES.md`. |
| 6. No slowdowns | Not applicable: no product code changed. |
| 7. Clean build | `scripts/gate.sh` passes on the merged tree. |

Changes after the last reviewed commit (206f10a), checked by the coordinator:

- Commit a0fab04 changes only `docs/CORPUS.md`. It records a second download
  of the whole corpus: each of the 230 hashes was equal to the manifest.
- In the merge commit, the coordinator gave the two corpus issues the
  identifiers I-CORPUS-01 and I-CORPUS-02, added L-005, and replaced a local
  path in the run instructions of `docs/CORPUS.md` with the names of the two
  environment variables.

## Workstream layout (branch `ws/layout`, head de17bda)

Content: the crate `randoc-layout`, a port of the Haskell library `doclayout`
0.6 (BSD-3-Clause): the `Doc` type with breakable spaces, nesting, prefixes,
blocks, styles and links, the render function with line wrapping, and the
character width rules with their table. Each writer of a text format builds
on it.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | Harness after the merge: 28 units pass, 0 regressions. |
| 2. Measured progress | No harness unit changes, because no writer on `main` uses the crate yet. The crate test `docs_match_oracle` compares 396 `Doc` expressions at 8 widths (3,168 plain renders and 3,168 ANSI renders, plus the queries) with results of the oracle (`pandoc lua tests/vectors.lua`). Known differences: I-LAYOUT-01 to I-LAYOUT-05 in `docs/ISSUES.md`. |
| 3. Tests | 19 unit tests and 5 documentation tests pass on the merged tree. Line coverage 97.67% (1717 lines, 40 missed). Mutation tests: the first sample had 44 of 440 mutants; after the engine changed in review round 2, a second sample tested 80 of the 160 mutants of that change (55 caught, 6 timeouts that count as caught, 14 missed, 5 not viable). Each missed mutant has a new test or is an equivalent mutant, with the reason in the report. The other 80 mutants of the change did not run. |
| 4. No panics | Fuzz target `render`: 900 s, 291,479 runs, no crash. Clone, compare, render and drop use explicit stacks: both reviewers ran 100,000 nested levels on a 2 MB stack with no overflow. |
| 5. Two reviews | Codex (GPT-6.1 Sol, fast tier): round 1 REQUEST CHANGES, round 2 REQUEST CHANGES at f462b56 (its read-only sandbox could not build Rust code, so the comparison with the oracle could not run; 1 low finding), round 3 (with a writable scratch folder) APPROVE at 08cac8f after 35,796 comparisons with the oracle and 0 differences. Opus 5.5 with a new context: round 1 REQUEST CHANGES, round 2 APPROVE at f462b56 after 76,653 comparisons with 0 differences, with 4 low findings. The low findings are the rows I-LAYOUT-04 and I-LAYOUT-05 and two documentation notes, which commit de17bda adds. |
| 6. No slowdowns | Not applicable: no conversion path on `main` uses the crate, so no benchmark changes. The reviewers recorded one cost that is equal in the oracle: 10,000 nested levels with a non-empty indent need quadratic time. |
| 7. Clean build | `scripts/gate.sh` passes on the merged tree. |

Changes after the reviewed commits, checked by the coordinator: each changed
line of `crates/randoc-layout/src` between f462b56 and de17bda is a test or a
comment (a script listed the changed lines that are not comments; all 41 are
in the test module).

Process note: an in-place mutation run changed the worktree during review
round 2. Both reviewers saw it and tested a clean copy of the commit. The
standing orders now forbid an in-place mutation run during a review.

## Workstream native (branch `ws/native`, head b097609, one squashed commit)

Content: the crate `randoc-native`, the reader and the writer for the format
`native` (the Haskell `show` form of the document tree), with the suite
`native/` (611 cases in 9 files). Pandoc's own test suite stores most expected
results in this format.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | Harness after the merge: 0 regressions against the baseline of 28 units. |
| 2. Measured progress | Passing units: 28 before, 949 after (1984 units in all; 921 new passes). Writer units without options: 370 of 373 pass. Reader units: 116 of 119 pass. The failing units are recorded: I-NATIVE-01 (the JSON form of special doubles, which the JSON codec branch corrects), I-NATIVE-04 (literal tabs, which the command line expands) and units that need options or the Markdown reader. |
| 3. Tests | 16 crate tests pass, with a replay of each recorded unit. Line coverage 94.5% (measured at an earlier commit, 100698e). Mutation tests: a sample of 164 of 562 mutants (54 caught, 10 missed, 100 not viable); new tests kill each of the 10 survivors, and a second run on the affected functions gave 23 caught, 0 missed. The mutants of the round 2 change: 4 of 4 caught. |
| 4. No panics | Fuzz targets `native_read` (3 runs of 900 s, the last one after the last lexer change: 1,276,887 runs) and `native_roundtrip` (901 s, 2,681,995 runs): no crash. Depth: the reader and the writer return an error at the nesting limit. Time: the reviewers found linear growth for 27 and 16 worst input classes. |
| 5. Two reviews | Codex (GPT-6.1 Sol, fast tier): round 1 REQUEST CHANGES (quadratic time for named escapes), round 2 APPROVE at 02b455a, round 3 APPROVE at 16ef15d. Opus 5.5 with a new context: round 1 REQUEST CHANGES (3 medium), round 2 REQUEST CHANGES (1 medium: the ribbon length needs 32-bit arithmetic), round 3 APPROVE at 16ef15d. In round 3 each reviewer compared each width from 1 to 2000 with the oracle (8,000 runs each) and found 0 differences. Low findings: the time test (closed, see below) and one digit of `show_double`, which is L-006 in `docs/ISSUES.md`. |
| 6. No slowdowns | The format is new, so `main` has no earlier time for it. The JSON path does not change in this merge; a check with release builds of `main` before and after the merge (15 alternating pairs, CPU time, a 17.7 MB document, `-f json -t json`) gave a ratio of minimums of 0.97 and a median paired ratio of 1.02, which is inside the noise of the loaded machine. Capacity note (I-NATIVE-05): a tree with 10,000 nested levels gives 400 MB of output; a release build needs 8.2 s and the oracle needs 12.1 s on the same machine. |
| 7. Clean build | `scripts/gate.sh` passes on the merged tree. |

Changes after the last reviewed commit (16ef15d), checked or made by the
coordinator:

- Commit b097609 of the worker closes the two low findings on the time test.
  A script listed its changed source lines that are not comments: all 130 are
  in the test module. The test now measures the CPU time of the test thread
  (the minimum of 5 runs at 64 KB and 256 KB, with a ratio limit of 10). The
  worker ran it 10 times on correct code (10 passes) and 2 times with the old
  quadratic lexer (2 failures).
- That commit adds the development dependency `nix` (MIT) for the thread CPU
  time. In the merge, the coordinator made this dependency and the time tests
  Unix-only (`cfg(unix)`), so that the tests still build on Windows.
- In the merge, the coordinator set the licence-text cell of the native row
  in `THIRD_PARTY.md` to "not applicable", because the writer copies no code
  and no data.

## Workstream json-codec (branch `ws/json-codec`, head c1887dc, one squashed commit)

Content: a new JSON reader and writer for the document tree in `randoc-ast`,
in place of the `serde` version. It gives the bytes, the accepted and refused
inputs and the error texts of Pandoc (the Haskell library `aeson`): a text
parser that writes one flat list of values, typed decoders that keep the error
path, an encoder with an explicit stack, the digit generation of GHC for
`Double` values, and tables of two Unicode general categories for error paths.
The suite `json/` has 551 cases in 8 files. `serde` is no longer a dependency
of `randoc-ast`.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | Harness after the merge: 0 regressions against the baseline of 949 units. |
| 2. Measured progress | Passing units: 949 before, 2217 after (3574 units in all; 1268 new passes). The JSON suite: 1283 of 1289 units that run pass; the 6 failures are I-JSON-01 (integer fields with an extreme exponent, where Pandoc stops with an internal error or does not end). The merge closes I-001 (JSON error texts), I-NATIVE-01 (special doubles in JSON) and L-006 (one digit of `show_double`). |
| 3. Tests | 51 unit tests of `randoc-ast` pass. The worker's differential tool compares 3000 generated documents with the oracle: 0 differences. Line coverage 95.37% (measured after round 1). Mutation tests: a first sample of 49 of 685 mutants and a run on each mutant of `types.rs`; then 67 mutants of the round 1 corrections (57 caught, 1 missed and killed by a new test, 9 not viable); then 84 of the 252 mutants of the speed rewrite (31 caught, 3 timeouts, 50 not viable, 0 missed). The 20 mutants of the last stack correction were all not viable, so the mutation tool gives no evidence for that change; its evidence is a depth test that stops with the old code. Open: 20 surviving mutants in helper methods of the document model (`Meta::get`, `Attr::has_class` and others), which this branch does not change; the coordinator owns them. |
| 4. No panics | Fuzz targets `json_read` (900 s, 7,090,878 runs) and `json_roundtrip` (900 s, 412,368 runs) on the final code: no crash. The encoder handles 100,000 nested levels on a 2 MB stack. The decoder reads each recursive kind at the deepest input that the default limit accepts on a 2 MB stack: the reviewers tested 405 and 242 shapes, with mixes of kinds, and found a smallest margin of 1.64. |
| 5. Two reviews | Four rounds. Codex (GPT-6.1 Sol, fast tier): round 1 REQUEST CHANGES, round 2 APPROVE at a8a14f4, round 3 REQUEST CHANGES at f79d25f (1 high: the decoder for metadata maps overflowed a 2 MB stack inside the default limit), round 4 APPROVE at c1887dc with no finding. Opus 5.5: round 1 REQUEST CHANGES, round 2 APPROVE at a8a14f4, round 3 (a new reviewer context, because the code was a speed rewrite) REQUEST CHANGES for the same defect, round 4 APPROVE at c1887dc with one low finding on the depth note of the document model, which this merge corrects. In round 4 the reviewers ran 1,534 and about 4,700 comparisons with the oracle with 0 differences. |
| 6. No slowdowns | After round 2 the branch was slower than `main` (slower in 26 of 30 pairs on a 17.7 MB document), so the merge was stopped and the worker rewrote the parser. Final measurement by the coordinator with release builds (15 alternating pairs, CPU time, `-f json -t json`, 17.7 MB): the branch needs 0.180 s and `main` at 85d5705 needs 0.321 s; ratio of minimums 0.56, median paired ratio 0.64, the branch is faster in 15 of 15 pairs, equal output bytes. `tools/bench/run.py` with its small inputs gave "inconclusive" under the machine load. |
| 7. Clean build | `scripts/gate.sh` passes on the merged tree. |

Changes that the coordinator made in the merge:

- The table tool keeps the rows of `main` for a conflict in `docs/ISSUES.md`.
  The branch had updated the rows I-001 and D-002, so the coordinator took the
  branch text for these two rows.
- The depth note of `randoc-ast` (decision D-003) now gives numbers by kind:
  200 nested tables or 510 nested levels of the other containers are safe on a
  2 MB stack for clone, compare and drop. The test covers these depths.
- The replay test of the native crate listed its known differences. Two of
  them are gone with the new codec, so the list changed. The test also read
  the JSON text of the oracle a second time before it compared; that read
  turns -0.0 into 0.0, as Pandoc does. The test now compares with the JSON
  text of the oracle directly.

## Workstream shared (branch `ws/shared`, head a7c6387, one squashed commit)

Content: the module `randoc_core::shared`, in place of copies that four
workstreams wrote for themselves: the plain text of a tree (`stringify`),
heading identifiers for the rules of Pandoc and for the rules of the
CommonMark reader with their sets for unique identifiers, one table of HTML
entities with numeric references, the emoji tables, the default abbreviation
list, and Unicode normalization (NFC). No program on `main` calls the module
yet. The readers and writers move to it at their next merge.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | Harness after the merge: 2217 units pass, 0 regressions. |
| 2. Measured progress | No harness unit changes, because no format on `main` uses the module yet. Replay tests compare the module with oracle results that tool scripts made: 94,752 headings (7,896 in each of 12 identifier modes), 800 blocks and 60 metadata values for the plain text, 2,551 entity references, 1,917 emoji aliases, 376 abbreviation candidates and 9,070 strings for NFC, all with 0 differences. Recorded: I-SHARED-01 to I-SHARED-05. |
| 3. Tests | 43 unit tests, 6 replay tests and 20 documentation tests pass on the merged tree. Line coverage of the module 99.8%. Mutation tests: 78 mutants of the first version (1 missed, killed by a new test), 105 of the NFC part (7 missed at the Hangul boundaries, each killed by a new test), 31 and 13 of the two correction rounds (0 missed). |
| 4. No panics | The module has no parser of a document format. No fuzz target. The reviewers ran trees with 100,000 nested levels on a 2 MB stack and 1,000,000 combining marks with no failure; two tests count steps to prove linear time. |
| 5. Two reviews | Three rounds. Codex (GPT-6.1 Sol, fast tier): round 1 APPROVE at aaacd23 (2 low), round 2 REQUEST CHANGES at 20f03a1 (1 medium: the NFC tables did not match the Unicode version of the oracle), round 3 APPROVE at a7c6387 (2 low, documentation). Opus 5.5 with a new context: round 1 REQUEST CHANGES at aaacd23 (1 high: an emoji alias in an identifier must come from the source text; 2 medium), round 2 REQUEST CHANGES at 20f03a1 (2 medium: the NFC version, and emoji in code and math), round 3 APPROVE at a7c6387 after 145,752 NFC strings and 5,120 headings with 0 differences (1 low: two test comments). |
| 6. No slowdowns | Not applicable: no conversion path on `main` uses the module. |
| 7. Clean build | `scripts/gate.sh` passes on the merged tree. |

Findings of the worker that only the oracle could give: Pandoc classifies
letters for identifiers with Unicode 15.1 data and normalizes with Unicode
18.0 data (818,502 probes for 272,834 code points: 142, 119, 55 and 0
differences from the versions 15.1, 16.0, 17.0 and 18.0). The abbreviation
list has 76 entries from 3,926,072 candidates that a script made mechanically
(each string of 1 to 4 letters, forms with one inner period, and the system
word list); no hand-written list and no Pandoc data file was used.

Changes that the coordinator made in the merge, after the reviewed commit:

- The replay test read its data files with the old generic JSON function,
  which the new JSON codec does not offer for types outside the document
  model. The test now reads the lists with `serde_json` (a development
  dependency) and each document with the codec. The production code of the
  module is unchanged.
- The low findings of round 3 on source comments: the documentation of
  `auto_identifier` and `commonmark_auto_identifier` now states the emoji
  rule of each, and two test comments stand at their tests.
- The branch added `third_party/licences/unicode.txt`; `main` already had
  `Unicode.txt` with the same content. Two names that differ only in letter
  case are one file on macOS and Windows, so the merge keeps `Unicode.txt`.
- Not corrected: the low finding on the text of the worker's report (later
  sections still name "explicit identifiers" and Unicode 16.0). The source
  documentation and `docs/ISSUES.md` are correct.

## Workstream templates (branch `ws/templates`, head 8d63d0b, one squashed commit)

Content: the crate `randoc-templates` (the Pandoc template language: parser
with the error texts of the oracle, renderer on views, the 16 pipes, partials
with a cache for one compile call) as a port of `doctemplates` 0.11.1, and the
new crate `randoc-data` with the 62 default template files and the dzslides
template (the files of Pandoc that have a permissive licence option). No
program on `main` calls the two crates yet: the command line connects them in
its own workstream.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | Harness after the merge: 4546 units, 2217 pass, 1959 fail, 370 skipped, 0 regressions against the baseline of 2217 units. The 972 new units are the `templates/` suite; they fail until the command line has `--template` and `-s`. |
| 2. Measured progress | New suite `templates/` (2 case files). The `templates/*` units need `--template` and `-s` of the command line, so they fail until `ws/cli` merges. The crate has its own oracle tests: 1,188 results from the Lua template interface of the oracle (`oracle_cases`: each construct, 508 error texts, 407 loop cases), and each of the 69 default templates gives the same bytes as the oracle for three contexts. Recorded: I-TPL-01 to I-TPL-04 and I-DATA-01 to I-DATA-05. |
| 3. Tests | `randoc-templates`: 33 tests pass, 1 ignored (a time measurement). `randoc-data`: 7 tests pass. Line coverage: 97.8% for `randoc-templates` and 100% for `randoc-data` (measured at a4e8c71, before the correction rounds, which added tests). Mutation tests: 160 mutants of the first version (3 missed, each killed by a new test or explained in the report), 39 and 17 of round 2 (9 and 1 missed; 9 killed by new tests), 30 of round 3 (1 missed: the `Pair` arm, killed in round 5 by `pairs_of_a_pairs_item`; the rerun of that mutant: caught). |
| 4. No panics | Fuzz target `render` (compile and render on a 2 MB thread for each input): 900 s, 59,002 runs, 0 crashes, 0 timeouts at c32addf. The last commit (8d63d0b) changes 23 lines of the renderer and no line of the parser; no new fuzz run. Depth: compile, render, clone and drop at the limit of 128 on a 2 MB thread; 10,000 and 100,000 levels give `TooDeep`. Time: step-count tests for 552 loop templates and for the 7 `pairs` forms. |
| 5. Two reviews | Five rounds. Codex (GPT-6.1 Sol, fast tier): round 1 REQUEST CHANGES at d5c90a2 (1 high, 1 medium), round 2 REQUEST CHANGES at 68309f6 (1 high: a lookup in a loop over a dotted variable copied data), round 3: the service stopped the run with a content flag before a report (no verdict), round 4 REQUEST CHANGES at c32addf (1 medium: `pairs` of `pairs` of a map was quadratic; 1 low), round 5 APPROVE at 8d63d0b (0 findings; 20,000 value results, 60 loop forms at two sizes, 69 default templates). Opus 5.5 with a new context: round 1 REQUEST CHANGES at d5c90a2 (1 high: a loop copied the context for each item; 2 medium), round 2: the review was lost when the computer stopped at 04:40 UTC (no verdict; the worker did its loop-scope task), round 3 REQUEST CHANGES at 46fb603 (1 high: six more loop forms were quadratic; 3 low), round 4 APPROVE at c32addf (2 low), round 5 APPROVE at 8d63d0b (0 findings; 28 `pairs` forms up to 64,000 entries, 824 templates at four widths, 69 default templates). Rounds 4 and 5 of Codex used a conformance prompt (values, time, error texts); they did not repeat the depth tests, which Opus round 4 covered. |
| 6. No slowdowns | No conversion path on `main` calls the two crates. A check with release builds of `main` before and after the merge (15 alternating pairs, CPU time, a 17.7 MB document, `-f json -t json`, equal output bytes) gave 0.098 s and 0.100 s: ratio of minimums 1.020, median paired ratio 1.017. The limit is 1.05. |
| 7. Clean build | `scripts/gate.sh` passes on the merged tree. |

Changes that the coordinator made in the merge, after the reviewed commit:

- The crate table in `docs/ARCHITECTURE.md` now has the row `randoc-data`,
  the real dependencies of `randoc-templates`, and the shared module of
  `randoc-core` (the repeated low finding TPL-R2-02 / TPL-R3-04).
- Row I-DATA-05 said that Randoc has no abbreviation list. Since the merge of
  the workstream shared, `randoc_core::shared::default_abbreviations` exists.
  The row now says so and points to I-SHARED-03.
- `THIRD_PARTY.md` and `docs/ISSUES.md`: the rows of `main` stay, and the 4
  and 9 rows of the branch are added.

Open after this merge: the command line must remove each CR and a byte order
mark from a template file before it calls `Template::compile` (the crate
documents this caller rule), and the command line branch has its own copy of
some default templates, which it must replace by `randoc-data`.

## Research reuse (documents only, one commit, no branch)

Content: `docs/PRIOR_ART.md` has a checked version, a licence and the URL of
the licence file for each reuse candidate. `docs/ADR.md` has three new
decision records: ADR 9 (ZIP containers: `zip` 9.5.0), ADR 10 (XML reading:
`quick-xml` 0.42.0) and ADR 11 (Lua: `mlua` 0.12.2 with `mlua-sys` 0.13.0 and
Lua 5.4.8 built by Randoc from the unchanged lua.org sources). The change
adds no dependency and no code. Each workstream that uses a crate adds the
dependency line with its own merge.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The commit changes two Markdown files under `docs/`. No build input and no test input reads them (`grep` for the two file names in `crates/`, `scripts/` and `tools/` gives no result), so the harness result of `main` is unchanged: 2217 baseline units, 0 regressions (measured at the start of the session on 9ffe433). |
| 2. Measured progress | Does not apply: no behaviour changes. Each candidate row now has its evidence; 4 candidates are recorded as not compatible or partly compatible (`unicode-math`, the data files of `unicode-collation`, the default features of `hayagriva` and of `ureq`). |
| 3. Tests | Does not apply: no code. The records name the tests that each later workstream must write (lists "To verify in the workstream"). |
| 4. No panics | Does not apply: no code. |
| 5. Two reviews | Four rounds. Opus 5.5 with a new context: round 1 REQUEST CHANGES (3 high, 4 medium, 5 low; no licence row permitted material that is not compatible), round 2 REQUEST CHANGES (1 medium, 5 low; all 25 round 1 findings of both reviewers closed), round 3 APPROVE (1 low), round 4 APPROVE of the final text. Codex (GPT-6.1 Sol, fast tier, with network access for the licence files): round 1 REQUEST CHANGES (3 high, 7 medium, 3 low), round 2 REQUEST CHANGES (3 high, 3 medium, 1 low in the corrected text), round 3 REQUEST CHANGES (1 medium: the restricted Lua mode must also restrict host functions; 1 low), round 4 APPROVE of the final text. From round 3, a record states only verified facts, and each implementation rule that is not fully probed is in a list "To verify in the workstream". |
| 6. No slowdowns | Does not apply: no code. |
| 7. Clean build | Does not apply: no build input changes. |

Changes that the coordinator made after the round 3 reviews (both reviewers
checked exactly these three changes in round 4): the host-function rule of
the restricted Lua mode in ADR 11, the `lua-src` versions in ADR 11, and the
named-entity table in rules 5 and 6 of ADR 10.

Decisions that the records need from the coordinator, all approved: the copy
of the 59 unchanged Lua source files is an exception to rule 5 of
`docs/PRIOR_ART.md`, with a hash test for each file; the one call of
`Lua::unsafe_new_with` gets a written safety argument and a focused test;
`randoc_core::Limits` gets size limits for an archive file and an input file
when the first container workstream starts.

Not checked by any reviewer or by the research agent: release builds; Linux
and Windows; the WebAssembly targets `wasm32-wasip1` with a C toolchain and
`wasm32-unknown-emscripten`; the licence files (not only the licence fields)
of the build dependencies `cc`, `cfg-if`, `pkg-config`, `find-msvc-tools`
and `shlex`.

## Workstream harness-flaky (branch `ws/harness-flaky`, head 5781931, one squashed commit)

Content: test code only, in the `mod tests` blocks of
`crates/randoc-harness/src/exec.rs` and `crates/randoc-harness/src/lib.rs`
(2 files, 97 insertions, 25 deletions). Three tests failed under a high load
because of short wall-clock limits. The cause in the no-result test: its
1 s limit also applied to the `--version` call of `record`. The changes:

- The no-result test is now two tests with one helper. In
  `a_program_that_a_signal_stops_is_never_recorded_and_never_passes`, the
  converter stops itself with `kill -9 $$`. In
  `a_program_that_never_ends_is_never_recorded_and_never_passes`, the
  converter runs `kill -STOP $$`, so the limits of `record` (10 s) and `run`
  (1 s) are always reached.
- The slow-program test has a limit of 3 s. Each script holds a named pipe
  (FIFO) open and writes `started` first. The test reads the FIFO to its end
  and expects exactly `started\n`, so a process that survives the stop makes
  the test fail. The FIFO is in a private folder from `mktemp -d` (mode 0700,
  FIFO mode 0600) with a `Drop` guard.
- The slow-program test and the pipes test assert an elapsed time below 20 s.

No line of the harness program changes.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | Harness before and after the merge: 4546 units, 2217 pass, 1959 fail, 0 no result, 0 not recorded, 370 skipped; baseline 2217 units, 0 regressions, 0 new passes. `verify-oracle`: 4176 units, 0 stored results are not current. |
| 2. Measured progress | Does not apply in the usual sense: no behaviour of the converter changes. In its place: the three tests that failed under load (now four tests) passed in 80 of 80 runs (20 runs of the four tests together, one-minute load average 12.3 to 20.7, worker report). |
| 3. Tests | `cargo test -p randoc-harness --lib`: 37 passed. Each of 10 mutations of the harness program fails a changed test (worker report, unit b: M1 to M4 of the worker, M5 to M8 of Opus round 1, `timeout_pass` and `timeout_record_count` of Codex round 1). The Codex reviewer of round 2 ran the mutations of the stop time, the pipe grace, the `started` write, the kill of the main process only and the two timeout mutations itself: each one fails a test. |
| 4. No panics | Does not apply: no parser and no archive reader changes. |
| 5. Two reviews | Two rounds. Round 1, full rounds at dec22ed: Codex (GPT-6.1 Sol) REQUEST CHANGES (1 medium: R1-01, the signal test removed the coverage of the timeout handling of `record` and `run`; 2 low: R1-02 cleanup of the FIFO after a failure, R1-03 predictable FIFO name and mode); Opus 5.5 with a new context APPROVE (4 low: R1-01 leftovers after a failure or a stop, R1-02 predictable name, R1-03 two lost detections of the stop time, R1-04 no proof that the script started). Round 2, verification rounds of the same reviewers (resumed) on the diff dec22ed..5781931: Opus APPROVE (0 new findings; all 7 findings of round 1 closed in its view); Codex APPROVE (report `harness-flaky-codex-r2b.md`; 0 new findings; Codex R1-01, R1-03 and Opus R1-02 to R1-04 closed; Codex R1-02 and Opus R1-01 low and partly open, one cleanup cause). The first Codex run of round 2 lost its final message when the computer stopped at 13:48 UTC; the same session was resumed for `r2b`. |
| 6. No slowdowns | Does not apply: the change is test code only, so no code of the program changes. No release build and no `bench-pair` run. |
| 7. Clean build | `scripts/gate.sh` passes on the merged tree (`merge-ws check`, second run). |

The first run of `merge-ws` on the merged tree (18:31 to 18:47 UTC) failed
in the test step: `a_program_that_a_signal_stops_is_never_recorded_and_never_passes`
panicked at `lib.rs:1171`, because `record` with the `cat` fixture as the
oracle gave "the output pipes stayed open after the program ended" for
`s/f/a#e2e:html`. That message comes from the 2 s limit `PIPE_GRACE` in
`exec.rs`, a wall-clock limit. The second run (18:47 to 18:58 UTC) passed
each step. Recorded as H-005.

The gate script passed on 5781931 at 18:19 UTC in the worktree (log
`env/tmp/coord/gate-harness-flaky-5781931-c.log`: baseline 2217 units,
0 regressions, 0 new passes). Two earlier gate runs did not end: the wrapper
edit of the coordinator at 13:30 UTC, and the stop of the computer at
13:48 UTC.

Limits of the gates: no `cargo mutants`, no coverage and no fuzzing (test
code only; 10 hand mutations in their place). The worker did not reach a
load of 40 in unit b. The Opus reviewer of round 2 did not run its own
mutations (its build waited more than 13 minutes for a build slot); it closed
two findings by reading. The Codex reviewer of round 2 did not repeat the
80-run experiment. No reviewer ran the tests on Linux or as a second user.

Open after this merge: H-003 (Codex R1-02, the work folder stays after an
assertion failure) and H-004 (Opus R1-01, folders and child processes stay
after a stop by a signal), both low, and H-005 (a wall-clock limit that
stays in a test of the shared helper). Requests of the worker to the
coordinator: none.

## Workstream layout-perf (branch `ws/layout-perf`, head d8fa6c0, one squashed commit, batch 1 of 2026-11-11)

Merged as a small block under owner instruction 17 (`docs/CONTRIBUTING.md`,
section "Commit cadence"). Content: in `randoc-layout`, a concatenation of
documents is one node `Node::Concat(Vec<Doc>, Summary)`. `+` appends to a
left concatenation in amortized constant time and never copies a right one.
`Summary` keeps the item count and `is_non_blank` of the first item, so
`offset`, `min_offset` and `update_column` are linear and `==` rejects
different item counts at once. The `Debug` text is the text of `main`.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The gate script passed on the branch head d8fa6c0 at 18:24 UTC (worker report): baseline 2217 units, 0 regressions, 0 new passes. The merged tree is checked at the end of the batch. |
| 2. Measured progress | No harness unit changes (a performance change). Tests `findings_of_round_one_are_linear` and `each_operation_is_linear_on_each_shape` count traversal steps: each ratio for 4 times the input is 4.0. |
| 3. Tests | 27 crate tests and 5 documentation tests pass. Line coverage 98.15 % (`lib.rs` 97.88 %, `render.rs` 98.38 %). Mutation sample of the diff `fde1bee..d8fa6c0`: not done (time limit of the worker). |
| 4. No panics | No parser changes. Stack tests `long_chains_use_no_call_stack` and `each_shape_uses_no_call_stack` pass with 100,000 steps on a 2 MB stack. |
| 5. Reviews | Round 1 on fde1bee: Codex REQUEST CHANGES (1 high, 1 low), Opus REQUEST CHANGES (1 medium, 3 low); the worker corrected each one in d8fa6c0. Round 2 by the resumed reviewers: Opus APPROVE at d8fa6c0 (each round 1 finding closed, no new finding); Codex in progress at the merge (`layout-perf-codex-r2.md` does not exist). Two approvals are not necessary under instruction 17. |
| 6. No slowdowns | Not measured at the merge; due at the review of the major block. The merge changes code of a path that existed on `main` before (the layout engine of the writers). Worker bench on a 997,475-byte document, CPU time, minimum of 7 runs, `main` / head: build and render 1.019, render at width 72 0.983, at width 20 0.968, no wrap 0.953. |
| 7. Clean build | The gate script passed on the branch head (see gate 1). The merged tree is checked at the end of the batch. |

Licence scan: no hit. Open findings: none with the level medium or higher.
Open checks: the mutation sample of the diff `fde1bee..d8fa6c0`; the Codex
report of round 2. Requests of the worker to the coordinator: none.

## Workstream yaml (branch `ws/yaml`, head 7170169, one squashed commit, batch 1 of 2026-11-11)

Merged as a small block under owner instruction 17. Content: the crate
`randoc-yaml`, a YAML reader for metadata blocks and metadata files with the
values and the error texts of Pandoc, its replay test against stored oracle
results, the changed-fixture campaign (`tests/changed.json`) and the tool
`tools/replay.py`. No program path calls the crate yet.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The gate script passed on the branch head 7170169 (second run, ended 13:46 UTC; worker report): baseline 2217 units, 0 regressions, 0 new passes. The merged tree is checked at the end of the batch. |
| 2. Measured progress | No harness unit changes (no program path calls the crate). Complete replay: 18,178 cases (17,178 metadata blocks, 1,000 files; 8,901 accepted, 8,631 with a value), 0 differ, 421 known divergences. Changed-fixture test: 654 texts, 0 wrong accepts, 0 wrong rejects (the control with a loose comparison gives 714 wrong accepts). |
| 3. Tests | `cargo test -p randoc-yaml` (13:12 UTC): 26 unit tests pass (1 ignored), 6 replay tests pass (2 ignored). Mutation sample and coverage of the last unit: not run. |
| 4. No panics | Fuzzing of the last unit (`parse`, 900 s): not run. |
| 5. Reviews | 8 rounds. Round 7 on 44a99b9: Opus APPROVE, Codex REQUEST CHANGES (2 medium, corrected in 7170169). Round 8 on 7170169: Opus REQUEST CHANGES (2 medium: yaml-R8-01, yaml-R8-02; 2 low), Codex REQUEST CHANGES (2 medium: yaml-R8-01, the same finding as Opus yaml-R8-02, and yaml-R8-02). No critical finding is open. Two approvals are not necessary under instruction 17. |
| 6. No slowdowns | Not measured at the merge; due at the review of the major block. The merge changes no code of a path that existed on `main` before. |
| 7. Clean build | The gate script passed on the branch head (see gate 1). The merged tree is checked at the end of the batch. |

Licence scan: no hit. Open findings with the level medium: I-YAML-15
(Opus yaml-R8-01), I-YAML-16 (Opus yaml-R8-02 and Codex yaml-R8-01),
I-YAML-17 (Codex yaml-R8-02). The low findings of round 8 (Opus yaml-R8-L01,
yaml-R8-L02; error texts only) are open. Open checks: fuzzing, the mutation
sample, coverage and the time table at 50,000 repetitions of the last unit.
Requests of the worker to the coordinator: none.

## Workstream citeproc (branch `ws/citeproc`, head 9459e58, one squashed commit, batch 1 of 2026-11-11)

Merged as a small block under owner instruction 17. Content: the crate
`randoc-citeproc` (a port of `citeproc` 0.14.1.1, ADR 17). Unit 1: the
CSL style and locale parser with its XML reader (`quick-xml` `=0.42.0`,
ADR 10). Unit 2: the reference model, the CSL JSON form with its string
markup, and the reader and writer `csljson`, registered in
`crates/randoc/src/formats.rs`.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The gate script passed on the branch head 9459e58 at 18:30 UTC (worker report): harness 4867 units, 2506 pass; baseline 2217 units, 0 regressions, 289 new passes. The merged tree is checked at the end of the batch. |
| 2. Measured progress | Suite `csljson/`: 321 units, 289 pass, 0 fail, 32 skipped (write units of inputs that the reader refuses). Parse replay of styles and locales: 145 of 145 equal. Open divergences: I-CITE-04, I-CITE-05, I-CITE-08, I-CITE-09, I-CITE-11, I-CITE-12 (low), I-CITE-10 (deliberate). |
| 3. Tests | Line coverage 94.1 % of the crate (at 59faa89; `csljson.rs` 71.8 %, because the harness suite runs its writer paths). Mutation sample: 9 of 660 mutants of the diff (shard 1/80): 2 caught, 6 unviable, 1 missed, which the test `the_depth_limit_counts_levels_exactly` now catches. |
| 4. No panics | Fuzzing, 0 crashes: `markup_parse` 901 s, `csljson_read` 901 s, `style_parse` 901 s, `locale_parse` 901 s (the last two after the XML corrections). Depth tests on a 2 MB stack at 10,000 and 100,000 levels give the depth error. |
| 5. Reviews | Round 1 on 60b097d (unit 1 only): Codex REQUEST CHANGES (1 high, 7 medium, 5 low), Opus REQUEST CHANGES (1 medium, 4 low). The worker reports each finding with the level medium or higher as closed in 9459e58. Opus citeproc-R1-04 (low) is open (I-CITE-13). Unit 2 has no review. The review of units 1 and 2 is due as the review of this major block. Two approvals are not necessary under instruction 17. |
| 6. No slowdowns | Not measured at the merge; due at the review of the major block. The merge changes no code of a conversion path that existed on `main` before; it adds the formats `csljson` to the format table. |
| 7. Clean build | The gate script passed on the branch head (see gate 1). The merged tree is checked at the end of the batch. |

Licence scan: only licence texts in the notice files. Open checks: the
mutation sample (9 of 660 mutants ran); the time table at 50,000
repetitions on the final code. Requests of the worker to the coordinator:
one shared Haskell `show` of a string in `randoc_core::shared` (Opus
citeproc-R1-04); a public JSON value parser in `randoc-ast` (I-CITE-09).

## Workstream math (branch `ws/math`, head 63cd0e6, one squashed commit, batch 1 of 2026-11-11)

Merged as a small block under owner instruction 17. Content: the crate
`randoc-math`: a TeX math parser with macros, and two writers with the
output of `texmath` as Pandoc 3.13 uses it: MathML text and a list of inline
elements. The tables of `src/symbols.rs` hold oracle results for 1,013
candidate command names from KaTeX 0.16.11 (ADR 14). No program path calls
the crate yet.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The gate script passed on the branch head 63cd0e6 (18:26 to 18:41 UTC; worker report): baseline 2217 units, 0 regressions, 0 new passes. The merged tree is checked at the end of the batch. |
| 2. Measured progress | No harness unit changes (no program path calls the crate). Replay `tests/oracle.tsv`: 35,904 formulas; MathML equal 35,068, inline equal 35,307, warnings equal 21,214 of 25,031; 118 formulas convert differently (I-MATH-06 (a) to (e), I-MATH-03 (a)); each other difference is a Randoc error with a message. Pair run of 237,006 formulas: 0 wrong conversions. |
| 3. Tests | Line coverage of `randoc-math` 97.89 % (2,279 of 2,328 lines; check on 63cd0e6 at 19:01 UTC). Step test `a_flat_formula_takes_a_linear_number_of_steps` at 1,000 and 4,000 repetitions passes; the 50,000 variant did not run. Mutation sample: not run. |
| 4. No panics | Fuzz target `convert` on 63cd0e6: 901 s, 2,122,308 runs, 0 crashes, peak memory 854 MB (check at 19:01 UTC). |
| 5. Reviews | Round 1 on c6dac3f and round 2 on baae821: Codex and Opus REQUEST CHANGES each time (round 2: Codex 3 high, 4 medium; Opus 2 high, 2 medium). The worker reports each finding as corrected or recorded in 63cd0e6. No review of 63cd0e6 exists. Two approvals are not necessary under instruction 17. |
| 6. No slowdowns | Not measured at the merge; due at the review of the major block. The merge changes no code of a path that existed on `main` before. |
| 7. Clean build | The gate script passed on the branch head (see gate 1). The merged tree is checked at the end of the batch. |

Conflicts in the merge: `Cargo.toml` (members and dependency lines of
`randoc-citeproc` and `randoc-math`: both kept), `docs/ADR.md` (ADR 14 of
the branch and ADR 17 of `main`: both kept, in number order),
`docs/PRIOR_ART.md` (the citeproc rows and the KaTeX row: both kept),
`THIRD_PARTY_NOTICES.md` and `docs/ISSUES.md` (`resolve-tables`: 1 and 6
rows of the branch added).

Licence scan: no hit. Open findings: I-MATH-06 (low); no review of the head
exists, so no finding of the head is known. Open checks: the mutation
sample; the step test at 12,500 and 50,000 repetitions. Requests of the
worker to the coordinator: none.

## Workstream lua (branch `ws/lua`, head dab508e, one squashed commit, batch 1 of 2026-11-11)

Merged as a small block under owner instruction 17. Content: the crate
`randoc-lua` with the 59 unchanged Lua 5.4.8 source files (built by the
build script with `cc`; ADR 11), the binding `mlua` `=0.12.2` and
`mlua-sys` `=0.13.0`, one `unsafe` call (ADR 11), the document types as a
port of `pandoc-lua-marshal` 0.3.2.1 without its GPL files (ADR 16), Lua
filters, the restricted and the full mode, and the modules `pandoc.path`,
`pandoc.text`, `pandoc.json`, `pandoc.format`, parts of `pandoc.utils`,
`pandoc.system` and `pandoc.structure`. No program option calls the crate
yet.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The gate script passed on 9228da6 (run ended 18:41 UTC; worker report): harness 4546 units, 2217 pass; baseline 2217 units, 0 regressions. The decision log names a passed gate on the head dab508e (18:45 UTC); dab508e changes only the test data file `tests/replay/cases.txt`. The merged tree is checked at the end of the batch. |
| 2. Measured progress | No harness unit changes (no program option calls the crate). Module matrix `tests/replay/modules.txt`: 21 filters, 1945 probes, 42 runs, 0 differences. Replay of `cases` (313 runs) and `walk` (164 runs): equal. |
| 3. Tests | 30 unit tests (2 ignored), 3 replay tests and 1 documentation test pass (worker report). Line coverage 92.20 % of 5065 lines (at 52d5ea7); `show.rs` 81.5 %, below 85 %, not measured again after its matrix group. Mutation sample: not run. |
| 4. No panics | Fuzzing, 0 crashes: `lua_roundtrip` 901 s, 5,997,589 runs, peak 558 MB; `lua_modules` 901 s, 196,303 runs, peak 471 MB. Depth tests on a 2 MB stack in both modes at 24 to 100,000 levels. |
| 5. Reviews | Round 1 on 9f39537: Opus REQUEST CHANGES (1 critical lua-R1-01, a binary chunk in restricted mode; 1 high lua-R1-02, the depth check; 3 medium; 4 low). The Codex run was stopped by its service and has only a partial report (no verdict). The worker reports lua-R1-01 and lua-R1-02 as closed with tests (71042e4, e504406), lua-R1-03 and lua-R1-04 as closed; lua-R1-05 is open (I-LUA-11). No review of the head exists. No known critical finding is open. Two approvals are not necessary under instruction 17. |
| 6. No slowdowns | Not measured at the merge; due at the review of the major block. The merge changes no code of a path that existed on `main` before. |
| 7. Clean build | The gate script passed on the branch (see gate 1). `cargo build` needs a C compiler. The merged tree is checked at the end of the batch. |

Conflicts in the merge: `Cargo.toml` (members and fuzz excludes: both
kept), `docs/ADR.md` (ADR 16 of the branch: kept, in number order between
ADR 14 and ADR 17), `Cargo.lock` (the version of `main`; the build wrote it
again), `THIRD_PARTY_NOTICES.md` and `docs/ISSUES.md` (`resolve-tables`: 6
and 14 rows of the branch added).

Licence scan: licence texts, lists of function names and format names,
oracle output (all permitted). Open findings with the level medium:
I-LUA-11 (walk time, lua-R1-05) and I-LUA-13 (the C locale of the Lua
state). Open checks: the mutation sample; coverage of `show.rs`. Requests of
the worker to the coordinator: host methods for `pandoc.system` and
`path.exists` that `randoc_core::Host` does not have; a decision on the
locale of the Lua state (I-LUA-13).
