# Contribution rules and the merge gate

Work happens in workstreams. One workstream owns one crate or one subsystem,
in its own branch and its own Git worktree. The coordinator merges a
workstream into `main` only when the full gate below passes.

Read [`ARCHITECTURE.md`](ARCHITECTURE.md) first.

## The merge gate

1. **No regressions.** `spec/baseline.json` lists each unit that passes on
   `main`. No unit of the baseline can fail.
2. **Measured progress.** The change raises the number of passing units. Each
   remaining difference from Pandoc is recorded as a known divergence with its
   reason in `docs/ISSUES.md`.
3. **Tests that find real defects.** Each new behaviour has conformance cases
   with results from the oracle, and the edge cases that the Pandoc manual
   describes. Run `cargo mutants` on the changed code. Each surviving mutant
   needs a new test or a written reason. Report line coverage of the new code
   with `cargo llvm-cov`. Coverage below 85% needs a written reason.
4. **No panics.** Each new or changed parser or archive reader has a fuzz
   target and 15 minutes of fuzzing with no crash.
5. **Two independent reviews.** One review is from GPT-6.1 Sol through the
   Codex CLI. One review is from an Opus 5.5 agent with a new context. Each
   reviewer tries to break the work. Critical, high and medium findings are
   fixed. Low findings go to `docs/ISSUES.md`.
   Severity levels of a finding:
   - **Critical.** Memory unsafety. A crash (panic, abort, stack overflow) or a run that does not end in 60 seconds, for an input below 1 MB through the program with default settings. Loss of document text for an input form that the Pandoc manual shows. A licence violation.
   - **High.** A crash or a stack overflow that needs the library interface with its default limits. Time or memory that grows faster than linear so that an input of 100 KB needs more than 5 seconds in a debug build. Wrong output, a wrong exit code or lost text for an input that occurs in real documents (the corpus, the Pandoc manual, or a form that the manual documents as normal use).
   - **Medium.** Wrong output for a form that an author can plausibly write on purpose, when the difference changes the meaning or the structure of the result (a different element, lost or added text, a wrong attribute). Time that grows faster than linear at any size. A wrong warning or a wrong error for a common mistake. A claim of the report that the tests do not support. A missing licence notice.
   - **Low.** A difference only for malformed or adversarial input (unbalanced or cut delimiters, random mixes that no author writes on purpose) where no text is lost. A difference only in the text or the position of an error or a warning for broken input. Documentation and code-quality items.

   A low finding is recorded as a known divergence in `docs/ISSUES.md` with its smallest input, and it stays open work. If you are not sure between two levels, choose the higher one.

   - **Scope gaps.** A row of `docs/ISSUES.md` that says that an extension, an option or a feature of a format is not implemented yet (for example the extension `sourcepos`, syntax highlighting, math conversion) is a scope gap. Gate 2 permits a merge with recorded scope gaps, so a scope gap is not a review finding and does not block an approval. The conditions: the row is exact, the gap loses no document text without a message, and the workstream report does not claim the feature. A row for a defect in behaviour that the workstream implements is a finding with a level: it must be low to stay open.
6. **No slowdowns.** Benchmarks of the changed paths are not more than 5%
   slower than `main`.
7. **Clean build.** `cargo fmt --all -- --check`, `cargo clippy` with
   `-D warnings` and `cargo doc` pass with no warnings. Each public item has
   documentation.

A gate that cannot apply to a change is recorded with its reason in
`PROGRESS.md`. The bar is never lowered without a record.

## Commit cadence (rule of the owner, 2026-11-11)

The owner changed gate 5 on 2026-11-11 (`OWNER_INSTRUCTIONS.md`,
instruction 17): the two reviews run after each major block of work, not
before each commit. A small block merges into `main` when all of these
hold: `scripts/gate.sh` passes on the merged tree (gates 1 and 7 and the
tests); the licence scan (`env/bin/licence-scan` on the coordinator
machine) shows no text that is copied from a copyleft source; no known
finding with the level critical is open. Each finding of an earlier
review that is still open is a row of `docs/ISSUES.md` with its level.
The record of the merge in `docs/MERGE_LOG.md` says which review rounds
exist for the block and which checks did not run (mutation tests,
coverage, fuzzing, benchmarks). A major block is a unit of a workstream
that adds a format, a subsystem or a large rule set. After a major
block, both reviewers review the state of `main` or of the branch, and
each finding with the level medium or higher is corrected in the next
commits. Gates 2, 3, 4 and 6 keep their text; their evidence is
collected for each major block.

## Reuse of open source

Rule of the owner (2026-11-10): do not build each part from zero. Study the
open-source code that does the same work, and build on it where that is
faster or more reliable. The full rule, the material in use and the list of
candidates are in [`PRIOR_ART.md`](PRIOR_ART.md).

Before you reuse anything:

1. Check its licence at the exact release. It must be compatible with
   `MIT OR Apache-2.0`. Pandoc itself and `texmath` are GPL and are not
   permitted.
2. Add a row to `docs/PRIOR_ART.md`.
3. Write a short record in [`ADR.md`](ADR.md) that says why you chose it.
4. Keep the attribution in `THIRD_PARTY_NOTICES.md` and the full licence
   text in `third_party/licences/`.

Prefer a dependency on a pinned upstream release to a copy of the code. Fork
or vendor only when you must change the code; keep the changes small,
describe them, and cover them with Randoc tests. Reused code passes the same
merge gate as your own code.

## Commands

Run each command from the root of your worktree.

```sh
# The mechanical checks of gates 1 and 7, and the tests.
scripts/gate.sh

# Conformance: record the oracle results for new cases, then compare.
cargo run -q -p randoc-harness -- record
cargo build -p randoc-cli && cargo run -q -p randoc-harness -- run --baseline spec/baseline.json
cargo run -q -p randoc-harness -- show 'SUITE/FILE/CASE#UNIT'

# Make the conformance suite a part of `cargo test` for the units of your branch.
# The test `crates/randoc-cli/tests/conformance.rs` checks spec/baseline.json and this local file.
target/debug/randoc-harness run --baseline target/harness/local-baseline.json --update-baseline

# Mutation tests on the lines that your branch changes. The tests of your crate
# and the conformance test of the program judge each mutant.
git diff main...HEAD > /tmp/branch.diff
cargo mutants --in-diff /tmp/branch.diff --package YOUR_CRATE \
    --test-package YOUR_CRATE --test-package randoc-cli --timeout 120
# A new crate can have thousands of mutants. Test a sample and report its size.
cargo mutants --package YOUR_CRATE --test-package YOUR_CRATE --test-package randoc-cli \
    --shard 1/20 --timeout 120

# Coverage of your crate.
cargo llvm-cov --package YOUR_CRATE --summary-only

# Fuzzing: 15 minutes for each target.
scripts/fuzz.sh crates/YOUR_CRATE TARGET 900
```

## Conformance cases

See [`../conformance/README.md`](../conformance/README.md). Keep many cases in
one file. Take edge cases from the Pandoc manual and from your own tests of
the oracle. When the result of the oracle surprises you, keep the case: the
oracle defines the correct result.

## Fuzz targets

A crate keeps its fuzz targets in `crates/CRATE/fuzz/`. Copy the layout of
`crates/randoc-ast/fuzz/`. Put a few small start inputs in
`fuzz/seeds/TARGET/`. Tell the coordinator the path, because the root
`Cargo.toml` must exclude the fuzz crate from the workspace.

## Commits

Use conventional commit messages, for example `feat(readers): add docx reader`.
A commit message has no co-author line and no tool attribution. `init.sh`
installs a hook that removes such lines.

## Text style

Write documentation, comments and messages in short, direct sentences. Keep a
comment only when it gives a reason that the code cannot show.
