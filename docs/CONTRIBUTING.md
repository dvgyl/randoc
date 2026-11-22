# Contribution rules and the merge gate

Read [`ARCHITECTURE.md`](ARCHITECTURE.md) and [`../AGENTS.md`](../AGENTS.md)
first.

## Quality bar and verification gate

1. **No regressions.** `spec/baseline.json` lists every conformance unit that
   passes on `main`. No baseline unit may regress.
2. **Measured progress.** Changes raise passing unit counts across the
   conformance harness and measurement suites (`tools/measure/measure.py`).
   Remaining differences from Pandoc 3.13 are recorded in
   [`ISSUES.md`](ISSUES.md).
3. **Tests that find real defects.** Add conformance cases with oracle results
   and edge cases from the Pandoc manual. For major subsystems, run mutation
   tests (`cargo mutants`) and coverage (`cargo llvm-cov`).
4. **No panics.** Parsers and archive readers must handle untrusted input
   without panicking and have fuzz targets under `crates/CRATE/fuzz/`.
5. **Code review & severity levels.** Run independent reviews on major blocks
   (see [`MODEL_ROUTING.md`](MODEL_ROUTING.md)). Critical, high, and medium
   findings are fixed; low findings are documented in [`ISSUES.md`](ISSUES.md).
   - **Critical:** memory unsafety, crash/panic/stack overflow or hang (>60 s)
     on <1 MB default input, lost document text on standard forms, or a licence
     violation.
   - **High:** crash/stack overflow via library API limits, super-linear
     time/memory (>5 s debug on 100 KB), or wrong output/exit code on real
     documents.
   - **Medium:** wrong output/structure on plausible author input, super-linear
     scaling, wrong warning/error on common mistakes, or missing licence notice.
   - **Low:** differences only on malformed/adversarial input with no text loss,
     error wording/position details on broken input, or docs/style nits.
   - **Scope gaps:** unimplemented features recorded accurately in
     [`ISSUES.md`](ISSUES.md) without silent text loss.
6. **No slowdowns.** Benchmarks of changed paths should not regress more than
   5% (`tools/bench/`).
7. **Clean build & licence check.** `cargo fmt --all -- --check`,
   `cargo clippy --workspace --all-targets -- -D warnings`,
   `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`, and
   `python3 -I tools/measure/licence_scan.py scan` must all pass.

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
# The mechanical checks of gates 1 and 7, and the tests. The conformance suite
# runs one time, inside `cargo test` (crates/randoc-cli/tests/conformance.rs).
scripts/gate.sh
# The merge check of the coordinator: the same checks, and the one suite run also
# writes spec/baseline.json and spec/capabilities.json.
scripts/gate.sh --update-spec

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
