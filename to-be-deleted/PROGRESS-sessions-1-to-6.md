# Randoc progress

This file and `spec/status.json` hold the project state. Read both at the start
of a session, then run `./init.sh`.

## Handoff

Written: 2026-11-15 11:10 UTC, at the end of the sixth session (2026-11-15, from
01:09 UTC). No worker, no review and no merge runs, and the watchdog is
stopped. The coordinator records (`decisions.tsv`, `units.tsv`, the gate
lines in `inbox/`) are under `/Volumes/Extreme Pro/Randoc/env/store/`.

Key points:

- `main` is at the commit that holds this text, pushed to
  <https://github.com/dvgyl/randoc>. This session made 19 block merges
  in 8 batches (A to H). Each batch passed the full merge check on the
  merged tree with 0 regressions. Batch H ended at 11:05 UTC (`669fc05`); the last measurement is `32fd131`.
- **Measured progress in this session** (start, then now): conformance
  harness 54,488 of 57,176 units, then 59,513 of 61,366; Pandoc's own
  test suite 607 of 2,231 tests, then 646; the corpus of real
  documents 159 of 690 units, then 162.
- **No finding with the level critical is known to be open on `main`.**
  The three critical findings of the last handoff are corrected:
  cli-R9-19 and cli-R9-03 (`023405c`) and Codex R3-02 of the Markdown
  reader (`baf721b`). The reviews of this session found five more, and
  each one is corrected: row I-MD-12 of the Markdown reader (a heading
  that ends with an escaped character lost that character);
  html-writer-R7b-02 (standalone HTML lost the text of a `description`
  that holds blocks); html-writer-R7b-01 (stored test results held a
  script that the reference generates; decision D7 below);
  markdown-writer-R2-01 (`plain` wrote `[TABLE]` for a table with a cell
  span; the cause was a correction of session 5); cli-R10-05 (one case
  of the template search wrote no text).
- **New on `main`:** standalone output of the Markdown writers through
  the template; the writer contract `renders_template` with
  `TemplateWriter` (a writer that does not render a template gives a
  scope-gap error, not output with lost text); each finding of command
  line round 9 with the level medium or higher; `pandoc.read` and
  `pandoc.write`, the module table `MODULES.md` and the entrances for
  custom readers, custom writers and the interpreter in `randoc-lua`;
  the citation evaluation for dates, names and one citation; each high
  row of the first LaTeX reader review; each finding of HTML writer
  round 7b; seven of the ten findings of HTML reader round 4.
- **Reviews of this session: 9 review runs ended, each one requests
  changes, no approval.** On `856692d`: the HTML reader, round 4 (Codex:
  2 high, 8 medium); the HTML writer, round 7b (Codex: 2 critical, 3
  high, 4 medium); citeproc, round 3b (Codex: 3 high, 5 medium; Opus: 1
  high, 5 medium); Lua, round 4 (Codex: stopped by its service, 1 high
  confirmed). On `eba317c`: the command line, round 10 (Opus: each
  correction of round 9 that it checked is closed, 0 new findings above
  low; Codex: 1 critical, 3 high, 2 medium new); the Markdown writer,
  round 2 (Opus: 1 critical, 5 high, 7 medium; Codex: three stack
  overflows through the library interface and about 30 other new high
  or medium findings). The corrections of this session wait for their
  verification.
- **One record of this session was wrong and is corrected.** In batch E
  the merge tool committed the Lua block under the title of the LaTeX
  reader block (`5ec6dcf`), and the LaTeX reader block reached `main`
  only in batch G. The code of `main` passed each merge check; the
  title of that one commit stays wrong, because the history is not
  rewritten. The section "Correction of the record of batch E" of
  `docs/MERGE_LOG.md` has the facts.
- **One regression of this session was found by the measurement, not by
  the harness, and is corrected:** the option rules of the LaTeX reader
  made a comment between an option and an argument an error (the Pandoc
  test `command/6114.md`). Measure after each batch: the two tools need
  5 minutes.
- **Decision D7 is decided** (the coordinator decided it on the order of
  the owner, instruction 26; the table "Decisions that wait for the
  owner" has the text; the owner can change it): the loader script that the reference writes for
  `--katex` is a literal of 480 bytes in the source of `randoc-html`,
  because the output must be byte-identical. No stored test result holds
  it any more. The script of `--email-obfuscation=javascript` has the
  same question when a later unit writes it (I-HTML-03); the row of D7
  in the table names two literals, and one exists now.
- **The usage limits.** At the start the weekly limit for all models
  was at 99%. The coordinator started four code workers through the
  Codex CLI for 15 minutes and said so; the owner reset the limits
  (instructions 23 to 25), and each worker was an Opus agent again. The
  open changes of the stopped Codex workers were input for the Opus
  workers, which found three defects in one of them. This session used
  about 36% (the limit for all models; 21% of the limit for Fable) of the weekly limit.
- **The machine.** Other programs of the owner ran on the computer: the
  load average was above 400 at 02:50 UTC and above 100 again from
  09:30 to 10:30 UTC. Then a gate run needs 30 to 65 minutes, and a unit
  can get no result in its time limit of 20 seconds, which fails a gate
  run of correct code.

### How to start the next session

0. Read `docs/OWNER_INSTRUCTIONS.md` first (25 instructions, in the words
   of the owner) and `docs/ORIGINAL_SPEC.md`, the full task text.
1. First check: `git status --short` shows nothing in the main checkout,
   and `git rev-parse main` is equal to `git rev-parse origin/main` after
   `git fetch origin`. If the local `main` is ahead, a merge batch did not
   end: read `env/tmp/merge-s6-*/state.txt` and the check log next to it;
   push only when the log shows a passed check for the tree of the local
   head; else run the batch tool again with the same name (it continues
   at the check).
2. Read the usage limits BEFORE the fan-out (the tool
   `mcp__ccd_session_mgmt__get_usage`): the 5-hour limit and the weekly
   limit. If a limit is near its end, say it to the owner in the first
   status lines and keep the delegation of the owner; do not change the
   worker model. Five Opus workers with gate runs used about 15% of a
   5-hour window in each hour.
3. Run `. "/Volumes/Extreme Pro/Randoc/env/env.sh"` before each command,
   then `./init.sh` in `/Volumes/Extreme Pro/Randoc/randoc`.
4. Start the watchdog `env/bin/randoc-watchdog` (version 3) in the
   background; it is stopped (look with `pgrep -f randoc-watchdog`). It
   writes each event to `env/tmp/watchdog-events.log`; it stopped no
   process in this session. Keep the limits: 3 build slots,
   `CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=3`, `GHCRTS=-M3g`, at most 5
   code workers.
5. The drive: `df -g`. It had 15 to 57 GB free in this session; one
   worktree with a build needs 10 GB. Remove a worktree when its unit
   ends: `git worktree remove --force`, then `rm -rf` of the folder with
   its full path (git leaves the build folder; both steps need minutes
   on this drive). Two worktrees exist: `wt/markdown-reader` (its unit ended; an idle shell of an ended agent had the folder open at the end of the session, so the coordinator left it: remove it at the start) and the frozen review tree `wt/review-s5` at `eba317c`.
6. Tools of this session in `env/bin`:
   - `merge-batch-s6 NAME LIST [--scan-ok]`: the merge steps of
     `briefs/merge-s5.md` as a plain script with a state file, with no
     merge agent (the licence scan, each squash, the tests of each
     block, `merge-ws check`, the record in `docs/MERGE_LOG.md`, the
     trailer check, the push). It stops at a licence scan hit outside
     the licence texts: look at each hit, then run it again with
     `--scan-ok`. A sixth field of a list line names the folders that
     only this workstream changes; a conflict there takes the branch
     version when `main` holds an earlier state of the same branch.
     After each run read EACH line of the state file (`MERGED`, `LEFT
     OUT`), not only the last one, and compare the files of each commit
     with its title.
   - `gate-branch NAME BRANCH [--merge-main] [--limit-test CRATE]`: a
     gate run with no agent on a branch that has no worker. Do not start
     one while a merge check runs: the check then waits for a build
     slot.
   - `review-chain-s6`: Codex verification reviews one after the other
     in the frozen tree `wt/review-s5` (move it with
     `git checkout --detach COMMIT`, then build `randoc-cli` and
     `randoc-harness` there). `mk-s5` makes each review brief.
   - `codex-worker`: a code worker through the Codex CLI. Use it only if
     the owner asks for it.
   Briefs: `s6-common.md` (the worker rules; new rules N, on stopping a
   process, and O, on known-difference lists of other crates), one
   `NAME-s6.md` for each unit, `clerk-s6-merge-record.md` (a Sonnet
   clerk writes the merge record of one block), `clerk-s6-NN.md`,
   `measure-s6.md`.
7. Put each block that has a gate line into one batch. A worker that
   continues on a branch after a squash must run `git merge origin/main`
   first; else the next squash has conflicts in its own crate. When the
   machine has much load and a gate run of a worker fails only in units
   of another crate that got no result in 20 seconds, the merge check on
   the merged tree can be the gate of that block; the merge record must
   say so.
8. Roles (owner instructions 14, 18, 20 and 25): the coordinator on
   Fable 5.2 writes briefs and decides; each worker is an Opus 5.5 agent
   (`pstack:poteto-agent`); a Sonnet 5.5 agent writes records, measures
   and does single checks; Codex with GPT-6.1 Sol and a new Opus 5.5
   agent review.

### What works

- Formats on `main`. Readers: `json`, `native`, `commonmark`, `gfm`,
  `commonmark_x`, `markdown` and its variants, `html`, `csljson`, `latex`
  (partial). Writers: `json`, `native`, `html`, `html4`, `csljson`,
  `markdown`, `gfm`, `commonmark`, `commonmark_x`, `plain` and the
  Markdown variants (with standalone output), `latex` and `beamer`
  (fragment output; `-s` gives a scope-gap error with exit code 4).
- The command line: the options, defaults files, the resource search,
  templates and standalone output for HTML and the Markdown writers,
  JSON filters, Lua filters with their options, the log, error texts and
  exit codes.
- Libraries on `main` that no program path calls yet: the citation
  evaluation of `randoc-citeproc` (dates, names, one citation) and the
  entrances of `randoc-lua` for custom readers, custom writers and the
  interpreter. `pandoc.read` and `pandoc.write` need two lines of the
  command line (see the table).
- Conformance harness (merge check of batch H, 2026-11-15 11:05 UTC, on the tree of `d1d5d9e`): 61,366 units, 59,513 pass, 1,433 fail, 420 skipped, 0 regressions.
- Capability matrix (`spec/capabilities.json`): input formats 4 passing and 9 partial of 52; output formats 7 passing and 11 partial of 77; extensions 61 passing, 17 partial and 1 failing of 90; options 94 passing and 14 partial of 109.
- Pandoc's own test suite (`tools/pandoc-testsuite`): 646 of 2,231 tests pass (measured on `669fc05`; 607 at the start of the session, 641 on `83c1183`). By group: `command` 618 of 1,884, `old` 28 of 174, and 0 for the reader groups `docx`, `odt`, `rtf`, `fb2`, `pptx`, `xlsx`. One test was lost during the session and passes again (`command/6114.md`).
- Corpus of real documents (`tools/corpus`): 162 of 690 units pass (measured on `669fc05`; 159 at the start of the session). By target: `json` 63 of 230, `html` 36 of 230, `markdown` 63 of 230. The gain is in the input format `latex` (15 of 24 units; 12 before).
- Quarto and R Markdown: not measured.

### State of each workstream

"Gate" means `scripts/gate.sh`. Read the top section of
`env/store/reports/NAME.md` before you trust a row. Each branch that a
row names is on the drive. Each block of this session is on `main`: no
branch holds work that is not merged, except where a row names a patch
file.

| Workstream | State | Next step |
| --- | --- | --- |
| cli | On `main` (`023405c`, `47c7f1b`, `8415084`, and the block of batch H): each finding of round 9 with the level medium or higher has a correction, except the part of cli-R9-16 in row I-CLI-53; of round 10 the critical finding cli-R10-05 and cli-R10-02 are corrected. Open from round 10: cli-R10-03 (partials that are used more than one time lose INFO and log records; the cause is in `crates/randoc-templates/src/parser.rs`, `p_partial`), 2 medium findings of the Codex report, 4 low findings of the Opus report, one new low finding (`NoTitleElement` only when the template uses `pagetitle`). | Set `FilterEnvironment::readers` and `writers` in `crates/randoc-cli/src/lua.rs` (the exact lines are in `reports/lua.md`); the open findings of round 10; then the verification by both reviewers (round 11; the Codex session of rounds 9 and 10 can be resumed). Then connect the script entrances of `randoc-lua` and `--citeproc`. |
| markdown-reader | On `main` (`baf721b`, `69c7466`): Codex R3-02, I-MD-12, R4-03 to R4-06 and most of I-MD-76. Open: R4-01 (28 rest cases), R4-07, I-MD-60, one form of I-MD-76 (medium), the counterexamples of R3-12 and R3-15, row I-CLI-55 (three metadata string forms). No Opus review of round 4. The worktree `wt/markdown-reader` exists (see point 5 above). | The open findings; fuzzing, coverage, the differential run; then the verification by both reviewers (round 5). |
| markdown-writer | On `main` (`90e425e` and the block of batch H): standalone output through the template, 9 high findings of round 1, markdown-writer-R2-01. Open: the high and medium findings of both reports of round 2 (Opus: 5 high, 7 medium; Codex: three stack overflows through the library interface and about 30 others), Codex R1-02 (nested tables are slow; cause in `randoc-layout`), row I-MDW-06 (high), the medium findings of round 1. A script with code for R2-13, R2-02 and R2-03 that is not built or tested: `env/tmp/markdown-writer-s6/item1.py`. | The three stack overflows first, then the findings where text is lost or changed, then the others (brief `markdown-writer-s6.md`, items 1 and 2). |
| latex-reader | On `main` (`9a3d42d`, `b1dc3ee`, and the block of batch H): the patch of the stopped unit, each high row of round 1, most medium rows, the rule for a comment around an option. Open: I-LTXR-42 (medium), I-LTXR-53 (high: two corpus documents differ), I-LTXR-54 (low: 38 comment forms), the low rows 16 to 28 and 49. Not run: fuzzing after the parser changes, coverage, the mutation sample. | The verification by both reviewers (round 2; the Codex session of round 1 can be resumed), then I-LTXR-53, `COMMANDS.md`, the fuzz run. Before each gate: the whole test binary through `limit-run 1500 300`. |
| lua | On `main`, inside the commit `5ec6dcf` (it has a LaTeX reader title; see `docs/MERGE_LOG.md`): `pandoc.read`, `pandoc.write`, `MODULES.md` (of 468 names 279 equal, 2 different, 187 absent), the entrances, lua-R4-01. Open: the classic custom writer (I-LUA-29), I-LUA-11, I-LUA-15 (the error kind `Error::Fatal` is on `main` now), the parts of I-LUA-31 in other crates. The Codex round 4 was stopped by its service. | Both reviews of the new block (a new Codex session; an Opus reviewer). Then the 187 absent names, the classic writer, I-LUA-15. |
| citeproc | On `main` (`b014620`, `9edfab8`, `ec01fc0`, `3d9e2ea`): the evaluation of dates, names and one citation (4,050 of 4,050 reference citations equal in the test of the worker), each finding of round 3b with a correction or a recorded scope gap (row I-CITE-16). No program path calls the evaluation. One residue: a trim of one text string moves O(n²) bytes for one input class. | The verification of round 3b by both reviewers. Then unit 4 of the plan (sorting, disambiguation, collapse, positions; brief `citeproc-s6.md`, item 3), then `--citeproc`. Correct the scope text of `docs/PRIOR_ART.md` line 106 (Codex R3b-11, low). |
| html-writer | On `main` (`a1c2525`): each finding of round 7b. Open: decision D7 (row I-HTML-36), I-HTML-03 (the e-mail script), I-HTML-37 (cause in `randoc-core`). Not run: coverage, the mutation sample. | The verification by both reviewers (round 8). |
| html-reader | On `main` (`33dcee6`, `ea3c976`): R4-01 to R4-05, R4-07 and R4-10 of round 4. Open: R4-06, R4-08 and R4-09 (medium; rows I-HTMLR-24 and I-HTMLR-25), four low findings, the `iframe` fetch. Not run: fuzzing, coverage, the mutation sample. | The three medium findings (brief `html-reader-s6.md`, items 3 to 5), then the verification by both reviewers. |
| commonmark-reader | Not touched in session 6. Round 8: Opus APPROVES; Codex requests changes with 4 medium (R8-01 to R8-04). | One short unit for the 4 medium findings and the 3 low Opus findings, then the Codex verification. |
| math | Not touched. Round 4: both request changes (Opus: 2 medium in the crate; Codex: 2 high, 3 medium). | Brief `math-s5.md` (add the Codex findings), then the verification. |
| yaml | Not touched. Three medium rows open (I-YAML-22 to 24). | Brief `yaml-s5.md`. |
| latex-writer | Not touched. Standalone output is not started; the writer contract is ready for it (`TemplateWriter`). | Brief `latex-writer-s5.md`. |
| harness-speed | Not touched. No review round. The time limit of 20 seconds for one unit fails correct code when the machine has much load. | The review with the next block that changes the harness; a second try for a unit that got no result. |
| contracts | Done in this session through the cli units: the two `NativeHost` defects, `Error::Fatal`, the writer contract. Open requests: `p_partial` of `randoc-templates` (cli-R10-03), the depth check of template variables in `meta_to_context` of `randoc-core` (html-writer-R7b-04), one indentation of the math warning detail in `randoc-core` (I-HTML-37), one shared module for the join rules of an inline list, public helpers for citeproc, the listings language table for the LaTeX reader (I-LTXR-03), two time defects of `randoc-layout`. | Brief `contracts-s5.md`; one unit for `randoc-core`, `randoc-templates` and `randoc-layout`. |
| highlight | Not touched since session 2. | The section "highlight" of the brief `open-branches-s4.md`. |

Not started: `docx` (brief `docx-reader.md`), `odt`, `epub`, `rst`, `org`,
`typst`, `djot`, `man`, `docbook`, `jats`, the wiki formats and the other
formats of the inventory, `--citeproc`, `--embed-resources`, PDF output
through an engine, and the `lua` and `server` modes of the program.

### Open issues

- **Approvals.** None in this session. A block is done only when both
  reviewers approve its head. Each block of the table waits for a
  verification or has open findings of its last review.
- **Licence.** Decisions D1 to D6 wait for the owner. D7 is new and is decided (instruction 26). The
  licence scan ran for each merged block; the coordinator judged each
  hit outside the licence texts, and each judgment is in the merge
  record of its block (short character comparisons, lists of names in
  the order of the alphabet, error texts of the reference that the
  program must print, text of the default templates that
  `crates/randoc-data` holds under its BSD-3-Clause option). The licence
  audit scripts of session 4 (`env/tmp/licence-audit-s4/`) did NOT run
  in sessions 5 and 6: run them at the start of the next session.
- **Gate evidence that is open.** Mutation samples, coverage and fuzzing
  did not run for most blocks of this session (each merge record says
  what ran). No benchmark (gate 6) was measured in sessions 4 to 6. Four
  blocks of batches G and H had no gate pass on their branch; the merge
  check on the merged tree was their gate (each record says why).
- **Merge couplings.** Two tests hold a list of known differences of
  another crate and fail when a listed difference starts to pass
  (`crates/randoc-cli/tests/writer_capabilities.rs`,
  `crates/randoc-lua/tests/replay/convert.txt`). A correction in the
  other crate then needs a change of the list in the same batch (rule O
  of `s6-common.md`).
- **Interpretations of the coordinator that the owner can change:** the
  working rule of decision D7; the merge steps as a plain script, and
  the small corrections of the coordinator in a merge (a format run,
  the branch version of a crate in a conflict, four removed entries of
  known-difference lists, two removed names of `spec/baseline.json`,
  one squash by hand); the pause of the citeproc worker for 45 minutes
  to keep the limit of 5 code workers when two critical findings needed
  a worker; the Codex workers of the first 15 minutes (instruction 23);
  the work after 09:00 UTC to correct two critical findings and one
  regression before this handoff; the three conditions for a small
  block, the `wip/NAME` branches and the severity levels, as in the
  last handoffs.

### Next three tasks

1. **Close the review loop.** Correct the open findings of the last
   reviews, the highest level first: the Markdown writer (round 2: three
   stack overflows, then lost or changed text), the command line (round
   10: cli-R10-03 and two medium), the HTML reader (three medium), the
   Markdown reader (round 4). Then the verification by both reviewers,
   Codex first, for each block whose findings are corrected: the
   command line, the Markdown reader, the Markdown writer, the LaTeX
   reader, the HTML writer, the HTML reader, Lua, citeproc. The short
   units for `commonmark`, `math` and `yaml`. Run the licence audit
   scripts of session 4. Measure `main` after each batch with a Sonnet
   agent.
2. **Connect what the libraries have.** The two lines for `pandoc.read`
   and `pandoc.write`, custom readers and writers and the interpreter
   mode in the command line; citeproc: unit 4 of its plan, then
   `--citeproc`; standalone output of the LaTeX writer.
3. **The next formats.** The shared contracts (`randoc-core`,
   `randoc-templates`, `randoc-layout`); then the `docx` reader (brief
   `docx-reader.md`), the `rst` reader, the `typst` writer and the `rst`
   writer, and `highlight`. The gate evidence that is open: fuzzing,
   coverage, mutation samples, benchmarks.

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
reader. D6 came up in review round 8 of the command line. D7 came up in review
round 7b of the HTML writer (2026-11-15). The coordinator
uses a working rule for each one until the owner decides.

| Decision | Working rule now |
| --- | --- |
| D1. GPL data files of Pandoc (reference documents, translations, `epub.css`, Lua files, 98 copyleft syntax definitions) | Not in this repository. A later data package with its own licences can hold them. Randoc looks for them in a data directory at run time. |
| D2. `default.csl` (CC BY-SA 3.0) | Not in this repository. Same data package as D1. |
| D3. The abbreviation list and the DocBook entity list | Randoc does not copy the Pandoc files. The abbreviation list of `randoc_core::shared` has 76 entries that black-box tests of the oracle confirmed, from 3,926,072 candidates that a script made mechanically. The list can be incomplete (I-SHARED-03). The DocBook entity list is not started. |
| D4. 34 syntax definitions with no licence grant (C, Python, JavaScript, Lua, SQL and others) | In no package. Load them at run time from a Pandoc, skylighting or KDE installation of the user. Byte-identical highlighting for these languages is a gap until a grant exists or new definitions are written. |
| D5. Test inputs from documents with a copyleft licence: the examples of the CommonMark specification (CC-BY-SA-4.0) and example text of the Pandoc manual (GPL) | Not in this repository. Workers write their own inputs. A tool under `tools/` downloads a public test suite at a pinned version, runs it outside the repository and stores only numbers in `spec/`. Each workstream merges as one squashed commit, so that an early commit with such a file does not reach `main`. Many CommonMark projects with a permissive licence keep the specification file in their repository as test data; the owner can decide to do the same. |
| D6. The history of `main` holds the completion scripts of Pandoc (GPL) | The four stored results were on `main` from commit `6089e1f` (2026-11-11 19:59 UTC) until the removal commit of 2026-11-11. The files of the head do not hold them. The old commits still do. A removal from the history needs a rewrite of the published history and a forced push, which the owner forbids. The owner decides. The same holds for the FODT results of the suite `templates` (the styles of the reference ODT) and for three table inputs of the suite `native` (example tables of the manual), which the licence audit of session 4 found; the head does not hold them after the correction of 2026-11-11. |
| D7. Two short scripts that the HTML writer of the reference writes into its output, and that Randoc must write to be byte-identical: the script of `--email-obfuscation=javascript` (the smallest form has 252 bytes) and the loader script for `--katex` (480 bytes) | The Codex reviewer of round 7b gave the level critical to stored test results that hold these scripts (finding html-writer-R7b-01), under the rule "no text of Pandoc in stored test results". Working rule: no stored test result holds one of the two scripts (a correction unit replaces each such record with a hash comparison). One of the two literals exists in the source of `randoc-html` now (the loader for `--katex`; Randoc does not write the e-mail script yet, row I-HTML-03). It stays, because the task asks for byte-identical output and the text comes from the output of the reference used as a black box. The old commits of `main` hold the stored results; a removal from the history needs a forced push, as for D6. The text of a default template that `crates/randoc-data` holds under its BSD-3-Clause option (for example the stylesheet of `styles.html`) is permitted in a stored result (ADR 5). The owner decides whether the two literals can stay. The other way is a script that Randoc writes itself, with a recorded difference from the reference output. **Decided on 2026-11-15 by the coordinator, on the order of the owner (instruction 26): the working rule is the decision.** A script text that the reference writes into its output is an output byte sequence that Randoc must write, as each fixed HTML fragment and each error text is; it stays as a literal in the source with a comment that names its origin (observed output of the reference). No stored test result holds such a script; a hash comparison tests it. The owner can change this decision. |

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

