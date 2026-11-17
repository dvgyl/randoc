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
