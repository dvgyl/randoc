# Randoc progress

This file and `spec/status.json` hold the project state. Read both at the start
of a session, then run `./init.sh`.

## Handoff

Written: 2026-11-10 09:06 UTC, at the end of the first session. The owner
stopped the session and starts a new one later. All background work is
stopped. Each workstream committed its state on its branch.

### How to start the next session

0. Read `docs/OWNER_INSTRUCTIONS.md` first: each instruction of the owner,
   in the owner's words, and `docs/ORIGINAL_SPEC.md`, the full task text.
1. Run `. "/Volumes/Extreme Pro/Randoc/env/env.sh"` before each command, then
   `./init.sh` in `/Volumes/Extreme Pro/Randoc/randoc`.
2. Start `env/bin/randoc-watchdog` in the background and keep the memory
   limits (section "Local environment").
3. Read `env/store/standing-orders.md` (the rules for each worker),
   `env/store/decisions.tsv` (the decision log, one row for each decision),
   `env/store/agents.tsv` and, for each workstream, the section "State at the
   stop" at the top of `env/store/reports/NAME.md`.
4. Read `docs/PRIOR_ART.md` and `docs/ADR.md`: the owner's rule of 2026-11-10
   says to study and reuse open source before writing new code.
5. The worktrees are in `/Volumes/Extreme Pro/Randoc/wt/NAME`, one for each
   branch `ws/NAME`. The branches exist only on this drive. They are not
   pushed on purpose: the early history of some branches holds files that
   must not be published (examples of the CommonMark specification, text of
   the Pandoc manual, completion scripts of Pandoc, a local path). Each
   workstream reaches `main` as one squashed commit (`env/bin/merge-ws`).

### What works

- `main` is at the commit that holds this text. It compiles, passes
  `scripts/gate.sh`, and is pushed to <https://github.com/dvgyl/randoc>.
- Formats on `main`: `json` and `native`, reader and writer of each.
- Libraries on `main` that no program path uses yet: the layout engine
  (`randoc-layout`), the template engine with the default templates
  (`randoc-templates`, `randoc-data`), and the shared utilities
  (`randoc_core::shared`: plain text of a tree, heading identifiers, HTML
  entities, emoji, the default abbreviation list, NFC).
- Conformance harness on `main`: 4546 units, 2217 pass, 1959 fail, 370 skipped, 0 regressions (measured on the merge commit 3dbc884).
  Each failing unit needs a part that is not on `main` yet (the command-line
  options, the Markdown reader, the HTML writer) or is a recorded divergence.
- Capability matrix (`spec/capabilities.json`): 2 of 52 input formats and 2 of 77 output formats are partial (`json`, `native`). 0 of 90 extensions and 2 of 109 options have passing units.
- Pandoc's own test suite (`tools/pandoc-testsuite`): the tool runs 2231 tests. The oracle passes 2231 (measured by the workstream testsuite). Randoc passes 0 (measured on 3dbc884 at 09:01 UTC). About 1939 more tests need Haskell and cannot run.
- Corpus of real documents (`tools/corpus`): 230 documents in 33 formats, 690 units. The oracle against itself passes 690 (measured by the workstream corpus). Randoc passes 0 (measured on 3dbc884 at 09:01 UTC), because `main` reads only `json` and `native`.
- Quarto and R Markdown: not measured. No path that they use (Markdown
  reader, HTML writer, Lua filters, citations) is on `main`.
- Nine workstream merges passed the full merge gate: testsuite, bench,
  research, corpus, layout, native, json-codec, shared, templates
  (`docs/MERGE_LOG.md` has the evidence of each). The foundation commit has
  two recorded gate exceptions (section "Merge gate records").

### What does not work

`main` converts only between `json` and `native`. Each other format, each
extension, almost each option, templates in the program, filters, Lua,
citations, highlighting, math conversion and the `lua` and `server` modes
are not on `main`. The branches below hold the work in progress. No branch
except `templates` had the approval of both reviewers at the stop.

| Workstream | Branch head | State at the stop | Next step |
| --- | --- | --- | --- |
| cli | `ws/cli` 9954e7b (wip) | Five review rounds. Round 5: both reviewers requested changes (the resource search for include files and templates, the read order, two sandbox defects). The corrections are written and not tested; the oracle matrix with 1010 cases is committed. Last gate pass: 2da6375. | Generate the UTF-8 test data, run the replay test and the gate, then review round 6. |
| yaml | `ws/yaml` 50d9b28 | Three review rounds. Round 3 items 1, 2, 3, 4 and 6 are corrected with oracle tests. Item 7 (scheme of a verbatim tag) is half-done; items 5 and 8 are not started. Last gate pass: 800a66a. | Finish items 5, 7, 8; gate; 15 minutes of fuzzing; review round 4. Then the Markdown reader, the CommonMark reader and the command line move to `randoc-yaml`. |
| commonmark-reader | `ws/commonmark-reader` 723475d (wip) | Four review rounds. All 8 round 4 items are corrected with oracle cases, and the reader uses `randoc_core::shared`. The last commit (a counter that overflowed) has no test run. Specification run: 652 of 652 at 6e79ef4. Last gate pass: 6e46f9a. | Tests and gate on 723475d, the specification tool, fuzzing, a mutation sample, then review round 5. |
| html-writer | `ws/html-writer` c803b4e | Three review rounds. Round 3: R3-01 to R3-04, R3-06, R3-07, O01 to O03, I-HTML-15 and I-HTML-17 are corrected with tests. Open: one decision function for a `Div` that starts with a heading, from the full matrix of `tools/sections.py` (R3-05 and the rest of N09). Last full gate pass: 31709f6. | The section matrix and the single rule, the gate, two differential runs, then review round 4. |
| templates | `ws/templates` 8d63d0b | Merged. | The command line connects `randoc-templates` and `randoc-data`. |
| highlight | `ws/highlight` dfec7d9 | No review verdict yet: round 1 started and was stopped with the session. All 1,386 samples and 12,600 mutations are equal to the oracle when all 200 definitions are loaded; 63 definitions are embedded (licence). Speed: 1 MiB in 2.84 s. Two step limits were added (I-HL-03). The gate passes at dfec7d9. | Review round 1 (`env/store/briefs/review-highlight-round1-codex.md`). Then the HTML and LaTeX writers call the crate. |
| markdown-reader | `ws/markdown-reader` 4c1edf9 | Two review rounds. `MANUAL.txt` gives the same JSON bytes as the oracle. Uses `randoc_core::shared`. Two high time findings are open: I-MD-28 (8 of 156 pairs of openers are still quadratic) and I-MD-27 (nested labels copy the text). Last gate pass: 2d3fb48. | The memo of inline results by (construct, start, context), then one shared text buffer; the gate; review round 3. |
| latex-writer | `ws/latex-writer` f85d334 | One review round. A signature report found the dominant difference classes; about 17 rules corrected. Random trees that differ: 4 of 500 for `latex`, 13 of 500 for `beamer` (before: 136 of 200). Last gate pass: b0f4099. | Gate on f85d334, fuzzing, a mutation sample, the classes of I-LATEX-09, then review round 2. |
| markdown-writer | `ws/markdown-writer` 235b8a8 | No review yet. 10,633 of 11,302 units of its suite pass. Small random trees that differ: 4 of 400. Open and high: table forms (I-MDW-02), `markua` (I-MDW-08). Math conversion waits for the workstream math. Last gate pass: ac1b191. | The table rule matrix, `markua`, the medium rows, the gate, then review round 1. |
| math | `ws/math` 1af6155 (wip) | First version of `randoc-math` (TeX math to MathML and to an inline list), written from oracle output because `texmath` is GPL. Equal to the oracle: MathML for 2,550 of 4,110 formulas, the inline list for 3,425 of 4,110, the warning text for 973 of 2,378. The full gate did not run. | Read `docs/PRIOR_ART.md` ("TeX math") for a permissive base, then the rows I-MATH-01 to I-MATH-04. |

Not started: each other reader and writer (HTML reader, `docx`, `odt`,
`epub`, `rst`, `org`, `typst`, `djot`, `man`, `docbook`, `jats`, wiki formats
and the rest of the 52 input and 77 output formats), Lua filters and custom
readers and writers, JSON filters, `--citeproc`, `--embed-resources`, PDF
output through an engine, and the `lua` and `server` modes.

### Open issues

- `docs/ISSUES.md` on `main` has 41 rows. Each branch has its own rows,
  which reach `main` with its merge.
- Decisions that wait for the owner: D1 to D5 (section below), and two
  coordinator decisions that the owner can reverse: the severity levels of
  gate 5 and the rule that a recorded scope gap is not a review finding
  (`docs/CONTRIBUTING.md`).
- The Codex service stopped 5 review runs with a content flag. A prompt
  that is a plain conformance review (`env/bin/codex-review-file`, the files
  `env/store/briefs/review-*-codex.md`) completes. Such a round does not
  cover the `--sandbox` path probes; the merge record must say so.
- The computer stopped one time (memory), and one oracle process used 22 GB.
  The limits in "Local environment" prevent both; keep them.
- The reuse rule is new. `THIRD_PARTY_NOTICES.md` is the new name of
  `THIRD_PARTY.md` on `main`. Each branch still has the old name; Git follows
  the rename in a merge, and `env/bin/resolve-tables` knows both names.
- Gate items that are open for merged work: mutation tests of the
  document-model helpers were run only in part (58 caught, 2 missed and now
  tested), H-001 and H-002 in `docs/ISSUES.md`.
- Housekeeping on the drive: the empty folder
  `/Volumes/Extreme Pro/Randoc.images` is not needed (an early attempt with
  a disk image). The worktrees of the merged workstreams (`wt/layout`,
  `wt/native`, `wt/json-codec`, `wt/shared`, `wt/templates`) can be removed
  with `git worktree remove`. The internal disk is small (about 14 GB free);
  the Codex folder and other caches there are not Randoc files.

### Next three tasks

1. **Bring the nearest branches to `main`.** Order: `yaml`, `cli`,
   `commonmark-reader`, `html-writer`, `highlight`. For each: run the gate on
   the head commit, finish the open items of its report, then one review
   round with both reviewers, then `merge-ws`. With the command line, one
   reader and one writer on `main`, measure the first end-to-end slice
   (CommonMark to HTML) with the harness, the Pandoc test suite and the
   corpus, and update `spec/`.
2. **Apply the reuse rule to the open work** (`docs/PRIOR_ART.md`, ADR 1).
   Check the licence of each candidate first. Compare the CommonMark reader
   with `commonmark-hs` to close its open rows; choose a permissive base for
   `randoc-math`; choose pinned crates for ZIP, XML and Lua before the
   workstreams that need them start; decide for the Markdown reader whether
   the memo design or another inline-parser design removes the two high
   time findings.
3. **Start the next wave** when the command line is on `main`: the HTML
   reader, the container formats (`docx`, `odt`, `epub`) on the chosen ZIP
   and XML crates, `rst` and `org`, JSON filters and Lua filters, and
   `--citeproc` (a port of `citeproc`, BSD-2-Clause, if its licence check
   passes). Each one is a workstream with a brief in `env/store/briefs/`.

## Workstreams that are merged

| Workstream | Content |
| --- | --- |
| foundation | Workspace, document model, shared contracts, conformance harness |
| testsuite | `tools/pandoc-testsuite`: runs Pandoc's own tests outside the repository |
| bench | `tools/bench` and the benchmark inputs (one squashed commit; do not push the branch, its old commits hold a local path) |
| research | `docs/DATA_FILES_LICENCES.md`: the licence of each Pandoc data file |
| corpus | `tools/corpus`: 230 real documents in 33 formats |
| layout | `randoc-layout`, a port of `doclayout` |
| native | The `native` reader and writer |
| json-codec | The JSON codec of the document model, with the error texts of the oracle |
| shared | `randoc_core::shared` |
| templates | `randoc-templates` and `randoc-data` |

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
| D3. The abbreviation list and the DocBook entity list | Randoc does not copy the Pandoc files. The abbreviation list of `randoc_core::shared` has 76 entries that black-box tests of the oracle confirmed, from 3,926,072 candidates that a script made mechanically. The list can be incomplete (I-SHARED-03). The DocBook entity list is not started. |
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
| Research reuse (documents `docs/PRIOR_ART.md`, `docs/ADR.md`; session 2) | 2, 3, 4, 6, 7 | The change has no code and no build input. Gate 5 applied in full: four review rounds, both reviewers approve the final text (`docs/MERGE_LOG.md`). |

