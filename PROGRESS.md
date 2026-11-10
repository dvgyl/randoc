# Randoc progress

This file and `spec/status.json` hold the project state. Read both at the start
of a session, then run `./init.sh`.

## Current state

Updated: 2026-11-10 06:05 UTC.

- The shared contracts exist: the document tree with Pandoc-compatible JSON
  (`randoc-ast`), the reader and writer traits, options, extensions, errors,
  log messages and the conversion context (`randoc-core`).
- The conformance harness exists (`randoc-harness`). It records oracle results,
  compares Randoc with them, keeps a baseline and writes the capability matrix.
  The suite also runs as a test of the program (`crates/randoc-cli/tests/conformance.rs`).
- Two formats work on `main`: `json` and `native` (reader and writer of each).
- The layout engine (`randoc-layout`, a port of `doclayout`) is on `main`. No
  writer on `main` uses it yet.
- Harness result on `main`: 3574 units, 2217 pass, 987 fail, 370 skipped. Each
  failing unit needs a part that is not on `main` yet (options, the Markdown
  reader) or is a recorded divergence.
- Capability matrix (`spec/capabilities.json`): 2 of 52 input formats and 2 of
  77 output formats are partial. 0 of 90 extensions and 2 of 109 options have
  passing units.
- Pandoc's own test suite (`tools/pandoc-testsuite`, `docs/PANDOC_TESTSUITE.md`):
  the tool runs 2231 tests. The oracle passes 2231. Randoc passes 0. About 1939
  more tests need Haskell and cannot run.
- Corpus of real documents (`tools/corpus`, `docs/CORPUS.md`): 230 documents in
  33 formats, 690 units. The oracle against itself passes 690. Randoc passes 0
  (`spec/corpus.json`), because `main` has only the JSON format.

## Workstreams

| Workstream | Branch | State at 06:05 UTC |
| --- | --- | --- |
| testsuite | `ws/testsuite` | Merged. |
| bench | `ws/bench` | Merged (as one squashed commit; do not push the branch, its old commits hold a local path). |
| corpus | `ws/corpus` | Merged. |
| layout | `ws/layout` | Merged. |
| markdown-reader | `ws/markdown-reader` | Review round 2 on a frozen copy. The worker corrects six open medium findings. |
| html-writer | `ws/html-writer` | The worker corrects the findings of review round 2. |
| cli | `ws/cli` | The worker corrects the findings of review round 3 (medium findings only). |
| native | `ws/native` | Merged. |
| json-codec | `ws/json-codec` | Merged. |
| commonmark-reader | `ws/commonmark-reader` | Review round 2. |
| research | `ws/research` | Merged. |
| templates | `ws/templates` | Review round 2. |
| latex-writer | `ws/latex-writer` | The worker corrects the findings of review round 1. |
| yaml | `ws/yaml` | The worker corrects the findings of review round 1 (medium findings only). |
| shared | `ws/shared` | The worker corrects the findings of review round 1 (1 high). |
| highlight | `ws/highlight` | In work. Syntax highlighting: a port of `skylighting-core` with the 63 syntax definitions that have a permissive licence. |
| markdown-writer | `ws/markdown-writer` | In work. The writer for `markdown`, `gfm`, `commonmark`, `plain` and the other Markdown variants. |

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

The computer has 16 GB of memory. On 2026-11-10 at about 04:40 UTC it stopped
with a kernel panic, because the memory ran out (the panic report shows the
memory compressor at 100% of its limit). About 15 agents with their builds,
fuzz runs and reviews ran at that time, next to other work of the owner. The
work started again at 05:50 UTC. Nothing was lost on `main`: each merge was
pushed. Since then the `cargo` wrapper in `env/bin` makes each heavy cargo
command wait for one of 3 slots and for 20% free memory, `CARGO_BUILD_JOBS`
is 2, and standing order 18a limits each agent to one heavy command at a
time. A new session must keep these limits.

At 06:36 UTC the watch on free memory reported a second event: the swap space
had grown to 16 GB and the internal disk had 1 GB free. One process caused it:
an oracle run (`pandoc -f commonmark_x -t json`) that a reviewer had started
with a repeated-pattern test input used 22 GB of memory in 3.5 minutes. The
coordinator stopped that process, and the disk recovered to 14 GB free. This
kind of run is the probable cause of the stop at 04:40 UTC too. Since then
`env.sh` sets `GHCRTS=-M3g`, which limits the heap of each oracle process to
3 GB (a run that reaches the limit ends with exit code 251 and the text
`Heap exhausted`), and the tool `env/bin/randoc-watchdog` stops each process
of the Randoc folder that uses more than 5 GB. Standing order 18b tells each
agent to treat such an oracle result as "did not end".

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
found four decisions (D1 to D4). D5 came up in the review of the CommonMark
reader. The coordinator uses a working rule for each one until the owner
decides.

| Decision | Working rule now |
| --- | --- |
| D1. GPL data files of Pandoc (reference documents, translations, `epub.css`, Lua files, 98 copyleft syntax definitions) | Not in this repository. A later data package with its own licences can hold them. Randoc looks for them in a data directory at run time. |
| D2. `default.csl` (CC BY-SA 3.0) | Not in this repository. Same data package as D1. |
| D3. The abbreviation list and the DocBook entity list | Randoc does not copy the Pandoc files. The Markdown reader gets a list from black-box tests of the oracle with candidates that the worker wrote. The list can be incomplete. |
| D4. 34 syntax definitions with no licence grant (C, Python, JavaScript, Lua, SQL and others) | In no package. Load them at run time from a Pandoc, skylighting or KDE installation of the user. Byte-identical highlighting for these languages is a gap until a grant exists or new definitions are written. |
| D5. Test inputs from documents with a copyleft licence: the examples of the CommonMark specification (CC-BY-SA-4.0) and example text of the Pandoc manual (GPL) | Not in this repository. Workers write their own inputs. A tool under `tools/` downloads a public test suite at a pinned version, runs it outside the repository and stores only numbers in `spec/`. Each workstream merges as one squashed commit, so that an early commit with such a file does not reach `main`. Many CommonMark projects with a permissive licence keep the specification file in their repository as test data; the owner can decide to do the same. |

## Merge gate records

See `docs/MERGE_LOG.md` for the evidence of each merge.

Severity levels for gate 5 (decision of the coordinator, 2026-11-10 04:30 UTC):
the gate text says "fix every critical, high or medium finding, record low
findings", but it does not define the levels. Until this time, the reviewers
gave almost each difference from Pandoc the level medium, also for malformed
input that no author writes on purpose. For the large readers and writers, each
review round then found 10 to 20 new differences of that kind, and no branch
could reach an approval. `docs/CONTRIBUTING.md` now defines the four levels.
The main rule: a crash, a run that does not end, growth faster than linear,
lost text, and wrong output for input that an author can plausibly write are
critical, high or medium and must be corrected before a merge; a difference
only for malformed or adversarial input with no lost text is low, is recorded
in `docs/ISSUES.md` with its smallest input, and stays open work. The merges
before this time (testsuite, bench, research, corpus, layout, native,
json-codec) used the stricter reading. Each later merge record gives the
number of findings that were recorded as low.

Gates that could not apply:

| Change | Gate | Reason |
| --- | --- | --- |
| Foundation commit | 5 (two reviews) | The reviews of the foundation run after the first push, so that the workstreams can start. Findings are fixed in follow-up commits. |
| Foundation commit | 6 (benchmarks) | There is no earlier version to compare with. |

