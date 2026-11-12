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

## Batch 1 of 2026-11-11

Branches, in merge order, each one squashed commit on `main`:
`layout-perf` d8fa6c0 (fabec83), `yaml` 7170169 (dda9676), `citeproc`
9459e58 (5c71e99), `math` 63cd0e6 (9bce369), `lua` dab508e (18cf178). No
branch was left out or undone.

`merge-ws check` on the final tree (18cf178) passed on its first run
(19:07 to 19:14 UTC): `record` 0 new units, 0 oracle errors;
`verify-oracle` 4465 units, 0 stored results are not current;
`scripts/gate.sh` passed (format, lints, documentation, tests, harness).

- Harness before the batch: 4546 units, 2217 pass, 1959 fail, 0 no result,
  0 not recorded, 370 skipped.
- Harness after the batch: 4867 units, 2506 pass, 1959 fail, 0 no result,
  0 not recorded, 402 skipped; baseline 2217 units, 0 regressions, 289 new
  passes (the suite `csljson/` of `citeproc`). `spec/baseline.json` now has
  2506 units.
- Capability summary: `{"extensions": {"not_started": 90}, "input_formats":
  {"failing": 1, "not_started": 48, "partial": 2, "passing": 1}, "options":
  {"failing": 4, "not_started": 103, "passing": 2}, "output_formats":
  {"not_started": 74, "partial": 2, "passing": 1}, "units": {"passing":
  2506, "total": 4465}}`.

## Workstream html-writer (branch `ws/html-writer`, head fb6c151, one squashed commit, batch 2 of 2026-11-11)

Merged as a small block under owner instruction 17. Content: the crate
`randoc-html` with the writers `html` and `html4` (sections, notes, tables,
lists, `--incremental` list classes, wrapper divs), registered in
`crates/randoc/src/formats.rs`, and its oracle matrices (`src/joint.jsonl`:
28,965 cases).

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The gate script passed on the branch head fb6c151 (run ended 18:43 UTC; worker report): harness 6966 units, 3486 pass; baseline 2217 units, 0 regressions, 1269 new passes. The merged tree is checked at the end of the batch. |
| 2. Measured progress | 1269 new passing units on the branch. Oracle matrices: joint 28,965 cases, `sections` 20,636, `classes` 4,912, `notes` 8,316, each with 0 differences (test `the_oracle_matrices_replay_with_no_difference`). Scope gaps: math conversion and syntax highlighting. |
| 3. Tests | `cargo test -p randoc-html`: 290 + 1 pass (on 74c37f6). Mutation sample and coverage: not run. |
| 4. No panics | No parser changes, so no fuzz run is due. 10,000 nested notes give the depth error (no stack overflow). |
| 5. Reviews | 5 rounds. Round 5 on eb29e00: Codex and Opus REQUEST CHANGES (1 critical, 2 high and 12 medium in all). The worker reports each one as closed in the branch, with the joint oracle matrix (0 differences). No review of the head exists. Two approvals are not necessary under instruction 17. |
| 6. No slowdowns | Not measured at the merge; due at the review of the major block. The merge changes no code of a path that existed on `main` before; it adds the writers to the format table. |
| 7. Clean build | The gate script passed on the branch head (see gate 1). The merged tree is checked at the end of the batch. |

Licence scan: generic table fragments and filler text only (4 inputs that
copied the manual were replaced in fb6c151). Open findings: I-HTML-28
(medium: time that grows faster than linear with the nesting depth; its
cause was the concatenation in `randoc-layout`, which batch 1 corrected;
the writer was not measured again after that correction) and I-HTML-29
(low). Open checks: the mutation sample, coverage. Requests of the worker
to the coordinator: none new.

Conflict in the merge: the root `Cargo.toml` (member and dependency line
of `randoc-yaml` and `randoc-html`: both kept).

The same commit adds the rows I-YAML-18 and I-YAML-19 (the low findings
Opus yaml-R8-L01 and yaml-R8-L02 of the `yaml` round 8 review, at the
request of the coordinator).

## Workstream html-reader (branch `ws/html-reader`, head a40e1e5, one squashed commit, batch 2 of 2026-11-11)

Merged as a small block under owner instruction 17. Content: the crate
`randoc-html-reader` with the reader `html`: a port of the `tagsoup`
tokenizer (ADR 13), the block and inline rules, tables with a cell budget
for the document, notes and the math and raw TeX extensions. It is
registered in `crates/randoc/src/formats.rs`.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The gate script passed on the branch head a40e1e5 at 18:50 UTC (worker report): baseline 2217 units, 0 regressions. The merged tree is checked at the end of the batch. |
| 2. Measured progress | New harness units of the reader (counted in the batch check). Open divergences: I-HTMLR-03 to I-HTMLR-19; scope gaps I-HTMLR-01, I-HTMLR-02, I-HTMLR-07. |
| 3. Tests | Crate tests on 4c85bd9: 35 passed, 1 ignored. Line coverage 97.11 % on 211400b (check at 13:33 UTC), not measured on the head. Mutation sample: not run. |
| 4. No panics | Fuzz target `html_read` on a40e1e5: 901 s, 2,514,691 runs, 0 crashes, 0 timeouts, peak memory 739 MB (`checks-fuzz-a.md`, check 1). |
| 5. Reviews | Round 1 on b346988: Codex and Opus REQUEST CHANGES. Round 2 on 211400b: Opus REQUEST CHANGES (1 critical html-reader-R2-01, the cell limit for each table; 1 high R2-02; 3 medium R2-03 to R2-05; 2 low); the Codex run of round 2 was lost when the computer stopped (no report). The worker closed R2-01, R2-02, R2-03 and R2-04 in a40e1e5 (55781f2, 4c85bd9); R2-05 is open (I-HTMLR-19); the low findings are in I-HTMLR-18. No review of the head exists. No known critical finding is open. Two approvals are not necessary under instruction 17. |
| 6. No slowdowns | Not measured at the merge; due at the review of the major block. The merge changes no code of a path that existed on `main` before; it adds the reader to the format table. Time table on 211400b: no class above a ratio of 2.6 for 2 times the input. |
| 7. Clean build | The gate script passed on the branch head (see gate 1). The merged tree is checked at the end of the batch. |

Conflicts in the merge: the root `Cargo.toml`, `crates/randoc/Cargo.toml`
and `crates/randoc/src/formats.rs` (registration and dependency lines: both
sides kept), `docs/ADR.md` (ADR 13 of the branch: kept, in number order),
`Cargo.lock` (the version of `main`; the build wrote it again),
`THIRD_PARTY_NOTICES.md` and `docs/ISSUES.md` (`resolve-tables`: 2 and 19
rows of the branch added).

Licence scan: the file `parse.rs` holds alphabetical lists of HTML tag
names that are equal to lists in the Pandoc source file `TagCategories.hs`
(37 runs of 8 words). The coordinator classes them as lists of names that
follow from the tested behaviour: the worker made its tag matrices from
oracle probes and was forbidden to open the Pandoc source. Item for the
reviewers of the major block: check this classification.

Open findings: I-HTMLR-19 (medium: lost text in a malformed note list) and
the low rows. Open checks: mutation, coverage and the time table on the
head. Requests of the worker to the coordinator: a field
`max_generated_cells` (200,000) in `randoc_core::Limits` for the table
budget.

## Workstream markdown-reader (branch `ws/markdown-reader`, head 0b983bf, one squashed commit, batch 2 of 2026-11-11)

Merged as a small block under owner instruction 17. Content: the crate
`randoc-markdown` with the reader `markdown` and its variants (block and
inline rules, tables, notes, citations, extensions), registered in
`crates/randoc/src/formats.rs`. The crate uses the dependency
`saphyr-parser` for YAML metadata until it moves to `randoc-yaml`; the
dependency line stays.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The gate script passed on the branch head 0b983bf (18:46 to 18:53 UTC; worker report): baseline 2217 units, 0 regressions, 1583 new passes. The merged tree is checked at the end of the batch. |
| 2. Measured progress | Suite `markdown/`: 1360 units, 1321 pass, 39 fail. Random differential runs (2,000 documents for each seed): 55 to 114 differ; the target of 1 % is not met. |
| 3. Tests | `cargo test -p randoc-markdown`: 40 pass, 2 ignored; step-count test `time_grows_about_linearly` passes. Mutation sample (413 mutants of the diff from 764eee8), coverage and the time table: not run. |
| 4. No panics | Fuzz target `markdown_read` on 0b983bf: 901 s, 1,953,581 runs, 0 crashes, peak memory 1176 MB (`checks-fuzz-b.md`, check 1). An old timeout file of 10 November runs in 0.02 s on the head. |
| 5. Reviews | 2 rounds, both in session 2: Codex and Opus REQUEST CHANGES in each round. The three units of session 3 have no review. Two approvals are not necessary under instruction 17. |
| 6. No slowdowns | Not measured at the merge; due at the review of the major block. The merge changes no code of a path that existed on `main` before; it adds the reader to the format table. |
| 7. Clean build | The gate script passed on the branch head (see gate 1). The merged tree is checked at the end of the batch. |

Conflicts in the merge: the root `Cargo.toml` and `crates/randoc/Cargo.toml`
(member and dependency lines: both sides kept), `Cargo.lock` (the version of
`main`; the build wrote it again).

Licence scan: 2 copied tables were replaced in 0b983bf; minimal table runs
remain. Open findings: the row I-MD-41 has 8 examined groups and some small
groups with no proven level; the worker says that they count as medium
until they are examined. Rows I-MD-37 and I-MD-38 (low). Scope gaps
I-MD-01 to I-MD-05 and I-MD-08. Open checks: mutation, coverage, the time
table. Requests of the worker to the coordinator: none.

## Workstream commonmark-reader (branch `ws/commonmark-reader`, head 22266f3, one squashed commit, batch 2 of 2026-11-11)

Merged as a small block under owner instruction 17. Content: the crate
`randoc-commonmark` with the readers `commonmark`, `gfm` and
`commonmark_x` (a port of the rules of `commonmark-hs`, ADR 12), the tools
`tools/commonmark-spec` and `tools/commonmark-cases`, registered in
`crates/randoc/src/formats.rs`. YAML metadata blocks are read with
`randoc-yaml`. The branch contains the merged commits of `ws/yaml`; the
squash changes no file of `crates/randoc-yaml` (`git diff --cached --stat`).

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The gate script passed on the branch head 22266f3 (18:36 to 18:51 UTC; worker report): baseline 2217 units, 0 regressions. The merged tree is checked at the end of the batch. |
| 2. Measured progress | Harness suite `commonmark/`: 1740 units, 1724 pass, 16 fail (`extra/sp-*`, scope gap I-CM-01). Specification 652 of 652 (at ba6e862). `matrices.py --compare`: 7,506 inputs, 0 different, 3 known (I-CM-17). YAML replay through the reader: 17,178 texts, 0 different. |
| 3. Tests | `cargo test -p randoc-commonmark`: 41 pass, 1 ignored. Mutation sample: 28 of 55 named mutants (21 caught, 6 missed, 1 timeout); shard 2/2 and the 162 mutants of the last unit not run. Coverage number: not measured. |
| 4. No panics | Fuzz target `read` on 22266f3: 901 s, 1,173,826 runs, 0 crashes, peak memory 604 MB (`checks-fuzz-b.md`, check 2). |
| 5. Reviews | 6 rounds. Round 6 on 412807c: Codex and Opus REQUEST CHANGES (1 high, 7 medium in all). The worker reports each one as closed in units b and c (the YAML findings by the move to `randoc-yaml`). No review of the head exists. Two approvals are not necessary under instruction 17. |
| 6. No slowdowns | Not measured at the merge; due at the review of the major block. The merge changes no code of a path that existed on `main` before; it adds the readers to the format table. |
| 7. Clean build | The gate script passed on the branch head (see gate 1). The merged tree is checked at the end of the batch. |

Conflicts in the merge: the root `Cargo.toml`, `crates/randoc/Cargo.toml`
and `crates/randoc/src/formats.rs` (member, dependency and registration
lines: both sides kept, a line that both sides have one time),
`docs/ADR.md` (ADR 12 of the branch: kept, in number order), `Cargo.lock`
(the version of `main`; the build wrote it again), `THIRD_PARTY_NOTICES.md`
and `docs/ISSUES.md` (`resolve-tables`: 6 and 5 rows of the branch added),
`docs/PRIOR_ART.md` (the "in use" row of the branch added; in the table
"Candidates to study", `main` had moved the row "Citations" and the branch
had moved the row "CommonMark and GFM reader" to the table of material in
use, so neither candidate row stays).

Licence scan: the copied inputs were replaced in 22266f3; generic table and
punctuation runs remain. Open findings: low rows (I-CM-10, I-CM-11,
I-CM-16, I-CM-17) and the scope gap I-CM-01 (`sourcepos`). Open checks:
half of the mutation sample, a coverage number. Requests of the worker to
the coordinator: none.

## Workstream cli (branch `ws/cli`, head cb85d5f, one squashed commit, batch 2 of 2026-11-11)

Merged as a small block under owner instruction 17. Content: the command
line of `randoc-cli` with the options of Pandoc 3.13, defaults files, the
resource search and the user data folder, input preparation (tab
expansion with `--tab-stop` and `--preserve-tabs`), the program suite
`cli/`, and one approved change in `randoc-core` (`Error::Utf8Decoding`
prints the byte with a space pad).

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The gate script passed on the branch head cb85d5f (18:59 to 19:05 UTC; worker report): 8074 units, 6362 pass, 1342 fail, 370 skipped; baseline 2217 units, 0 regressions. The merged tree is checked at the end of the batch. |
| 2. Measured progress | Suite `cli/`: 3535 units, 3457 pass, 78 fail. Replay `tests/resources.rs`: 2732 of 2732 resource cases and 236 of 236 UTF-8 cases equal. Differential tool `cli_diff.py` on 9d62b55: 3,548 command lines, 3,189 equal, 359 differ in 22 signatures (two medium classes corrected in 0eb932b). |
| 3. Tests | `cargo test -p randoc-cli`: all pass (64 unit tests, replay tests included). Mutation sample and coverage: not run. |
| 4. No panics | Fuzz targets on cb85d5f (`checks-fuzz-a.md`, check 2): `args` 901 s, 2,854,115 runs, 0 crashes, 0 timeouts, peak memory 722 MB; `defaults` 901 s, 13,796,134 runs, 0 crashes, 0 timeouts, peak memory 1124 MB. |
| 5. Reviews | 7 rounds. Round 7 on 5895561: Codex and Opus REQUEST CHANGES (3 high, 4 medium in all). The worker reports each one as closed in a46ec01. No review of the head exists. Two approvals are not necessary under instruction 17. |
| 6. No slowdowns | Not measured at the merge; due at the review of the major block. The merge changes code of a path that existed on `main` before: the program that each conversion and each conformance unit runs. |
| 7. Clean build | The gate script passed on the branch head (see gate 1). The merged tree is checked at the end of the batch. |

Conflicts in the merge: `Cargo.lock` (the version of `main`; the build
wrote it again) and `THIRD_PARTY_NOTICES.md` (`resolve-tables`: 3 rows of
the branch added). No source file of `crates/randoc` changed.

Licence scan: 8 cases that stored the text of Pandoc data files were
removed in cb85d5f (row I-CLI-31, scope gap); the remaining hits are the
program interface (completion scripts, error messages, the version text).

Rows closed by this merge: I-002 (the first command line; the branch
replaces it, and each remaining difference is a row I-CLI-NN) and
I-NATIVE-04 (tab expansion before the reader: the 3 units
`native/read/literal-tab` pass with the merged program). The same commit
removes a second copy of row I-002 with older text, which the squash of
`markdown-reader` (d5386de) had added to `docs/ISSUES.md`.

Open findings: the low rows I-CLI-28, I-CLI-29, I-CLI-30 and the other low
rows and scope gaps of the branch. Open checks: mutation, coverage.
Requests of the worker to the coordinator: none.

## Batch 2 of 2026-11-11

Branches and commits, in the real merge order: `html-writer` fb6c151
(4d4373f), `html-reader` a40e1e5 (6eed2df), the `yaml` test file of
8612488 (4bce317), `markdown-reader` 0b983bf (d5386de),
`commonmark-reader` 22266f3 (6ba44b5), `cli` cb85d5f (6089e1f). The order
changed from the list because `commonmark-reader` and `cli` waited for
their fuzz results (both 0 crashes). No branch was left out or undone.

`merge-ws check` on the final tree (6089e1f) passed on its first run
(19:46 to 19:58 UTC): `record` 0 new units, 0 oracle errors;
`verify-oracle` 20536 units, 0 stored results are not current;
`scripts/gate.sh` passed (format, lints, documentation, tests, harness).

- Harness before the batch: 4867 units, 2506 pass, 1959 fail, 0 no
  result, 0 not recorded, 402 skipped.
- Harness after the batch: 20938 units, 19375 pass, 1161 fail, 0 no
  result, 0 not recorded, 402 skipped; baseline 2506 units, 0 regressions,
  16869 new passes. `spec/baseline.json` now has 19375 units.
- Capability summary: `{"extensions": {"failing": 1, "not_started": 11,
  "partial": 9, "passing": 69}, "input_formats": {"not_started": 40,
  "partial": 8, "passing": 4}, "options": {"failing": 1, "partial": 22,
  "passing": 86}, "output_formats": {"not_started": 71, "partial": 4,
  "passing": 2}, "units": {"passing": 19375, "total": 20536}}`.
- Checks on the `yaml` head 7170169 that ran after its merge in batch 1
  (`yaml` report, section "Checks on 7170169"): fuzzing 901 s with
  10,812,449 runs and 0 crashes; coverage 96.35 % of lines; the mutation
  sample of the last diff, all 63 mutants: 42 caught, 11 unviable, 10
  missed, of which 2 are killed by the test file of 8612488 and 8 are
  equivalent with the reason of each in the report.
- `layout-perf` round 2, Codex (`layout-perf-codex-r2.md`): APPROVE at
  d8fa6c0; no finding with the level medium or higher; one low finding
  (layout-perf-R2-01, `Debug` formatter flags), recorded as I-LAYOUT-06 in
  this commit.

## Workstream contracts (branch `ws/contracts`, head cbead12, one squashed commit, session 4 of 2026-11-11)

Merged as a small block under owner instruction 17. Content: shared
contracts for the next units. `Limits::max_table_cells` (the HTML and
CommonMark readers use it in place of their own constants) and
`Limits::max_input_size`; 15 new methods of the trait `Host` with the
types `PathKind`, `PathTimes` and `ProcessOutput`; one Haskell `show` of a
string (`randoc_ast::json::{show_string, show_char}`, five copies removed);
the public JSON value parser `randoc_ast::json::parse_value`, which
replaces `serde_json` in `randoc-citeproc`; the pin `zip = "=9.5.0"` with
the licence texts of `zip`, `flate2` and `zlib-rs`; ADR 18.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The gate script passed on the branch head cbead12 (run ended 21:29 UTC; worker report). The merged tree is checked at the end of the batch. |
| 2. Measured progress | No new units. The `show` matrix: 2,600 strings against the oracle, 0 differences (`crates/randoc-ast/tests/show_matrix.tsv`). |
| 3. Tests | `cargo test` of `randoc-core`, `randoc-ast`, `randoc-citeproc`, `randoc-html-reader`, `randoc-commonmark` and `randoc-harness` on the merged tree: 327 pass, 0 fail, 6 ignored. Coverage and mutation tests: not run. |
| 4. No panics | Fuzzing: not run. |
| 5. Reviews | No review round exists for this block. It is a small block under owner instruction 17; its review is due with the next major block that uses these contracts. |
| 6. No slowdowns | Benchmarks: not run. |
| 7. Clean build | `cargo build --workspace --all-targets` passed on the merged tree. The gate script passed on the branch head (see gate 1). |

Conflicts in the merge: none. Licence scan: the only hits are lists of
HTML element names in `crates/randoc-commonmark/src/blocks.rs` and
`crates/randoc-html-reader/src/parse.rs`. These lists are on `main`
already, and the branch does not change them.

Open findings: the rows I-CONTRACTS-01, I-CONTRACTS-02 and I-CONTRACTS-03
of `docs/ISSUES.md`. Open checks: coverage, mutation tests, fuzzing,
benchmarks. Requests of the worker to the coordinator: I-CONTRACTS-03
(the `--sandbox` host), I-CONTRACTS-01 (a processor clock needs a
dependency or `unsafe`), and a fourth string escape in
`crates/randoc-markdown/src/yaml.rs` (`quote`), which this block did not
change.

## Workstream yaml (branch `ws/yaml-2`, head 33ed724, one squashed commit, session 4 of 2026-11-11)

Merged as a small block under owner instruction 17. Content: corrections
of `randoc-yaml` from review rounds 8 and 9. The end rule below a block
scalar (`after_block_scalar` with `opened_collections`: an item or a key at
the column of each collection that the header line opens), the exact
decimal comparison and the document marker rule of the replay test, the
`%YAML` version limits and texts, and the changed-fixture campaign
(`tools/replay.py --kinds`, 11,000 texts).

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The gate script passed on the branch head 33ed724 (run ended 23:22 UTC; worker log `env/tmp/yaml-s4/gate2.log`). The earlier pass on 63af78a: baseline 19375 units, 0 regressions, 0 new passes. The merged tree is checked at the end of the batch. |
| 2. Measured progress | Replay `tests/replay.json`: 33517 cases, 0 differ, 421 known divergences. Matrix `nested_end_rows`: 17,440 metadata blocks against the oracle, 0 differences in the decision, the shape of the value and the error text. Changed-fixture campaign: 11,000 texts, 0 wrong accepts, 0 wrong rejects, 355 parser differences (error text only, I-YAML-12). |
| 3. Tests | `cargo test -p randoc-yaml` on the merged tree: unit 27 pass (1 ignored), alias 2 pass, replay 6 pass (2 ignored). `cargo test -p randoc-commonmark` on the merged tree: 42 pass (1 ignored). Mutation on 63af78a: shard 0/6, 7 mutants, 7 caught; shards 1/6 to 5/6 not run. Coverage of the head: not run. |
| 4. No panics | Fuzz target `parse` on 63af78a: 8,056,542 runs in 901 s, 0 crashes. The head 33ed724 changes the parser again; no fuzz run on the head. |
| 5. Reviews | 9 rounds. Round 9 on 63af78a: Opus REQUEST CHANGES (1 medium, yaml-R9-01, corrected in 33ed724; 3 low), Codex REQUEST CHANGES (4 medium, 2 low; reports `yaml-opus-r9.md` and `yaml-codex-r9.md`). Codex yaml-R9-04 is the same finding as Opus yaml-R9-01 and is corrected. No review of the head exists. Two approvals are not necessary under instruction 17. |
| 6. No slowdowns | Not measured at the merge. Codex round 9 measured the changed paths at 25,000 and 50,000 repetitions: no growth faster than linear. |
| 7. Clean build | `cargo build --workspace --all-targets` passed on the merged tree. The gate script passed on the branch head (see gate 1). |

Conflicts in the merge: none. Licence scan: no hit.

Identifiers in `docs/ISSUES.md`: the branch added two rows with the
identifiers I-YAML-18 and I-YAML-19, which `main` already used (batch 2,
the low findings Opus yaml-R8-L01 and yaml-R8-L02). This commit gives the
two rows of the branch the free identifiers I-YAML-20 (Opus yaml-R9-01,
closed) and I-YAML-21 (Opus yaml-R9-L03, the tab lines below a block
scalar, low, open). The rows of `main` keep their identifiers.

Open findings: three medium rows from Codex round 9, each "Open; first
item of the next unit": I-YAML-22 (Codex yaml-R9-01, a document marker
skips all replay value checks; test code), I-YAML-23 (Codex yaml-R9-02, a
valid blank line with a tab is refused) and I-YAML-24 (Codex yaml-R9-03,
tab indentation is accepted when a key follows it). I-YAML-23 and
I-YAML-24 name I-YAML-21. Low: I-YAML-21, Opus yaml-R9-L01 and L02
(corrected in 33ed724), Codex yaml-R9-05 and yaml-R9-06 (not recorded as
rows). Open checks: mutation shards 1/6 to 5/6, coverage of the head,
fuzzing of the head. Requests of the worker to the coordinator: none.

## Workstream markdown-reader (branch `ws/markdown-reader-2`, head 91b869a, one squashed commit, session 4 of 2026-11-11)

Merged as a small block under owner instruction 17. Content: corrections
of the difference groups of I-MD-41 and of the random differential runs
(bare definition markers, lazy quote lines, list markers that the flavor
does not read, final backslashes, `\ ` before heading attributes, `[` and
`]` in link destinations, backtick fences in quotes, `#.` lists), and YAML
metadata through `randoc-yaml`: `crates/randoc-markdown/src/yaml.rs` now
only converts the node tree. The branch removes the dependency line
`saphyr-parser` from `crates/randoc-markdown/Cargo.toml`; the root
`Cargo.toml` keeps its `saphyr-parser` line because `randoc-cli` still
uses it.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The gate script passed on the branch head 91b869a (22:20 to 23:18 UTC; worker report): baseline 19375 units, 0 regressions, 39 new passes. The merged tree is checked at the end of the batch. |
| 2. Measured progress | Suite `markdown/`: 1398 units, 1345 pass, 53 fail (before: 1360, 1306, 54). Matrix `differ.py matrix lazy` (2,592 inputs): 437 differ on 98806b9, 73 on the head, 0 new. Random runs with the abbreviation list of Pandoc (3 seeds of 2,000 documents): 32, 72, 29 differ. YAML replay `tests/yaml_replay.json`: 17,196 block texts, 0 differ apart from 417 error texts that `randoc-yaml` gives alone. About 60 small difference groups (1 or 2 inputs each) of the differential runs have no proven level yet. |
| 3. Tests | `cargo test -p randoc-markdown` on the merged tree: 39 pass, 3 ignored. Coverage of the head: 95.69 % of lines (`yaml.rs` 87.04 %). Mutation sample (shard 1/20 of 300 mutants of the diff): 15 mutants, 14 caught, 1 timeout, 0 missed. |
| 4. No panics | Fuzz target `markdown_read` on 91b869a: 901 s, 1,200,552 runs, 0 crashes, peak memory 720 MB. |
| 5. Reviews | 2 rounds, both in session 2. The units of sessions 3 and 4 have no review. Review round 3 of the major block starts after this merge. Two approvals are not necessary under instruction 17. |
| 6. No slowdowns | Time table (rule 21c, debug program, 25,000 and 50,000 repetitions of 11 changed paths): largest ratio 2.7, peak memory 125 MB. |
| 7. Clean build | `cargo build --workspace --all-targets` passed on the merged tree. The gate script passed on the branch head (see gate 1). |

Conflicts in the merge: none (`docs/ISSUES.md` and `Cargo.lock` merged
without a conflict). Licence scan: the hits are code fragments of
character tests, a list of HTML element names and minimal pipe and grid
table runs in `block.rs`, `inline.rs` and `tables.rs`; the worker reports
no hit in new text.

Rows closed by this merge: I-MD-22 (low) and the old I-MD-41. Open
findings: the rows I-MD-41 to I-MD-53 of the branch (all low) and the
scope gaps I-MD-01 to I-MD-05 and I-MD-08. Open checks: the level of
about 60 small difference groups, the rest of the mutation sample.
Requests of the worker to the coordinator: `abbreviations()` in
`crates/randoc-cli/src/convert.rs` must return
`randoc_core::shared::default_abbreviations()` when the data folder has no
`abbreviations` file (I-MD-08, 15 units); the alias limit error text of
`randoc-yaml` (I-MD-53).

## Workstream latex-reader (branch `ws/latex-reader`, head af83697, one squashed commit, session 4 of 2026-11-11)

Merged as a small block under owner instruction 17. Content: the new crate
`randoc-latex-reader` with the reader `latex`, unit 1: the tokenizer, macro
definitions (`\newcommand`, `\renewcommand`, `\providecommand`, `\def`,
`\let`, `\newenvironment`) with a depth limit and a token budget, raw TeX,
and the most frequent constructs (text, accents, fonts, sections, lists,
verbatim, footnotes, links, figures, tables, math, references, citations,
title data, `\input` and `\include`). The tool `tools/latex_diff.py`, a fuzz
target `latex_read`, 523 conformance cases in 7 files, and five rows in
`docs/PRIOR_ART.md` (no reuse, so no ADR). It is registered in
`crates/randoc/src/formats.rs`.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The gate script passed on the branch head af83697 (run ended 23:15:57 UTC; worker report): baseline 19,375 units, 0 regressions, 497 new passes. The merged tree is checked at the end of the batch. |
| 2. Measured progress | Suite `latex-reader/`: 523 units, 497 pass, 26 fail. Differential run (`latex_diff.py diff`, 200 documents of the grammar, seed 11): 189 equal, 11 different in 4 classes. |
| 3. Tests | `cargo test -p randoc-latex-reader` on the merged tree: 11 pass, 1 ignored (the wall-time test). `cargo test -p randoc`: 6 pass. Coverage and mutation tests: not run. |
| 4. No panics | Depth tests, loop and budget tests and a step test for 17 repeated constructs pass. The fuzz target `latex_read` exists; the 900 seconds of fuzzing did not run. |
| 5. Reviews | No review round. This is unit 1 of a new workstream; the review of the major block is due after unit 2. Two approvals are not necessary under instruction 17. |
| 6. No slowdowns | Time table (debug program, 25,000 and 50,000 repetitions of 12 constructs) in the worker report. The merge changes no code of a path that existed on `main` before; it adds the reader to the format table. |
| 7. Clean build | `cargo build --workspace --all-targets` passed on the merged tree. The gate script passed on the branch head (see gate 1). |

Conflicts in the merge: none. The registration lines of the root
`Cargo.toml` (member, workspace dependency, fuzz exclude),
`crates/randoc/Cargo.toml` and `crates/randoc/src/formats.rs` merged
without a conflict, and each line of both sides stays. Licence scan: no
hit.

Open findings: the rows I-LTXR-01 to I-LTXR-06 of `docs/ISSUES.md`.
I-LTXR-02 has the level medium; it is recorded and it is the first item of
unit 2. The worker reports more known differences without a row (`\today`
in the harness, a space after `\i` in an accent, `\emph*{x}`, `\cites`,
and an unclosed ``` `` ``` repeated beyond the depth limit gives exit 64
where Pandoc gives text). Open checks: the 900 seconds of fuzzing,
coverage, mutation tests, the corpus run. Requests of the worker to the
coordinator: the text of `LogMessage::MacroAlreadyDefined` in
`randoc-core` (I-LTXR-04); a shared module for the `Inlines` join rules
(two copies, in `randoc-html-reader` and `randoc-latex-reader`).

## Session 4 of 2026-11-11, batch of three branches

Branches and commits, in the merge order: `yaml` 33ed724 (2c8e284),
`markdown-reader` 91b869a (c145443), `latex-reader` af83697 (6a01107). No
branch was left out or undone.

`merge-ws check` on the final tree (6a01107) passed on its first run
(23:36 to 00:12 UTC): `record` 0 new units, 0 oracle errors;
`verify-oracle` 21093 units, 0 stored results are not current;
`scripts/gate.sh` passed (format, lints, documentation, tests, harness).
After the check, `conformance/cases/cli/info.toml` has no completion case
(the four cases that c494dd5 removed did not come back).

- Harness before the batch: baseline 19375 units (`spec/baseline.json`).
- Harness after the batch: 21495 units, 19911 pass, 1182 fail, 0 no
  result, 0 not recorded, 402 skipped; baseline 19375 units, 0
  regressions, 536 new passes. `spec/baseline.json` now has 19911 units.
- Capability summary: `{"extensions": {"failing": 1, "not_started": 11,
  "partial": 10, "passing": 68}, "input_formats": {"not_started": 39,
  "partial": 9, "passing": 4}, "options": {"not_started": 1, "partial": 21,
  "passing": 87}, "output_formats": {"not_started": 71, "partial": 4,
  "passing": 2}, "units": {"passing": 19911, "total": 21093}}`. The input
  format `latex` is partial (497 of 523 read units pass).

## Licence correction (branch `ws/licence-2`, head f4fb436, one squashed commit, session 4 of 2026-11-12)

Merged as a small block under owner instruction 17. Content: a licence
correction from the licence audit of 2026-11-11 (report
`env/store/reports/licence-audit-s4.md`). The stored results of the three
FODT cases `fodt-empty`, `fodt-title` and `fodt-many` of
`templates/defaults` held the styles of the reference ODT of Pandoc
(GPL-2.0-or-later); the three cases are removed (row I-TPL-05). The inputs
of the table cases `simple`, `multiline` and `grid` of `native/tables` were
example tables of the Pandoc manual; new inputs with the same table
structure replace them, and their stored results are new. Row D6 of
`PROGRESS.md` records that the old commits still hold the removed text.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | All 9 units of the three changed table cases pass and are in the baseline. The merged tree is checked at the end of the batch. |
| 2. Measured progress | No new units. 3 FODT cases are removed from the suite `templates`. |
| 3. Tests | No crate test: the block changes test data and documents only. |
| 4. No panics | Not applicable (no code). |
| 5. Reviews | No review round. The licence audit of session 4 found the items. |
| 6. No slowdowns | Not applicable (no code). |
| 7. Clean build | `cargo build --workspace --all-targets` passed on the merged tree. |

Conflicts in the merge: none (`PROGRESS.md` and `docs/ISSUES.md` merged
without a conflict). Licence scan: the hits are tokens of the native AST
format (`AlignDefault`, `RowSpan 1`) in `native/tables.json` and lines of
the LaTeX default template (BSD-3-Clause, as `docs/ADR.md` records for
the templates) in `templates/defaults.json`.

Open findings: I-TPL-05 (scope gap). Open checks: a check outside the
repository that stores only a hash of the FODT results; the 6 test names
of class E in `spec/pandoc-testsuite.json` (the owner decides).

## Workstream html-reader (commit 7824b0d of the branch `ws/html-reader-2`, one squashed commit, session 4 of 2026-11-12)

Merged as a small block under owner instruction 17. Content: the
correction of a crash. A table header cell with a very large `colspan`
caused a panic (an arithmetic overflow) in the computation of the row
head. The row head of each body row is now the wrapping sum of the
`colspan` of its leading `th` cells and of each `th` above it that a
`rowspan` carries down, clipped to the columns. The worker continues on
the same branch, so this commit (not the branch head) is merged.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The gate script passed on 7824b0d (run ended 23:54 UTC; worker report): baseline 19375 units, 0 regressions, 268 new passes. The merged tree is checked at the end of the batch. |
| 2. Measured progress | Oracle matrix of 16,321 table inputs (`tools/differential.py cases`): 0 differences after the correction. Replay `tables rowhead-001..270`: 268 pass, 2 known differences (I-HTMLR-20). |
| 3. Tests | `cargo test -p randoc-html-reader` on the merged tree: 38 pass, 0 fail, 1 ignored. New tests `row_head_sums_of_the_largest_spans_do_not_overflow` and `the_row_head_counts_carried_and_leading_header_spans`. Coverage and mutation tests: not run. |
| 4. No panics | New fuzz seed `fuzz/seeds/html_read/table-spans.html`. The fuzz run of 300 s did not run. |
| 5. Reviews | Round 3 on main 98806b9: Opus REQUEST CHANGES (1 critical, 4 high, 7 medium, 5 low); Codex stopped by its service before the final message (its partial report names the same critical finding). This commit closes the critical finding (Codex html-reader-R3-01, Opus html-reader-R3-01). The other findings are in work on the same branch. |
| 6. No slowdowns | Not measured. |
| 7. Clean build | `cargo build --workspace --all-targets` passed on the merged tree. |

Conflicts in the merge: none (`docs/ISSUES.md` merged without a
conflict). Licence scan: no hit.

Open findings: the high, medium and low findings of round 3 (in work on
the branch); new row I-HTMLR-20 (low). Open checks: the fuzz run, coverage,
mutation tests.

## Workstream html-writer (branch `ws/html-writer-2`, head 789f670, one squashed commit, session 4 of 2026-11-12)

Merged as a small block under owner instruction 17. Content: the
corrections of review round 6 (the time of nested section divs:
`Html::leading_heading_level` walks each div one time); standalone output
through a template, with the new shared function
`randoc_core::shared::meta_context` (`meta_to_context`, `normalize_date`)
and the writer fields of `crates/randoc-html/src/standalone.rs`; `--toc`
and `--toc-depth`; math through `randoc-math` (`to_mathml`, `to_inlines`).
New workspace dependencies of `randoc-html`: `randoc-math`,
`randoc-templates`, and `randoc-data` as a dev dependency.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The gate script passed on the branch head 789f670 (run ended 23:22 UTC; worker report): baseline 19375 units, 0 regressions, 22 new passes. The merged tree is checked at the end of the batch. |
| 2. Measured progress | `tools/joint.py check`: 0 differences in 29,633 cases (668 new). Standalone replay `tools/standalone.py`: 690 cases, 0 differences. `tools/math_matrix.py`: 2,808 cases, 0 differences. |
| 3. Tests | `cargo test -p randoc-html -p randoc-core` on the merged tree: 372 pass, 0 fail. Coverage of 789f670 (lines): `html.rs` 99.31 %, `table.rs` 99.73 %, `lib.rs` 95.52 %, `standalone.rs` 79.08 % (below 85 %: the KaTeX `math` variable, error paths and the diagnostic output of the replay test are not covered), `meta_context.rs` 91.86 %. Mutation sample: not run. |
| 4. No panics | Fuzzing: not run (the template path is new). |
| 5. Reviews | Round 6 on main 98806b9: Opus REQUEST CHANGES (1 medium, 4 low), Codex REQUEST CHANGES (1 new medium on a report claim, the same time finding, 2 low). The worker corrected each finding with the level medium or higher in this branch. No review of the new code (standalone output, math) exists; it is due as the next review of this block. |
| 6. No slowdowns | Div chains with a div first: 0.06 / 0.13 / 0.29 s at 2,500 / 5,000 / 10,000 levels (before: 0.07 / 0.34 / 1.06 s). The CPU table of the new paths (toc, meta context, math) did not run. |
| 7. Clean build | `cargo build --workspace --all-targets` passed on the merged tree. |

Conflicts in the merge: none (`crates/randoc-core/src/shared/mod.rs`,
`docs/ISSUES.md`, `docs/PRIOR_ART.md` and `Cargo.lock` merged without a
conflict). Licence scan: one hit, 4 shingles of the CSS of `styles.html`
(a default template, BSD-3-Clause) in the recorded output
`src/standalone.jsonl`.

Rows closed: I-HTML-28 (medium), I-HTML-01 (scope gap, math, for the
writer). Open findings: I-HTML-31, I-HTML-32, I-HTML-33 (low), I-HTML-34
(scope gap: translations), I-HTML-35 (by design). Open checks: fuzzing,
the mutation sample, the conformance cases with `-s`, the coverage of
`standalone.rs`, the CPU table of the new paths. Requests of the worker to
the coordinator: the translation data for `abstract-title` (I-HTML-34);
the default template for `html` and `html4` with `-s` in `convert.rs`.

## Workstream cli (commit 81a4d37 of the branch `ws/cli-2`, one squashed commit, session 4 of 2026-11-12)

Merged as a small block under owner instruction 17. Content: the
corrections of review round 8 (the data folder of a nested `defaults`
key, the format from the first input file with a known extension,
`--citation-abbreviations` as ordered metadata, metadata files in UTF-16
and UTF-32, the `math-method` grammar, an empty data folder,
`--number-offset`, `--variable-json` with `=`); templates (the new module
`template`: `--template`, the default templates of `randoc-data`, partials,
`WriterOptions::template` and `WriterOptions::variables`); the default
abbreviation list `randoc_core::shared::default_abbreviations()` when the
data folder has no `abbreviations` file (I-MD-08); East Asian line breaks
in the program. `crates/randoc-cli/data/templates.bundle` is removed.
New workspace dependencies of `randoc-cli`: `randoc-data`,
`randoc-templates`, `randoc-layout`. The commit holds a merge of `main`
(the yaml and markdown commits of batch A), so those changes are equal on
both sides. The worker continues on the same branch, so this commit (not
the branch head) is merged.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The gate script passed on 81a4d37 (run 23:37:57 to 23:58:30 UTC; worker report): 21002 units, 19454 pass, 1146 fail, 402 skipped; baseline 19375 units, 0 regressions, 79 new passes. The merged tree is checked at the end of the batch. |
| 2. Measured progress | Suite `cli/` (gate of 19bc27f): 3565 units, 3486 pass, 79 fail (before: 3535, 3457, 78). `tests/resources.rs`: 3007 cases, 0 differences. Differential run on 9b2e18c: 3548 command lines, 3380 equal, 168 differ in 14 signatures, all recorded rows of level low or scope gaps. |
| 3. Tests | `cargo test -p randoc-cli` on the merged tree: 80 pass, 0 fail (73 unit tests, 7 integration tests). Coverage and the mutation sample: not run. |
| 4. No panics | Fuzzing: not run. |
| 5. Reviews | Round 8 on main 98806b9: Opus REQUEST CHANGES (1 high, 1 medium, 6 low); Codex stopped by its service (its partial report: 1 critical licence finding, corrected on `main` in c494dd5; 2 high, 3 medium, 7 low). The worker corrected each finding with the level medium or higher. |
| 6. No slowdowns | Not measured. The new loops (`remove_east_asian_line_breaks`, `template::variables`) are linear. |
| 7. Clean build | `cargo build --workspace --all-targets` passed on the merged tree. |

Conflicts in the merge: none (`docs/ISSUES.md` and `Cargo.lock` merged
without a conflict). Licence: after the squash,
`conformance/cases/cli/info.toml` has no completion case (`grep -c` gives
0), and `conformance/expected/cli/info.json` does not hold the four
completion records (its only matches are the option lines of the
`--help` output, unchanged from `main`). Licence scan: the hits are
program interface texts (error messages of the command line), tokens of
the native AST format, the CSS of `styles.html` (BSD-3-Clause) in recorded
output, and the licence text in `crates/randoc-cli/NOTICE.md`.

Open findings: the low rows I-CLI-35 and I-CLI-37 to I-CLI-46, the scope
gap I-CLI-36, I-CLI-43 (Markdown reader part). Open checks: fuzzing,
coverage, the mutation sample, `randoc-yaml` for defaults and metadata.
Requests of the worker to the coordinator: the path of the default
templates in `THIRD_PARTY_NOTICES.md` (now `crates/randoc-data/data/templates`);
the text of `LogMessage::NoTitleElement` (I-CLI-35); the East Asian line
break handling in the Markdown reader (I-CLI-43); closing I-HTMLR-11 and
I-HTMLR-12 (their 6 units pass); the check of I-CLI-36 in
`convert::convert` after the HTML writer renders templates.

## Session 4 of 2026-11-12, batch of five items

Items and commits, in the merge order: `licence-2` f4fb436 (4fd5203),
`html-reader` 7824b0d (7f8b193), `html-writer` 789f670 (24b5136), `cli`
81a4d37 (f6d82a9). The item `markdown-writer` 682724e is left out: the
squash gave a conflict in `crates/randoc-html/src/html.rs` that is not
purely additive (both sides changed the start of `html::write`: `main`
has `Html::new` and the template context of `html-writer`, the branch has
the new `write_blocks` with an `Html { .. }` literal). The branch must
merge `main` and resolve the conflict. No item was undone.

`merge-ws check` on the final tree (f6d82a9) passed on its first run
(00:30 to 01:21 UTC): `record` 0 new units, 0 oracle errors;
`verify-oracle` 21390 units, 0 stored results are not current;
`scripts/gate.sh` passed (format, lints, documentation, tests, harness).
After the check, `conformance/cases/cli/info.toml` has no completion case.

- Harness before the batch: baseline 19911 units (`spec/baseline.json`).
- Harness after the batch: 21792 units, 20243 pass, 1147 fail, 0 no
  result, 0 not recorded, 402 skipped; baseline 19911 units, 0
  regressions, 332 new passes. `spec/baseline.json` now has 20243 units.
- Capability summary: `{"extensions": {"failing": 1, "not_started": 11,
  "partial": 8, "passing": 70}, "input_formats": {"not_started": 39,
  "partial": 9, "passing": 4}, "options": {"not_started": 1, "partial": 22,
  "passing": 86}, "output_formats": {"not_started": 71, "partial": 3,
  "passing": 3}, "units": {"passing": 20243, "total": 21390}}`. The output
  formats `markdown`, `gfm`, `commonmark` and `plain` are not in this
  batch (the item `markdown-writer` is left out).

## Workstream html-reader (branch `ws/html-reader-2`, head 254b702, one squashed commit, session 4 of 2026-11-12)

Merged as a small block under owner instruction 17. Content: the
corrections of review round 3 after the crash correction (7f8b193).
Repeated math scans are now linear (Codex R3-02: 128 KB in 0.04 s;
before: 12.56 s). The text of a `textarea` is escaped again (Codex
R3-04). A nested quotation of the other kind is read before the outer
close (Codex R3-03, 8,307 oracle inputs equal). Relative column widths
(Codex R3-05), the attribute order of `main` (Codex R3-06), close tags in
the title (Codex R3-09), stray close tags in tables (Opus R3-02). A
licence sentence for the tag-name lists in
`crates/randoc-html-reader/NOTICE.md`. New row I-HTMLR-20 (low).

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The gate script passed on 254b702 (run ended 01:02 UTC; worker report): baseline 19375 units, 0 regressions, 1094 new passes. The merged tree is checked at the end of the batch. |
| 2. Measured progress | Oracle matrices of the worker: 8,307 quotation inputs, 1,331 width inputs, 202 `main` inputs, 492 stray close tag inputs, 408 title inputs, 122 `textarea` inputs: all equal. `tools/differential.py toml` on the 13 case files: 7,415 inputs, 14 differences, all known rows. |
| 3. Tests | `cargo test -p randoc-html-reader` on the merged tree: 40 pass, 0 fail, 1 ignored. Coverage and the mutation sample: not run. |
| 4. No panics | The fuzz run of 300 s on 7824b0d: 648,712 runs, 0 crashes. The fuzz run of 900 s: not run. |
| 5. Reviews | Round 3 on main 98806b9: Opus REQUEST CHANGES (1 critical, 4 high, 7 medium, 5 low); Codex stopped by its service (its partial report has the same findings). This block and 7f8b193 close the critical finding, 3 high and 4 medium findings. |
| 6. No slowdowns | CPU through the program at 2 sizes for the changed math and quotation paths: linear (math 64 KB 0.024 s, 128 KB 0.042 s). |
| 7. Clean build | `cargo build --workspace --all-targets` passed on the merged tree. |

Conflicts in the merge: `conformance/cases/html-reader/tables.toml` and
`conformance/expected/html-reader/tables.json`. The first commit of the
branch (7824b0d) is on `main` as 7f8b193, and `main` has no other change
in these paths, so the branch version holds the change of `main`; the
branch version is taken. `docs/ISSUES.md` merged without a conflict.
Licence scan: hits only in the licence text of
`crates/randoc-html-reader/NOTICE.md` and in the tag-name lists of
`src/parse.rs`; both round 3 reviewers confirmed these lists from the
behaviour of the reference, so they are not copied text.

Open findings: Codex R3-07 (row I-HTMLR-21), Codex R3-08 (I-HTMLR-22),
Codex R3-10 (I-HTMLR-23) and I-HTMLR-19, all medium and the first item of
the next unit; the rows for the low findings of round 3 are not written
yet. Open checks: the fuzz run of 900 s, coverage, the mutation sample,
the differential sweep on new seeds.

## Workstream latex-reader (commit 176adbb of the branch `ws/latex-reader`, one squashed commit, session 4 of 2026-11-12)

Merged as a small block under owner instruction 17. Content: unit 2 of
the LaTeX reader. It corrects a CRITICAL defect that is on `main` since
unit 1 (6a01107): a `tabular` row loop did not end at a stray `}` and
allocated memory without a bound (a process of 10 GB on 2026-11-12 at
01:26 UTC). It also corrects known differences (block groups, the star
rules, `\cites`, `\iftrue`/`\iffalse`/`\newif`, `\enquote`, `\char`,
`\mbox`, `\maketitle`), adds the rule cases `latex-reader/rules.toml`,
the command table `crates/randoc-latex-reader/COMMANDS.md` with
`tools/names.tsv`, and changes the text of
`LogMessage::MacroAlreadyDefined` in `crates/randoc-core` to the Pandoc
3.13 text (decision of the coordinator). The worker continues on the
same branch, so this commit (not the branch head) is merged. New row
I-LTXR-13 (critical, closed for the `tabular` row loop).

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The gate script passed on 176adbb (run ended 01:54 UTC; worker report): baseline 19911 units, 0 regressions, 2188 new passes. The merged tree is checked at the end of the batch. |
| 2. Measured progress | Harness suite `latex-reader/` (code of fbfeee5): 3625 units, 2685 pass. Rule matrix: 350 cases, 349 pass. Differential, seed 23: 499 of 500 equal (the one difference is corrected in fbfeee5). Pandoc test suite: 71 new passes, 0 lost. Command table: 162 equal, 68 partial, 4 different, 117 not started. |
| 3. Tests | `cargo test -p randoc-latex-reader -p randoc-core` on the merged tree: 92 pass, 0 fail, 1 ignored. New test `tests::a_stray_brace_in_a_table_ends`. Coverage and the mutation sample: not run. |
| 4. No panics | Fuzz run on fbfeee5: 135 of 900 s ran, 0 crashes, 1 timeout (the loop defect, corrected by this commit). The full fuzz run of 900 s: not run. |
| 5. Reviews | No review round for this workstream yet; the review of the major block is due after unit 3. |
| 6. No slowdowns | CPU through the program at 25,000 and 50,000 repetitions for 10 new constructs (worker report): each ratio 2.0 or less. |
| 7. Clean build | `cargo build --workspace --all-targets` passed on the merged tree. |

Conflicts in the merge: none (`docs/ISSUES.md` merged without a
conflict). Licence scan: 7 hits in `crates/randoc-core/src/logging.rs`,
all in the `lang` warning texts that are on `main` already (message
texts of the reference, not in the change of this commit).

Open findings: the audit of each other parser loop of the same shape
(unit 3 does it now; the class of I-LTXR-13 is not closed yet); I-LTXR-07
(medium), I-LTXR-08 (medium), I-LTXR-09 (low), I-LTXR-10 (low), I-LTXR-11
(high: for 7 corpus documents the reference stops with a parse error and
Randoc does not). Open checks: the full fuzz run (900 s), coverage, the
mutation sample.

## Workstream commonmark-reader (branch `ws/commonmark-reader-2`, head cc9b6f3, one squashed commit, session 4 of 2026-11-12)

Merged as a small block under owner instruction 17. Content: the
corrections of review round 7 and two ported extension modules. Source
lookup by binary search (Codex R7-05, high: linear at 2,000 and 8,000
sources). Task items as `taskListItemBlockSpec` (Opus R7-01, Codex
R7-03). Metadata blocks: the first document that is a mapping is the
block, NFC of each string, HTML blocks in inline values (Codex R7-01,
R7-02, R7-04, Opus R7-02). The absolute bottom of `processBs` (I-CM-17
closed). Ports of `Alerts.hs` (Unicode upper case of the name) and
`FancyList.hs` (letters beyond ASCII). A list item that starts with a
reference definition. The crate's own handling of
`east_asian_line_breaks` is removed: the command line does it. New row
I-CM-18 (low).

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The gate script passed on cc9b6f3 (run ended 02:00 UTC; worker report): baseline 20243 units, 0 regressions. The merged tree is checked at the end of the batch. |
| 2. Measured progress | 18 oracle matrices through the library: 9,389 inputs, 0 different. Differential run (2000 documents, seed 2026, program of 2908ae4): commonmark 1993, gfm 1996, commonmark_x 1965 equal. Harness suite `commonmark/`: 1740 units, 1724 pass (16 fail: `extra/sp-*`, I-CM-01). |
| 3. Tests | `cargo test -p randoc-commonmark` on the merged tree: 44 pass, 0 fail, 1 ignored. New test `tests::the_work_grows_linearly_with_the_number_of_sources`. Coverage and the mutation sample: not run. |
| 4. No panics | Fuzz `read` on 2908ae4: 901 s, 620,811 runs, 0 crashes. |
| 5. Reviews | Round 7 on main 98806b9: Codex REQUEST CHANGES (1 high, 4 medium, 3 low), Opus REQUEST CHANGES (2 medium, 6 low). The worker corrected each finding with the level medium or higher; the verification by the reviewers is due. |
| 6. No slowdowns | CPU table for 8 changed paths at 25,000 and 50,000 (worker report): each ratio 2.04 or less. |
| 7. Clean build | `cargo build --workspace --all-targets` passed on the merged tree. |

Conflicts in the merge: none (`docs/ISSUES.md` and
`THIRD_PARTY_NOTICES.md` merged without a conflict). Licence scan: hits
only in the BSD disclaimer of `crates/randoc-commonmark/NOTICE.md`, the
HTML block tag list of `src/blocks.rs` and two generic character tests
of `src/inlines.rs` (code fragments, not copied text).

Open findings: I-CM-01 (scope gap), I-CM-10, I-CM-18. 16 documents of the
differential run in two classes (`BulletList` with commonmark_x and gfm,
`Div`) are not reduced yet. Open checks: the specification run, coverage,
the mutation sample.

## Workstream markdown-writer (branch `ws/markdown-writer`, head 0d455dc, one squashed commit, session 4 of 2026-11-12)

Merged as a small block under owner instruction 17. New workstream on
`main`. Content: the crate `randoc-markdown-writer` with the writers
`markdown`, `gfm`, `commonmark`, `commonmark_x`, `plain` and the
variants (`markdown_strict`, `markdown_phpextra`, `markdown_mmd`). Tables,
figures and images in HTML form call the new public function
`randoc_html::render_blocks` (with one test) and the new internal
function `html::write_blocks` of `crates/randoc-html`; `html::write` is
unchanged. Math without math syntax uses `randoc_math::to_inlines`. The
branch merged `main` at 3d61cbb and resolved the earlier conflict in
`crates/randoc-html` (it keeps the code of `main`). New matrices as
cases: table forms (3360 units) and edges (2500 units). The last commit
(0d455dc) changes only `docs/ISSUES.md` on top of the merge commit
8464487. New row in `PROGRESS.md`.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The gate script passed on 8464487 (run ended 02:12 UTC; worker report): baseline 20243 units, 0 regressions, 17511 new passes. The merged tree is checked at the end of the batch. |
| 2. Measured progress | Writer harness at bf90c2c: 17162 units, 16982 pass, 180 fail. Per format: markdown 1207/1209, gfm 1171/1173, commonmark 1153/1160, commonmark_x 613/614, markdown_strict 728/734, plain 1163/1174. Large random trees (seed 7, 200 each) still differ: markdown 177, gfm 180, commonmark 172. |
| 3. Tests | `cargo test -p randoc-markdown-writer -p randoc-html -p randoc` on the merged tree: 317 pass, 0 fail, 1 ignored. Coverage and mutation tests: not run. |
| 4. No panics | Fuzzing: not run. `tests/limits.rs` passes (deep test 9.8 s). |
| 5. Reviews | No review round of the current code; the review of the major block is due. |
| 6. No slowdowns | CPU table: not run. |
| 7. Clean build | `cargo build --workspace --all-targets` passed on the merged tree. |

Conflicts in the merge: none. The lines of the branch in the root
`Cargo.toml`, `crates/randoc/Cargo.toml`, `crates/randoc/src/formats.rs`
and `crates/randoc-html` are additions only. Licence scan: no hit.

Open findings: large random trees still differ (I-MDW-11; many small
rules); 28 option units fail (I-MDW-05 `--ascii`, I-MDW-06 a reference
form; `-s` is not connected for these writers); `plain` passes 1163 of
1174 units; the other open rows I-MDW-01, -02, -04, -07, -08, -09, -12,
-13. Open checks: fuzzing, coverage, mutation tests.

## Session 4 of 2026-11-12, batch of three items

Items and commits, in the merge order: `latex-reader` 176adbb (97d69d8),
`commonmark-reader` cc9b6f3 (6e8fcd9), `markdown-writer` 0d455dc
(c0a90c0). No item was left out and no item was undone. No conflict.

`merge-ws check` on the final tree (c0a90c0) passed on its first run
(02:27 to 03:01 UTC): `record` 0 new units, 0 oracle errors;
`verify-oracle` 42480 units, 0 stored results are not current;
`scripts/gate.sh` passed (format, lints, documentation, tests, harness).
After the check, `conformance/cases/cli/info.toml` has no completion case
and `conformance/cases/templates/defaults.toml` has no `fodt-` case.

- Harness before the batch: baseline 21069 units (`spec/baseline.json`).
- Harness after the batch: 42882 units, 40768 pass, 1712 fail, 0 no
  result, 0 not recorded, 402 skipped; baseline 21069 units, 0
  regressions, 19699 new passes. `spec/baseline.json` now has 40768 units.
- Capability summary: `{"extensions": {"failing": 1, "not_started": 11,
  "partial": 23, "passing": 55}, "input_formats": {"not_started": 39,
  "partial": 9, "passing": 4}, "options": {"not_started": 1, "partial": 25,
  "passing": 83}, "output_formats": {"failing": 1, "not_started": 61,
  "partial": 9, "passing": 6}, "units": {"passing": 40768, "total":
  42480}}`. The output formats `markdown`, `gfm`, `commonmark`,
  `commonmark_x`, `plain` and `markdown_strict` are partial;
  `markdown_github`, `markdown_mmd` and `markdown_phpextra` pass; `markua`
  fails (0 of 1 unit).

## Workstream markdown-reader (commit dc2b736 of the branch `ws/markdown-reader-3`, one squashed commit, session 4 of 2026-11-12)

Merged as a small block under owner instruction 17. Content: the
correction of the two CRITICAL findings of review round 3 that are on
`main` (Codex markdown-reader-R3-01: the HTML closer cache lost document
text, because a remainder line reused the key of its line; Codex R3-02:
the HTML closer indices counted literal tags inside code, math, escapes,
comments and attribute values). The cache key is now the shared text and
the position in it, and the closer scans skip these tokens. In test
builds, `cache_tests.rs` compares each cached answer with a plain scan on
24,000 generated inputs. The worker continues on the same branch, so this
commit (not the branch head) is merged. New row I-MD-57 (high, open) for
the other findings of round 3 (the branch already used I-MD-54 to I-MD-56).

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The gate script passed on dc2b736 (worker statement in the launch message; the worker report has no section for this commit). The merged tree is checked at the end of the batch. |
| 2. Measured progress | Matrix `differ.py matrix closer` (commit message): 302 of 1,004 inputs differed on 3d61cbb, 108 now, none new (rows I-MD-54, I-MD-55). |
| 3. Tests | `cargo test -p randoc-markdown` on the merged tree: 40 pass, 0 fail, 3 ignored. Coverage and the mutation sample: not run. |
| 4. No panics | Fuzzing on dc2b736: not run. |
| 5. Reviews | Round 3 on main a0b9535: Codex REQUEST CHANGES (2 critical, 15 high, 3 medium), Opus REQUEST CHANGES (1 high, 4 medium, 8 low). This commit closes the two critical findings; the other findings are in work on the same branch. |
| 6. No slowdowns | CPU table: not run. |
| 7. Clean build | `cargo build --workspace --all-targets` passed on the merged tree. |

Conflicts in the merge: none (`docs/ISSUES.md` merged without a
conflict). Licence scan: hits only in the HTML tag list of
`src/block.rs` and generic character tests of `src/block.rs` and
`src/inline.rs` (code fragments, not copied text).

Open findings: I-MD-56 (high, Codex R3-03: superlinear time on many HTML
elements on one line), I-MD-57 (high: the other 15 high and 7 medium
findings of round 3), I-MD-54 and I-MD-55 (low). Open checks: fuzzing,
coverage, the mutation sample.

## Workstream latex-reader (commit 9dea9a8 of the branch `ws/latex-reader`, one squashed commit, session 4 of 2026-11-12)

Merged as a small block under owner instruction 17. Content: one shared
guard for each parser loop of the LaTeX reader (`Parser::round`). Each
loop that calls other rules stops with an error when a round consumes no
token, and all rounds together have a step budget of 64 rounds for each
input or expansion token plus 100,000. Rules that put consumed tokens
back count them. New module `src/stress.rs` with the stress test of 109
opening forms without a closer, with 23 stray closers and at the end of
the input, in 6 containers. This closes the class of the critical row
I-LTXR-13 for each loop of the reader. The worker continues on the same
branch, so this commit (not the branch head) is merged.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The gate script passed on 9dea9a8 (worker statement in the launch message; the top section of the worker report is for 176adbb). The merged tree is checked at the end of the batch. |
| 2. Measured progress | Stress test `stress::each_unclosed_or_stray_form_ends_without_the_loop_guard`: 31,392 inputs in each of 2 extension sets end; with the row loop of before 176adbb it reports 963 inputs. |
| 3. Tests | `cargo test -p randoc-latex-reader` on the merged tree: 14 pass, 0 fail, 1 ignored. New tests `stress::each_unclosed_or_stray_form_ends_without_the_loop_guard`, `stress::the_guard_stops_a_round_without_progress_and_at_the_budget`. Coverage and the mutation tests: not run. |
| 4. No panics | The fuzz run of 900 s: not run (unit 3 does it). |
| 5. Reviews | No review round for this workstream yet; due after unit 3. |
| 6. No slowdowns | CPU table: not run. |
| 7. Clean build | `cargo build --workspace --all-targets` passed on the merged tree. |

Conflicts in the merge: `crates/randoc-latex-reader/src/parse.rs` (the
new fields of the guard; the version of the commit is taken, because the
crate on `main` is equal to 176adbb, an ancestor of 9dea9a8) and
`docs/ISSUES.md` (`resolve-tables`; the row I-LTXR-13 takes the text of
the commit; `resolve-tables` put back the row I-CM-17, which `main`
removed when it closed the row, so the row is removed again). Licence
scan: 7 hits in `crates/randoc-core/src/logging.rs`, in the `lang`
warning texts that are on `main` already (message texts of the
reference).

Open findings: I-LTXR-07 (medium), I-LTXR-08 (medium), I-LTXR-09 (low),
I-LTXR-10 (low), I-LTXR-11 (high). Open checks: the fuzz run of 900 s,
coverage, the mutation tests.

## Workstream math (branch `ws/math-2`, head 3f1c845, one squashed commit, session 4 of 2026-11-12)

Merged as a small block under owner instruction 17. Content: the
corrections of review round 3 (each finding with the level medium or
higher; Opus math-R3-01 to R3-04, Codex math-R3-01 to R3-07), each one
rule from an oracle matrix, with 11,050 new rows in `tests/oracle.tsv`;
the new table `src/unprintable.rs` (711 ranges from probes of the
reference); and the warning text of the reference: the detail now shows
the line and the column of the problem and the `expecting` lines of the
reference. The warning text of this crate changes, so units of the
suites `html-writer` and `markdown-writer` with a stored warning text of
the reference can move from fail to pass.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The gate script passed on 3f1c845 (02:27 to 02:48 UTC; worker report): baseline 19911 units, 0 regressions. The merged tree is checked at the end of the batch. |
| 2. Measured progress | Pair run, seed 4: 237,552 formulas, 0 wrong conversions. Replay: 46,954 formulas, MathML equal 45,272, inline equal 45,549. Warning matrix: 1,216 formulas, MathML-channel warnings equal 1,700 of 1,798; replay warnings equal 29,162 of 31,961 (26,634 before). |
| 3. Tests | `cargo test -p randoc-math -p randoc-html -p randoc-markdown-writer` on the merged tree: 329 pass, 0 fail, 3 ignored. Coverage and the mutation sample: not run. |
| 4. No panics | Fuzz `convert` on 3f1c845: 901 s, 1,634,921 runs, 0 crashes. |
| 5. Reviews | Round 3 on main 98806b9: Opus REQUEST CHANGES (1 high, 3 medium, 8 low), Codex REQUEST CHANGES (3 high, 4 medium, 3 low). The worker corrected each finding with the level medium or higher; the verification by the reviewers is due. |
| 6. No slowdowns | `\newcommand\f[1]{#1}` and `\f%` 33,326 times: 0.05 s and 17 MB in a debug build (14.7 s before). CPU table: not run. |
| 7. Clean build | `cargo build --workspace --all-targets` passed on the merged tree. |

Conflicts in the merge: none (`docs/ISSUES.md` merged without a
conflict). Licence scan: no hit.

Open findings: I-MATH-01 (scope gap), I-MATH-02, I-MATH-06 (all low);
the low findings Opus math-R3-L1 to L6, L8 and Codex math-R3-08. Open
checks: coverage, the mutation sample.

## Workstream cli (branch `ws/cli-2`, head f11f643, one squashed commit, session 4 of 2026-11-12)

Merged as a small block under owner instruction 17. Content: unit 2 of
the command line. JSON filters (new module `filter`: the search of the
filter, the arguments and the environment of the reference, the
interpreter of a file without `#!`, the error texts, the order with
`-F`, `-L` and `filters:`). Defaults and metadata files are read with
`randoc-yaml`: `src/yaml.rs` is removed, the new module `src/node.rs`
keeps the JSON view of a node, and the root `Cargo.toml` line
`saphyr-parser` is removed (no crate uses it now). The HTML writer
renders the template of `randoc-data` with its partials (I-CLI-36
closed). One message text in `crates/randoc-core` changes
(`LogMessage::NoTitleElement`, a decision of the coordinator). Rows:
I-CLI-05 and I-CLI-36 closed; I-CLI-47 and I-CLI-48 new (low); I-CLI-35
(the level of `NoTitleElement`) is now medium.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The gate script passed on f11f643 (01:43 to 02:14 UTC; worker report): 21817 units, 20278 pass, 1137 fail, 402 skipped; baseline 20243 units, 0 regressions, 35 new passes. The merged tree is checked at the end of the batch. |
| 2. Measured progress | YAML replay: 2833 of 2833 defaults and metadata texts equal (old code: 2828). Filter cases `cli/reader-options/filter*`: 25 of 25 pass. `cli/standalone` and `html-writer`: 2564 units, 2535 pass, 29 fail. |
| 3. Tests | `cargo test -p randoc-cli` on the merged tree: 79 pass, 0 fail, 0 ignored. New test `defaults_and_metadata_texts_give_the_result_of_pandoc`. Coverage and the mutation sample: not run. |
| 4. No panics | Fuzz `defaults` on f11f643: 900 s, 4,256,750 runs, 0 crashes. Fuzz `args`: 900 s, 1,892,970 runs, 0 crashes. |
| 5. Reviews | Round 8 findings are corrected on `main`. No review of unit 2 (JSON filters, the move to `randoc-yaml`): due with the next review of this block. |
| 6. No slowdowns | Not measured. The new loops are linear (worker report). |
| 7. Clean build | `cargo build --workspace --all-targets` passed on the merged tree. |

Conflicts in the merge: none (`docs/ISSUES.md`, `Cargo.toml`,
`Cargo.lock` and `THIRD_PARTY_NOTICES.md` merged without a conflict).
Licence: after the squash, `conformance/cases/cli/info.toml` has no
completion case (`grep -c` gives 0). Licence scan: the hits are program
interface and message texts of the reference (`--version` text, option
errors, the `lang` warning texts in `crates/randoc-core/src/logging.rs`).

Open findings: I-CLI-35 (medium: ten `template-html` cases fail, because
`randoc-core` gives `NoTitleElement` the level WARNING and the reference
gives INFO), I-CLI-02 (Lua filters and citeproc), I-CLI-47, I-CLI-48 and
the other low rows. The matrix of 300 standalone command lines is not
done. Requests of the worker to the coordinator: remove the
`saphyr-parser` lines of `THIRD_PARTY_NOTICES.md` and `docs/PRIOR_ART.md`
and `third_party/licences/saphyr-parser.txt`. Open checks: coverage, the
mutation sample.

## Workstream markdown-reader (branch `ws/markdown-reader-3`, head ef9dae7, one squashed commit, session 4 of 2026-11-12)

Merged as a small block under owner instruction 17. The first commit of
the branch (dc2b736) is on `main` as 8508188. Content: the complete
correction of the two CRITICAL cache findings of review round 3 (Codex
markdown-reader-R3-01 and R3-02): the HTML closer cache, the div closer
cache and the span index keep the slice length and keep no nested answer
after an indented line, and `cache_tests.rs` compares each cache with the
plain scan on 24,000 generated inputs. Six time findings, each with a
step-count form in `time_grows_about_linearly` (Codex R3-04, R3-05,
R3-06, R3-13, R3-16, Opus R3-04); Codex R3-07 (nested-list line
shortcut); the abbreviation rule after an escaped period (finding of the
coordinator); a part of row I-MD-56 (the text after a comment, a
processing instruction or a tag on its line is read in a loop). Rows:
I-MD-56 changed, I-MD-57 (Codex R3-12) and I-MD-58 (Codex R3-18) new;
the summary row of `main` that had the identifier I-MD-57 is now I-MD-59.

| Gate | Evidence |
| --- | --- |
| 1. No regressions | The gate script passed on ef9dae7 (end 04:41 UTC; worker report): baseline 20243 units, 0 regressions, 45 new passes. It also passed on bfb1790 (03:27 to 04:02 UTC). The merged tree is checked at the end of the batch. |
| 2. Measured progress | Cache test: 0 differences in 24,000 inputs (92 with the old key). Matrix `differ.py matrix closer`: 302 of 1,004 inputs differ on 3d61cbb, 108 on dc2b736, 0 new. Abbreviation matrix: 6 of 280 differ before, 0 after. CPU s at 1,000, before then after: `<div><!-- c --> ` 2.20 / 0.25; `<div>\n<!--\n` 0.41 / 0.18. Suite `markdown/`: 1443 units, 1406 pass, 37 fail (bfb1790). |
| 3. Tests | `cargo test -p randoc-markdown` on the merged tree: 40 pass, 0 fail, 3 ignored. Coverage and the mutation sample: not run. |
| 4. No panics | Fuzzing: not run. |
| 5. Reviews | Round 3 on main a0b9535: Codex REQUEST CHANGES (2 critical, 15 high, 3 medium), Opus REQUEST CHANGES (1 high, 4 medium, 8 low). Closed: both critical findings, Codex R3-04, R3-05, R3-06, R3-07, R3-13, R3-16, Opus R3-04. |
| 6. No slowdowns | The step-count test passes; CPU table: not run. |
| 7. Clean build | `cargo build --workspace --all-targets` passed on the merged tree. |

Conflicts in the merge: `crates/randoc-markdown` (`src/block.rs`,
`src/cache_tests.rs`, `tools/differ.py`) and
`conformance/cases/markdown/review.toml`,
`conformance/expected/markdown/review.json`: the version of the branch is
taken (the newer one; these paths on `main` are equal to dc2b736).
`docs/ISSUES.md`: the rows I-MD-56 to I-MD-58 of the branch are taken,
and the summary row of `main` gets the identifier I-MD-59 with the list
of what is still open. Licence scan: hits only in the HTML tag list and
generic character tests of `src/block.rs` and `src/inline.rs`, and in
minimal table forms of `src/tables.rs` (code fragments and format facts,
not copied text).

Open findings: I-MD-56 (high, the rest of Codex R3-03), I-MD-57 (high,
Codex R3-12), I-MD-58 (high, Codex R3-18), I-MD-59 (high: Codex R3-08,
R3-09, R3-10, R3-11, R3-14, R3-15, R3-20 and Opus R3-01 high; Codex
R3-17, R3-19 and Opus R3-02, R3-03, R3-05 medium), I-MD-54 and I-MD-55
(low). Open checks: fuzzing, coverage, the mutation tests.
