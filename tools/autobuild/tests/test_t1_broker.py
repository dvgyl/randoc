"""ops push, review, ask and holdout through the broker: requests in the inbox, answers that only the broker writes."""

from __future__ import annotations

import json
import os
import time
import unittest

from fixture import ITEMS_HEADER, TempRepo, fake_broker, item_line, row

SHA256 = "ab" * 32


class BrokerTest(unittest.TestCase):
    def setUp(self) -> None:
        self.repo = TempRepo()
        self.addCleanup(self.repo.cleanup)
        self.replies: list[dict] = []
        self.fake = fake_broker(self, self.repo, lambda kind, data: self.replies.pop(0))
        self.out, self.results = self.fake.out, self.fake.results

    def ops(self, *args: str) -> tuple[int, str, str]:
        return self.repo.ops(*args)

    def ok(self, *args: str) -> str:
        code, out, err = self.ops(*args)
        self.assertEqual(code, 0, f"ops {' '.join(args)} failed: {err}")
        return out

    def refused(self, words: str, *args: str) -> str:
        code, out, err = self.ops(*args)
        self.assertNotEqual(code, 0, f"ops {' '.join(args)} did not refuse")
        self.assertIn(words, err + out)
        return err + out

    def start_item(self) -> None:
        r = self.repo
        r.append("plan/scoreboard/build.tsv", row("2026-11-17T09:00:00Z", r.head(), "-", "harness.units", 5, 9))
        r.write("plan/ITEMS.tsv", ITEMS_HEADER + item_line("M1-01", "active", baseline="5/9"))
        r.must_commit("chore(plan): start M1-01 [ops]")
        r.write("crates/x/src/a.rs", "fn a() {}\n")
        r.must_commit("feat(x): a [M1-01]")

    # -- push --------------------------------------------------------------------

    def test_push_asks_for_the_tip_of_gpt_build_and_records_the_pushed_tip(self) -> None:
        tip = self.repo.head()
        self.replies.append({"status": "pushed", "commit": tip, "detail": "guard: may be pushed"})
        out = self.ok("push")
        self.assertEqual(self.fake.requests, [{"kind": "push", "commit": tip}])
        self.assertIn("push of gpt/build at", out)
        self.assertEqual(self.repo.out("rev-parse", "refs/remotes/origin/gpt/build"), tip)

    def test_a_refused_push_changes_nothing(self) -> None:
        self.replies.append({"status": "rejected", "reason": "the guard refused the push", "detail": "licence scan: x"})
        out = self.refused("rejected", "push")
        self.assertIn("licence scan: x", out)
        self.assertNotEqual(self.repo.git("rev-parse", "-q", "--verify", "refs/remotes/origin/gpt/build").returncode, 0)

    def test_a_stale_heartbeat_refuses_before_any_request(self) -> None:
        old = time.time() - 3600
        os.utime(self.out / "heartbeat.json", (old, old))
        self.refused("the broker does not run", "push")
        self.refused("the broker does not run", "holdout", "request")
        self.assertEqual(self.fake.requests, [])

    # -- review --------------------------------------------------------------------

    def reviews(self) -> list[tuple[str, str]]:
        lines = self.repo.read("plan/REVIEWS.tsv").splitlines()[1:]
        return [(line.split("\t")[2], line.split("\t")[5]) for line in lines]

    def test_a_review_round_is_used_before_the_session_and_a_failed_session_keeps_it(self) -> None:
        self.start_item()
        self.replies.append({"status": "error", "reason": "the session ended with exit 1"})
        code, out, err = self.ops("review", "M1-01")
        self.assertEqual(code, 1)
        self.assertEqual(self.reviews(), [("1", "started"), ("1", "failed")])
        request = self.fake.requests[-1]
        self.assertEqual((request["kind"], request["item"], request["round"]), ("review", "M1-01", 1))
        self.assertIn("feat(x): a [M1-01]", request["prompt"])
        self.replies.append({"status": "done", "session": "thread-9", "final": "notes\nVERDICT: approve\n"})
        self.ok("review", "M1-01")
        self.assertEqual(self.reviews()[2:], [("2", "started"), ("2", "approve")])
        self.refused("a third round is not allowed", "review", "M1-01")

    def test_a_block_needs_the_test_or_command_and_its_output(self) -> None:
        self.start_item()
        self.replies.append({"status": "done", "final": "VERDICT: block\nEVIDENCE: cargo test -p x reads_past_end\n"})
        self.assertIn("not a valid block", self.ok("review", "M1-01"))
        self.replies.append({"status": "done", "session": "thread-2", "final":
                             "- reads past the end\nVERDICT: block\nEVIDENCE: cargo test -p x reads_past_end\n"
                             "OUTPUT:\nthread 'reads_past_end' panicked at index 9\n"})
        self.ok("review", "M1-01")
        rows = [line.split("\t") for line in self.repo.read("plan/REVIEWS.tsv").splitlines()[1:]]
        self.assertEqual([(r[2], r[5]) for r in rows if r[5] != "started"], [("1", "approve"), ("2", "block")])
        self.assertEqual(rows[-1][4], "thread-2")
        self.assertEqual(rows[-1][6], "cargo test -p x reads_past_end => thread 'reads_past_end' panicked at index 9")

    # -- ask -------------------------------------------------------------------------

    def test_ask_needs_evidence_and_has_one_question_for_each_item_and_reason(self) -> None:
        self.start_item()
        self.refused("needs --evidence", "ask", "--reason", "two-fixes-failed", "Why?")
        self.assertEqual(self.fake.requests, [])
        answer = {"status": "done", "session": "thread-5", "final": "Look at the table.\nSUMMARY: fix the table.\n"}
        self.replies.append(dict(answer))
        out = self.ok("ask", "--reason", "two-fixes-failed", "--evidence", "cargo test x: 2 fail after fixes a1, b2",
                      "Why do the units fail?")
        self.assertIn("Look at the table.", out)
        request = self.fake.requests[-1]
        self.assertEqual((request["kind"], request["item"]), ("advice", "M1-01"))
        self.assertIn("cargo test x: 2 fail after fixes a1, b2", request["prompt"])
        advice = self.repo.read("plan/ADVICE.md")
        self.assertRegex(advice, r"event=M1-01/two-fixes-failed reason=two-fixes-failed session=\d{8}T\d{6}Z-advice-")
        self.assertIn("Session: thread-5", advice)
        self.assertIn("> cargo test x: 2 fail after fixes a1, b2", advice)
        self.ok("ask", "--decision", "Fix the table, then measure.")
        self.repo.append("plan/scoreboard/build.tsv", row("2026-11-17T13:00:00Z", self.repo.head(), "M1-01",
                                                          "harness.units", 4, 9))
        self.refused("already has its one question", "ask", "--reason", "two-fixes-failed", "--evidence", "new", "Again?")
        self.replies.append(dict(answer))
        self.ok("ask", "--reason", "no-next-step", "--evidence", "the table is fixed; 2 units still fail", "Next?")

    def test_a_sent_question_is_used_also_when_the_session_gives_no_answer(self) -> None:
        self.start_item()
        self.replies.append({"status": "error", "reason": "timeout"})
        self.refused("is used: decide without an answer", "ask", "--reason", "no-next-step", "--evidence", "log", "Next?")
        advice = self.repo.read("plan/ADVICE.md")
        self.assertIn("event=M1-01/no-next-step reason=no-next-step", advice)
        self.assertIn("Answer: none (timeout)", advice)
        self.refused("has no decision yet", "ask", "--reason", "no-next-step", "--evidence", "log", "Next?")
        self.ok("ask", "--decision", "Split the item.")
        self.refused("already has its one question", "ask", "--reason", "no-next-step", "--evidence", "log", "Next?")
        self.assertEqual(len(self.fake.requests), 1)

    def test_a_session_that_changes_the_lead_tree_is_refused_and_logged(self) -> None:
        self.start_item()

        def write_into_the_tree(reply: dict):
            (self.repo.root / "crates/x/src/a.rs").write_text("fn changed() {}\n")
            return reply

        self.fake.answer = lambda kind, data: write_into_the_tree(self.replies.pop(0))
        self.replies.append({"status": "done", "session": "thread-7", "final": "notes\nVERDICT: approve\n"})
        self.refused("the lead tree changed during the review session", "review", "M1-01")
        self.assertEqual(self.reviews(), [("1", "started"), ("1", "failed")])
        log = self.repo.read("plan/REVIEW_LOG.md")
        self.assertIn("blocked-on-owner M1-01", log)
        self.assertIn("crates/x/src/a.rs", log)
        self.assertIn("randoc-review", self.repo.read("plan/BLOCKED_ON_OWNER.md"))
        self.repo.git("checkout", "--", "crates/x/src/a.rs")
        self.replies.append({"status": "done", "session": "thread-8", "final": "Edit the file.\nSUMMARY: edit.\n"})
        self.refused("the lead tree changed during the adviser session", "ask", "--reason", "no-next-step",
                     "--evidence", "log", "Next?")
        self.assertIn("Answer: none (not used: the lead tree changed", self.repo.read("plan/ADVICE.md"))
        self.assertNotIn("Answer (summary)", self.repo.read("plan/ADVICE.md"))

    # -- held-out ------------------------------------------------------------------

    def result(self, name: str, **fields) -> None:
        data = {"status": "done", "utc": "2026-11-17T12:00:00Z", "commit": self.repo.head(), "binary_sha256": SHA256,
                "binary_provenance": "built", "sets": {"corpus": {"pass": 4, "total": 10}, "generated": {"pass": 7, "total": 9}}}
        (self.results / f"{name}.json").write_text(json.dumps({**data, **fields}))

    def test_a_request_names_the_commit_only(self) -> None:
        self.replies.append(None)
        out = self.ok("holdout", "request")
        self.assertIn("the broker builds it first", out)
        deadline = time.time() + 5
        while not self.fake.requests and time.time() < deadline:
            time.sleep(0.02)
        self.assertEqual(self.fake.requests, [{"kind": "evaluate", "commit": self.repo.head()}])
        other = self.repo.out("commit-tree", self.repo.out("rev-parse", "HEAD^{tree}"), "-p", "HEAD", "-m", "x")
        self.repo.git("checkout", "-q", "--detach", other)
        self.refused("is not on gpt/build", "holdout", "request")
        self.refused("is not on gpt/build", "holdout", "request", "--commit", other)
        self.refused("is not a commit", "holdout", "request", "--commit", "0" * 40)

    def test_a_request_can_name_the_earlier_candidate_after_a_deferral(self) -> None:
        candidate = self.repo.head()
        self.repo.write("tools/x.py", "x = 1\n")
        self.repo.must_commit("chore(tools): x [ops]")
        self.repo.write("tools/x.py", "x = 2\n")
        self.replies.append(None)
        self.ok("holdout", "request", "--commit", candidate[:10])
        deadline = time.time() + 5
        while not self.fake.requests and time.time() < deadline:
            time.sleep(0.02)
        self.assertEqual(self.fake.requests, [{"kind": "evaluate", "commit": candidate}])

    def test_only_a_complete_result_counts(self) -> None:
        self.result("1-no-hash", utc="2026-11-17T13:00:00Z", binary_sha256="abc")
        self.result("2-one-set", utc="2026-11-17T14:00:00Z", sets={"corpus": {"pass": 4, "total": 10}})
        self.result("3-bad-count", utc="2026-11-17T15:00:00Z",
                    sets={"corpus": {"pass": 11, "total": 10}, "generated": {"pass": 7, "total": 9}})
        out = self.ok("holdout", "results")
        self.assertIn("held-out: no complete result yet", out)
        for name, why in (("1-no-hash", "no full binary SHA-256"), ("2-one-set", "set generated"),
                          ("3-bad-count", "set corpus")):
            self.assertIn(f"not counted: {name}.json", out)
            self.assertIn(why, out)
        self.result("0-good")
        (self.results / "4-deferred.json").write_text(json.dumps(
            {"status": "deferred", "utc": "2026-11-17T16:00:00Z", "next_run": "2026-11-17T22:00:00Z"}))
        out = self.ok("holdout", "results", "--record")
        self.assertIn("held-out: corpus 4/10, generated 7/9", out)
        self.assertIn("newest answer: deferred (2026-11-17T22:00:00Z)", out)
        board = self.repo.read("plan/scoreboard/build.tsv")
        self.assertIn(f"\tholdout.corpus\t4\t10\tholdout@{SHA256[:12]}\t{self.results / '0-good.json'}", board)
        self.assertIn("\tholdout.generated\t7\t9\t", board)
        self.assertIn("already", self.ok("holdout", "results", "--record"))


if __name__ == "__main__":
    unittest.main()
