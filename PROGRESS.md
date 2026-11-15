# Randoc progress

This file and `spec/status.json` hold the project state. Read both at the start
of a session, then run `./init.sh`.

## Handoff

Written: 2026-11-15 00:56 UTC, in the fifth session (it started on 2026-11-12 at
05:20 UTC, was stopped by the usage limit of the account at about 07:20
UTC, and continued on 2026-11-14 at 20:50 UTC). This is the state after the safe pause that the owner ordered: no worker, no review and no merge runs, no worktree exists, and the watchdog is stopped. The coordinator
records (`decisions.tsv`, `units.tsv`, the gate lines in `inbox/`) are
under `/Volumes/Extreme Pro/Randoc/env/store/`.

Key points:

- `main` is at the commit that holds this text, pushed to <https://github.com/dvgyl/randoc>.
  This session made 16 block merges in 4 batches (A to D). Each batch passed the full merge check on the merged tree with 0 regressions; one block (the LaTeX reader) was reverted in batch B for a memory defect and merged again in batch D with a proof.
- **Measured progress in this session** (start, then now): conformance
  harness 46,457 of 49,305 units, then 54,488 of 57,176; Pandoc's own test suite
  509 of 2,231 tests, then 607; the corpus of real documents 133
  of 690 units, then 159.
- **New on `main`:** Lua filters in the program (`--lua-filter`, `-L`),
  the level of each log message from a reference matrix, the corrections
  of the review findings of the Markdown reader (round 3), citeproc
  (round 2) and Lua (round 3), three ported modules of the citation
  evaluation, and a faster conformance harness. Also: the libraries `lpeg` and `re` for Lua filters (LPeg 1.1.0), the option globals of a filter, about half of the corrections of the first LaTeX reader review, the closed medium rows of the HTML reader with EPUB section types, and the Markdown writer corrections for divs, figures and nested emphasis.
- **Two critical findings of this session, both corrected:** the Markdown
  writer lost table captions and visible text of adjacent reference links
  (first review of that writer; corrected on `main` in `f292078`); a depth test of the LaTeX
  reader used more than 4 GB, because the reader copied token lists at
  each nesting level (the watchdog stopped it in a merge check, the block
  was reverted on `main`; the corrected block is on `main` as `c46be2e`, and the merge check ran the whole test binary of the crate under `limit-run 1500`).
- **Reviews of this session.** First reviews: the LaTeX reader (Opus: 4
  high, 5 medium; Codex: 21 high, 8 medium) and the Markdown writer (Opus:
  2 high, 7 medium; Codex: 2 critical, 16 high, 3 medium). Verification
  reviews: the CommonMark reader (Opus APPROVES; Codex requests changes
  with 4 medium), math (both request changes: 4 high, 5 medium in all, two
  of the high ones with their cause in the Markdown writer), Lua (both
  request changes: 7 high, 8 medium; corrected on the branch). The Markdown reader, round 4 (Codex): one critical finding of round 3 is still open, and 4 high and 3 medium are new. The command line, round 9 (Codex): 2 critical, 12 high, 10 medium. No Opus review ran for these two rounds.
- **The usage limit.** About 10 Opus agents in parallel used up the 5-hour
  window of the account in about 2 hours. A stop comes without a warning.
  Since then: a merge check runs detached with a state file
  (`env/tmp/merge-s5-*/state.txt`), each worker commits after each item,
  appends a gate line to `env/store/inbox/NAME.log` and writes its report
  section early, and Codex reviews (which do not use this limit) start
  before Opus reviews.

### How to start the next session

0. Read `docs/OWNER_INSTRUCTIONS.md` first (22 instructions, in the words
   of the owner) and `docs/ORIGINAL_SPEC.md`, the full task text.
1. First check: `git status --short` shows nothing in the main checkout,
   and `git rev-parse main` is equal to `git rev-parse origin/main` after
   `git fetch origin`. If the local `main` is ahead, a merge batch did not
   end: read `env/tmp/merge-s5-*/state.txt` and the check log next to it;
   push only when the log shows a passed check (`merge-ws: summary`, 0
   regressions) for the tree of the local head; else run
   `env/bin/merge-ws check`.
2. Run `. "/Volumes/Extreme Pro/Randoc/env/env.sh"` before each command,
   then `./init.sh` in `/Volumes/Extreme Pro/Randoc/randoc`.
3. Start the watchdog `env/bin/randoc-watchdog` (version 3) in the
   background; it is stopped (look with `pgrep -f randoc-watchdog`). It
   writes each event to `env/tmp/watchdog-events.log`. Keep the limits: 3 build slots,
   `CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=3`, `GHCRTS=-M3g`, at most 5
   code workers.
4. The drive: `df -g`. It had 33 to 53 GB free in this session; one
   worktree with a build needs 10 GB or more. Two clean-ups of old
   scratch data ran (`reports/disk-s5.md`). Remove a worktree when its
   workstream has no next unit: `git worktree remove --force`, then
   `rm -rf` of the folder with its full path (git leaves the build
   folder).
5. Tools of this session in `env/bin`: `drain-s5` (one short state of
   each unit of `env/store/current-s5.txt`, the gate lines and the
   machine), `mk-s5` with the templates `review-s5-new` and
   `review-s5-resume` (the frozen review tree is `wt/review-s5`; move it
   with `git checkout --detach COMMIT` and build `randoc-cli` and
   `randoc-harness` there), the new `merge-ws` (`check` runs
   `scripts/gate.sh --update-spec`: one suite run that also writes the
   baseline and the matrix; the old tool is `merge-ws.s4.bak`). Briefs:
   `s5-common.md` (the worker rules), `merge-s5.md`, one `NAME-s5.md` for
   each unit, `clerk-s5-NN.md`.
6. A merge check needs 20 to 30 minutes when five workers run; the first
   check after a clean of `target/harness/verified-oracle.json` needs 80
   minutes more, because it compares each stored result with the
   reference program again.
7. Roles (owner instructions 14, 18 and 20): the coordinator on Fable 5.2
   writes briefs and decides; each worker is an Opus 5.5 agent
   (`pstack:poteto-agent`); a Sonnet 5.5 agent writes records, measures
   and does single checks; Codex with GPT-6.1 Sol and a new Opus 5.5
   agent review.

### What works

- Formats on `main`. Readers: `json`, `native`, `commonmark`, `gfm`,
  `commonmark_x`, `markdown` and its variants, `html`, `csljson`, `latex`
  (partial). Writers: `json`, `native`, `html`, `html4`, `csljson`,
  `markdown`, `gfm`, `commonmark`, `commonmark_x`, `plain` and the
  Markdown variants (partial), `latex` and `beamer` (fragment output).
- The command line: the options, defaults files, the resource search,
  templates and standalone HTML output, JSON filters (`--filter`), Lua
  filters (`--lua-filter`, `-L`) with the libraries `lpeg` and `re` and
  the option globals, the log, error texts and exit codes. Three critical
  review findings are open (section "Open issues").
- Libraries on `main` that no program path calls yet: the CSL style and
  locale parser and four modules of the citation evaluation of
  `randoc-citeproc`.
- Conformance harness (merge check of batch D, 2026-11-15 00:53 UTC, on
  the tree of `e3455ba`): 57,176 units, 54,488 pass, 2,268 fail, 420
  skipped, 0 regressions.
- Capability matrix (`spec/capabilities.json`): input formats 4 passing
  and 9 partial of 52; output formats 5 passing, 12 partial and 1 failing
  of 77; extensions 53 passing, 25 partial and 1 failing of 90; options 90
  passing and 18 partial of 109.
- Pandoc's own test suite (`tools/pandoc-testsuite`): 607 of 2,231 tests pass (measured on `e3455ba`; 509 at the start of the session, 592 on 2026-11-12). By group: `command` 582 of 1,884, `old` 25 of 174, and 0 for the reader groups `docx`, `odt`, `rtf`, `fb2`, `pptx`, `xlsx`. No test was lost.
- Corpus of real documents (`tools/corpus`): 159 of 690 units pass (measured on `e3455ba`; 133 at the start of the session). By target: `json` 62 of 230, `html` 35 of 230, `markdown` 62 of 230. The number did not change between 2026-11-12 and the end of the session, although the harness gained 6,300 passing units: most corrections of this session came from review findings and reference matrices, not from the corpus documents.
- Quarto and R Markdown: not measured.

### State of each workstream

"Gate" means `scripts/gate.sh`. Read the top section of
`env/store/reports/NAME.md` before you trust a row. A patch or a staged
file that a row names is in `env/store/patches-s5/NAME/` (a copy; the
original is in the scratch folder `env/tmp/NAME-s5*/`). No worktree
exists: start each unit on the named branch in a new worktree.

| Workstream | State | Next step |
| --- | --- | --- |
| cli | On `main` (`fef04d5`: Lua filters, the level of each log message, the `saphyr-parser` rows removed). **Round 9 (Codex): 2 critical, 12 high, 10 medium, all open.** Critical: cli-R9-19 (standalone output silently drops titles and template text) and cli-R9-03 (`--sandbox` silently ignores the default template of the user; the worker found the cause: Randoc permits only the `--template` candidates and then uses the built-in template, the reference reads the template of the user with full file access). No code of unit 4 exists (branch `ws/cli-4` is at `main`). No Opus review of round 9 yet. `-s` needs no change: the command line already gives the template and the variables to each writer. | Brief `cli-s5b.md`, item 0 (a) first and alone. Then its other items; new: set `FilterEnvironment::readers` and `writers` when the Lua branch has `pandoc.read`. |
| markdown-reader | On `main` (`52e5f01`, `4333b6e`). **Round 4 (Codex): Codex R3-02 is STILL OPEN at critical** (the scans for HTML closers still consume literal tags inside code), the reviewer has counterexamples against the closure of R3-11, R3-12, R3-15 and Opus R3-03, and 4 new high and 3 new medium findings. Unit 5 stopped with nothing committed: a patch (818 lines; the same content is `stash@{0}` of the repository) corrects R4-03, R4-04, R4-05, R4-06 and holds half of R4-01, R4-07 and I-MD-60. No Opus review of round 4 yet. | Codex R3-02 first (it was not in the brief `markdown-reader-s5b.md`: add it as item 0, with the method of `markdown-reader-s4b.md`: each cache against the plain scan). Then apply the patch on `ws/markdown-reader-5`, run the crate tests, make the replay data again, the gate. |
| markdown-writer | On `main`: both critical findings of round 1 are corrected (`f292078`), then divs, figures, nested emphasis, the `columns` panic (`ff5a64b`). Branch `ws/markdown-writer-3` at `5498568` has NO gate pass: standalone output (all 330 standalone replay units pass in the library test), 9 high Codex findings, a faster depth test; pushed as `wip/markdown-writer`. Two patches: one math rule, about 40 replay cases without reference results. Open: nested lists and tables are slow (cause in `randoc-layout`), the medium findings, items 4 to 6 of the brief. | Gate on `5498568`, merge. Then the rest of `markdown-writer-s5.md`. Then the verification by both reviewers. |
| latex-reader | On `main` (`c46be2e`, the state of `4fad0ed`). Round 1 (both reviewers: 25 high, 13 medium): about half is corrected; each open finding is a row I-LTXR-16 to I-LTXR-49. The memory defect of a depth test is closed (I-LTXR-51). 5 of 8 corpus documents are equal. Unit 5 stopped with a patch (10 findings, 10 new matrices, the new file `scope.rs`); with it one baseline unit fails (`latex-reader/structure/structure-021#read`, not examined). | Apply the patch on `ws/latex-reader-4`, examine that unit, the gate. Then the open rows, the ignored wall-time test, `COMMANDS.md`. Before each gate: the whole test binary of the crate through `limit-run 1500 300`. |
| lua | On `main`: the round 3 findings (`9c224f6`), the option globals, `lpeg` and `re` (`c75b776`, ADR 19), step-count time tests (`54ae392`). Open medium rows I-LUA-11 and I-LUA-15. `pandoc.read` and `pandoc.write` are written as staged files, not compiled. | Brief `lua-s5.md` from item 3 (the staged files), then `MODULES.md`, the entrances for custom readers and writers and the interpreter. Then the verification by both reviewers (Codex log `lua-codex-r3`). |
| citeproc | On `main`: the round 2 findings (`b95fb95`), text cases, the output tree, the evaluation of `text`, `group`, `choose` (`11e9493`), `number` and `label` (`d3f5a8a`). Branch `ws/citeproc-3` at `1fbd17f` (the date module, 323 of 323 reference cases; NO gate pass; pushed as `wip/citeproc`). `names` is a patch script with 450 recorded reference results. No review of the new modules. | Gate on `1fbd17f`, merge. Apply the names patch. Then the entry for one citation, unit 4 of the plan, `--citeproc`. A Codex verification of round 2 (brief `review-citeproc-round3-codex.md` is ready). |
| html-reader | On `main` (`becae30`). The four medium rows of round 3 are closed; EPUB section types and the TeX annotation of MathML are read. Open: the `iframe` fetch, the checks (fuzz, coverage, mutation). | The verification by both reviewers (round 4). |
| commonmark-reader | On `main` since session 4. Round 8: Opus APPROVES; Codex requests changes with 4 medium (R8-01 to R8-04). | One short unit for the 4 medium findings and the 3 low Opus findings, then the Codex verification (log `commonmark-reader-codex-r8`). |
| math | On `main` since session 4. Round 4: both request changes (Opus: 2 medium in the crate; Codex: 2 high, 3 medium). The two Opus high findings had their cause in the Markdown writer and are corrected there. | Brief `math-s5.md` (add the Codex findings), then the verification. |
| yaml | Not touched in session 5. Three medium rows open (I-YAML-22 to 24). | Brief `yaml-s5.md`. |
| html-writer | Not touched. The review of the new code of session 4 is due; the briefs `review-html-writer-round7-*.md` are ready (make them again for the new head). | The review. |
| latex-writer | Not touched. Standalone output is not started. | Brief `latex-writer-s5.md`. |
| harness-speed | On `main` (`4146317`, `052eb5b`). No review round. Not done: a run mode that selects units for a worker. | The review with the next block that changes the harness. |
| contracts | Open requests: two defects of `NativeHost` (a copy of a file onto itself empties it; a variable that is not UTF-8 makes `environment` panic; no caller reaches them now), an error kind that a Lua `pcall` cannot catch (I-LUA-15), one shared module for the join rules of an inline list (three copies), public helpers for citeproc (Haskell `isSpace`, a general-category table, the `aeson` string writer), the listings language table for the LaTeX reader (I-LTXR-03), two time defects of `randoc-layout` (the indent prefix is copied at each nesting level; no fast path for the width of ASCII text). | Briefs `contracts-s5.md` and items 1 and 2 of `cli-s5b.md`; one new unit for `randoc-layout`. |
| highlight | Not touched since session 2. | The section "highlight" of the brief `open-branches-s4.md`. |

Not started: `docx` (brief `docx-reader.md`), `odt`, `epub`, `rst`, `org`,
`typst`, `djot`, `man`, `docbook`, `jats`, the wiki formats and the other
formats of the inventory, custom readers and writers, `--citeproc`,
`--embed-resources`, PDF output through an engine, and the `lua` and
`server` modes.

### Open issues

- **Critical findings that are open on `main`.** Three: cli-R9-19 and
  cli-R9-03 (the command line) and Codex R3-02 of the Markdown reader.
  Until they are corrected, no other unit of these two crates merges.
- **Work that is not in git.** The patches and staged files of the
  stopped workers are in `env/store/patches-s5/` (the store is a local
  Git repository). Each report says how to apply them.
- **Approvals.** One in this session (the CommonMark reader, Opus). Each
  other review requests changes. A block is done only when both reviewers
  approve its head.
- **Licence.** Decision D6 still waits for the owner (the old commits of
  `main` hold three kinds of copyleft text; the head does not). Decisions
  D1 to D5 wait too. New material of this session: LPeg 1.1.0 (MIT; a
  copy of its C sources with a hash test, ADR 19; the coordinator
  permitted the one download from the page of its author). The licence
  audit scripts of session 4 (`env/tmp/licence-audit-s4/`) did NOT run
  again in this session: run them before the next handoff.
- **Gate evidence that is open.** Mutation samples, coverage and fuzzing
  did not run for several blocks (each merge record says what ran); no
  benchmark (gate 6) was measured in sessions 4 and 5.
- **The machine.** Load averages of 40 to 90 came from several gate runs
  at the same time on the exFAT drive and from other work of the owner on
  the same computer. Memory stayed above 35% free. The watchdog stopped
  one process of the project (the LaTeX reader test at 4 GB).
- **Interpretations of the coordinator that the owner can change:** the
  download of LPeg under the reuse rule; the pause of the citeproc unit
  to keep the limit of 5 code workers when a critical finding needed a
  worker; the three conditions for a small block, the `wip/NAME` branches
  and the severity levels, as in the last handoff.

### Next three tasks

1. **Correct the three open critical findings, then close the review
   loop.** First the command line (brief `cli-s5b.md`, item 0 (a)) and the
   Markdown reader (Codex R3-02). Then apply the patches of the stopped
   units (the table above), run their gates and merge. Then, for each
   block: the verification by the same reviewers, Codex first. Measure
   `main` after each batch with a Sonnet agent.
2. **Connect the Lua runtime and citations.** The staged `pandoc.read`
   and `pandoc.write`, the table `MODULES.md`, custom readers and
   writers, the interpreter mode; the options and the format lookups
   that the command line must give to a filter. citeproc: `names`, the
   entry for one citation, unit 4 of its plan, then `--citeproc`.
3. **The next formats.** Finish the LaTeX reader (the open rows, then
   `COMMANDS.md`) and standalone output of the Markdown and LaTeX
   writers; the short units for `commonmark`, `math` and `yaml`; the
   shared contracts (`contracts-s5.md`, `randoc-layout`); then the `docx`
   reader (brief `docx-reader.md`), the `rst` reader, the `typst` writer
   and the `rst` writer, and `highlight`.

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
| reuse | Documents only: a checked licence for each reuse candidate in `docs/PRIOR_ART.md`; ADR 9 (ZIP), ADR 10 (XML) and ADR 11 (Lua) in `docs/ADR.md` |
| harness-flaky | Tests of `randoc-harness` that do not depend on the speed of the machine (test code only) |
| layout-perf | `randoc-layout`: the concatenation of documents is constant work |
| yaml | `randoc-yaml`: the YAML reader for metadata (no program path calls it yet) |
| citeproc | `randoc-citeproc`: the CSL style and locale parser, the reference model and the `csljson` reader and writer |
| math | `randoc-math`: TeX math to MathML and to inline elements (no program path calls it yet) |
| lua | `randoc-lua`: the Lua runtime with Lua 5.4.8, the document types, Lua filters and the first modules (no program option calls it yet) |
| html-writer | `randoc-html`: the writers `html` and `html4` |
| contracts | Shared contracts for the next units: two new `Limits`, 15 new `Host` methods, one Haskell `show`, the public JSON value parser, the `zip` pin (ADR 18) |
| latex-reader | `randoc-latex-reader`: the reader `latex`, unit 1 (tokenizer, macro definitions, the most frequent constructs) |
| markdown-writer | `randoc-markdown-writer`: the writers `markdown`, `gfm`, `commonmark`, `commonmark_x`, `plain` and variants (partial: large generated documents still differ) |
| latex-writer | `randoc-latex`: the writers `latex` and `beamer` (fragment output; standalone output is not connected yet) |
| harness-speed | `randoc-harness` and `scripts/gate.sh`: the gate runs the conformance suite one time; less file work for each unit (test tool only) |

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
reader. D6 came up in review round 8 of the command line. The coordinator
uses a working rule for each one until the owner decides.

| Decision | Working rule now |
| --- | --- |
| D1. GPL data files of Pandoc (reference documents, translations, `epub.css`, Lua files, 98 copyleft syntax definitions) | Not in this repository. A later data package with its own licences can hold them. Randoc looks for them in a data directory at run time. |
| D2. `default.csl` (CC BY-SA 3.0) | Not in this repository. Same data package as D1. |
| D3. The abbreviation list and the DocBook entity list | Randoc does not copy the Pandoc files. The abbreviation list of `randoc_core::shared` has 76 entries that black-box tests of the oracle confirmed, from 3,926,072 candidates that a script made mechanically. The list can be incomplete (I-SHARED-03). The DocBook entity list is not started. |
| D4. 34 syntax definitions with no licence grant (C, Python, JavaScript, Lua, SQL and others) | In no package. Load them at run time from a Pandoc, skylighting or KDE installation of the user. Byte-identical highlighting for these languages is a gap until a grant exists or new definitions are written. |
| D5. Test inputs from documents with a copyleft licence: the examples of the CommonMark specification (CC-BY-SA-4.0) and example text of the Pandoc manual (GPL) | Not in this repository. Workers write their own inputs. A tool under `tools/` downloads a public test suite at a pinned version, runs it outside the repository and stores only numbers in `spec/`. Each workstream merges as one squashed commit, so that an early commit with such a file does not reach `main`. Many CommonMark projects with a permissive licence keep the specification file in their repository as test data; the owner can decide to do the same. |
| D6. The history of `main` holds the completion scripts of Pandoc (GPL) | The four stored results were on `main` from commit `6089e1f` (2026-11-11 19:59 UTC) until the removal commit of 2026-11-11. The files of the head do not hold them. The old commits still do. A removal from the history needs a rewrite of the published history and a forced push, which the owner forbids. The owner decides. The same holds for the FODT results of the suite `templates` (the styles of the reference ODT) and for three table inputs of the suite `native` (example tables of the manual), which the licence audit of session 4 found; the head does not hold them after the correction of 2026-11-11. |

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

