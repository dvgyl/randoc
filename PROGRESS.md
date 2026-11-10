# Randoc progress

This file and `spec/status.json` hold the project state. Read both at the start
of a session, then run `./init.sh`.

## Handoff

Written: 2026-11-10 13:18 UTC, at the end of the second session. The owner
asked for the handoff because the limit of the model Fable 5.2 was almost
reached. Each worker committed its state on its branch and stopped.

Key points:

- `main` has no new code since the first session. It converts only between
  `json` and `native`. It compiles, and its tests and its harness baseline
  pass (measured at the start of this session).
- One merge in this session: the reuse research (documents only). It gives
  a checked licence for each reuse candidate and three decision records:
  `zip` for ZIP containers, `quick-xml` for XML, `mlua` with Lua 5.4.8
  built by Randoc for Lua.
- No code branch merged. Each review round of this session found real
  defects with the level medium or higher. The branch `yaml` is the
  nearest: round 5 left three medium findings, two small ones in the
  reader (flow collections) and one in its replay test.
- Two new workstreams started: `lua` and `html-reader`.
- A reviewer agent deleted the briefs folder of the coordinator by mistake.
  All files are rebuilt. The store is now a local Git repository.

### How to start the next session

0. Read `docs/OWNER_INSTRUCTIONS.md` first: each instruction of the owner,
   in the owner's words, and `docs/ORIGINAL_SPEC.md`, the full task text.
1. Run `. "/Volumes/Extreme Pro/Randoc/env/env.sh"` before each command, then
   `./init.sh` in `/Volumes/Extreme Pro/Randoc/randoc`.
2. Start `env/bin/randoc-watchdog` in the background and keep the memory
   limits (section "Local environment"). The watchdog ends after each
   process that it stops: start it again then.
3. Read `env/store/standing-orders.md` (the rules for each worker; rule 19a
   on deleting files is new), `env/store/decisions.tsv` (the decision log,
   one row for each decision), `env/store/units.tsv` and, for each
   workstream, the section "State at the stop" at the top of
   `env/store/reports/NAME.md`.
4. Read `docs/PRIOR_ART.md` and `docs/ADR.md` (ADR 1 and ADR 9 to 11).
5. The worktrees are in `/Volumes/Extreme Pro/Randoc/wt/NAME`, one for each
   branch `ws/NAME`. The branches exist only on this drive. They are not
   pushed on purpose (the early history of some branches holds files that
   must not be published). Each workstream reaches `main` as one squashed
   commit (`env/bin/merge-ws`).
6. The briefs of this session are in `env/store/briefs/`: `s2-common.md`
   (the rules for a worker that continues a workstream), `NAME-s2.md` and
   `NAME-s2b.md` (the units), `review-NAME-roundN.md` (the review prompts).
   `env/store` is a local Git repository with no remote: commit it after
   each group of changes (`git -C "/Volumes/Extreme Pro/Randoc/env/store" add -A`
   and `commit`). If files of the store are lost, `git checkout` restores
   them; `env/bin/recover-briefs` can rebuild the briefs from the session
   transcripts.


### What works

- `main` is at the commit that holds this text, pushed to
  <https://github.com/dvgyl/randoc>. Its code is the code of the first
  session. `./init.sh` passed on it at 09:17 UTC on 2026-11-10: the build,
  the tests, and the harness baseline (2217 units, 0 regressions).
- Formats on `main`: `json` and `native`, reader and writer of each.
- Libraries on `main` that no program path uses yet: the layout engine
  (`randoc-layout`), the template engine with the default templates
  (`randoc-templates`, `randoc-data`), and the shared utilities
  (`randoc_core::shared`).
- Conformance harness on `main`: 4546 units, 2217 pass, 1959 fail, 370
  skipped, 0 regressions (measured in the first session on 3dbc884; the
  code did not change since then).
- Capability matrix (`spec/capabilities.json`): 2 of 52 input formats and 2
  of 77 output formats are partial (`json`, `native`). 0 of 90 extensions
  and 2 of 109 options have passing units.
- Pandoc's own test suite (`tools/pandoc-testsuite`): the tool runs 2231
  tests. The oracle passes 2231. Randoc passes 0 (measured in the first
  session on 3dbc884).
- Corpus of real documents (`tools/corpus`): 230 documents, 690 units.
  Randoc passes 0 (measured in the first session on 3dbc884).
- Quarto and R Markdown: not measured. No path that they use is on `main`.
- Ten merges passed the merge gate: the nine workstreams of the first
  session and the reuse research of this session (`docs/MERGE_LOG.md`).

### What does not work

`main` converts only between `json` and `native`. Each other format, each
extension, almost each option, templates in the program, filters, Lua,
citations, highlighting, math conversion and the `lua` and `server` modes
are not on `main`. The branches below hold the work in progress. No branch
had the approval of both reviewers at the stop. "Gate" means
`scripts/gate.sh`. The exact state of each branch is the section "State at
the stop" at the top of `env/store/reports/NAME.md`; read it before you
trust a row of this table, because each worker wrote it after this table.

| Workstream | Branch head at the stop | State | Next step |
| --- | --- | --- | --- |
| yaml | `ws/yaml` `f96ec26` | Five review rounds. Gate passed at `f96ec26`. All round 4 findings of level medium or higher are closed. Round 5: both reviewers request changes. Opus found no defect in the reader in about 4,700 texts, and one medium finding in the replay test (R5-01): the comparison removes Markdown characters and, in file mode, compares only letters and digits, so a wrong string of ordinary form can pass. Codex found two medium findings in the reader: properties on separate lines inside a flow collection are read as content (YAML-R5-01), and a property before an alias after a line break is accepted (`a: [&a,!` / ` *a]`; Pandoc refuses it) (YAML-R5-02). | Correct the two reader findings with one oracle matrix for properties in flow collections (each property form, each kind of next token, with and without a line break). Make the replay comparison exact for each string (find an entrance of the oracle that keeps strings literal; check whether the `metadata` key of a defaults file is one). Review round 6 on the diff. Merge. Then move the Markdown reader, the CommonMark reader and the command line to `randoc-yaml`. |
| cli | `ws/cli` `78bb0d7` | Six review rounds. Gate passed at `2c28b35`. Round 6: both reviewers request changes (1 high: a repeated `--resource-path` replaces the earlier folders; 9 medium: the template search, a folder as a defaults name, named files that are missing, with and without `--sandbox`). The sandbox probes found no file that Randoc reads and the oracle refuses. Unit 2 (brief `cli-s2b.md`) was stopped at `78bb0d7`: each round 6 finding has code and a matrix case (the resource replay gives 2182 of 2182 cases), and the approved change in `randoc-core` is made. Warning from the worker: the cli suite of the harness gives 3455 passes on `78bb0d7`, and it gave 3457 before, so 2 or more units that passed now fail; the gate did not run on this commit. | Find the units that fail now (`randoc-harness run --baseline spec/baseline.json`) and correct the cause. Run the 40 reproductions (`env/tmp/cli-s2b/repro.py`), the gate, the mutation sample and the fuzz targets. Then review round 7, the merge, and the units "templates in the program" and "defaults files on `randoc-yaml`". |
| commonmark-reader | `ws/commonmark-reader` `9615173` | Five review rounds. Gate passed at `5b7cc50`; the specification run gave 652 of 652. Round 5: both reviewers request changes (7 medium: the emphasis rule for delimiter runs of different length, reference definitions and metadata links with `rebase_relative_paths`, a definition list after a note definition, a YAML tag that changes a value, an extra list block, a missing notice row). Unit 2 (brief `commonmark-reader-s2b.md`) was stopped at `9615173`: the delimiter algorithm is a port of `commonmark` 0.3.1 (BSD-3-Clause) and a matrix of 3,810 paragraphs shows 0 differences; each round 5 finding with the level medium or higher has a correction and oracle cases; the suite gives 1724 of 1740 passing units (the 16 others are the `sourcepos` gap); the specification run gives 652 of 652; fuzzing 901 s with 0 crashes. The gate on `9615173` was stopped in its test step, so it has no result. The comparison with the upstream source is half-done (the report lists the modules that are not compared). | Run the gate on `9615173` and the other mutation shards. Then review round 6. The decision record on the branch has the title `ADR (commonmark-hs)`: give it the next free number at the merge. |
| html-writer | `ws/html-writer` `5d91b64` (wip) | Four review rounds. Gate passed at `eee175e`. One decision function for section containers from an oracle matrix of 14,092 cases. Round 4: both reviewers request changes (1 high: a div with the class `columns` or `column`; 14 medium: classes with a special meaning, the place of block notes, heading attributes, time that grows faster than linear with the nesting depth). Unit 2 (brief `html-writer-s2b.md`) was stopped after 12 minutes at a `wip` commit that adds tools and probe data only (`src/classes.jsonl`, 3,856 oracle cases; the Rust code is the code of `aff07e3`). The report lists 7 rules that the data shows and that are not implemented. | Finish `html-writer-s2b.md` from step 1: the class table and its function, the note matrix (`tools/notes.py record`), the section axes, the nesting time. Then review round 5. |
| highlight | `ws/highlight` `a9c1784` (wip) | One review round. Gate passed at `82d9059`. An ordered search replaced the two fixed step limits; the 15 long inputs of the reviews give the oracle tokens; the replay with all 200 definitions shows 0 differences. Open: one AsciiDoc line of 50,000 non-ASCII letters needed 49.9 s in a release build; the worst-line table has 4 of 63 languages. Unit 2 (brief `highlight-s2b.md`) was stopped at a `wip` commit whose last three edits are not compiled. A probe that proves "no match" for a whole pattern makes repeated look-ahead scans linear; the worst line of 54 of 63 embedded languages is below 1 s in a release build, and 9 are above (Julia 31.6 s, AsciiDoc 22.5 s, Rust and Odin 14.0 s). The approved change `Host::list_dir` in `randoc-core` is written, not compiled and not tested. | Compile and test the head. Finish `highlight-s2b.md` from the next steps of its report (the 9 worst lines, the profile of Log File and AsciiDoc against the oracle time, the checks). Then review round 2. Then the HTML and LaTeX writers call the crate. |
| markdown-reader | `ws/markdown-reader` `72250dc` (wip) | Two review rounds. Gate passed at `3d644ec`. Both high time findings are corrected: a memo of failed inline attempts makes all 156 pairs of openers linear, and sub-parses read windows of one buffer. `MANUAL.txt` gives the JSON bytes of the oracle. Open: three rows with the level medium (I-MD-28 list form, I-MD-29, I-MD-30); two strikeout forms need 6.7 times the time for 4 times the size; the last fuzz run had only 5,293 runs in 910 s. Unit 2 (brief `markdown-reader-s2b.md`) was stopped at a `wip` commit: the cause of the strikeout time is found and corrected (the path at the depth limit), the list form of I-MD-28 is linear (1,000 lines: 37.4 s before, 0.12 s after), I-MD-30 is corrected; I-MD-29 is half-done and has no test run; `docs/ISSUES.md` is not updated; the fuzz rate is not examined. | Finish `markdown-reader-s2b.md` from the next steps of its report (the tests on the head first). Then review round 3. |
| latex-writer | `ws/latex-writer` `d5a005e` | Two review rounds. Gate passed at `d5a005e`. Random trees on a new seed: 1 of 500 (`latex`) and 2 of 500 (`beamer`) differ. Round 2: Opus requests changes (4 high: a heading in a `column` div, the class `onlytextwidth`, an empty `#refs` div, an empty last References section with `--natbib`; 4 medium). Codex also requests changes: 7 new medium findings (inline code with `--listings` in headings, terms and captions; language and direction environments of Beamer section divs; block text in captions; notes inside emphasis; the identifier encoding of internal links; quadratic time in the cleanup of a citation suffix), 5 new low findings, and it rates 5 recorded rows as medium (two classes of I-LATEX-09, I-LATEX-10, I-LATEX-03, I-LATEX-05). The mutation sample, coverage and the time table did not run. | Write the correction brief from both round 2 reports (one oracle matrix for the Beamer frame structure, one for the bibliography forms). Then review round 3. |
| math | `ws/math` `6f8fbf9` | No review yet. Gate passed at `bb9ece0` (the merge of `main`), not on the head. A Fable 5.2 worker ran the brief `math-s2.md`. Replay of 14,164 formulas on the head: MathML equal for 14,034, the inline list for 14,086, the warning text for 12,912 of 13,726 (at the start of the session: 2,550 and 3,425 of 4,110 formulas). The 87 document formulas are equal in both outputs. Reuse: the command names of KaTeX 0.16.11 (MIT) are the candidate list for the oracle probes; the parser is own code. Fuzzing: 901 s with 0 crashes, one commit before the head. Coverage and the mutation sample did not run. | Run the gate, coverage, the mutation sample and a fuzz run on the head. Put the two requests of its report on `main` at the merge (a row in `docs/PRIOR_ART.md`, a decision record for the KaTeX names). Then review round 1. Then the HTML writer and the Markdown writer call the crate. |
| markdown-writer | `ws/markdown-writer` `235b8a8` | Not touched in this session. No review yet. Its own HTML renderer (`src/html.rs`) is a copy of work that the HTML writer does. | After the HTML writer is on `main`: use `randoc-html` and delete the copy; then the table rule matrix and `markua`. No session 2 brief exists. |
| lua (new) | `ws/lua` `87747d4` (wip) | First unit (brief `lua.md`): the Lua 5.4.8 build of ADR 11, the runtime with two modes, the document types in Lua as a port of `pandoc-lua-marshal`, Lua filters. Stopped at a `wip` commit; no commit passed the gate. The build and the runtime with two modes are done; the document types and the filters are half-done; the last replay run gave 262 of 268 runs equal. The process of 10 GB was a test helper that doubled a tree at each level; it is corrected. A licence question is open (section "Open issues"). | Read its report. Decide the licence question first. Then continue `lua.md` from the next steps of the report. |
| html-reader (new) | `ws/html-reader` `457f577` (wip) | First unit (brief `html-reader.md`). The tokenizer is a port of `tagsoup` 0.14.8 (BSD-3-Clause). Gate passed at `76b5527`; the head is a later `wip` commit with no test run. Harness suite `html-reader/` before the last change: 1212 units, 1198 pass. The branch calls its decision record "ADR 9"; `main` has ADR 9 to 11, so give it the next free number at the merge. | Read its report. Run the tests and the gate on the head, the fuzz run, the mutation sample. Then review round 1. |

Not started: each other reader and writer (`docx`, `odt`, `epub`, `rst`,
`org`, `typst`, `djot`, `man`, `docbook`, `jats`, wiki formats and the rest
of the 52 input and 77 output formats), custom readers and writers, JSON
filters, `--citeproc`, `--embed-resources`, PDF output through an engine,
and the `lua` and `server` modes.

### Open issues

- **The review pattern.** In this session, 12 review runs on code gave 12
  times "request changes", each with real findings (13 of 13 with the Codex report of `yaml` round 5). Three methods lowered
  the number of new findings and must stay: (1) one rule from a full oracle
  matrix that stays as replay data, not a correction of single cases;
  (2) a port from the upstream source where its licence permits; (3) a
  report or a record states only what a test or a probe shows. A worker
  report said "done" for an item that was half-done one time (yaml, round
  4): each review brief tells the reviewer to treat each claim as not
  proven.
- **Licence question in the workstream `lua` (decide before its merge).**
  The package `pandoc-lua-marshal` 0.3.2.1 has an MIT `LICENSE` file, but
  three of its files (`Walk.hs`, `SpliceList.hs`, `Topdown.hs`) have the
  header "License: GNU GPL, version 2 or above". The worker printed these
  files before it saw the header. It reports that it did not port them and
  that `crates/randoc-lua/src/walk.rs` follows `doc/lua-filters.md`,
  `Shared.hs` (MIT), `pandoc-types` and oracle probes. Working rule of the
  coordinator until the owner decides: these three files are not
  permitted material; before the merge, a new worker that did not read
  them writes the tree walk again from the documented interface and oracle
  probes, and a reviewer checks the result. Record the three files in
  `docs/PRIOR_ART.md` as not permitted.
- **A program binary can be a mutant.** After an in-place mutation run,
  `target/debug/randoc` of a worktree was a mutated program one time
  (markdown-reader). Build the program again after each mutation run and
  before each harness run or review.
- **Changes to shared contracts that the coordinator approved** and that
  arrive with a branch: in `randoc-core`, `Error::Utf8Decoding` prints the
  byte with a space pad (branch `ws/cli`, row I-CLI-25); `Host::list_dir`
  and `Context::list_dir` (branch `ws/highlight`). Check each one at the
  merge. The rows I-002 and I-NATIVE-04 of `docs/ISSUES.md` close on `main`
  with the merge of `cli`.
- **Decisions of ADR 9 to 11 that are not done yet:** pin `zip` as
  `=9.5.0` in the root `Cargo.toml` and decide the feature list of the
  harness; add size limits for an archive file and an input file to
  `randoc_core::Limits` when the first container workstream starts. The
  59 Lua source files and the one `unsafe` call are approved (ADR 11).
- **The incident of 11:31 UTC.** The Opus reviewer of `cli` round 6 ran a
  cleanup command after a failed `cd` and deleted each file of
  `env/store/briefs`. All 101 files are rebuilt from the session
  transcripts (`env/bin/recover-briefs`); the check against the prompts in
  64 Codex logs covers 55 logs byte for byte, and the other 9 hold an older
  text of a file that was edited later. Standing order 19a and each launch
  message now give rules for deleting files. That reviewer left the folder
  `env/tmp/cli-opus-r6` with files that it could not delete.
- **The model limit.** The owner asked for workers on Fable 5.2 (instruction
  11) and then ended the session at 96% of the Fable limit (instruction
  12). Ask the owner, or read the newest instruction, before you start many
  Fable 5.2 workers again. The reviewers of gate 5 stayed Codex and Opus.
- Decisions that wait for the owner: D1 to D5 (section below), the severity
  levels of gate 5, and the rule that a recorded scope gap is not a review
  finding (`docs/CONTRIBUTING.md`).
- Codex: a prompt that is a plain conformance review completes. Such a
  round does not cover the `--sandbox` path probes; the Opus reviewer
  covers them, and the merge record must say so.
- Gate items that are open for work of this session: the mutation samples
  of `html-writer`, `latex-writer` and `commonmark-reader` (6 of 127
  mutants tested) are not complete; coverage was not measured for any
  branch in this session.
- Gate items that are open for merged work of the first session: the
  mutation tests of the document-model helpers, H-001 and H-002 in
  `docs/ISSUES.md`.
- The drive: 158 GB were free at 12:07 UTC. `env/tmp` holds about 35 GB of
  old scratch folders of reviewers; some are evidence in `decisions.tsv`
  (`bench-pair-*.json`, `codex-commonmark-reader-r3b`). The internal disk
  is small (19 GB free at 12:07 UTC).

### Next three tasks

1. **Merge `yaml`, then the first end-to-end slice.** `yaml` needs one
   correction in its replay test and one review round on the diff. Then
   finish the stopped units of `cli`, `commonmark-reader` and `html-writer`
   (the briefs `NAME-s2b.md`), each with one review round with both
   reviewers, then `merge-ws`. With the command line, one reader and one
   writer on `main`, measure the slice CommonMark to HTML with the harness,
   the Pandoc test suite and the corpus, and update `spec/`.
2. **Bring the other open branches to a review.** `highlight`,
   `markdown-reader`, `latex-writer` and `math`: finish the stopped unit of
   each one, then one review round. `markdown-writer` waits for the HTML
   writer on `main`.
3. **Continue the next wave.** `lua` and `html-reader` have a first unit in
   work. Then the container formats (`docx`, `odt`, `epub`) on `zip` and
   `quick-xml` (ADR 9, ADR 10), `rst` and `org`, JSON filters, and
   `--citeproc` as a port of `citeproc` (BSD-2-Clause, checked in
   `docs/PRIOR_ART.md`). Each one is a workstream with a brief in
   `env/store/briefs/`; the briefs `lua.md` and `html-reader.md` show the
   form.

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

