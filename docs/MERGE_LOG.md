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
