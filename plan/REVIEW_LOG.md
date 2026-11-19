# Review log

The handover to the later review. Append only. `tools/autobuild/ops log KIND "TEXT"`
writes each entry.

Entry heading: `## UTC KIND ITEM COMMIT`. Kinds: `decision`, `deviation`,
`shortcut`, `weak-spot`, `cut`, `blocked-on-owner`, `milestone`.

## 2026-11-17T22:24:12Z decision - e1b8f89f0545

Preparation: a separate clone. The build works in the clone `/Volumes/Extreme Pro/Randoc/gpt/randoc` on the branch `gpt/build`, from the branch base `d40a641`. It does not work in a worktree of the owner's repository `/Volumes/Extreme Pro/Randoc/randoc`. Why: the build sandbox then has no write access to the Git folder that holds `main`. Only `gpt/build` goes to the remote `https://github.com/dvgyl/randoc.git`.

## 2026-11-17T22:24:12Z decision - e1b8f89f0545

Preparation: the sandbox profile. Each build session runs under the Codex profile `randoc-gpt` (`~/.codex/randoc-gpt.config.toml`). The profile denies the held-out store and the Pandoc source folders, and gives no write access to the owner's checkout. The build has no GitHub credential: the profile denies `gh`, the keychain credential helper, `security` and `~/.config/gh`. A nested sandbox is not possible on macOS (`sandbox_apply: Operation not permitted`, tested). For this reason the broker, a support program of the owner outside the sandbox, starts each review and adviser session as a top-level session under the read-only profile `randoc-review`.

## 2026-11-17T22:24:12Z decision - e1b8f89f0545

Preparation: the clean room. Pandoc is GPL-2.0-or-later; Randoc is MIT OR Apache-2.0. The team never reads Pandoc source code. A tool reads the files of `env/pandoc-src/test` and `env/pandoc-src/pandoc-lua-engine/test` at run time only, never a file of `env/pandoc-src/src`. The repository holds only numbers and the test identifiers that the tools make (`plan/SPEC.md` section 8).

## 2026-11-17T22:24:12Z decision - e1b8f89f0545

Preparation: the hashed licence scan. `tools/autobuild/licence_scan.py` compares 8-word runs of the added text with a one-way hash index of the copyleft reference text. Why: the sandbox denies the Pandoc source folders, so a scan cannot read them. After review round 1: the pre-push guard scans the text that each outgoing commit adds (a protected copy of the scanner, installed with the guard); the broker scans each commit again before it pushes; `scan --all` scans the whole tree at `HEAD` and marks a hit that `origin/main` already has as `inherited`; an allow row binds to the SHA-256 of the file content.

## 2026-11-17T22:24:12Z decision - e1b8f89f0545

Preparation: the held-out evaluator with a reserved FINAL part. The held-out store is `/Volumes/Extreme Pro/Randoc/holdout/`; only the evaluator reads it, outside the build sandbox, and it reports totals only. It writes its results to `/Volumes/Extreme Pro/Randoc/holdout-results/`, which the build can read and cannot write; a result needs both sets, valid counts and the full SHA-256 of the program. A request names a commit only; the broker builds that commit in its own build folder and records the commit and the program's SHA-256. The candidate program runs in a sandbox that denies the store except its own input, with no network. One evaluation each 6 hours. The evaluator chose a FINAL part once (about 25% of each held-out set). It never reports that part to the build; its totals go only to `holdout/runs/`, for the later review.

## 2026-11-17T22:24:12Z decision - e1b8f89f0545

Preparation: measurement plugins. `measure.py` loads suites from `tools/autobuild/suites/` and metric entries from `tools/autobuild/metrics.d/` (commit `52fd2c3`). Why: a worker adds a measurement as new files and never edits a shared registry. The registry test checks the consistency of the registry, not a fixed count of active keys.

## 2026-11-17T22:24:12Z decision - e1b8f89f0545

Preparation, review round 1: two independent reviews (findings S1 to S15 and A1 to A17) and the clarity check of the lead model (C1 to C17) gave these decisions. (1) `ops milestone check` uses one candidate run (`plan/SPEC.md` 4.1), with the new requirement kinds `min_total` and `exact_total`. (2) Held-out integrity: the results folder, the FINAL part, the sandbox of the candidate. (3) The guard allows commits only on `gpt/build` and on `gpt/w/<ID>` of an active item, and each outgoing commit must descend from `d40a641`. (4) Counters: the commit counter resets only with the rows of a full run of `HEAD` or its parent; the gain is the latest row; a fourth experiment is refused. (5) The licence scan of each outgoing commit. (6) A registry test of consistency. (7) `ops next` clusters per metric. (8) `merge` and `drop` refuse a worktree with changes. (9) `ops` reads the model table of `AGENTS.md` section 3; the effort `low` for routine worker items. (10) A review round is recorded before its launch; a block needs a test or a command and its output; one adviser question for each (item, reason); `ops` checks that the lead tree did not change. (11) Two machine-wide measurement slots, a hashed copy of the programs for each run, no start below 15 GB free. (12) Tool corrections (`ops correction add`, `plan/corrections.tsv`) and a gate that compares failing unit ids of each active suite. (13) A more exact `plan/SPEC.md`. (14) An install gate in the final goal. (15) These handover records. (16) A clearer `AGENTS.md` and ExecPlan. (17) The items M1-27, M1-28 and M1-29.

## 2026-11-17T22:24:12Z decision - e1b8f89f0545

Clarity check of the plan, round 1: the lead model made a read-only dry run and answered `CLEAR: no`. Its main points: (a) `ops status` sent it to M1-01, while the first open ExecPlan step was the held-out request; (b) the acceptance of M1-01 was unclear, and no step owned the check of two harness runs with `--jobs 8`; (c) the method of the CPU limit of a harness unit was open; (d) a stricter capability rule can lower PASS at the same TOTAL, and no procedure covered it; (e) `ops item done` came before the gate, with no recovery; (f) workers had files that the manual reserved for the lead, and the worker example ran `ops item start`; (g) no rule said when work on the next milestone may start; (h) the clean-room boundary of test extraction was not exact; (i) Quarto, R and the data pack had no defined path; (j) M9-04 used a held-out metric for its experiments, and a deferred request had no rule. Its one proposed change: make `ops status` the one tested source for the next action. What changed: the startup order is "the `ops status` action, then the first open step", and the held-out request is marked as made; M1-01 owns the `--jobs 8` check and the gain rule is written out; the CPU limit uses `ulimit -t`; tool corrections cover stricter rules; the gate runs before `ops item done`; `AGENTS.md` section 9 lists the files of a worker, and `ops worker start` activates a worker item; the BLOCKED ON OWNER rule for the next milestone; the clean-room boundary in `AGENTS.md` section 4 and `plan/SPEC.md` section 8; a failing unit in place of 0/0 for a missing program, and BLOCKED ON OWNER entries; M9-04 uses `casegen.visible`, and a deferred request means "continue, then request again".

## 2026-11-17T22:24:12Z decision - e1b8f89f0545

Review round 1b: the preparation put five positions to the two reviewers. They disagreed on positions 1, 2, 3 and 5, and round 1b changed them. Position 1 (pushes): the build has no GitHub credential; `tools/autobuild/ops push` asks the broker, which pushes only `gpt/build` to `gpt/build`, fast-forward, after the licence scan of each commit. Position 2 (attempts): each `measure.py --item ID` run writes an attempt record first (`/Volumes/Extreme Pro/Randoc/gpt/runs/attempts.tsv`); the experiment count is the larger of the scoreboard runs and the attempt records. Position 3 (reviewers and adviser): the broker runs `ops review` and `ops ask` as top-level sessions under the read-only profile `randoc-review`. Position 5 (held-out program): `ops holdout request` names a commit only, and the broker builds that commit itself. Both reviewers agreed on position 4: a UTC time stamp on a done step or a decision records the past and stays; the plan has no date or duration of future work. Position 4 is closed.

## 2026-11-17T22:24:12Z weak-spot - e1b8f89f0545

Accepted residual risk: push credentials. GitHub cannot limit a credential to one branch. The broker pushes with the owner's credential; only the broker's code, the guard and the GitHub ruleset (no forced push, no deletion of `main` and `gpt/build`) limit what it pushes. A separate GitHub identity for the broker is an optional owner decision (`plan/BLOCKED_ON_OWNER.md`). A deliberate bypass outside the sandbox stays possible; it is visible in the history and can be reverted.

## 2026-11-17T22:24:12Z weak-spot - e1b8f89f0545

Accepted residual risk: experiment records. The attempt records are in `/Volumes/Extreme Pro/Randoc/gpt/runs/attempts.tsv`, a folder that the build can write, because `measure.py` writes them inside the sandbox. A removed attempt record lowers the count to the count of scoreboard runs, never below it. The later review can compare the records with the evidence folders of `gpt/runs/`.

## 2026-11-17T22:24:12Z weak-spot - e1b8f89f0545

Accepted residual risk: reviewer and adviser sessions. A nested sandbox is not possible on macOS, so the read-only access of these sessions depends on the broker's profile `randoc-review`, which is outside the repository. After each review and adviser run, `ops` checks that the lead tree did not change: a detective control, not a preventive one.

## 2026-11-17T22:24:12Z weak-spot - e1b8f89f0545

Accepted residual risk: held-out scores. The broker builds each requested commit (about 10 GB of disk and 15 minutes per build, an estimate of the preparation), so a held-out request loads the 16 GB machine. The limit of one evaluation each 6 hours bounds this load. Repeated scores of the reported part still permit slow tuning to that part; the FINAL part, which the build never sees, lets the later review detect it.

## 2026-11-17T22:24:12Z weak-spot - e1b8f89f0545

Known weak spot: `ops milestone check M9` checks scoreboard rows only. Items 2, 4, 6, 7, 8 and 9 of the final goal (`plan/SPEC.md` section 7) also need evidence that no row shows: the `not_extracted` list, the excluded units with their reasons, the exit-code triggers, the whole-tree licence scan, and the real PDF engines. The `milestone` entry of M9 must name that evidence.

## 2026-11-18T00:34:21Z decision - 91b492a67bfa

Review round 2: two independent reviews and the second clarity check of the lead model gave these decisions, now in the tools with tests (commits 450f696 and 91b492a). (1) `ops milestone check` takes each `holdout.*` requirement from the broker result that matches the candidate: the candidate commit, or a later commit whose diff from it changes only `plan/`. (2) Only a result with `binary_provenance` `built` counts for a gate; the older results stay for the history. (3) The candidate commit must be `HEAD`, or `HEAD` differs from it only in `plan/`. (4) The gate never runs the suite of `gate.steps`; `gate.py --report FILE` writes the report of one gate run; a runner names the exit codes after which `measure.py` reads its report (the contract is in `plan/exec/M1.md`, step A2). (5) `ops item done` refuses without a passing full gate of the current commit, and `ops status` names `ops gate` first. (6) A cut passes the commit limit and never waits for another exhausted item, in `ops` and in the guard. (7) `ops status` reads the gain of a worker from the committed rows of its branch, and gives no cut advice while the worker runs. (8) For an item whose metric is not active, `ops status` names `--suite NAME` after the suite is built, never `--quick`. (9) Only rows with a completed run manifest (`run.json`) reset the 10-commit counter. (10) The unit check compares the earlier passing ids with the current passing ids. (11) An adviser question counts when it is sent. (12) After each review and adviser session, `ops` compares `HEAD` and `git status` of the lead tree with the state before, and refuses a change. (13) A failed `git status` in a worker worktree counts as not clean. (14) `ops item cut` withdraws each correction of the item; step A4 records its correction before the first row of the new rule. (15) `ops holdout request --commit SHA`. (16) The guard checks the added lines of each staged text file, except `AGENTS.md`, the guard and its tests. (17) `ops item done --min-total N` and `--all-pass`.

## 2026-11-18T00:34:21Z weak-spot - 91b492a67bfa

Binary and large files: the licence scan skips binary files and files above 5 MB, so it cannot check them. `AGENTS.md` section 14 now asks for a row in `THIRD_PARTY_NOTICES.md` or an origin note in the same commit. Why not fixed in the tools: the scanner cannot read such a file. No tool checks the rule; the later review must look at each new binary or large file.

## 2026-11-18T00:34:21Z weak-spot - 91b492a67bfa

Machine and recovery limits: `measure.py` holds no lock on the shared target folder from the build to the copy of the programs; after a failed revert, `ops item cut` runs `git reset --hard HEAD` in the lead tree, not in a clean worktree; the suites of the two measurement slots have no combined memory budget. Why not fixed now: the watchdog stops each Randoc process above 4 GB, and the two measurement slots limit the parallel runs, so the damage stays small.

## 2026-11-18T00:34:22Z weak-spot - 91b492a67bfa

Cut streak: `ops status` counts the items cut in a row by the order of `plan/ITEMS.tsv` and by the exact metric key, not by the time of the cut or by area. A new id for the same cause hides a streak. `AGENTS.md` section 6 now forbids the reuse of a cause under a new id. Why not fixed in the tools: the items table has no cut time; the later review can read the `cut` entries of this log in time order.

## 2026-11-18T00:34:22Z decision - 91b492a67bfa

Open questions of review round 2 go to the steps of their items, where the lead decides each with a `decision` entry before the work: M1-13 (`plan/exec/M1.md`, Phase C): the three pivot trees of each element type of `lua.filters` when P has fewer, the expected return cases of each hook, and the test of the `lua` mode with `-i`; M1-29 (step B5): how the `tags.*` keys are registered when the set of keys with units depends on the measured units; M8-09: the boundary of the install subset (20 conversion units of each active suite, but `gate.steps` holds gate checks, `install.units` itself becomes active, and a suite can have fewer than 20 units). `plan/exec/M8.md` does not exist yet: when the lead writes it, the step of M8-09 must hold this question.

## 2026-11-18T00:34:22Z decision - 91b492a67bfa

M1-27, M1-28 and M1-29 stay open by design: they are items of the first milestone, which the build does.

## 2026-11-18T00:34:22Z weak-spot - 91b492a67bfa

Unit check: an excluded or gone unit has no metric list in the evidence of the current run, so a correction of any metric of its suite covers it. A narrower rule needs the metrics of each passing unit in the evidence, which about doubles the size of the harness evidence.

## 2026-11-18T00:34:22Z weak-spot - 91b492a67bfa

Commit counter: the guard does not check that a run covers each active suite; a completed `--suite` run of a clean tree also resets the counter. This keeps a way out when one suite tool fails at the limit, because a run with a failed suite writes no `run.json`.

## 2026-11-18T00:34:22Z weak-spot - 91b492a67bfa

Worker items and the guard: while a worker item has 3 attempt records and its gain is only in the rows of its branch, the guard of the lead tree sees no gain and refuses each commit of the lead until `ops worker merge` or a cut. `ops status` no longer advises the cut in that state, but the lead cannot commit until the merge.

## 2026-11-18T06:25:09Z decision - e4cba841f066

Review round 3, machine and recovery limits: both reviewers rejected the reason of round 2. Decision: kept as a weak spot, not fixed. Reason: other controls bound the combined memory. The watchdog stops each Randoc process above 4 GB, and it stops the largest one when free memory falls below 15%. The cargo wrapper needs 20% free memory and has 3 slots, and measurements have 2 slots. A race on the shared target folder needs a second heavy command in one session, which `AGENTS.md` section 15 forbids; workers build in their own worktrees. The `git reset --hard` of a cut runs only when the revert fails, on a tree that `ops item cut` requires to be clean. A global memory scheduler costs more than the remaining risk on this machine.

## 2026-11-18T06:25:09Z decision - e4cba841f066

Review round 3, cut streak order: one reviewer rejected and one accepted the reason of round 2. Decision: kept as a weak spot, not fixed. Reason: the counter is advice in `ops status`, not a gate. The review log has the time of each cut, so the later review can rebuild the true order.

## 2026-11-18T06:25:09Z decision - e4cba841f066

Review round 3: two reviews of the commits 450f696, 91b492a and 1aedb6d (both `VERDICT: not ready`) gave these decisions, now in the tools with tests (commit e4cba84). (1) A run resets the 10-commit counter only when its manifest (`run.json`) lists each active suite of the registry at its commit; the guard checks the manifest also for the committed rows below the limit. (2) A correction of a metric covers only the units with that metric in their list; the evidence lists the passing units of each metric (`passing_by_metric`), and a unit with no list is never covered. (3) The column `accept` of `plan/ITEMS.tsv` (`ops item add --all-pass` or `--min-total N`): the guard, `ops` and `measure.py` count a latest row below the rule as no gain, and `ops item done` has no coverage option. (4) The pre-commit guard refuses a staged binary file or a file above 5 MB without a row of its path and SHA-256 in `tools/autobuild/provenance.tsv`. The machine limits and the cut streak order stay weak spots, with the reasons in the two entries before this one. The rows of the preparation runs have no run manifest, so the commit counter counts from the branch base: the first action of the build is `ops measure`.

## 2026-11-18T06:30:59Z decision - 58be7a5d2302

Final check of review round 3: both reviewers found that a commit that cuts an item skipped the file-origin check, so an unrelated binary file could enter with the cut. Now the guard checks the file origin before the cut exception. In a cut commit, a file that the history of HEAD holds at the same path needs no row, because the revert of the item brings it back (HEAD has 7 binary fuzz seeds with no row). Each other binary or large file of a cut commit needs its row.

## 2026-11-18T06:52:46Z decision M1-01 4a62b2d19e7b

Round 3 machine points (both reviewers rejected the first reason): ops item cut now refuses untracked files outside plan/ before a revert that can end in a reset, and measure.py waits for 25% free memory before it takes a measurement slot. The remaining risk (a shared target folder in one session) stays a weak spot: AGENTS.md allows one heavy command per session.

## 2026-11-18T07:12:38Z decision M1-01 9d29d4c48a06

M1-01 uses the existing gate report as the source of the gate.steps row. A step that did not run counts as a failure. A change to a program copy adds a failed unit, even if each scheduled step passed.

## 2026-11-18T07:12:38Z weak-spot M1-01 9d29d4c48a06

The M1-01 harness enforces 60 s per process and 600 s wall time. It still relies on the machine watchdog for memory, so it does not yet enforce the 3,000 MB process group limit in plan/SPEC.md 3.9. The gate source scan detects direct oracle references and direct calls to pandoc. It does not resolve dynamically constructed executable names.

## 2026-11-18T07:28:43Z decision M1-01 c69cb950c63e

M1-01 review round 1 found a raw string form that the source scan missed. The scan now accepts any number of raw string delimiters and reports a source read error as a failed step. A filtered gate runs the earlier steps that it can depend on and writes a quick report. Two test fixtures now use their own metric overlays. The 49 tests of the affected modules passed, and the no-oracle-call diagnostic wrote evidence without a scoreboard row.

## 2026-11-18T07:38:44Z decision M1-01 89edce20db9d

After the second M1-01 review, the source scan handles Rust line comments and nested block comments. The launcher uses system Perl after sh sets the CPU limit. It drops the shell variables and keeps argument zero and process status. The full environment assertion and all 44 harness tests passed. AGENTS.md section 10 limits a change to two review rounds. The regression tests and the full gate will check these fixes.

## 2026-11-18T08:56:50Z weak-spot M1-01 20ea0e7db8eb

Item M1-01 done. Gain on gate.steps: the metric now has rows (best PASS 15) (baseline 0/0). Gate: 20261118T081646Z-20ea0e7db8eb.

Weak spots: The harness uses system Perl on Unix and relies on the machine watchdog for memory. It still needs the 3,000 MB process group limit from plan/SPEC.md section 3.9. The source scan checks direct oracle references and launches with literal executable names. Dynamic executable names need a separate check.

## 2026-11-18T09:41:17Z deviation M1-02 50281ad0008a

I started M1-02 before I read the action from ops status after the M1-01 push. AGENTS.md section 5 puts that action first. I ran ops next before changing M1-02. It listed product corpus clusters. The status counter had compared the full run with the preceding item experiment and missed the first full gate score. I will check the counter against clean, completed full runs before I act on its advice.

## 2026-11-18T09:43:33Z decision M1-02 50281ad0008a

The AGENTS.md section 7 counter now compares only clean, completed full build runs, as validated by the Git guard. An item experiment cannot become the baseline for that counter. A full run gains on a metric when it establishes the metric or raises PASS without adding failures. No scoreboard row or metric rule changed. The full M1-01 run now resets the streak, and ops status directs the lead to continue M1-02.

## 2026-11-18T09:45:55Z decision M1-02 5e2ef6ab3373

The tests confirmed that the preparation had fixed the Pandoc suite and corpus baseline checks. M1-02 reuses the three existing baseline test paths and adds one scored gate step for each. Each fixture checks one selected regression against a passing baseline. The Pandoc suite and corpus fixtures use the same file as the old baseline and the new report. All three steps passed on their own. The plan and specification now describe the verified baseline behavior.

## 2026-11-18T09:53:16Z weak-spot M1-02 0d40f4396a6e

Review round 1 showed that each Python tool replaced its input baseline with a failed report when the output named the same file. An identical retry then exited 0. A filtered success could also remove unselected passing units. The follow-up in M1-02 preserves the shared file if the new result omits a saved passing unit. Both regression tests now retain the saved file, and the failed run still exits 1 on retry.

## 2026-11-18T09:53:41Z decision M1-02 ca434aaa8abd

The baseline check now compares the complete saved set of passing units before it replaces a shared input and output file. This covers both a failure and a filtered check that omits a saved unit. A report for a separate output can still record the current failures. plan/SPEC.md now states this file rule. The gate suite continues to count one baseline check per tool.

## 2026-11-18T10:00:06Z decision M1-02 ca434aaa8abd

The second M1-02 review reproduced a report label that replaced the saved passing list and made a failing retry exit 0. The tool now keeps that list when a labeled output would replace it. It checks the list type when it reads a baseline. Both new tests failed before the change and passed after it. All 59 suite tool tests passed. AGENTS.md section 10 limits the item to two reviews. The build continues with the scored gate experiment.

## 2026-11-18T11:13:11Z weak-spot M1-02 f03459414a0f

Item M1-02 done. Gain on gate.steps: +3 (baseline 15/15). Gate: 20261118T103542Z-f03459414a0f.

Weak spots: The baseline self-checks use local stand-ins. The corpus oracle checksum and duplicate-key rule still need M1-27. The reader test exit code and standard error rules still need M1-28.

## 2026-11-18T12:04:32Z decision M1-03 14939a79cedb

After M1-02 the roadmap order and ops status select M1-03. AGENTS.md section 2 puts the roadmap before the ExecPlan, which lists M1-27 and M1-28 first. The lead will follow M1-03, M1-04 and M1-05 in roadmap order. For M1-03, a case opts in to a snapshot of changes to its work folder and names the paths of ZIP output files. The harness calls a standard-library Python snapshot program for bounded file reads and SHA-256. Stored results contain hashes of file contents, including ZIP entries.

## 2026-11-18T12:42:39Z decision M1-03 132455099958

The first M1-03 review showed a false pass for a written EPUB with its mimetype entry last and compressed. Under plan/SPEC.md 3.5, a written ZIP now records whether its first entry is mimetype and is uncompressed. The original oracle EPUB passes and the repacked copy fails on file changes. All 83 stored file results matched the second oracle run.

## 2026-11-18T12:42:39Z weak-spot M1-03 132455099958

The snapshot program checks the ZIP member count after the Python library reads the central directory. The written file byte limit and snapshot deadline apply before comparison. Review this resource limit with the ZIP suite in M1-19.

## 2026-11-18T12:44:54Z decision M1-03 1fbc710a7055

The files suite makes DOCX, ODT, EPUB and PPTX inputs in the evidence folder from one authored document. Each oracle extraction wrote PNG, JPEG and SVG file content. All four extraction results matched a second oracle run. Randoc failed all four probes in the diagnostic run.

## 2026-11-18T12:47:07Z decision M1-03 324c5d358a0f

A diagnostic checked 115 earlier command cases with output, extraction or log options and defaults. Pandoc wrote files in 76 cases. File capture keeps the prior verdicts of all 76. The older PDF case wrote no file and reported a different temporary path in each error. M1-20 measures PDF under plan/SPEC.md 3.6.

## 2026-11-18T12:47:09Z decision M1-03 41180cd29f0b

Tool correction of caps.options by M1-03: The old probes for epub-cover-image and extract-media checked the exit code and console output. The new probes also compare file changes. TOTAL stays 109 and no option units are removed. Diagnostic PASS moves from 94 to 92. All earlier harness units keep their pass or fail status. The rows of caps.options before 2026-11-18T12:47:09Z no longer count for gains, regressions or milestone checks.

## 2026-11-18T12:52:53Z decision M1-03 ad46c131cf95

The second review showed that an explicit ZIP directory entry could cause a false failure. The comparison now ignores directory entries under N1 in plan/SPEC.md 3.12. The new test failed before the fix and passes after it. A real oracle EPUB with one extra directory entry passes. A copy with its mimetype entry moved and compressed still fails. All 159 stored file results match the oracle. AGENTS.md section 10 limits this item to two reviews.

## 2026-11-18T13:30:13Z weak-spot M1-03 f7b85c53f274

Item M1-03 done. Gain on files.units: the metric now has rows (best PASS 95) (baseline 0/0). Gate: 20261118T125326Z-f7b85c53f274.

Weak spots: 68 of 163 file cases still fail. They include all four archive media extraction cases. The ZIP helper reads the central directory before it checks its entry count. M1-19 covers archive resource limits. The older PDF probe reports a temporary path in its error and writes no file. M1-20 covers PDF.

## 2026-11-18T14:26:07Z decision M1-04 a5d08cdcfaa1

M1-04 uses the authored file-suite tree first, then 80 other form trees, 194 highlighting trees, 60 math trees and 65 seeded trees. Pandoc 3.13 accepts each tree with -f json -t json. Two full generations produced the same SHA-256 and the same bytes. The HTML writer marks all 194 language cases as highlighted. MathML rejects all 20 malformed expressions. The writer measurement is still in progress.

## 2026-11-18T14:31:55Z decision M1-04 1f93e8966f73

M1-04 notebook cell identifiers. Under plan/SPEC.md 3.10 and 3.13, apply N5 only to UUID version 4 values in cells[*].id of fmt.write.ipynb standard output. Use only the cell positions where two oracle runs have different UUID values. The paired runs differ only at those fields for forms/inline-str, forms/table-row-span, highlight/python and seed/0001. The explicit sample-cell identifier stays exact. Each measured result records the positions to which the rule applies.

## 2026-11-18T14:44:56Z decision M1-04 69fc910e7f87

M1-04 records each writer result with two oracle runs in separate folders. The cache holds raw results outside the repository. Its key includes the oracle SHA-256, the command, the input and the code that sets the environment and compares results. The first-tree diagnostic checked all 76 writers with zero oracle exclusions. Randoc passed 18. The 81 notebook form probes had no oracle exclusions. A generated cell identifier changed in 72 of those probes.

## 2026-11-18T14:44:58Z decision M1-04 f69df7c9bd43

Tool correction of caps.writers by M1-04: The old score used the harness capability matrix alone. Each non-PDF writer now needs to pass every counted pivot unit and each applicable harness conversion. PDF uses the score of pdf.units. TOTAL stays 77 and no formats are removed. The prior full score was 7/77. The rows of caps.writers before 2026-11-18T14:44:58Z no longer count for gains, regressions or milestone checks.

## 2026-11-18T14:56:52Z decision M1-04 d8e99c6a41b3

The first M1-04 review found different environments for the two converters. The writer tool now gives both the exact variables of plan/SPEC.md 3.1, including GHCRTS and RANDOC_DATA_DIR. A test checks the full environment for both roles. A saved oracle-limit result now gets a fresh pair on the next run. The milestone recheck keeps the first result and records any new differences. The score parser requires an entry with evidence for each exclusion. Raw observations now stay in bounded batches, with the first oracle run stored on disk until the second run finishes. The notebook diagnostic checked all 81 form trees with zero oracle exclusions after these changes.

## 2026-11-18T15:05:10Z decision M1-04 871a08503081

The second M1-04 review showed that a timeout during an oracle recheck could erase the original result and the parts that had varied. The cache now keeps both. A retry compares each new result with that original. The review also showed that a damaged compressed ZIP member could stop the suite. The tool now records a standard output mismatch. Both targeted tests failed before the fixes and pass after them. AGENTS.md section 10 limits this item to two reviews.

## 2026-11-18T15:13:43Z weak-spot M1-04 f90e6083063e

The full writer diagnostic starts a new Python process for each limited conversion. A small timing check on one HTML table used seven interleaved pairs under the same process limits. Median time was 222 ms with the current launcher (214 to 240 ms) and 159 ms with the in-process prototype (89 to 193 ms). The exit codes, bytes and file observations matched in all seven pairs. The full diagnostic continues to collect end-to-end evidence.

## 2026-11-18T16:11:59Z decision M1-04 f90e6083063e

The owner paused work on 2026-11-18. The full M1-04 diagnostic had completed 33 of 76 writers and 13,200 of 30,400 cases. All completed oracle pairs were repeatable. Under AGENTS.md section 4, the lead sent SIGINT only to the diagnostic process, PID 62087. The sandbox refused it and the process still had the log open. At resumption, check the existing log and report before starting another run. No scored M1-04 experiment has run.

## 2026-11-18T17:36:11Z decision M1-04 f90e6083063e

The M1-04 diagnostic continued in its original process after the owner resumed work. Program checksums matched. The report contains all 30,400 distinct writer cases. All oracle pairs were repeatable. The diagnostic scored fmt.write 5,510/30,400 and caps.writers 2/77. All 61,449 harness verdicts matched the M1-03 full run. The report at $RANDOC_RUNS/m1-04-writers-diagnostic.bR6e7B/writers-report.json gives the 391 cases and cell positions affected by the notebook identifier rule.

## 2026-11-18T18:33:41Z decision M1-04 d5d028524887

M1-04 experiment 1 produced 78 clean rows and reproduced all 30,400 writer verdicts and all 61,449 harness verdicts from the diagnostic. The full gate failed in two plugin tests because the base registry activated a metric owned by the writer plugin. Moving the active entry into the writer overlay fixed both tests. All 14 focused tests passed. The loaded definitions and their order matched the prior registry. The ten-commit limit in AGENTS.md section 7 requires the full branch measurement before the next experiment. That measurement includes the full gate.

## 2026-11-18T20:48:59Z weak-spot M1-04 507cb968a7a7

Item M1-04 done. Gain on fmt.write: the metric now has rows (best PASS 5510) (baseline 0/0). Gate: 20261118T183417Z-8c9b53c6816b.

Weak spots: Only csljson and native meet the strict capability rule. The JSON writer passes all 400 pivot trees and has failing harness conversions. The writer suite starts a separate limiter process for each conversion.

## 2026-11-18T22:27:56Z decision M1-04 3c25dea3d9c8

Both writer experiments reproduced all 30,400 writer verdicts and all 61,449 harness verdicts. The full measurement of the closure commit 3c25dea recorded 95 rows. It passed all 18 gate checks and matched all six suites of the preceding full run. The measured writer baseline is 5,510/30,400 and the strict writer capability baseline is 2/77. The next lead item in plan/ROADMAP.md is M1-05.

## 2026-11-18T22:42:15Z decision M1-05 84f3d6a9504e

Tool correction of caps.readers by M1-05: The old rule used the harness matrix alone. The strict rule also requires every counted fmt.read.F input to pass. TOTAL stays 52. The rows of caps.readers before 2026-11-18T22:42:15Z no longer count for gains, regressions or milestone checks.

## 2026-11-18T22:42:18Z decision M1-05 5720844bbb55

Tool correction of caps.readers. The old harness-only score was 4/52. Under plan/SPEC.md section 5, a format now passes only if every counted fmt.read.F input and each harness unit with from F passes. TOTAL stays 52. The inventory keeps all 52 formats.

## 2026-11-18T23:19:21Z decision M1-05 503ddbe55a81

The first review reproduced an unsafe cache update and an unbounded corpus file read. The reader cache now checks its latest result under a write lock after the oracle call. The corpus loader reads at most one byte past the pinned size. Both regression tests failed before these fixes and all 13 reader tests passed after them. The full diagnostic in $RANDOC_RUNS/measure/20261118T225047Z-503ddbe55a81 uses both fixes.

## 2026-11-18T23:19:21Z decision M1-10 503ddbe55a81

The option-order group now needs a command-line option on each side of a defaults file. Both valid extraction probes now record the extracted image. The second review found that default-image-extension passed without an image. In the old report 70 of 103 pairs of valid settings had equal oracle results. Commit 31434f5 requires an observable difference to pass and gives several keys inputs that exercise their effect. The two image-extension probes produced layout.svg and layout.png. Record the tool correction before the next scored defaults run. AGENTS.md section 10 limits this item to the two review rounds already used.

## 2026-11-18T23:24:42Z decision M1-12 503ddbe55a81

The first Lua review found a process-output attribution error and a binary probe that measured only a file prefix. The revised worker run recorded 243 filter matches among 468 names and zero matches in standalone mode. The second review found a bold probe that passed a no-op. Commit 375d659 makes that no-op fail and keeps the literal probe passing. The worker marked its branch abandoned after three attempts. The recorded 0/468 row meets the first-baseline rule of plan/SPEC.md 4.1, and that row predates 375d659. Preserve the branch and evidence under AGENTS.md section 9.

## 2026-11-19T01:20:16Z decision M1-05 503ddbe55a81

The unscored reader diagnostic passed 27,474 of 39,973 inputs across 52 formats. It found no oracle exclusions and no Randoc process without a result. The writer cache contains one failed oracle write in the paired inventory, rst forms/block-header-zero with exit code 1. The remaining 15,999 pivot outputs all appear in the report, along with 23,736 distinct harness inputs and 238 corpus documents. AGENTS.md section 7 requires a clean full measurement at ten commits. Run it before recording the second review.

## 2026-11-19T01:21:13Z decision M1-12 503ddbe55a81

The Git guard refused the M1-05 fix commit because the abandoned M1-12 worker had used three attempts and the build stream has no Lua score. The worker branch has a 0/468 row under the first-baseline coverage rule. AGENTS.md sections 6, 7 and 9 govern the guard refusal and the abandoned branch. Close the item with ops item cut. Retain the worktree, commits and evidence for review.

## 2026-11-19T01:21:13Z cut M1-12 503ddbe55a81

Item M1-12 cut after 3 experiments with no gain on lua.api. Reason: The worker abandoned the branch after three attempts. The Git guard refuses further build commits until this item closes. The 468-name worker row predates the final verified styling fix and the abandoned branch stays available for review.

## 2026-11-19T04:39:15Z decision M1-05 974671396252

The clean full run at 768abcb wrote 148 rows and passed all 18 gate checks. It retained each of the 61,449 earlier conformance verdicts and added 440 reader cases. All 39,973 reader verdicts match the unscored diagnostic. The guard had reached its ten-commit limit after closing M1-12. The run checked the committed reader suite before the two review fixes. The scoreboard commit reset the limit. Restored both fixes and their review records from the verified patch, then passed all 13 reader tests.

## 2026-11-19T04:39:44Z decision M1-10 4c5caec91199

Tool correction of defaults.units by M1-10: The old worker score of 92/103 could pass a key whose two valid settings gave equal oracle results. A passing key now needs different oracle results for those settings. TOTAL stays 103. No key is removed. The rows of defaults.units before 2026-11-19T04:39:44Z no longer count for gains, regressions or milestone checks. The baseline of M1-10 is now 0/0: its first row with a TOTAL above 0 is its gain.

## 2026-11-19T09:47:28Z decision M1-10 695621169c86

The worker merge passed all 18 gate checks and reproduced all 103 defaults verdicts at 45/103. The merge brought the branch to its ten-commit limit because the guard counts the six worker commits. The guard refused the closure commit. The required clean full measurement on 6956211 wrote 149 rows and preserved every earlier verdict. Restored the exact measured M1-10 row from its verified copy. When planning later merges, count the commits that the worker branch adds and allow for the closure commit.

## 2026-11-19T09:47:28Z weak-spot M1-10 695621169c86

Item M1-10 done. Gain on defaults.units: the metric now has rows (best PASS 45) (baseline 0/0). Gate: 20261119T064209Z-695621169c86.

Weak spots: Forty-three of the 103 keys have equal oracle results for their two valid settings. They need inputs that show the effect of each key. Fifteen of the 60 keys with a visible effect still fail compatibility checks.

## 2026-11-19T10:20:54Z blocked-on-owner M1-05 17e5239a98ff

M1-12 left the M1 gate without a lua.api row. The worker recorded 0/468 on its branch, which meets the coverage rule for a new metric. The lead guard counted all three attempts and had no scored row on gpt/build. The item was cut and its branch gpt/w/M1-12 stays at 375d659. The measured row predates the final probe fix. AGENTS.md sections 6 and 9 prevent a repeat item for the same cause or a merge of the cut branch.

## 2026-11-19T10:57:01Z decision M1-11 9eb271ae13a2

The first review showed that exit/43 and exit/47 used a scratch folder inside the measured work folder. Pandoc left a different media directory on each run. Both oracle runs were stable when the scratch folder was outside the measured work folder. Move the scratch folder and keep both probes in the score. Send the review finding to the worker in the active item title.
