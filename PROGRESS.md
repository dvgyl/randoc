# Randoc progress

This file and `spec/status.json` hold the project state. Read both at the start
of a session, then run `./init.sh`.

## Handoff

Written: 2026-11-12 05:15 UTC, in the fourth session (it started on
2026-11-11 at 20:34 UTC). This is the state at the end of the session:
no worker, no review and no merge runs, and no worktree exists. The
coordinator records (`decisions.tsv`, `units.tsv`) are under
`/Volumes/Extreme Pro/Randoc/env/store/`.

Key points:

- `main` is at the commit that holds this text, pushed to
  <https://github.com/dvgyl/randoc>. This session merged 23 small
  blocks in 8 batches. Each batch passed the full merge check on
  the merged tree with 0 regressions.
- **Measured progress in this session** (start, then now): conformance
  harness 19,375 of 20,938 units, then 46,457 of 49,305; Pandoc's own
  test suite 282 of 2,231 tests, then 509 at the last measurement (03:15 UTC, before the last two batches); the corpus of real documents
  79 of 690 units, then 133 at the same measurement.
- **New on `main`:** the reader `latex` (partial), the writers `markdown`,
  `gfm`, `commonmark`, `commonmark_x` and `plain` (partial), standalone
  HTML output through the default template (`-s`, `--toc`), math in the
  HTML and Markdown writers through `randoc-math`, JSON filters
  (`--filter`), defaults files and metadata blocks through `randoc-yaml`,
  the default abbreviation list, and the East Asian line-break transform.
- **The reviews of the major blocks are done** (task 1 of the last
  handoff). Each of the nine blocks got both reviewers. No reviewer
  approved: each report requests changes. The correction units are on
  `main` for `cli`, `html-writer`, `yaml`, `commonmark-reader`, `math`
  and, for the larger part, `html-reader` and `markdown-reader`; `lua` and
  `citeproc` are in work or wait. The verification of the corrections by
  the same reviewers is due for each block (section "Next three tasks").
- **Three licence corrections on `main`.** A review and a licence audit
  found that stored test results held text with a copyleft licence: the
  completion scripts of Pandoc (the output of `--bash-completion` and
  `--completion`), the styles of its reference ODT (the output of
  `-t fodt -s`), and three table inputs from its manual. The head of
  `main` does not hold them any more. The old commits of `main` still do:
  see decision D6, which waits for the owner.
- **Critical findings of this session, all corrected on `main`:** a crash
  of the HTML reader (a table header cell with a very large `colspan`);
  lost document text in the Markdown reader (two caches for HTML closers);
  a loop of the LaTeX reader that did not end and allocated without a
  bound (it used 10 GB on this machine before the watchdog stopped it).
- **The machine.** No stop of the computer in this session. The watchdog
  is now version 3 (a poll each half second). A new tool
  `env/bin/limit-run` runs one command with a memory and a time limit;
  each worker must use it for fuzz artifacts and stress inputs (standing
  order 18c).

### How to start the next session

0. Read `docs/OWNER_INSTRUCTIONS.md` first (18 instructions, in the words
   of the owner) and `docs/ORIGINAL_SPEC.md`, the full task text.
1. Run `. "/Volumes/Extreme Pro/Randoc/env/env.sh"` before each command,
   then `./init.sh` in `/Volumes/Extreme Pro/Randoc/randoc`.
2. Start `env/bin/randoc-watchdog` in the background (version 3: it does
   not end; it writes each event to `env/tmp/watchdog-events.log`; read
   that file at each drain). Keep the limits: 3 build slots in
   `env/bin/cargo`, `CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=3`,
   `GHCRTS=-M3g`.
3. Look at the free space of the external drive (`df -g`). One worktree
   with a full build needs about 10 GB; a review tree with only the two
   programs needs 2 GB. The drive had 30 to 61 GB free in this session.
   Remove a worktree as soon as its unit ends (the branch keeps the
   commits). Do not run `du` or `find` on `wt/`: it needs minutes on this
   file system.
4. Limits that worked: at most 5 code workers at the same time (with 6 or
   7, one gate run waited 40 to 58 minutes for a build slot), about 9
   agents and Codex runs in all, free memory above 40%. One gate run needs
   20 to 60 minutes and one full merge check about 50 minutes under this
   load, so batch several blocks into one merge check.
5. Read `env/store/standing-orders.md`, `env/store/decisions.tsv` (the
   rows of 2026-11-11 from 20:34 UTC and of 2026-11-12),
   `env/store/units.tsv` and, for each workstream, the top section of
   `env/store/reports/NAME.md`.
6. Read `docs/PRIOR_ART.md`, `docs/ADR.md` (ADR 18 is new) and the section
   "Commit cadence" of `docs/CONTRIBUTING.md`.
7. Tools and brief forms of this session, in `env/bin` and
   `env/store/briefs`:
   - `s4-common.md`: the rules for each worker (with the `limit-run`
     rule). A launch message is short: the two brief files, the worktree,
     the time limit, and three rules inline (memory, delete, licence and
     git). A final answer has at most 12 lines.
   - `merge-s4.md`: the merge agent for a batch (one squash commit for
     each block, one full check on the final tree, then the push). The
     launch message lists the blocks. A worker that continues on its
     branch after a merge runs `git merge main` first.
   - `review-s4-new.template.md` and `review-s4-resume.template.md` with
     the tool `mk-s4`: the briefs for a review of a block that is on
     `main`. Reviewers read a frozen tree (`wt/review-s4b`, a detached
     checkout with built programs), not a worktree of a worker.
   - `codex-review-file PROMPT OUT` (a new Codex session) and
     `codex-resume PRIOR OUT PROMPT` (the session of an earlier round by
     the identifier in its log). An Opus reviewer of an earlier session
     cannot be resumed from a new session: start a new one.
   - `licence-scan BRANCH...` and `wip-push BRANCH NAME` (a snapshot
     commit on `wip/NAME` after a clean scan).
   - `limit-run MAX_MB SECONDS COMMAND...`.
   - `clerk.md` and the files `clerk-s4-NN.md`: the form of a record task
     for a Sonnet agent. Tell each agent that commits: no co-author line,
     no tool attribution, no other trailer.
8. Roles (owner instructions 14 and 18): the coordinator on Fable 5.2
   writes briefs and decides; each worker is an Opus 5.5 agent
   (`pstack:poteto-agent`); a Sonnet 5.5 agent writes records and runs
   single checks and measurements; Codex with GPT-6.1 Sol and a new Opus
   5.5 agent review.

### What works

- Formats on `main`. Readers: `json`, `native`, `commonmark`, `gfm`,
  `commonmark_x`, `markdown` and its variants, `html`, `csljson`, `latex`
  (partial). Writers: `json`, `native`, `html`, `html4`, `csljson`,
  `markdown`, `gfm`, `commonmark`, `commonmark_x`, `plain` and the
  Markdown variants (partial).
- The command line: the options, defaults files (through `randoc-yaml`),
  the resource search, templates and standalone HTML output, JSON
  filters, the log, error texts and exit codes.
- Libraries on `main` that no program path calls yet: `randoc-lua` (Lua
  5.4.8, document types, filters, first modules) and the CSL style and
  locale parser of `randoc-citeproc`.
- Conformance harness (merge check of the last batch, 2026-11-12 at about 05:15 UTC, on the tree of `8c723ae`): 49,305 units,
  46,457 pass, 2,446 fail, 402 skipped, 0 regressions.
- Capability matrix (`spec/capabilities.json`): input formats 4 passing and 9 partial of 52; output formats 5 passing, 12 partial and 1 failing (`markua`) of 77 (`beamer` passes, `latex` is partial); extensions 52 passing and 26 partial of 90; options 84 passing and 24 partial of 109. Five entries went from passing to partial in the last batch, because 330 new standalone units of the Markdown writer fail; no unit of the baseline was lost.
- Pandoc's own test suite (`tools/pandoc-testsuite`, measured at
  03:15 UTC on `a6fa07a`; NOT measured after the last two batches, which added the writers `latex` and `beamer`): 509 of 2,231 tests pass (282 at the start of the session). By group: `command` 493 of 1,884, `old` 16 of 174, and 0 for the reader groups `docx`, `odt`, `rtf`, `fb2`, `pptx`, `xlsx`.
- Corpus of real documents (`tools/corpus`, same time): 133 of 690 units pass (79 at the start). By target: `json` 58 of 230, `html` 31 of 230, `markdown` 44 of 230.
- Quarto and R Markdown: not measured.

A correction of the last handoff: the 1,939 tests that the test-suite tool
counts as "unsupported" do not wait for a missing format. They need
Haskell code, so an external program cannot run them. Of the 2,231 tests
that can run, the count of tests for which one missing format is the only
missing thing is: `latex` reader 235, `markdown` writer 193, `docx` reader
115, `latex` writer 110, `rst` reader 78, `typst` writer 73, `rst` writer
42, `odt` reader 41, `org` reader 36 (`env/store/reports/next-wave-demand.md`,
counted at the start of this session).

### State of each workstream

"Gate" means `scripts/gate.sh`. Read the top section of
`env/store/reports/NAME.md` before you trust a row. "Round N" is the last
review round; its reports are `env/store/reviews/NAME-opus-rN.md` and
`NAME-codex-rN.md` (a Codex report with the ending `.partial` is the last
file of a run that its service stopped).

| Workstream | State | Next step |
| --- | --- | --- |
| harness-flaky, layout-perf | On `main` since session 3, both with two approvals. | None. |
| contracts | On `main` (`4713a06`): two new fields of `Limits`, 15 `Host` methods, one Haskell `show`, a public JSON value parser, the `zip` pin, ADR 18. No review round. Rows I-CONTRACTS-01 (processor time) and I-CONTRACTS-02 (the process environment) are open. | Review it with the next block that uses it. Then the open shared requests (section "Open issues"). |
| yaml | On `main` (`2c8e284`). Round 9: both request changes. The nested list regression of this session is corrected. Three medium rows are open: I-YAML-22 (a document marker skips the replay value checks), I-YAML-23 (a blank line with a tab is refused), I-YAML-24 (tab indentation before a key is accepted). Row I-YAML-02 lists examples that match now. | One unit for the three rows, then resume both reviewers of round 9 (Codex log `yaml-codex-r9`). |
| cli | On `main` (`f6d82a9`, `34fb476`). Round 8: Opus requests changes; the Codex run was stopped (partial report). Each finding with the level medium or higher is corrected. New: templates, the default abbreviation list, defaults files through `randoc-yaml`, JSON filters, East Asian line breaks. Open: row I-CLI-35 (medium: `randoc-core` gives the "no title" message the level WARNING, the reference INFO; ten template cases fail), the matrix of 300 standalone command lines, `-s` is stopped for the LaTeX and Markdown writers, `--lua-filter` is not connected. No review of unit 2. | Unit 3: the level of that message, `-s` for each writer that renders a template, `--lua-filter` through `randoc_lua::apply_filter_files` (the exact call is in `reports/lua.md`), the removal of the `saphyr-parser` notice rows, the standalone matrix. Then the verification review. |
| html-writer | On `main` (`24b5136`). Round 6: both request changes (the time of nested section divs; one report claim); corrected. New and not reviewed: standalone output, `--toc`, math through `randoc-math`. Coverage of `standalone.rs` is 79%. Row I-HTML-34 (`abstract-title` needs the translation files of Pandoc, decision D1). | The review of the new code. Fuzzing, the mutation sample, the `-s` conformance cases. |
| html-reader | On `main` (`7f8b193`, `8e95526`). Round 3: Opus requests changes (1 critical, 4 high, 7 medium); the Codex run was stopped (partial report with the same findings). Corrected: the crash, 3 high, 4 medium. Open medium rows: I-HTMLR-19, I-HTMLR-21, I-HTMLR-22, I-HTMLR-23. The licence question on the tag-name lists is answered: both reviewers confirm each list from the behaviour of the reference. | Unit 2 (brief `html-reader-s4b.md`), then the verification. |
| commonmark-reader | On `main` (`6e8fcd9`). Round 7: both request changes (1 high, 6 medium); each one is corrected. Open: 16 documents of the differential run in two classes are not reduced; the specification run was not repeated after the unit; no coverage number. | The verification by the reviewers (Codex log `commonmark-reader-codex-r7`). The specification run. |
| markdown-reader | On `main` (`c145443`, `8508188`, and the last batch). Round 3: Codex requests changes (2 critical, 15 high, 3 medium), Opus requests changes (1 high, 4 medium). Corrected: both critical findings (a test compares each cache with the plain scan), seven time findings, the abbreviation rule. Open: the rows I-MD-56 (the rest of a depth finding: one form overflows a 2 MB stack at 10,000 repetitions), I-MD-57 and I-MD-58 (quadratic time), and eight high and five medium conformance findings (row I-MD-59 lists them). Fuzzing, coverage and mutation tests did not run in this session. | Unit 4: the open high findings first (`reports/markdown-reader.md` has a plan for I-MD-57 and I-MD-58). Then the verification. This reader is the most used one. |
| math | On `main` (`e145a28`). Round 3: both request changes (4 high, 7 medium); each one is corrected from 11,050 new reference rows. The warning shows the line and the column of the reference. Open: coverage, the mutation sample, formulas with characters that have no conversion (I-MATH-01). | The verification by the reviewers (Codex log `math-codex-r3`). |
| lua | On `main` (the last batch). Round 2: both request changes (5 high, 8 medium); all are corrected. New: the locale of the environment, each function of `pandoc.system`, the entrance `apply_filter_files`. Open: `lpeg` and `re` (not started: they need a second `unsafe` block; ADR 11 permits one; the coordinator of session 4 recommends to permit it with a safety argument and a test, as for the first one), row I-LUA-11 (walk time with the depth), `cputime`. No program option calls the crate yet. | The command line connects `--lua-filter`. Then `lpeg` and `re`, the other modules, `pandoc.read` and `pandoc.write`. The verification (Codex log `lua-codex-r2`). |
| citeproc | On `main` (units 1 and 2, and the last batch). Round 2: Codex requests changes (1 critical, 3 high, 9 medium), Opus requests changes (2 medium). Corrected: the critical finding (a table lost its text in CSL JSON) and two high findings. Open: Codex R2-07 (render functions overflow a 2 MB stack) and the medium findings (`reports/citeproc.md` lists them; a first probe of two of them found 46 of 204 metadata cases that differ). | The open findings (brief `citeproc-s4.md`, items 2 to 4), then unit 3 of the plan in `reports/citeproc-research.md`. |
| latex-reader | New in this session. On `main` (`6a01107`, `97d69d8`, `27f478c`, and the last batch). No review round. Two critical defects that its own fuzz run found are corrected (a loop without end; a parameter count without a bound); each parser loop has a shared guard and the reader has a step budget. A table of 351 common commands (`crates/randoc-latex-reader/COMMANDS.md`): 162 equal in each probed context, 68 partial, 4 different, 117 not started. For 3 of the 8 corpus documents the reference stops with a parse error and Randoc does not (row I-LTXR-11). | Unit 4: the command table and row I-LTXR-11 (brief `latex-reader-u3.md`, items 3 to 6). Then the first review. |
| markdown-writer | New on `main` in this session (`c0a90c0`, and the last batch). No review round. With the trees of the reference as input, 57 of 60 corpus documents are correct. Open: standalone output is half-done (330 replay units with reference results fail, because the writer does not render the template yet; a draft that is not compiled is in `env/tmp/markdown-writer-s4/`), grid tables with row spans (I-MDW-02), `--ascii` (I-MDW-05), one reference form (I-MDW-06), `markua`; 177 of 200 large random trees differ for `markdown` (many small rules). | Finish standalone output (brief `markdown-writer-u3.md`, item 2), then items 3 to 6. Then the first review. |
| latex-writer | New on `main` in this session (the last batch): the writers `latex` and `beamer`, fragment output. The findings of the old review round 2 are corrected. No review of the current code. Open: standalone output is not started; rows I-LATEX-09 and I-LATEX-10 have old text; scope gaps math and highlighting; the mutation sample and coverage did not run. | Standalone output with `default.latex` and `default.beamer` (the cut of the HTML writer). Then the review. |
| highlight | Not touched in sessions 3 and 4. Branch `ws/highlight` at `a9c1784` (a `wip` commit whose last three edits are not compiled). Not on GitHub. | The section "highlight" of the brief `open-branches-s4.md` (the merge of `main`, the licence table of each definition file, then `highlight-s3.md`). |

Not started: `docx` (a brief is ready: `docx-reader.md`), `odt`, `epub`,
`rst`, `org`, `typst`, `djot`, `man`, `docbook`, `jats`, the wiki formats
and the other formats of the inventory, custom readers and writers,
`--lua-filter` in the program, `--citeproc`, `--embed-resources`, PDF
output through an engine, and the `lua` and `server` modes.

### Open issues

- **Verification of the corrections.** No block has an approval of this
  session. For each block whose correction unit is on `main`, the same
  reviewers verify: resume the Codex session (`codex-resume` with the log
  of the round in the table) and start a new Opus reviewer with the
  template `review-s4-new` on a new frozen tree of `main`.
- **Licence.** (1) Decision D6 waits for the owner: the old commits of
  `main` hold the completion scripts, the FODT results and the manual
  tables; a removal needs a forced push. (2) The licence scan of a branch
  diff is not sufficient: a stored reference result can hold copyleft
  text that the reference program generates. The audit of this session
  (`env/store/reports/licence-audit-s4.md`) checked all 751 tracked files
  of `c494dd5`; run its scripts (`env/tmp/licence-audit-s4/`) again
  before each handoff. (3) `wip/highlight` is still not pushed: nobody
  checked the licence of each syntax definition file (the brief
  `open-branches-s4.md` has the item). (4) Decisions D1 to D5 still wait.
- **Requests to the coordinator that are not done (shared contracts).**
  One shared module for the join rules of an inline list (three copies:
  `randoc-html-reader`, `randoc-latex-reader`, `randoc-lua`; the port of
  `Text.Pandoc.Builder` belongs in `randoc-ast`); `is_space` of
  `randoc_core::shared` public for `randoc-commonmark`; the level of
  `LogMessage::NoTitleElement` (the reference gives INFO); the removal of
  the `saphyr-parser` rows in `THIRD_PARTY_NOTICES.md` and
  `docs/PRIOR_ART.md` and of its licence file (no crate uses it); the
  rows I-CONTRACTS-01 to 03 (the Lua unit in work handles them).
- **Gate evidence that is open.** Mutation samples and coverage did not
  run for most blocks of this session (each merge record says what ran);
  no benchmark (gate 6) was measured; the Markdown writer and the LaTeX
  reader have no review round yet.
- **Codex.** Two more Codex runs were stopped by the service with a
  content flag (`cli` round 8, `html-reader` round 3), seven in all. Their
  last report files held real findings, among them both critical ones. A
  stopped run can leave a child process: look for processes whose working
  directory is in its scratch folder.
- **Speed of the checks.** One gate run needs 20 to 60 minutes and the
  harness has more than 40,000 units. Most of the time is file input and
  output on the exFAT drive. A faster harness (fewer temporary files, a
  filter for the suites that a branch changes) would raise the merge rate.
- **Interpretations of the coordinator that the owner can change:** the
  three conditions for a small block; the `wip/NAME` snapshot branches;
  the severity levels; test names of Pandoc as identifiers in
  `spec/pandoc-testsuite.json` (the licence audit classed them as not
  clear; the coordinator keeps them).

### Next three tasks

1. **Close the review loop of each block.** Merge the units that are in
   work (the table above). Then, for each block: the verification by the
   same reviewers, and one correction unit for what stays open. Start
   with the blocks that had critical findings (`markdown-reader`,
   `html-reader`, `latex-reader`), then the first review round of the
   Markdown writer and the LaTeX reader. Measure `main` after each batch
   with a Sonnet agent (the report form is in
   `env/store/reports/measure-s4.md`).
2. **Connect the Lua runtime and citations.** `--lua-filter` in the
   command line (the Lua unit gives one public entrance for it), then the
   other `pandoc` modules, `pandoc.read` and `pandoc.write`, custom
   readers and writers, and `randoc lua`. `citeproc` units 3 to 6 (the
   brief `citeproc.md`) with the round 2 findings first, then
   `--citeproc`. Quarto and R Markdown need both.
3. **The next formats, in the order of the demand table:** finish the
   LaTeX reader (the command table `COMMANDS.md`) and the Markdown writer
   (real documents first), merge the LaTeX writer, then the `docx` reader
   (brief `docx-reader.md`; it also builds the shared archive and XML
   layer for `odt` and `epub`), the `rst` reader, the `typst` writer and
   the `rst` writer. `highlight` (brief `open-branches-s4.md`) gives
   syntax highlighting to the HTML and LaTeX writers. Give each new crate
   a differential tool from its first unit, a step budget for each parser
   loop, and a test that compares each cache with the plain scan.

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

