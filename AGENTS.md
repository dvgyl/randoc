# Randoc build manual (branch `gpt/build`)

This file is the operating manual for each model session that works on this
branch. Codex loads it at the start of each session. Read it completely.
Read it again after each context compaction and after each restart.

## 1. The task

Randoc is a Rust reimplementation of Pandoc 3.13. The goal is a complete
drop-in replacement: a user puts Randoc on `PATH` under the name `pandoc`,
and everything works unchanged. The exact, measurable definition of the goal
is in `plan/SPEC.md`. The goal never becomes weaker.

You build toward that goal on the branch `gpt/build` in the clone
`/Volumes/Extreme Pro/Randoc/gpt/randoc`. The branch `main` belongs to
earlier work. You never change it. A later review takes the good parts of
this branch into `main`. That review reads `plan/REVIEW_LOG.md` first.

Progress is measured, not claimed. A change is good only when a scoreboard
number moves and no other number falls.

## 2. Which instruction wins

When two instructions disagree, the first one in this list wins:

1. A direct instruction of the owner in the current session.
2. This file (`AGENTS.md`).
3. `plan/SPEC.md` (what "compatible" means and how it is measured).
4. `plan/ROADMAP.md` and `plan/milestones.toml` (the order of the work and
   the milestone gates).
5. The ExecPlan of the current milestone (`plan/exec/M<n>.md`).
6. `plan/ARCHITECTURE.md` (how the remaining work is built).
7. All other files.

The files `PROGRESS.md`, `docs/CONTRIBUTING.md`,
`docs/WORKER_STANDING_ORDERS.md`, `docs/OWNER_INSTRUCTIONS.md` and
`docs/MERGE_LOG.md` describe the process of the earlier phase on `main`.
On this branch, their PROCESS rules do not apply. Use them only for technical facts and
history. Their hard rules that still apply are repeated in sections 4, 13,
14 and 15 of this file.

When you stop, change direction, or skip a rule, write the file and the rule
that made you do it in `plan/REVIEW_LOG.md`.

## 3. Models and effort (the only place with model names)

| Role | Model id | Reasoning effort | Use |
| --- | --- | --- | --- |
| Lead model | `gpt-6.2-sol` | `high` | The lead session: plans, items, commits, merges. |
| Worker | `gpt-6.2-sol` | `high` (`low` for routine edits) | One item in one worktree (`ops worker run`). |
| Reviewer | `gpt-6.2-sol` | `high` | One review round in a new session (`ops review`). |
| Adviser model | `gpt-6.2-astra` | `xhigh` | One question when the lead is stuck (`ops ask`). Advice only; it never edits. |

The `ops` commands read the role, the model id and the effort from this
table (`tools/autobuild/opslib/codex.py` parses it). A change of a model
changes only this table. Use the effort `low` for a routine worker item:
`ops worker run ID --effort low`.

Rules for every model call:

- Use `-c service_tier=default` in every `codex` command. Do not use Fast
  mode (the `priority` or `ultrafast` service tier, or `/fast`) unless the
  owner explicitly tells you to. Fast mode uses more credits.
- Never use the effort `max` or `ultra` unless the owner explicitly tells
  you to.
- Use the `ops` commands for workers, reviews and the adviser. They set the
  model, the effort and the service tier. Do not start subagents in another
  way, because the other ways do not log the call and do not keep the limits
  of this file.
- The broker is the owner's support program outside the sandbox. It runs
  each review and each adviser question as a new top-level session with
  read-only access. It also pushes (section 12) and builds held-out
  candidates (section 11).

## 4. Hard limits

The sandbox and the Git guard enforce these limits. Do not try to get around
them: each attempt is a defect that the later review reverts.

- **Branches.** Commit only on `gpt/build` (the lead) and `gpt/w/<ITEM>`,
  where `<ITEM>` is an active item (a worker). Never change, merge into,
  rebase or push `main` or any other branch. The build has no GitHub
  credential: push only with `tools/autobuild/ops push`. The broker then
  scans the licence of each new commit and pushes `gpt/build` to
  `gpt/build`, fast-forward only. Never rewrite pushed history: undo a
  change with `git revert`. Never change `core.hooksPath`, never use
  `--no-verify`, never delete or edit the hooks. The guard is in
  `/Volumes/Extreme Pro/Randoc/env/gpt-hooks/` (its source:
  `tools/autobuild/hooks/guard.py`).
- **Old local branches.** The owner's checkout
  `/Volumes/Extreme Pro/Randoc/randoc` has old `ws/*` and `wip/*` branches.
  Their early history holds files that must never be published. Never
  merge, cherry-pick, fetch into a branch, or push a commit of them. To reuse
  their code, copy the files (`git -C "/Volumes/Extreme Pro/Randoc/randoc" show COMMIT:PATH`),
  run the licence scan, and commit them as a new commit. The guard refuses
  each outgoing commit that is not on `origin/main` and does not descend
  from the branch base `d40a641`.
- **Held-out data.** Never try to read `/Volumes/Extreme Pro/Randoc/holdout/`
  or to find its contents in another way (for example by logging the inputs
  that the evaluator gives to the program). The sandbox denies it. Only the
  evaluator reads it. Why: it detects overfitting; if you see it, the final
  number means nothing.
- **Clean room.** Pandoc is GPL-2.0-or-later. Never read Pandoc source code:
  not in `env/pandoc-src` (the sandbox denies its source folders) and not on
  the web. You may read the Pandoc manual and documentation. A tool may read
  the files of `env/pandoc-src/test` and
  `env/pandoc-src/pandoc-lua-engine/test` at run time only, to run Pandoc's
  tests as an external check; never a file of `env/pandoc-src/src`. Learn
  the behaviour from the manual and from the oracle's output.
- **Configuration.** Never edit `~/.codex/config.toml`,
  `~/.codex/randoc-gpt.config.toml`, `~/.codex/AGENTS.md` or anything in
  `/Volumes/Extreme Pro/Randoc/env` except the folders that the sandbox lets
  you write. If one of them blocks you, write it under BLOCKED ON OWNER.
- **Deletes.** Delete only files that you created, with a full absolute path,
  and only under `/Volumes/Extreme Pro/Randoc/gpt/`. Never join a delete to a
  `cd` with `;` or `&&`. Stop only processes that you started, by their
  process id.

## 5. Start of each session

Run these commands from the repository root:

    . tools/autobuild/env.sh
    tools/autobuild/ops status

`ops status` shows the counters of section 7 and the next required action.
Do that action first (for example `ops measure` when the commit counter is
at its limit). Then read the active ExecPlan (`plan/exec/M<n>.md`) and its
`Progress` section, and continue from the first open step.

## 6. The work loop

All work is organized in work items. A work item is a short task with one
scoreboard metric and an expected gain.

1. **Choose.** Take the next item of the current milestone from
   `plan/ROADMAP.md`, or run `tools/autobuild/ops next` to get items from the
   newest failure lists. An item is short: it targets one cluster of failing
   units with one cause (at most 40 units), and it names those units. If
   you cannot name the failing units, the item is too large: split it. Split
   a roadmap row with a gain above 40 before its first item starts
   (`plan/ROADMAP.md` rule 4).
2. **Register.** `ops item add M<n>-<nn> --milestone M<n> --metric KEY --target +N --title "..."`,
   then `ops item start M<n>-<nn>`. The start records the baseline. For a
   worker item, `ops worker start` does the start (section 9). An item that
   must cover all units or a minimum TOTAL gets `--all-pass` or
   `--min-total N` (the column `accept` of `plan/ITEMS.tsv`).
3. **Change.** Make the smallest change that can move the metric. Add or
   update tests built from oracle output. Commit small steps with the item id
   at the end of the subject (section 12).
4. **Experiment.** `ops measure --item M<n>-<nn> --quick`. An item that
   builds a measurement uses `--suite NAME` in place of `--quick`, and runs
   its first experiment only when its suite files exist. One run is one
   experiment: it writes an attempt record before it starts, so a failed
   run also counts. Look at the change of the item's metric and at the
   other metrics of the run. A run with `--filter` and no `--item` is a
   diagnostic, not an experiment.
5. **Decide.** Gain: the latest row of the item's metric has a PASS above
   the baseline and a failing count (TOTAL - PASS) not above the
   baseline's (`plan/SPEC.md` 4.1).
   - Gain and no regression: run the gate (`ops gate`). When it passes, run
     `ops item done M<n>-<nn> --weak-spots "..."`, then `ops measure`,
     commit its rows, and `ops push`. When the gate fails, the item stays
     active: fix the cause and run the next experiment. `done` refuses
     with no passing gate of the current commit. For an item with a
     coverage rule, a latest row below the rule is no gain.
   - No gain: find the cause, change, and run the next experiment.
   - Third experiment with no gain: `ops item cut M<n>-<nn> --reason "..."`.
     The command reverts the item's commits and withdraws its tool
     corrections. `measure.py` refuses a fourth experiment. Write what you
     learned in the log. Do not start the same cause again under a new id
     (a new id hides the cut streak of section 7); choose a different cause
     or a different item.
6. **Measure the branch.** After each finished item, and at least each 10
   commits, run the full measurement on `gpt/build` (`ops measure`) and
   commit its rows.
7. **Tool corrections.** A change of how a tool counts the units of an
   existing metric is a tool correction. Before the first row with the new
   rule, run `ops correction add METRIC --item ID --reason TEXT` and write a
   `decision` entry (`plan/SPEC.md` 3.13). Older rows of METRIC then no
   longer count.

Work that does not move a metric is not allowed, with two exceptions: work
on the measurement itself (the first milestone builds it), and a fix of a
crash, a hang, unbounded memory or a licence defect. A fix of that kind
still needs an item; its metric is the test that shows the defect (for
example a new conformance case).

## 7. Counters that stop loops

These counters exist because models can loop for a long time with many
commits and no progress. `ops status` shows each one. The Git guard enforces
the first three.

| Counter | Limit | What happens at the limit |
| --- | --- | --- |
| Commits since the newest measured commit | 10 | The guard refuses the next commit until you commit the rows of a completed full measurement (clean tree, item `-`, each active suite) of `HEAD` or its parent. Only such a run resets the counter. |
| Experiments of one item with no gain | 3 | `measure.py` refuses the next experiment; the guard refuses each commit until you cut the item. |
| Commit tag | an active item, or `[ops]` | An `[ops]` commit may change only measurement, plan and tool files. Product code needs an active item. |
| Scoreboard rows of the active item's metric with no gain | 2 in a row | You are stuck (section 8). |
| Items cut in a row in one area | 3 | Park the area: write a `weak-spot` entry, ask the adviser one strategy question (`--reason no-next-step`), and move to the next area of the roadmap. |
| Full `gpt/build` measurements with no gain on any metric | 3 in a row | Stop the current approach. Finish or cut the active item. Then run `ops next` and take the largest cluster in a different area. |
| Review rounds of one change | 2 | No third round. The lead decides with the evidence. |
| Questions to the adviser per stuck event | 1 | Decide and continue. |

Never set a target of code size, and never start work whose only purpose is
to remove lines. Delete code only as a part of an item that moves a metric.

## 8. Stuck: definition and the one question

You are stuck only in one of these four cases:

1. A failure resisted two fixes (`two-fixes-failed`).
2. A design failed one acceptance attempt (`acceptance-failed`).
3. Two scoreboard rows of the active item's metric show no gain
   (`two-rows-no-gain`; `ops status` detects it).
4. You know no next step (`no-next-step`).

Only then, ask the adviser one question:

    tools/autobuild/ops ask --reason two-fixes-failed --evidence "TEST OR COMMAND AND ITS OUTPUT" "QUESTION WITH ALL CONTEXT"

The reasons `two-fixes-failed`, `acceptance-failed` and `no-next-step` need
`--evidence`; `ops status` detects `two-rows-no-gain`. The adviser does not
see your session. Put the facts into the question: the item, the failing
case, the command and its output, what you tried and why it failed. Take the
one answer, decide, write the decision in `plan/ADVICE.md` (the command
starts the entry), and continue. One event is one (item, reason) pair: one
question each. A sent question counts also when no answer comes.

## 9. Workers

Use workers when two or more items of the current milestone are independent
(they change different crates or files and need no result of each other).

- `ops worker start ITEM` activates the queued item, records its baseline,
  and makes the worktree `/Volumes/Extreme Pro/Randoc/gpt/wt/ITEM` on
  branch `gpt/w/ITEM` from `gpt/build`. Do not run `ops item start` for a
  worker item. At most 4 workers. The command refuses when the drive has
  less than 25 GB free.
- `ops worker run ITEM` starts the worker in the background with the prompt
  `tools/autobuild/prompts/worker.md` (`--effort low` for a routine item).
  A worker does one item, measures it on its branch (its rows go to
  `plan/scoreboard/ITEM.tsv`), and stops.
- `ops worker merge ITEM` measures the item's metric again on the merged
  tree, runs the gate there, and merges with `--no-ff` only when both show
  the gain and no regression. Then it removes the worktree.
- `ops worker drop ITEM` removes a worker that failed or was cut. Its branch
  stays for the review; it is never merged. `merge` and `drop` refuse a
  worktree that is not registered, not on its branch, or has changes.

What a worker may change: NEW files under `tools/<name>/`,
`tools/autobuild/suites/`, `tools/autobuild/metrics.d/`,
`tools/autobuild/tests/test_suite_*.py` and `conformance/cases/`; its own
rows in `plan/scoreboard/ITEM.tsv`; new rows of
`tools/autobuild/provenance.tsv` for the files that it adds (section 14);
the product code of its item outside
the shared contracts. The lead owns every other file of `plan/` and
`tools/autobuild/`, the shared contracts (`randoc-ast`, `randoc-core`, the
format registry, the harness), and every change of an existing metric. The
lead changes them only on `gpt/build`. A worker that needs such a change
stops and writes the request in its final message.

## 10. Reviews

Review only critical code: a change to a shared contract; a parser path that
untrusted input reaches (depth limits, size limits, loops); `unsafe` code;
code that touches files, archives, URLs or processes; and a change that can
break the licence position. Other changes need no review: the measurement
and the tests are the check.

- `ops review ITEM` asks the broker for one review round in a new
  read-only session. At most 2 rounds for one change. `ops` records the
  round before the launch: a failed or malformed run also uses its round.
  After each review and each adviser run, `ops` checks that your tree did
  not change.
- A reviewer blocks a change only with evidence: the name of a failing test,
  or a command, together with the output that shows the defect. A finding
  without that evidence is a note, not a block. The lead writes notes as
  `weak-spot` entries in the log.
- Fix each blocking finding, then run the second round, if needed. After the
  second round, the lead decides and writes the decision in the log.

## 11. Held-out measurement

A part of the corpus and of the generated cases is held out. You never see
it. The evaluator runs outside the sandbox and gives only totals. The rules
are in `plan/SPEC.md` section 6.

- `ops holdout request` names a commit only; the broker builds it. Request
  an evaluation at each milestone gate. One evaluation each 6 hours: an
  earlier request gets `deferred` and the next allowed time. Then continue
  with other items, and request again after that time
  (`ops holdout request --commit SHA` names the earlier candidate).
- `ops holdout results --record` writes the newest totals of
  `/Volumes/Extreme Pro/Randoc/holdout-results/` as the rows
  `holdout.corpus` and `holdout.generated`. A reserved FINAL part of each
  set is never reported to you.
- If the visible numbers rise and the held-out numbers do not, you overfit:
  write a `weak-spot` entry and change the approach (general logic, not
  special cases).
- If `ops holdout results` says that the evaluator heartbeat is older than
  10 minutes, the evaluator does not run. Write it under BLOCKED ON OWNER
  and continue.

## 12. Commits, pushes and the review log

- Small commits: one logical change each, with its test. A later reviewer
  must be able to cherry-pick or revert each commit alone.
- Subject: `type(scope): plain description [ITEM]`, for example
  `fix(markdown-writer): keep the text of a heading with an escape [M2-04]`.
  `type` is one of `feat`, `fix`, `perf`, `test`, `docs`, `chore`,
  `refactor`. The tag is an active item id, or `[ops]` for measurement,
  plan and tool files.
- No attribution lines (`Co-Authored-By:`, `Claude-Session:`, "Generated
  with") and no tool or model names in commit messages. The guard removes or
  refuses them.
- Push `gpt/build` after each finished item and after each milestone:
  `tools/autobuild/ops push`. Never use `git push`: the build has no GitHub
  credential. Before each push, run
  `python3 -I tools/autobuild/licence_scan.py scan`; it must exit 0. The
  broker scans each commit again before it pushes. If a push fails, keep the
  local commits, write the reason in the log, and continue.
- Write `plan/REVIEW_LOG.md` entries with `ops log KIND "TEXT"`. Kinds:
  `decision`, `deviation` (from the plan), `shortcut`, `weak-spot`, `cut`,
  `blocked-on-owner`, `milestone`. This log is the handover to the later
  review. Write each shortcut and each known weak spot there at the time you
  take it, not later.
- Write a blocker that only the owner can remove with `ops blocked "TEXT"`
  (it goes to `plan/BLOCKED_ON_OWNER.md`). Then continue with other work.

Do not use the words "HOLD" and "fresh complete review" in any file, log or
commit message. The guard refuses them. Why: they started review loops
without progress in an earlier project.

## 13. Engineering rules for Randoc code

- The oracle is `/Volumes/Extreme Pro/Randoc/env/oracle/pandoc-3.13-arm64/bin/pandoc`
  (`$RANDOC_ORACLE_PANDOC`). It is a test tool only. Randoc never calls it.
- Implement general logic. Never add a special case for one test input.
  Never edit a stored oracle result to make a test pass. If an oracle output
  looks wrong, record it in the log; the oracle still defines correct.
- Tests are required: each new behaviour gets tests built from oracle
  output, including the edge cases of the manual. A bug fix gets a test that
  fails before the fix.
- Treat all input as untrusted. Keep the nesting limit
  (`Limits::max_nesting_depth`), the decompressed-size limit, the archive
  path checks and the fetch limits. Each tree walk has a depth limit, with
  tests at 10,000 and 100,000 levels on a 2 MB stack. No recursion that a
  deep input can overflow; this includes derived `Clone` and `Drop` of deep
  trees.
- No `unwrap()`, `expect()`, `todo!()`, `unimplemented!()` or `panic!()` in
  code that input can reach. `unsafe_code` is forbidden in the workspace.
- No scan inside a loop that makes the time grow faster than linear. Each
  parser loop consumes input or stops. Each cache has a test that compares
  it with a plain scan.
- Tests that measure time must not fail under load: count steps, or take the
  minimum of several runs with a wide margin.
- A tool that runs a program under test sets the limits of `plan/SPEC.md`
  3.9. It sets the CPU limit with `sh -c 'ulimit -t N; exec PROGRAM ARGS'`,
  which macOS enforces, and needs no `unsafe` code.
- Some tests hold lists of known differences of another crate
  (`crates/randoc-cli/tests/writer_capabilities.rs`,
  `crates/randoc-lua/tests/replay/convert.txt`, and others named in
  `plan/ARCHITECTURE.md`). A fix that makes a listed difference pass must
  update the list in the same commit.
- Formatting and lints: `cargo fmt --all`, and clippy with `-D warnings`.
  `ops gate` runs both.
- Reuse permissively licensed code where it helps (see section 14). Prefer a
  pinned dependency to a copy.

## 14. Licence rules

Randoc is MIT OR Apache-2.0. Pandoc is GPL-2.0-or-later.

Never put into the repository, in any commit, what `plan/SPEC.md`
section 8 lists. In short:

- Pandoc source code, code ported from it, `texmath`, the GPL
  `skylighting` package, other GPL code.
- Pandoc test files, text or examples of the Pandoc manual, examples of the
  CommonMark or GFM specification. Write your own inputs.
- Pandoc data files with no permissive licence option
  (`docs/DATA_FILES_LICENCES.md`).
- Stored test results that embed copyleft text (for example `-s -t docx`
  output): check such a result with a hash.
- Any file from `/Volumes/Extreme Pro/Randoc/env/store`.

Before you use third-party code: read the licence header of each file (a
package licence file can differ from a file header), check that the licence
is on the list in `docs/PRIOR_ART.md` (MIT, Apache-2.0, BSD-2-Clause,
BSD-3-Clause, ISC, Zlib, Unicode-3.0, CC0-1.0, Unlicense), and add a row to
`docs/PRIOR_ART.md`, a record to `docs/ADR.md`, a row to
`THIRD_PARTY_NOTICES.md` and the licence text to `third_party/licences/`.
The scan skips binary files (a NUL byte in the first 8 KB) and files above
5 MB. In the same commit, give each such file a row in
`tools/autobuild/provenance.tsv`: path, the SHA-256 of the content, origin
and licence. The guard refuses the commit without it, and a changed file
needs a new row.

The scan `python3 -I tools/autobuild/licence_scan.py scan` compares the
text that the branch adds with a hashed index of the copyleft reference
text. Run it before each push and in each gate. The pre-push guard scans
the text that each outgoing commit adds. At each milestone gate, run
`licence_scan.py scan --all` (the whole tree at `HEAD`; a hit that
`origin/main` already has is marked `inherited`). Fix each hit. If a hit is
correct use (for example an error text that the program must print), add a
row with the reason to `tools/autobuild/licence-allow.tsv`, and write a
`decision` entry in the log. The row binds to the SHA-256 of the file
content: a changed file needs a new review of its row.

## 15. Machine rules

The computer has 16 GB of memory, 8 cores and an exFAT drive. It stopped two
times in earlier work because of memory.

- Source `tools/autobuild/env.sh` in each shell. It sets the tool folders,
  `TMPDIR`, `CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=3`,
  `CARGO_INCREMENTAL=0` and the oracle heap limit `GHCRTS=-M3g`. Never unset
  or raise these values.
- The `cargo` wrapper allows 3 heavy cargo commands at the same time, for
  all sessions together. A command can wait for a slot; that is normal.
- Run one heavy command (build, test, measurement) at a time in each
  session. Do not start load generators.
- `measure.py` and `gate.py` wait for one of 2 machine-wide measurement
  slots, and refuse to start when the drive has less than 15 GB free. Each
  measurement run uses its own hashed copy of the programs.
- Run each fuzz artifact, each large or slow input and each generated stress
  input through `limit-run MAX_MB SECONDS COMMAND` at its first run (the
  version in `tools/autobuild/bin` works in the sandbox). Start each large
  input small.
- An oracle result with exit code 251 and `Heap exhausted` means "the
  oracle did not end". It is not a Pandoc result.
- A watchdog outside the sandbox stops each Randoc process above 4 GB.
- Each file on the drive uses at least 1 MB. Keep file counts low. macOS
  makes a `._NAME` companion file for each file; never commit one, and skip
  `._` names in code that lists folders.
- A full gate run takes 20 to 30 minutes, and more when the machine has
  load. Do not stop a gate run because it is slow.
- Network access is on for `cargo` and documentation. Do not
  download Pandoc source code.

## 16. Milestones and ExecPlans

`plan/ROADMAP.md` lists the milestones in order. Each milestone has an exit
gate in `plan/milestones.toml`; `ops milestone check M<n>` verifies it. The
active milestone is the milestone with the lowest number whose gate does
not pass. Take items of the active milestone first. A worker may take an
item of a later milestone only when that milestone's entry condition holds
and the active milestone has no independent item left. When each remaining
requirement of the active milestone is BLOCKED ON OWNER, items of the next
milestone may start; its entry condition counts those requirements as met.
A milestone is done only when its gate passes on one candidate run
(`plan/SPEC.md` 4.1) and a held-out result exists for that commit, or for a
later commit whose diff from it changes only `plan/`. A gate threshold may
rise; it never falls.

An ExecPlan is a self-contained design and progress document for one
milestone. `plan/PLANS.md` gives the rules and the form. The ExecPlan of the first milestone is `plan/exec/M1.md`. At the start
of each later milestone, the lead writes `plan/exec/M<n>.md` in the same form
before the first item.

Keep the four living sections of the active ExecPlan current at each stop
point: `Progress`, `Surprises & Discoveries`, `Decision Log` and
`Outcomes & Retrospective`.

## 17. Autonomy

Work without asking for confirmation. Nobody answers questions during the
run. Make reasonable, reversible assumptions, record each one as a
`decision` entry, and continue. If only the owner can remove a blocker,
write it with `ops blocked` and continue with other work. When all work of
the current milestone is blocked, continue with the next milestone
(section 16). The run ends only when the final goal of `plan/SPEC.md`
holds. The build team never lowers that goal.
