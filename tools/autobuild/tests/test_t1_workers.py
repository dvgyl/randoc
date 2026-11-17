"""Worker cleanup keeps work that is not committed, and a worker run takes its model and effort from AGENTS.md."""

from __future__ import annotations

import os
import time
import unittest
from unittest import mock

from fixture import TempRepo, row

from opslib import codex, core, fleet
from opslib.core import Refusal


class WorkerTest(unittest.TestCase):
    def setUp(self) -> None:
        self.repo = r = TempRepo()
        self.addCleanup(r.cleanup)
        patch = mock.patch.object(fleet, "MIN_FREE_BYTES", 0)  # the test folder is on the internal disk
        patch.start()
        self.addCleanup(patch.stop)
        r.append("plan/scoreboard/build.tsv", row("2026-11-17T09:00:00Z", r.head(), "-", "harness.units", 5, 9))
        r.must_commit("chore(plan): rows [ops]")
        self.ok("item", "add", "M1-01", "--milestone", "M1", "--metric", "harness.units", "--target", "+2",
                "--title", "cases")
        self.ok("worker", "start", "M1-01")
        self.wt = r.gpt / "wt" / "M1-01"

    def ok(self, *args: str, **env: str) -> str:
        code, out, err = self.repo.ops(*args, **env)
        self.assertEqual(code, 0, f"ops {' '.join(args)} failed: {err}")
        return out

    def refused(self, words: str, *args: str) -> None:
        code, out, err = self.repo.ops(*args)
        self.assertNotEqual(code, 0, f"ops {' '.join(args)} did not refuse")
        self.assertIn(words, err)

    def wait_finished(self) -> None:
        deadline = time.time() + 20
        while "finished" not in self.ok("worker", "list") and time.time() < deadline:
            time.sleep(0.2)

    def test_drop_refuses_a_folder_that_git_does_not_know(self) -> None:
        stray = self.repo.gpt / "wt" / "M1-09"
        stray.mkdir()
        self.refused("is not a registered worktree on branch gpt/w/M1-09", "worker", "drop", "M1-09")
        self.assertTrue(stray.is_dir())

    def test_drop_refuses_a_worktree_on_another_branch(self) -> None:
        self.repo.git("checkout", "-q", "--detach", cwd=self.wt)
        self.refused("is not a registered worktree on branch gpt/w/M1-01", "worker", "drop", "M1-01")
        self.assertTrue(self.wt.is_dir())

    def test_drop_and_merge_keep_a_worktree_with_changes(self) -> None:
        (self.wt / "notes.txt").write_text("work that is not committed\n")
        self.refused("has changes that are not committed (notes.txt)", "worker", "drop", "M1-01")
        self.refused("has changes that are not committed (notes.txt)", "worker", "merge", "M1-01")
        self.assertTrue((self.wt / "notes.txt").is_file())
        (self.wt / "notes.txt").unlink()
        (self.wt / "plan" / "ITEMS.tsv").write_text("changed\n")
        self.refused("has changes that are not committed (plan/ITEMS.tsv)", "worker", "drop", "M1-01")
        self.repo.git("checkout", "--", "plan/ITEMS.tsv", cwd=self.wt)
        (self.wt / "._notes.txt").write_text("companion\n")
        self.ok("worker", "drop", "M1-01")
        self.assertFalse(self.wt.exists())
        self.assertTrue(self.repo.out("rev-parse", "gpt/w/M1-01"), "the branch stays for the review")

    def test_a_pid_that_another_process_took_is_not_the_run(self) -> None:
        run = codex.RunDir.new(self.repo.gpt / "runs" / "probe")
        run.pid.write_text(f"{os.getpid()}\n")
        run.started.write_text("1\n")
        self.assertEqual(codex.state(run).name, "lost")
        self.assertFalse(codex.stop(run, os.getpid()), "no signal to a process with another start time")
        run.started.write_text(f"{codex.process_start(os.getpid())}\n")
        self.assertEqual(codex.state(run).name, "running")

    def test_a_worker_run_takes_the_model_and_the_effort_from_the_role_table(self) -> None:
        self.ok("worker", "run", "M1-01", "--effort", "low")
        self.wait_finished()
        argv = self.repo.codex_calls()[-1]["argv"]
        self.assertEqual(argv[argv.index("-m") + 1], "test-worker")
        self.assertIn("model_reasoning_effort=low", argv)
        self.assertIn("service_tier=default", argv)
        self.ok("worker", "run", "M1-01")
        self.wait_finished()
        self.assertIn("model_reasoning_effort=high", self.repo.codex_calls()[-1]["argv"])
        ctx = core.load_context(self.repo.root)
        with self.assertRaises(Refusal) as refusal:
            codex.worker_setting(ctx, "xhigh")
        self.assertIn("not in the worker row of AGENTS.md (high, low)", refusal.exception.rule)


if __name__ == "__main__":
    unittest.main()
