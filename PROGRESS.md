# Randoc progress

This file and `spec/status.json` hold the project state. Read both at the start
of a session, then run `./init.sh`.

## Current state

Updated: 2026-11-09 23:50 UTC.

- The shared contracts exist: the document tree with Pandoc-compatible JSON
  (`randoc-ast`), the reader and writer traits, options, extensions, errors,
  log messages and the conversion context (`randoc-core`).
- The conformance harness exists (`randoc-harness`). It records oracle results,
  compares Randoc with them, keeps a baseline and writes the capability matrix.
  The suite also runs as a test of the program (`crates/randoc-cli/tests/conformance.rs`).
- One format works on `main`: `json` (reader and writer).
- Harness result on `main`: 36 units, 28 pass, 8 fail. All 28 passing units are
  byte-identical. The 8 failing units are JSON error messages (issue I-001).
- Capability matrix (`spec/capabilities.json`): 1 of 52 input formats and 1 of
  77 output formats are partial. 0 of 90 extensions and 2 of 109 options have
  passing units.
- Pandoc's own test suite (`tools/pandoc-testsuite`, `docs/PANDOC_TESTSUITE.md`):
  the tool runs 2231 tests. The oracle passes 2231. Randoc passes 0. About 1939
  more tests need Haskell and cannot run.
- Corpus of real documents: built by the workstream corpus (242 documents),
  not merged yet.

## Workstreams

| Workstream | Branch | State at 23:50 UTC |
| --- | --- | --- |
| testsuite | `ws/testsuite` | Merged. |
| bench | `ws/bench` | Merged (as one squashed commit; do not push the branch, its old commits hold a local path). |
| corpus | `ws/corpus` | Review round 1. |
| layout | `ws/layout` | The worker runs its gate checks. |
| markdown-reader | `ws/markdown-reader` | In work. |
| html-writer | `ws/html-writer` | In work. |
| cli | `ws/cli` | In work. |
| native | `ws/native` | In work. |
| json-codec | `ws/json-codec` | In work. It corrects the JSON findings of the foundation reviews. |
| commonmark-reader | `ws/commonmark-reader` | In work. |
| research | `ws/research` | Merged. |

The brief, the report and the reviews of each workstream are in
`/Volumes/Extreme Pro/Randoc/env/store/` (`briefs/`, `reports/`, `reviews/`).

## The oracle

- Pandoc 3.13, official release file `pandoc-3.13-arm64-macOS.zip` from
  <https://github.com/jgm/pandoc/releases/tag/3.13>.
- SHA-256 of the zip: `f436d18bebbfb3ddd326597686dacab9fac5dc23bd060495020f35a28a43a7c3`.
- SHA-256 of `bin/pandoc`: `8f17378a70e46eed7b63b5433fff6f5a0802f92659aa5782418548e0ca4a8e44`.
- `pandoc --version`: `pandoc 3.13`, features `+server +lua`, Lua 5.4.
- Document model version in JSON output: `[1,23,1,2]`.
- Pandoc source for the external test-suite run: tag `3.13`, commit
  `30b135468435dc4fbccfc2694508fa8e3e9ddc33`. The clone is outside this repository.

## Local environment of the first sessions

The work runs on a macOS arm64 laptop. All data is in one folder on an external
exFAT drive:

| Path | Content |
| --- | --- |
| `/Volumes/Extreme Pro/Randoc/randoc` | This repository |
| `/Volumes/Extreme Pro/Randoc/env` | Tool data: `env.sh`, the Rust toolchains, the Cargo home, the oracle, the Pandoc source, the corpus |
| `/Volumes/Extreme Pro/Randoc/wt` | One Git worktree for each workstream |

Run `. "/Volumes/Extreme Pro/Randoc/env/env.sh"` before each command. The file
sets `CARGO_HOME`, `RUSTUP_HOME`, `TMPDIR` and `RANDOC_ORACLE_PANDOC`.

The exFAT file system has effects that a new session must know:

- Each file uses 2 MB of disk space or more, because the allocation block is
  1 MB and macOS adds a `._` companion file for each file. Keep the number of
  files low. Put many test cases in one file.
- `._` files appear next to each file. `.gitignore` ignores them. Code that
  lists a directory must skip names that start with `._`.
- After `git gc` or `git fetch`, remove the companion files in `.git`:
  `find .git -name '._*' -delete`. Automatic garbage collection is off.
- Incremental compilation is off (`CARGO_INCREMENTAL=0`) to keep the target
  directory small.
- The fuzz script keeps the corpus in memory for the same reason.

## Decisions that wait for the owner

The licence research (`docs/DATA_FILES_LICENCES.md`, "Decisions for the owner")
found four decisions. The coordinator uses a working rule for each one until
the owner decides.

| Decision | Working rule now |
| --- | --- |
| D1. GPL data files of Pandoc (reference documents, translations, `epub.css`, Lua files, 98 copyleft syntax definitions) | Not in this repository. A later data package with its own licences can hold them. Randoc looks for them in a data directory at run time. |
| D2. `default.csl` (CC BY-SA 3.0) | Not in this repository. Same data package as D1. |
| D3. The abbreviation list and the DocBook entity list | Randoc does not copy the Pandoc files. The Markdown reader gets a list from black-box tests of the oracle with candidates that the worker wrote. The list can be incomplete. |
| D4. 34 syntax definitions with no licence grant (C, Python, JavaScript, Lua, SQL and others) | In no package. Load them at run time from a Pandoc, skylighting or KDE installation of the user. Byte-identical highlighting for these languages is a gap until a grant exists or new definitions are written. |

## Merge gate records

See `docs/MERGE_LOG.md` for the evidence of each merge.

Gates that could not apply:

| Change | Gate | Reason |
| --- | --- | --- |
| Foundation commit | 5 (two reviews) | The reviews of the foundation run after the first push, so that the workstreams can start. Findings are fixed in follow-up commits. |
| Foundation commit | 6 (benchmarks) | There is no earlier version to compare with. |

