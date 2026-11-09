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
