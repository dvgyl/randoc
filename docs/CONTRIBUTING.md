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
6. **No slowdowns.** Benchmarks of the changed paths are not more than 5%
   slower than `main`.
7. **Clean build.** `cargo fmt --all -- --check`, `cargo clippy` with
   `-D warnings` and `cargo doc` pass with no warnings. Each public item has
   documentation.

A gate that cannot apply to a change is recorded with its reason in
`PROGRESS.md`. The bar is never lowered without a record.

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
