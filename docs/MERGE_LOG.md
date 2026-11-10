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
