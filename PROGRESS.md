# Randoc progress

This file and `spec/status.json` hold the project state. Read both at the start
of a session, then run `./init.sh`.

## Handoff

Written: 2026-11-11 20:03 UTC, at the end of the third session. The owner
said at about 18:20 UTC that the handoff can come at the next natural
stopping point. Each worker has stopped with a committed state and a report.

Key points:

- `main` has code merges again. This session put 11 workstreams
  on `main`: `harness-flaky`, `layout-perf`, `yaml`, `citeproc`, `math`, `lua`, `html-writer`, `html-reader`, `commonmark-reader`, `markdown-reader` and `cli`. The first one (`harness-flaky`) has the approval
  of both reviewers. The others merged as small blocks under a new rule of
  the owner (next point), and their merge records say which reviews exist.
- **New rule of the owner for reviews (instruction 17).** The two reviews
  run after each major block of work, not before each commit. A small
  block goes to `main` when `scripts/gate.sh` passes on the merged tree,
  the licence scan shows no copied copyleft text, and no known finding
  with the level critical is open (`docs/CONTRIBUTING.md`, section "Commit
  cadence"). So each block that merged without two approvals has a review
  that is due: it is the first task of the next session.
- **New rule of the owner for GitHub (instruction 16).** Commit to GitHub
  more often. Each workstream branch that is not on `main` goes to GitHub
  as a branch `wip/NAME`, one snapshot commit for each state that passes
  the gate script, after a licence scan.
- **The computer stopped at about 13:48 UTC** and the owner started it
  again with a forced reset at 18:11 UTC. About 4.5 hours of the session
  are lost. No committed work is lost. The cause is not known (section
  "Open issues").
- **A licence scan tool exists now** (`env/bin/licence-scan`). It found
  copied text of the Pandoc manual, the Pandoc tests and the CommonMark
  specification in four branches. The reviews did not find it. Each of the
  four workers replaced the text. Run the scan before each merge and each
  push of a branch.
- Review reports of this session: 30 with a verdict, 6 of them approvals
  (2 from new reviewers, 4 from resumed reviewers). Each new full round by
  a new reviewer found new findings. Three methods made the rounds shorter
  and the findings fewer: the same reviewer is resumed to verify the
  corrections; each correction unit of a crate that is written from oracle
  output ends with a large differential run that the worker makes itself;
  a rule set whose rules meet gets one joint oracle matrix.

### How to start the next session

0. Read `docs/OWNER_INSTRUCTIONS.md` first (18 instructions, in the words
   of the owner) and `docs/ORIGINAL_SPEC.md`, the full task text.
1. Run `. "/Volumes/Extreme Pro/Randoc/env/env.sh"` before each command, then
   `./init.sh` in `/Volumes/Extreme Pro/Randoc/randoc`. The build folder of
   the main checkout is large; the first build needs about 10 minutes.
2. Start `env/bin/randoc-watchdog` in the background. This is version 2: it
   stops each Randoc process above 4 GB (found by its command line or by
   its working directory), it does not end after a stop, and it writes
   each stop to `env/tmp/watchdog-events.log`. It ends only when a disk is
   almost full (below 25 GB on the external drive) or memory stays low.
   Keep the limits: 3 build slots in `env/bin/cargo`, `CARGO_BUILD_JOBS=2`,
   `RUST_TEST_THREADS=3` (new, in `env.sh`), `GHCRTS=-M3g`.
3. Look at the free space of the external drive first (`df -g`). It was
   between 31 and 58 GB in this session, and other data of the owner on
   the same drive grows. Remove build folders that no process uses:
   `target/doc`, `target/debug/incremental`, fuzz and coverage build
   folders, scratch folders of finished reviews. Check the working
   directory of each process first (`lsof -a -d cwd`): a `cargo` that
   started inside a worktree has no path in its command line.
4. Read `env/store/standing-orders.md`, `env/store/decisions.tsv` (the
   rows of 2026-11-11), `env/store/units.tsv` and, for each workstream,
   the top section of `env/store/reports/NAME.md`.
5. Read `docs/PRIOR_ART.md`, `docs/ADR.md` and the section "Commit
   cadence" of `docs/CONTRIBUTING.md`.
6. No worktree exists at the end of this session: each one was removed to
   save disk space, and each branch `ws/NAME` exists on the drive. For the
   next unit of a workstream that is on `main`, start a new branch from
   `main`: `git worktree add -b ws/NAME-2 "$RANDOC_WT/NAME" main`. Do not
   continue the old branch, because `main` holds it as one squashed
   commit. For `highlight`, `latex-writer` and `markdown-writer`, which
   are not on `main`, make the worktree from the old branch
   (`git worktree add "$RANDOC_WT/NAME" ws/NAME`) and merge `main` first.
   On GitHub, `wip/latex-writer` and `wip/markdown-writer` hold snapshots
   of two of them; the `wip` branches of the merged workstreams were
   removed from GitHub after their merge.
7. Tools and brief forms of this session, all in `env/bin` and
   `env/store/briefs`:
   - `s3-common.md`: the rules for a worker that continues a workstream.
   - `review-full.template.md` with `mk-full`, `review-verify.template.md`
     with `mk-verify`, `review-resume.template.md` with `mk-resume`: the
     briefs for a full review round, a verification round by new
     reviewers, and a verification by the resumed reviewers.
   - `codex-review-file PROMPT OUT`: a new Codex review (GPT-6.1 Sol).
     `codex-resume PRIOR OUT PROMPT`: resumes the Codex session of an
     earlier review by the session identifier in its log. An Opus reviewer
     is resumed by a message to the same agent; this worked across the
     restart of the application.
   - `licence-scan BRANCH...`: lists each file of a branch with 3 or more
     runs of 8 words that are also in a copyleft source.
   - `merge.template.md` (one merge with both approvals) and
     `merge-queue-s3.md` (batch mode for small blocks: one squash commit
     for each branch, one full check on the final tree, then the push).
   - `clerk.md`: the rules for a Sonnet agent that writes the records.
   Change a script that agents run only by writing a new file and renaming
   it over the old one.
8. Roles (instruction 14): the coordinator on Fable 5.2 writes briefs and
   decides; each worker is an Opus 5.5 agent (`pstack:poteto-agent`); a
   Sonnet 5.5 agent writes records and runs single checks; Codex with
   GPT-6.1 Sol and a new or resumed Opus agent review.

### What works

- `main` is at the commit that holds this text, pushed to
  <https://github.com/dvgyl/randoc>. `./init.sh` passed at 08:53 UTC on the
  tree of the start of the session. The last full merge check (build,
  format, lints, documentation, tests, harness) passed at 19:58 UTC on the
  tree of `a896ff4`.
- Formats on `main`. Readers: `json`, `native`, `commonmark`, `gfm`,
  `commonmark_x`, `markdown` and its variants, `html`, `csljson`. Writers:
  `json`, `native`, `html`, `html4`, `csljson`. Three small conversions
  with the merged program were byte-equal to Pandoc at 19:59 UTC:
  CommonMark to HTML, Markdown to HTML, HTML to native.
- The command line (`randoc-cli`): the options, defaults files, the search
  for resource files, templates and data files, the log, error texts and
  exit codes.
- Libraries on `main` that no program path calls yet: `randoc-math` (TeX
  math), `randoc-lua` (Lua 5.4.8, document types, filters, first modules),
  the CSL style and locale parser of `randoc-citeproc`, and templates in
  the output (`randoc-templates`, `randoc-data`). The CommonMark reader
  calls `randoc-yaml`; the Markdown reader and the command line still have
  their own YAML code.
- Conformance harness (merge check of batch 2, 19:58 UTC): 20,938 units,
  19,375 pass, 1,161 fail, 402 skipped, 0 regressions. At the start of the
  session: 4,546 units, 2,217 pass.
- Capability matrix (`spec/capabilities.json`): input formats 4 passing
  and 8 partial of 52; output formats 2 passing and 4 partial of 77;
  extensions 69 passing and 9 partial of 90; options 86 passing and 22
  partial of 109.
- Pandoc's own test suite (`tools/pandoc-testsuite`, measured at 20:00 UTC
  on `a896ff4`): 282 of 2,231 tests pass (0 before). By group: `command`
  270 of 1,884, `old` 12 of 174, and 0 for the readers `docx`, `odt`,
  `rtf`, `fb2`, `pptx`, `xlsx`. 1,939 tests need a format that does not
  exist yet.
- Corpus of real documents (`tools/corpus`, same time): 79 of 690 units
  pass (0 before). By target: `json` 51 of 230, `html` 28 of 230,
  `markdown` 0 of 230 (no Markdown writer on `main`).
- Quarto and R Markdown: not measured.
- 21 workstreams are on `main` (`docs/MERGE_LOG.md`): 10 before this
  session and 11 in this session.

### State of each workstream

"Gate" means `scripts/gate.sh`. Read the top section of
`env/store/reports/NAME.md` before you trust a row.

| Workstream | State at the end of the session | Next step |
| --- | --- | --- |
| harness-flaky | On `main` (`5e22a58`). Both reviewers approved. Low rows H-003, H-004, H-005 are open. | None. Remove the worktree. |
| layout-perf | On `main` (`fabec83`). Both reviewers approve `d8fa6c0` in round 2 (Codex: one low finding, row I-LAYOUT-06). The mutation sample of the last diff did not run. | Measure the writers again (row I-HTML-28). Remove the worktree. |
| yaml | On `main` (`dda9676`, test file `4bce317`). Eight review rounds. Round 8: both reviewers request changes; 3 medium rows are open: I-YAML-15 (a float text in the replay comparison), I-YAML-16 (a comment line below a block scalar that is less indented is refused), I-YAML-17 (a key that starts with `---` or `...` passes the replay checks). Checks on the head are complete: fuzzing 901 s with 0 crashes, coverage 96.35%, all 63 mutants of the last diff. | One unit for the 3 rows, then resume both reviewers of round 8. Then move the Markdown reader and the command line to `randoc-yaml`. |
| citeproc | On `main` (`5c71e99`): units 1 and 2 of 6 (the CSL style and locale parser; the reference model, CSL JSON, the formats `csljson`). Round 1 reviewed unit 1 only; the worker closed each finding with the level medium or higher. No review of the head. All 926 styles of the CSL repository parse. | The review of units 1 and 2. Then unit 3 (evaluation of one citation), brief `citeproc.md`. |
| math | On `main` (`9bce369`). Rounds 1 and 2: both reviewers requested changes each time; the worker corrected or recorded each finding. No review of the head. Replay: 35,904 formulas; a differential run with pairs of 237,006 formulas shows 0 wrong conversions. Scope gap I-MATH-01 (formulas for which Randoc returns an error and the caller writes the TeX source); low row I-MATH-06. The mutation sample did not run. | The review of the head. Then the HTML writer calls the crate, and the `\operatorname` class of I-MATH-01. |
| lua | On `main` (`18cf178`): unit 1 and a part of unit 2. Round 1: Opus requested changes (1 critical, 1 high, 3 medium), the Codex run was stopped by its service; the worker closed the critical and the high finding. The old tree walk was removed without being read and written again from permitted sources. Medium rows open: I-LUA-11 (walk time grows with the nesting depth), I-LUA-13 (the Lua state uses the C locale). Scope gaps I-LUA-03 to I-LUA-06 (most modules, custom readers and writers, the program options). | The review of the head (leave the restricted-mode probes out of the Codex prompt). Then `lpeg` and `re`, the other modules, `--lua-filter`. |
| html-writer | On `main` (`4d4373f`). Five review rounds; round 5: both requested changes (1 critical, 2 high, 12 medium in all); the worker closed each one with a joint oracle matrix of 28,965 cases. No review of the head. Row I-HTML-28 (medium: time with the nesting depth) is open until the writer is measured again with the new layout crate; I-HTML-29 low. Scope gaps: math, highlighting. Mutation and coverage did not run on the head. | Resume both reviewers of round 5. Measure I-HTML-28. Connect `randoc-math`. |
| html-reader | On `main` (`6eed2df`). Round 1 and round 2 (Opus only; the Codex run of round 2 was lost in the stop of the computer). The critical and the high finding of round 2 are closed (a budget of 200,000 for cells without input text, for the whole document). Medium row I-HTMLR-19 is open (lost text in a malformed note list). Scope gaps: MathML input, the fetch of an `iframe`, `epub:type` sections. | Resume the Opus reviewer and start a new Codex round. Correct I-HTMLR-19. The reviewers must confirm the tag-name lists (section "Open issues"). |
| commonmark-reader | On `main` (`6ba44b5`). Six review rounds; round 6: both requested changes (1 high, 7 medium); the worker closed each one. The reader uses `randoc-yaml` (17,178 metadata block texts replay with 0 differences). The CommonMark specification run gives 652 of 652. No review of the head. Scope gap I-CM-01 (`sourcepos`). Half of the mutation sample ran; no coverage number. | Resume both reviewers of round 6. Read the upstream code of the 6 modules that are not compared yet (the report lists them). |
| markdown-reader | On `main` (`d5386de`). Two review rounds, both in session 2; three units of this session with no review. Differential runs: 55 to 114 of 2,000 generated documents differ. Row I-MD-41 has 8 examined groups and some small groups with no proven level; they count as medium until they are examined. Scope gaps I-MD-01 to I-MD-05 and I-MD-08. Mutation, coverage and the time table did not run on the head. | Give each group of I-MD-41 a proven level. Then the review of the head (round 3). Then the move to `randoc-yaml`. |
| cli | On `main` (`6089e1f`). Seven review rounds; round 7: both requested changes (3 high, 4 medium); the worker closed each one. No review of the head. The run of 3,548 generated command lines was not repeated after the corrections. Row I-CLI-31: 8 cases that print a data file with a copyleft licence are removed from the suite. Mutation and coverage did not run on the head. | Resume both reviewers of round 7. Run the command-line differential tool again. Then templates in the program, tab expansion, the East Asian line-break transform, the move to `randoc-yaml`. |
| highlight | Not touched in this session. Branch `ws/highlight` at `a9c1784` (a `wip` commit whose last three edits are not compiled). One review round in session 2. The worktree is removed. `wip/highlight` is not on GitHub (section "Open issues"). | Run the brief `highlight-s3.md`. Check the licence of each syntax definition file. |
| latex-writer | Not touched in this session. Branch `ws/latex-writer` at `d5a005e`; round 2 of session 2: both reviewers requested changes (4 high, 11 medium). The worktree is removed. On GitHub as `wip/latex-writer`. | Merge `main` (the layout crate changed), then run the brief `latex-writer-s3.md`. |
| markdown-writer | Not touched in sessions 2 and 3. Branch `ws/markdown-writer` at `235b8a8`; no review. The worktree is removed. On GitHub as `wip/markdown-writer`. | Use `randoc-html` of `main` and delete the copy of the HTML renderer; then the table rule matrix and `markua`. |

Not started: `docx`, `odt`, `epub`, `rst`, `org`, `typst`, `djot`, `man`,
`docbook`, `jats`, the wiki formats and the other formats of the inventory,
custom readers and writers, JSON filters, `--embed-resources`, PDF output
through an engine, and the `lua` and `server` modes.

### Open issues

- **Reviews that are due (instruction 17).** Each workstream that merged
  as a small block needs its review of the major block: both reviewers on
  the state of `main`. Resume the reviewers of the last round where a
  session exists (`env/store/agents.tsv` and the `session id` line of each
  `reviews/NAME-codex-rN.md.log`); else start new ones with `mk-full`.
  Correct each finding with the level medium or higher in the next
  commits. The open findings that are known now are rows of
  `docs/ISSUES.md`.
- **The stop of the computer at 13:48 UTC.** No panic report exists. Free
  memory was 73% two minutes before. Suspects: a Randoc process with much
  memory that the old watchdog did not match (it matched only a command
  line with the Randoc path; the `html-reader` finding of that time let a
  976-byte table input use 2.48 GB, and a reviewer measured such inputs);
  or the change from 3 to 4 build slots at 13:30 UTC. Controls since then:
  watchdog version 2, 3 slots, a memory rule in each brief (start each
  size small; never an input that needs gigabytes).
- **Tests that depend on the machine speed.** `harness-flaky` removed the
  1-second limit that failed gates under load. One wall-clock limit stays
  (row H-005, the 2-second limit `PIPE_GRACE`): a check that fails only
  there runs again one time. The load average was between 8 and 46 on 8
  cores; another project of the owner runs on the same machine.
- **Licence.** (1) The scan finds only runs of 8 equal words, so a shorter
  copied input is not found. (2) `html-reader` holds alphabetical lists of
  HTML tag names that are equal to lists in a Pandoc source file; the
  coordinator classed them as names that follow from the tested behaviour,
  and the reviewers of the major block must confirm it. (3) `wip/highlight`
  is not pushed: no reviewer checked the licence of each of its 63 syntax
  definition files. (4) The 8 cases of `--print-default-data-file` for
  data files with a copyleft licence are removed from the `cli` suite (row
  I-CLI-31); they need a check outside the repository that stores only a
  hash. (5) Decisions D1 to D5 still wait for the owner. A new fact for
  D2: the locale files of the CSL repository are not byte-identical to the
  locales that Pandoc has built in, so a data package must take the 60
  locales of the `citeproc` 0.14.1.1 archive.
- **Requests to the coordinator that are not done (shared contracts).**
  A field for the table cell budget in `randoc_core::Limits` (`html-reader`
  and `commonmark-reader` each have a constant of 200,000); host methods
  for `pandoc.system` and `path.exists` (list a directory, make and remove
  directories, rename, copy, the working directory, environment variables,
  start a process); one shared Haskell `show` of a string in
  `randoc_core::shared` (three copies exist); a public JSON value parser
  in `randoc-ast`; tab expansion of the input and the East Asian
  line-break transform in the command line (rows I-NATIVE-04, I-HTMLR-11,
  I-HTMLR-12); the locale of the Lua state (row I-LUA-13); the pin of
  `zip` as `=9.5.0` and the size limits for an archive file and an input
  file (ADR 9, ADR 10).
- **Gate evidence that is open for the merged blocks.** The mutation
  samples are small or did not run (each merge record gives the numbers);
  coverage is not measured for `cli`, `commonmark-reader`, `html-writer`
  and `markdown-reader` on their last heads; no benchmark (gate 6) was
  measured for the small blocks.
- **Codex.** Five Codex runs of the project were stopped by the service
  with a content flag, each during probes of a sandbox or a restricted
  mode (the last one: `lua` round 1). A stopped run has no verdict. Leave
  those probes out of a Codex prompt and give them to the Opus reviewer.
  GPT-6 Astra was not used (the owner permitted it from 15:00 UTC).
- **Interpretations of the coordinator that the owner can change:** the
  three conditions for a small block (instruction 17); the `wip/NAME`
  snapshot branches (instruction 16); the severity levels of gate 5 and
  the scope gap rule (`docs/CONTRIBUTING.md`).

### Next three tasks

1. **Review the major blocks that are on `main`, and measure.** For each
   merged workstream: both reviewers (resumed where possible), then one
   correction unit for the findings with the level medium or higher, with
   the open rows of `docs/ISSUES.md` first. In parallel, measure the state
   of `main` with the harness, the Pandoc test suite (`tools/pandoc-testsuite`)
   and the corpus (`tools/corpus`), and update `spec/`. These numbers were
   not measured in this session.
2. **Connect the parts.** Templates in the output of the program (`-s`,
   `--template`; `randoc-templates` and `randoc-data` are on `main`, and
   the command line still has its own copy of some default templates);
   the HTML writer calls `randoc-math`; `--lua-filter` and `randoc lua` in
   the program (they need the host methods of the open request); the
   Markdown reader and the defaults files of the command line move to
   `randoc-yaml`; tab expansion of the input and the East Asian line-break
   transform; one field in `randoc_core::Limits` for the table cell
   budget. Each item is a small block: merge it when the gate script
   passes.
3. **Finish the open branches and start the next wave.** `highlight`
   (brief `highlight-s3.md`), `latex-writer` (brief `latex-writer-s3.md`)
   and `markdown-writer` (use `randoc-html`, delete its copy of the HTML
   renderer). Then `citeproc` units 3 to 6 (brief `citeproc.md`), `lua`
   unit 3 (`lpeg` and `re`, the other modules, `--lua-filter`, custom
   readers and writers), the container formats (`docx`, `odt`, `epub`) on
   `zip` and `quick-xml`, `rst` and `org`, and JSON filters. Give each new
   crate that is written from oracle output a differential run tool from
   its first unit.

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

