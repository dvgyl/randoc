# Agent guide for Randoc

Randoc is a native Rust drop-in replacement for **Pandoc 3.13** (`MIT OR Apache-2.0`), targeting identical formats, options, output bytes, warnings, and exit codes.

## Communication preference

**Keep talking to the user.** Regularly share what you are currently doing and what work remains—do not go silent during long multi-step tasks. You do not need to narrate every single tool call, just keep the user generally updated as you move through stages.

## Model routing

See [`docs/MODEL_ROUTING.md`](docs/MODEL_ROUTING.md) for the full default routing policy.

- **For this repository (Randoc):** only use **Gemini 4 Neon** (across available providers) or **Opus 5.5** if needed.
- Skip model routing whenever the user asks not to use it or the task is menial.

## Environment & hardware constraints

Everything lives on the external exFAT drive under `/Volumes/Extreme Pro/Randoc` on a 16 GB RAM macOS arm64 machine.

Always source the environment script before running `cargo`, tests, or oracle commands:

```sh
. "/Volumes/Extreme Pro/Randoc/env/env.sh"
```

Key constraints enforced by `env/env.sh`:
- Keep all tool data, builds, caches, and temp files under `/Volumes/Extreme Pro/Randoc` (`CARGO_HOME`, `RUSTUP_HOME`, `TMPDIR`, `RANDOC_ORACLE_PANDOC`, `RANDOC_PANDOC_SRC`, `RANDOC_CORPUS`).
- **exFAT drive:** `CARGO_INCREMENTAL=0` and `COPYFILE_DISABLE=1` are set because every file occupies at least 1 MB (2 MB with macOS `._` companion files). Keep file counts low (group test cases into shared files), skip `._*` entries in directory walks, and clean `.git` companion files after `git fetch`/`git gc` (`find .git -name '._*' -delete`).
- **16 GB RAM:** `CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=3`, and `GHCRTS="-M3g"` (caps Haskell Pandoc oracle heap to 3 GB; exit 251 / `Heap exhausted` counts as oracle limit, not a Randoc failure). Run heavy or adversarial inputs through `tools/measure/bin/limit-run`.

## Clean-room & licence rules

- Randoc is licensed under **`MIT OR Apache-2.0`**. Pandoc 3.13 is **`GPL-2.0-or-later`**.
- **Never** copy Pandoc source code, Pandoc test files, or copyleft data files into this repository. Reimplement behaviour from the Pandoc manual, documented interfaces, permissively licensed libraries, and black-box oracle output.
- **Randoc never invokes Pandoc.** Only `crates/randoc-harness`, `tools/`, and tests invoke the oracle (`$RANDOC_ORACLE_PANDOC`).
- Before reusing any open-source library or data: verify its licence is compatible with `MIT OR Apache-2.0`, record it in [`docs/PRIOR_ART.md`](docs/PRIOR_ART.md), add a short ADR in [`docs/ADR.md`](docs/ADR.md), and add attribution in [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md) + `third_party/licences/`.
- Run the licence scan before committing new code or data:
  ```sh
  python3 -I tools/measure/licence_scan.py scan
  ```

## Engineering standards

- **No `unsafe` code** (forbidden by workspace lints; the only audited exceptions are the C FFI boundaries in `randoc-lua` per ADRs 11 and 19).
- **Treat all input as untrusted:** no `unwrap()`, `expect()`, `panic!()`, `todo!()`, or `unimplemented!()` on reachable input paths; enforce `context.limits` (`max_nesting_depth`, `max_input_size`, `max_table_cells`, archive limits).
- **System access via `Context` / `Host`:** readers and writers must never call `std::fs`, `std::env`, `std::time`, or `std::process` directly.
- **Documentation & lints:** every public item must be documented (`cargo clippy --workspace --all-targets -- -D warnings` and `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`).
- **Commit hygiene:** use conventional commit messages (e.g., `fix(markdown): ...`). Never include `Co-Authored-By:`, `Claude-Session:`, or `"Generated with"` lines (`./init.sh` installs the `commit-msg` hook). Never force-push or rewrite published history.

## Common commands

```sh
# Build, run all Rust tests, and check conformance baseline
./init.sh

# Fast mechanical gate (fmt, clippy, rustdoc, cargo test + conformance)
scripts/gate.sh

# Full verification gate (adds licence scan, oracle checks, Python tool tests, harness verdicts)
python3 tools/measure/gate.py --quick

# Run specific differential measurement suites
python3 tools/measure/measure.py --suite harness
python3 tools/measure/measure.py --suite readers --filter html
python3 tools/measure/measure.py --suite writers --filter forms/sample-cell

# Run all Python tool unit tests
for d in tools/measure/tests tools/casegen tools/corpus tools/pandoc-testsuite tools/commonmark-spec; do
  python3 -B -m unittest discover -s "$d"
done
```

## Navigation

See [`README.md`](README.md) for the full repository layout, [`docs/README.md`](docs/README.md) for the documentation index, and [`PROGRESS.md`](PROGRESS.md) for current compatibility numbers and open work.
