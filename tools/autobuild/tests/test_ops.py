"""The ops command against a temporary repository with the real guard and a fake codex."""

from __future__ import annotations

import json
import os
import subprocess
import time
import unittest
from pathlib import Path
from unittest import mock

from fixture import AUTOBUILD, ITEMS_HEADER, ROWS_HEADER, TempRepo, item_line, row

from opslib import core, fleet

FAKE_GATE = """import json, os, sys
with open(os.environ["FAKE_GATE_LOG"], "a") as log:
    log.write(json.dumps(sys.argv[1:]) + "\\n")
sys.exit(int(os.environ.get("FAKE_GATE_EXIT", "0")))
"""
FAKE_MEASURE = """import json, os, sys
print("measure args: " + json.dumps(sys.argv[1:]))
sys.exit(3)
"""


class OpsTest(unittest.TestCase):
    def setUp(self) -> None:
        self.repo = TempRepo()
        patcher = mock.patch.object(fleet, "MIN_FREE_BYTES", 0)
        patcher.start()
        self.addCleanup(patcher.stop)

    def tearDown(self) -> None:
        self.repo.cleanup()

    # -- helpers -------------------------------------------------------------

    def ok(self, *args: str, **kw) -> str:
        code, out, err = self.repo.ops(*args, **kw)
        self.assertEqual(code, 0, f"ops {' '.join(args)} failed:\n{out}\n{err}")
        return out

    def refused(self, words: str, *args: str, **kw) -> str:
        code, out, err = self.repo.ops(*args, **kw)
        self.assertEqual(code, 1, f"ops {' '.join(args)} did not refuse:\n{out}")
        self.assertIn(words, err)
        self.assertIn("next:", err)
        return err

    def add(self, item_id: str, metric: str = "harness.units", target: str = "+2") -> None:
        self.ok("item", "add", item_id, "--milestone", "M1", "--metric", metric, "--target", target,
                "--title", f"cases of {item_id}")

    def rows(self, *lines: str, stream: str = "build", cwd: Path | None = None) -> None:
        rel = f"plan/scoreboard/{stream}.tsv"
        path = (cwd or self.repo.root) / rel
        self.repo.write(rel, (path.read_text() if path.exists() else ROWS_HEADER) + "".join(lines), cwd=cwd)

    def started(self, item_id: str = "M1-01", baseline: tuple[int, int] = (5, 9)) -> str:
        """An active item with a committed baseline row; returns the measured commit."""
        measured = self.repo.head()
        self.rows(row("2026-11-17T09:00:00Z", measured, "-", "harness.units", *baseline))
        self.add(item_id)
        self.ok("item", "start", item_id)
        return measured

    def code_commit(self, item_id: str, name: str, text: str, cwd: Path | None = None) -> str:
        self.repo.write(f"crates/x/src/{name}.rs", text, cwd=cwd)
        return self.repo.must_commit(f"feat(x): {name} [{item_id}]", cwd=cwd)

    def subject(self, rev: str = "HEAD") -> str:
        return self.repo.out("log", "-1", "--format=%s", rev)

    def item_status(self, item_id: str) -> str:
        line = next(l for l in self.repo.read("plan/ITEMS.tsv").splitlines() if l.startswith(item_id + "\t"))
        return line.split("\t")[2]

    def commit_gate(self) -> None:
        self.repo.write("tools/autobuild/gate.py", FAKE_GATE)
        self.repo.must_commit("chore(tools): fake gate [ops]", ["tools"])

    def wait_finished(self, item_id: str) -> str:
        for _ in range(60):
            out = self.ok("worker", "list")
            if f"{item_id}: finished" in out:
                return out
            time.sleep(0.5)
        self.fail(f"worker {item_id} did not finish:\n{out}")

    # -- items -----------------------------------------------------------------

    def test_item_add_list_and_refusals(self) -> None:
        self.add("M1-01")
        self.assertIn("M1-01\tM1\tqueued\tharness.units\t-\t+2\tcases of M1-01", self.repo.read("plan/ITEMS.tsv"))
        self.assertIn("plan/ITEMS.tsv", self.repo.out("diff", "--cached", "--name-only"))
        self.refused("is never used again", "item", "add", "M1-01", "--milestone", "M1", "--metric", "k", "--target", "+1", "--title", "t")
        self.refused("is not an item id", "item", "add", "X1", "--milestone", "M1", "--metric", "k", "--target", "+1", "--title", "t")
        self.refused("does not belong to milestone M2", "item", "add", "M1-02", "--milestone", "M2", "--metric", "k", "--target", "+1", "--title", "t")
        self.refused("is not a target", "item", "add", "M1-02", "--milestone", "M1", "--metric", "k", "--target", "12", "--title", "t")
        self.refused("banned phrase", "item", "add", "M1-02", "--milestone", "M1", "--metric", "k", "--target", "+1", "--title", "put it on HOLD")
        self.assertIn("M1-01  queued   harness.units  baseline -  target +2  cases of M1-01", self.ok("item", "list"))

    def test_item_start_takes_the_baseline_from_the_newest_build_row(self) -> None:
        measured = self.started("M1-01", (5, 9))
        self.assertIn("M1-01\tM1\tactive\tharness.units\t5/9\t", self.repo.read("plan/ITEMS.tsv"))
        self.assertEqual(self.subject(), "chore(plan): start item M1-01 [ops]")
        self.assertIn(f"measured at {measured}", self.repo.out("log", "-1", "--format=%b"))
        self.assertEqual(self.repo.out("status", "--porcelain", "--untracked-files=no"), "")
        self.add("M1-02")
        self.refused("the lead already has an active item on this branch (M1-01)", "item", "start", "M1-02")

    def test_item_start_without_a_row_uses_a_zero_baseline(self) -> None:
        self.add("M1-01", metric="lua.api")
        out = self.ok("item", "start", "M1-01")
        self.assertIn("Baseline: 0/0 of lua.api", out)
        self.assertIn("M1-01\tM1\tactive\tlua.api\t0/0\t", self.repo.read("plan/ITEMS.tsv"))
        self.rows(row("2026-11-17T10:00:00Z", self.repo.head(), "M1-01", "lua.api", 0, 0))
        self.refused("shows no gain", "item", "done", "M1-01", "--weak-spots", "none")
        self.rows(row("2026-11-17T11:00:00Z", self.repo.head(), "M1-01", "lua.api", 0, 80))
        self.repo.must_commit("chore(plan): rows [M1-01]", ["plan/scoreboard"])
        self.assertIn("next: `tools/autobuild/ops item done M1-01", self.ok("status"))
        self.ok("item", "done", "M1-01", "--weak-spots", "none known")
        self.assertEqual(self.item_status("M1-01"), "done")

    def test_status_shows_the_counters_and_names_the_cut_first(self) -> None:
        self.started()
        self.code_commit("M1-01", "a", "fn a() {}\n")
        for n in range(3):
            self.rows(row(f"2026-11-17T1{n}:00:00Z", self.repo.head(), "M1-01", "harness.units", 5, 9))
        out = self.ok("status")
        self.assertIn("measurement: 2 commits since the newest measured commit (limit 10)", out)
        self.assertIn("M1-01 (lead) harness.units baseline 5/9 target +2: experiments 3/3, gain +0", out)
        self.assertIn("stuck: two rows with no gain", out)
        self.assertIn("next: `tools/autobuild/ops item cut M1-01 --reason TEXT`", out)

    def test_cut_reverts_the_code_in_one_commit_with_the_rows_at_the_limit(self) -> None:
        self.started()
        for n in range(8):
            self.code_commit("M1-01", f"f{n}", f"fn f{n}() {{}}\n")
        self.repo.write("plan/notes.md", "kept\n")
        self.repo.must_commit("docs(plan): notes [M1-01]")
        for n in range(3):
            self.rows(row(f"2026-11-17T1{n}:00:00Z", self.repo.head(), "M1-01", "harness.units", 5, 9))
        plain = self.repo.git("commit", "-q", "--allow-empty", "-m", "docs: x [M1-01]")
        self.assertIn("10 commits since the last measured commit", plain.stderr)
        before = self.repo.head()
        out = self.ok("item", "cut", "M1-01", "--reason", "the parser change did not move the units")
        self.assertIn("reverted 8 commits in one commit", out)
        self.assertEqual(self.repo.out("rev-parse", "HEAD~1"), before)
        self.assertEqual(self.subject(), "chore(plan): cut item M1-01 and revert its code [M1-01]")
        self.assertFalse((self.repo.root / "crates/x/src/f0.rs").exists())
        self.assertTrue((self.repo.root / "plan/notes.md").exists())
        self.assertEqual(self.item_status("M1-01"), "cut")
        committed = self.repo.out("show", "HEAD:plan/scoreboard/build.tsv")
        self.assertEqual(committed.count("\tM1-01\t"), 3)
        self.assertIn("cut M1-01", self.repo.out("show", "HEAD:plan/REVIEW_LOG.md"))
        self.assertEqual(self.repo.out("status", "--porcelain", "--untracked-files=no"), "")

    def test_a_cut_whose_revert_conflicts_changes_nothing(self) -> None:
        self.started()
        self.repo.write("tools/x.py", "b = 1\n")
        self.repo.must_commit("feat(tools): b [M1-01]")
        self.repo.write("tools/x.py", "c = 1\n")
        self.repo.must_commit("chore(tools): c [ops]")
        self.ok("log", "decision", "a note that must stay")
        before = self.repo.head()
        self.refused("the revert of item M1-01 failed, nothing changed", "item", "cut", "M1-01", "--reason", "no gain")
        self.assertEqual(self.repo.head(), before)
        self.assertEqual(self.item_status("M1-01"), "active")
        self.assertEqual(self.repo.read("tools/x.py"), "c = 1\n")
        self.assertIn("a note that must stay", self.repo.read("plan/REVIEW_LOG.md"))
        self.assertEqual(self.repo.out("diff", "--cached", "--name-only"), "plan/REVIEW_LOG.md")
        self.assertEqual(self.repo.out("diff", "--name-only"), "")

    def test_cut_with_no_code_is_an_ops_commit(self) -> None:
        self.add("M1-01")
        self.ok("item", "cut", "M1-01", "--reason", "another item covers these units")
        self.assertEqual(self.subject(), "chore(plan): cut item M1-01 [ops]")
        self.assertEqual(self.item_status("M1-01"), "cut")
        self.refused("is cut", "item", "start", "M1-01")

    def test_done_needs_a_gain_and_logs_the_weak_spots(self) -> None:
        self.started()
        self.code_commit("M1-01", "a", "fn a() {}\n")
        self.rows(row("2026-11-17T10:00:00Z", self.repo.head(), "M1-01", "harness.units", 5, 9))
        self.refused("shows no gain on harness.units", "item", "done", "M1-01", "--weak-spots", "none")
        self.rows(row("2026-11-17T11:00:00Z", self.repo.head(), "M1-01", "harness.units", 8, 9))
        self.ok("item", "done", "M1-01", "--weak-spots", "tables with spans are not checked")
        self.assertEqual(self.subject(), "chore(plan): finish item M1-01 [ops]")
        log = self.repo.out("show", "HEAD:plan/REVIEW_LOG.md")
        self.assertIn("weak-spot M1-01", log)
        self.assertIn("Gain on harness.units: +3", log)
        self.assertIn("tables with spans are not checked", log)
        self.assertIn("\tM1-01\tharness.units\t8\t9", self.repo.out("show", "HEAD:plan/scoreboard/build.tsv"))

    def test_entry_point_runs_as_a_program(self) -> None:
        self.add("M1-01")
        result = subprocess.run([str(AUTOBUILD / "ops"), "item", "list"], cwd=self.repo.root,
                                capture_output=True, text=True, env={**os.environ, **self.repo.env()})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("M1-01  queued", result.stdout)

    # -- adviser and reviews -----------------------------------------------------

    def test_ask_once_per_stuck_event_with_the_adviser_role(self) -> None:
        self.started()
        self.refused("needs the stuck state", "ask", "--reason", "two-rows-no-gain", "why?")
        for n in range(2):
            self.rows(row(f"2026-11-17T1{n}:00:00Z", self.repo.head(), "M1-01", "harness.units", 4, 9))
        self.assertIn("next: commit the new scoreboard rows", self.ok("status"))
        self.repo.must_commit("chore(plan): rows [M1-01]", ["plan/scoreboard"])
        self.assertIn("next: stuck (two rows, no gain)", self.ok("status"))
        final = "Look at the escape table.\nSUMMARY: fix the escape table first.\n"
        out = self.ok("ask", "--reason", "two-rows-no-gain", "Why do the units fail?", FAKE_CODEX_FINAL=final)
        self.assertIn("Look at the escape table.", out)
        call = self.repo.codex_calls()[-1]
        argv = call["argv"]
        self.assertEqual(argv[:2], ["exec", "--dangerously-bypass-approvals-and-sandbox"])
        self.assertEqual(argv[argv.index("-m") + 1], "gpt-6.2-astra")
        self.assertIn("model_reasoning_effort=xhigh", argv)
        self.assertIn("service_tier=default", argv)
        self.assertIn("--ephemeral", argv)
        self.assertEqual(argv[-1], "-")
        self.assertIn("Why do the units fail?", call["prompt"])
        self.assertIn("read only", call["prompt"])
        advice = self.repo.read("plan/ADVICE.md")
        self.assertIn("event=M1-01/2 reason=two-rows-no-gain session=thread-123", advice)
        self.assertIn("Answer (summary): fix the escape table first.", advice)
        self.refused("has no decision yet", "ask", "--reason", "two-fixes-failed", "again?")
        self.ok("ask", "--decision", "Fix the escape table, then measure.")
        self.refused("already has its one question", "ask", "--reason", "two-fixes-failed", "again?")
        self.rows(row("2026-11-17T13:00:00Z", self.repo.head(), "M1-01", "harness.units", 4, 9))
        self.ok("ask", "--reason", "two-fixes-failed", "New event?", FAKE_CODEX_FINAL=final)

    def test_review_rounds_and_blocks(self) -> None:
        self.started()
        self.code_commit("M1-01", "a", "fn a() {}\n")
        block = "- reads past the end\nVERDICT: block\nEVIDENCE: cargo test -p x reads_past_end\n"
        self.ok("review", "M1-01", FAKE_CODEX_FINAL=block)
        argv = self.repo.codex_calls()[-1]["argv"]
        self.assertEqual(argv[argv.index("-m") + 1], "gpt-6.2-sol")
        self.assertIn("model_reasoning_effort=high", argv)
        self.assertIn("--ephemeral", argv)
        self.assertIn("feat(x): a [M1-01]", self.repo.codex_calls()[-1]["prompt"])
        out = self.ok("review", "M1-01", FAKE_CODEX_FINAL="VERDICT: block\n")
        self.assertIn("not a valid block", out)
        reviews = [l.split("\t") for l in self.repo.read("plan/REVIEWS.tsv").splitlines()[1:]]
        self.assertEqual([(r[2], r[5]) for r in reviews], [("1", "block"), ("2", "approve")])
        self.assertEqual(reviews[0][6], "cargo test -p x reads_past_end")
        self.assertEqual(reviews[0][4], "thread-123")
        self.refused("a third round is not allowed", "review", "M1-01")
        self.assertEqual(len(self.repo.codex_calls()), 2)

    def test_a_failed_model_run_records_nothing(self) -> None:
        self.started()
        self.code_commit("M1-01", "a", "fn a() {}\n")
        code, _, err = self.repo.ops("review", "M1-01", FAKE_CODEX_EXIT="1", FAKE_CODEX_FINAL="")
        self.assertEqual(code, 1)
        self.assertIn("no round was used", err)
        self.assertFalse((self.repo.root / "plan/REVIEWS.tsv").exists())

    # -- workers ---------------------------------------------------------------

    def test_worker_start_run_merge(self) -> None:
        self.commit_gate()
        measured = self.repo.head()
        self.rows(row("2026-11-17T09:00:00Z", measured, "-", "harness.units", 5, 9))
        self.repo.must_commit("chore(plan): rows [ops]")
        self.add("M1-01")
        self.ok("worker", "start", "M1-01")
        wt = self.repo.gpt / "wt" / "M1-01"
        self.assertEqual(git_branch(wt), "gpt/w/M1-01")
        self.assertEqual(self.item_status("M1-01"), "active")
        self.assertIn("M1-01\tM1\tactive", self.repo.out("show", "gpt/w/M1-01:plan/ITEMS.tsv"))
        t0 = time.monotonic()
        self.ok("worker", "run", "M1-01", FAKE_CODEX_SLEEP="3", FAKE_CODEX_FINAL="RESULT: 5 -> 7\n")
        self.assertLess(time.monotonic() - t0, 2.5, "worker run must return at once")
        self.assertIn("M1-01: running (pid", self.ok("worker", "list"))
        self.wait_finished("M1-01")
        argv = self.repo.codex_calls()[-1]["argv"]
        self.assertEqual(argv[argv.index("-C") + 1], str(wt))
        self.assertEqual(argv[argv.index("-m") + 1], "gpt-6.2-sol")
        self.assertIn("model_reasoning_effort=high", argv)
        self.assertIn("service_tier=default", argv)
        self.assertNotIn("--ephemeral", argv)
        self.assertIn("Worktree: " + str(wt), self.repo.codex_calls()[-1]["prompt"])
        self.code_commit("M1-01", "w", "fn w() {}\n", cwd=wt)
        self.rows(row("2026-11-17T10:00:00Z", git_head(wt), "M1-01", "harness.units", 7, 9), stream="M1-01", cwd=wt)
        self.repo.append("plan/REVIEW_LOG.md", "\n## worker entry\n", cwd=wt) if (wt / "plan/REVIEW_LOG.md").exists() \
            else self.repo.write("plan/REVIEW_LOG.md", "# log\n\n## worker entry\n", cwd=wt)
        self.repo.must_commit("chore(plan): rows and log [M1-01]", cwd=wt)
        self.ok("log", "decision", "the lead keeps working")
        self.repo.must_commit("docs(plan): log [ops]", ["plan"])
        build_before = self.repo.head()
        out = self.ok("worker", "merge", "M1-01", FAKE_GATE_LOG=str(self.repo.tmp / "gate.log"))
        self.assertIn("merged gpt/w/M1-01", out)
        parents = self.repo.out("log", "-1", "--format=%P").split()
        self.assertEqual(parents, [build_before, self.repo.out("rev-parse", "gpt/w/M1-01")])
        self.assertEqual(json.loads((self.repo.tmp / "gate.log").read_text()), ["--base", build_before])
        log = self.repo.read("plan/REVIEW_LOG.md")
        self.assertIn("worker entry", log)
        self.assertIn("the lead keeps working", log)
        self.assertFalse(wt.exists())
        self.assertEqual(git_branch(self.repo.root), "gpt/build")
        self.ok("item", "done", "M1-01", "--weak-spots", "none known")

    def test_worker_merge_keeps_build_when_the_gate_fails(self) -> None:
        self.commit_gate()
        self.started()
        self.ok("item", "cut", "M1-01", "--reason", "moved to a worker")
        self.add("M1-02")
        self.ok("worker", "start", "M1-02")
        wt = self.repo.gpt / "wt" / "M1-02"
        self.code_commit("M1-02", "w", "fn w() {}\n", cwd=wt)
        self.rows(row("2026-11-17T10:00:00Z", git_head(wt), "M1-02", "harness.units", 9, 9), stream="M1-02", cwd=wt)
        self.repo.must_commit("chore(plan): rows [M1-02]", cwd=wt)
        before = self.repo.head()
        self.refused("the gate failed", "worker", "merge", "M1-02",
                     FAKE_GATE_LOG=str(self.repo.tmp / "gate.log"), FAKE_GATE_EXIT="1")
        self.assertEqual(self.repo.head(), before)
        self.assertEqual(git_branch(self.repo.root), "gpt/build")
        self.assertTrue(wt.exists())

    def test_worker_merge_needs_a_gain(self) -> None:
        self.commit_gate()
        self.add("M1-01")
        self.ok("worker", "start", "M1-01")
        self.refused("show no gain", "worker", "merge", "M1-01", FAKE_GATE_LOG=str(self.repo.tmp / "gate.log"))
        self.assertFalse((self.repo.tmp / "gate.log").exists())

    def test_a_worker_cut_abandons_the_branch(self) -> None:
        self.commit_gate()
        self.add("M1-01")
        self.ok("worker", "start", "M1-01")
        wt = self.repo.gpt / "wt" / "M1-01"
        self.refused("only the lead changes plan/ITEMS.tsv", "item", "done", "M1-01", "--weak-spots", "x", cwd=wt)
        self.ok("item", "cut", "M1-01", "--reason", "three experiments with no gain", cwd=wt)
        self.assertEqual(self.item_status("M1-01"), "active")
        self.assertIn("abandoned", self.ok("worker", "list"))
        self.assertIn("ops worker drop M1-01", self.ok("status"))
        self.refused("the lead never merges it", "worker", "merge", "M1-01")
        self.ok("worker", "drop", "M1-01")
        self.assertFalse(wt.exists())
        self.assertTrue(self.repo.out("rev-parse", "--verify", "gpt/w/M1-01"))
        self.ok("item", "cut", "M1-01", "--reason", "the worker found no gain")
        self.assertEqual(self.item_status("M1-01"), "cut")

    def test_worker_limits(self) -> None:
        for n in range(1, 5):
            self.add(f"M1-0{n}")
            self.ok("worker", "start", f"M1-0{n}")
        self.add("M1-05")
        self.refused("4 workers exist (limit 4)", "worker", "start", "M1-05")
        self.ok("worker", "drop", "M1-04")
        with mock.patch.object(fleet, "MIN_FREE_BYTES", 10**18):
            self.refused("a new worker needs", "worker", "start", "M1-05")
        self.assertEqual(self.item_status("M1-05"), "queued")

    def test_drop_refuses_a_running_worker_unless_told_to_stop_it(self) -> None:
        self.add("M1-01")
        self.ok("worker", "start", "M1-01")
        self.ok("worker", "run", "M1-01", FAKE_CODEX_SLEEP="30")
        self.refused("runs (pid", "worker", "drop", "M1-01")
        self.ok("worker", "drop", "M1-01", "--stop")
        self.assertFalse((self.repo.gpt / "wt" / "M1-01").exists())

    def test_deletes_only_under_wt_and_runs(self) -> None:
        ctx = core.Context(self.repo.root, self.repo.gpt, self.repo.gpt / "runs", core.Git(self.repo.root), None)
        outside = self.repo.tmp / "outside"
        outside.mkdir()
        (self.repo.gpt / "wt" / "link").symlink_to(outside)
        for path in (outside, self.repo.gpt / "wt", self.repo.gpt / "wt" / "link", self.repo.gpt / "wt" / ".." / "x"):
            with self.assertRaises(core.Refusal):
                core.remove_tree(ctx, path)
        self.assertTrue(outside.exists())
        (self.repo.gpt / "runs" / "old").mkdir()
        self.assertTrue(core.remove_tree(ctx, self.repo.gpt / "runs" / "old"))
        self.assertFalse((self.repo.gpt / "runs" / "old").exists())

    # -- held-out, next, milestones, notes, pass-through ---------------------------

    def test_holdout_request_and_results(self) -> None:
        binary = self.repo.tmp / "randoc"
        binary.write_text("binary")
        self.ok("holdout", "request", "--binary", str(binary))
        requests = list((self.repo.gpt / "holdout-queue" / "requests").glob("*.json"))
        self.assertEqual(len(requests), 1)
        request = json.loads(requests[0].read_text())
        self.assertEqual(request["commit"], self.repo.head())
        self.assertTrue(Path(request["binary"]).is_relative_to(self.repo.gpt / "runs"))
        self.assertIn("waits already", self.ok("holdout", "request", "--binary", str(binary)))
        results = self.repo.gpt / "holdout-queue" / "results"
        results.mkdir()
        (results / "1.json").write_text(json.dumps({
            "utc": "2026-11-17T12:00:00Z", "commit": self.repo.head(), "binary_sha256": "ab" * 32,
            "sets": {"corpus": {"json": {"pass": 3, "total": 5}, "html": {"pass": 1, "total": 5}},
                     "generated": {"ext-matrix": {"pass": 7, "total": 9}}},
        }))
        (results / "2.json").write_text(json.dumps({"utc": "2026-11-17T13:00:00Z", "status": "deferred"}))
        out = self.ok("holdout", "results", "--record")
        self.assertIn("held-out: corpus 4/10, generated 7/9", out)
        self.assertIn("deferred", out)
        board = self.repo.read("plan/scoreboard/build.tsv")
        self.assertIn("\tholdout.corpus\t4\t10\tholdout@abababababab\t", board)
        self.assertIn("\tholdout.generated\t7\t9\t", board)
        self.assertIn("already", self.ok("holdout", "results", "--record"))

    def test_next_proposes_clusters_of_at_most_40_units(self) -> None:
        run = self.repo.gpt / "runs" / "measure" / "20261117T100000Z-abc"
        run.mkdir(parents=True)
        failing = [f"writers/markdown/table-{n}" for n in range(90)] + [f"readers/rst/x{n}" for n in range(5)]
        (run / "harness.json").write_text(json.dumps({
            "rows": [{"metric": "harness.units", "pass": 1, "total": 96}], "failing": failing + ["flaky"],
            "unstable": ["flaky"], "duration_s": 1.0,
        }))
        out = self.ok("next")
        sizes = [int(line.split(": ")[1].split()[0]) for line in out.splitlines() if " units (metric " in line]
        self.assertEqual(sum(sizes), 95)
        self.assertTrue(all(size <= 40 for size in sizes))
        self.assertEqual(sizes, sorted(sizes, reverse=True))
        self.assertNotIn("flaky", out)
        listing = Path(out.split("units: ")[1].splitlines()[0])
        self.assertEqual(len(listing.read_text().split()), sizes[0])

    def test_milestone_check(self) -> None:
        self.repo.write("plan/milestones.toml", """
[[milestone]]
id = "M1"
title = "First"
[[milestone.require]]
metric = "harness.units"
min_pass = 5
[[milestone.require]]
metric = "lua.api"
exists = true
[[milestone.require]]
metric = "fmt.read.markdown"
min_fraction = 0.5
""")
        head = self.repo.head()
        self.rows(row("2026-11-17T10:00:00Z", head, "-", "harness.units", 6, 9),
                  row("2026-11-17T10:00:00Z", head, "-", "lua.api", 0, 0),
                  row("2026-11-17T10:00:00Z", head, "-", "fmt.read.markdown", 4, 10))
        code, out, _ = self.repo.ops("milestone", "check", "M1")
        self.assertEqual(code, 1)
        self.assertIn("FAIL  fmt.read.markdown PASS/TOTAL >= 0.5 (4/10)", out)
        self.assertIn("pass  harness.units PASS >= 5 (6/9)", out)
        self.assertIn("FAIL  the newest held-out result", out)
        self.rows(row("2026-11-17T11:00:00Z", head, "-", "fmt.read.markdown", 6, 10))
        results = self.repo.gpt / "holdout-queue" / "results"
        results.mkdir(parents=True)
        (results / "1.json").write_text(json.dumps({"utc": "2026-11-17T12:00:00Z", "commit": head, "sets": {}}))
        code, out, _ = self.repo.ops("milestone", "check", "M1")
        self.assertEqual(code, 0, out)
        self.refused("is not in plan/milestones.toml", "milestone", "check", "M9")

    def test_log_and_blocked(self) -> None:
        self.ok("log", "shortcut", "the table writer skips spans")
        self.refused("banned phrase", "log", "decision", "put it on HOLD")
        self.ok("blocked", "the evaluator does not run", "--owner-must", "start the evaluator", "--meanwhile", "visible work")
        log = self.repo.read("plan/REVIEW_LOG.md")
        self.assertIn("shortcut - ", log)
        self.assertIn("blocked-on-owner - ", log)
        blocked = self.repo.read("plan/BLOCKED_ON_OWNER.md")
        self.assertIn("- The owner must: start the evaluator", blocked)
        self.assertIn("blocked on owner: 1 open entries", self.ok("status"))
        staged = self.repo.out("diff", "--cached", "--name-only")
        self.assertIn("plan/BLOCKED_ON_OWNER.md", staged)

    def test_measure_and_gate_pass_their_arguments(self) -> None:
        code, _, err = self.repo.ops("gate", "--quick")
        self.assertEqual(code, 1)
        self.assertIn("tools/autobuild/gate.py does not exist yet", err)
        self.repo.write("tools/autobuild/measure.py", FAKE_MEASURE)
        result = subprocess.run([str(AUTOBUILD / "ops"), "measure", "--quick", "--item", "M1-01"], cwd=self.repo.root,
                                capture_output=True, text=True, env={**os.environ, **self.repo.env()})
        self.assertEqual(result.returncode, 3)
        self.assertIn('measure args: ["--quick", "--item", "M1-01"]', result.stdout)


def git_branch(cwd: Path) -> str:
    return subprocess.run(["git", "symbolic-ref", "--short", "HEAD"], cwd=cwd, capture_output=True, text=True).stdout.strip()


def git_head(cwd: Path) -> str:
    return subprocess.run(["git", "rev-parse", "HEAD"], cwd=cwd, capture_output=True, text=True).stdout.strip()


if __name__ == "__main__":
    unittest.main()
