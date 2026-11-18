"""The Git guard with real Git commands: each rule refuses and allows as the data model says."""

from __future__ import annotations

import hashlib
import json
import unittest

from fixture import ITEMS_HEADER, REGISTRY, TempRepo, item_line, row

PROVENANCE = "tools/autobuild/provenance.tsv"
PROVENANCE_HEADER = "path\tsha256\torigin\tlicence\n"


class GuardTest(unittest.TestCase):
    def setUp(self) -> None:
        self.repo = TempRepo()

    def tearDown(self) -> None:
        self.repo.cleanup()

    def refused(self, result, words: str) -> None:
        self.assertNotEqual(result.returncode, 0, "Git accepted a change that the guard must refuse")
        self.assertIn(words, result.stderr)

    def items(self, *lines: str) -> None:
        self.repo.write("plan/ITEMS.tsv", ITEMS_HEADER + "".join(lines))

    def test_commits_only_on_gpt_build_and_on_the_branch_of_an_active_item(self) -> None:
        r = self.repo
        r.write("plan/a.md", "a\n")
        self.assertEqual(r.commit("docs: add a [ops]").returncode, 0)
        r.git("checkout", "-q", "--detach")
        r.write("plan/b.md", "b\n")
        self.refused(r.commit("docs: add b [ops]"), "only on gpt/build and on gpt/w/ID")
        r.git("rm", "-q", "--cached", "plan/b.md")
        (r.root / "plan/b.md").unlink()
        r.git("checkout", "-q", "gpt/build")
        self.assertEqual(r.git("branch", "gpt/w/M1-01").returncode, 0)
        r.git("checkout", "-q", "gpt/w/M1-01")
        r.write("plan/w.md", "w\n")
        self.refused(r.commit("docs: w [ops]"), "item M1-01 is not active in gpt/build")
        r.git("rm", "-q", "--cached", "plan/w.md")
        r.git("checkout", "-q", "gpt/build")
        self.items(item_line("M1-01", "active", baseline="1/9"))
        r.must_commit("chore(plan): start M1-01 [ops]", ["plan/ITEMS.tsv"])
        r.git("checkout", "-q", "gpt/w/M1-01")
        self.assertEqual(r.commit("docs: w [ops]", ["plan/w.md"]).returncode, 0)

    def test_refs_outside_gpt_build_and_worker_branches_and_tags_are_refused(self) -> None:
        r = self.repo
        self.refused(r.git("branch", "feature"), "only gpt/build and gpt/w/ID branches may change")
        self.refused(r.git("branch", "gpt/x"), "only gpt/build and gpt/w/ID branches may change")
        self.refused(r.git("tag", "v1"), "tags are not used")
        self.assertEqual(r.git("branch", "gpt/w/M1-01").returncode, 0)
        self.refused(r.git("branch", "-D", "gpt/w/M1-01"), "each branch stays for the review")
        r.git("checkout", "-q", "gpt/w/M1-01")
        self.refused(r.git("branch", "-D", "gpt/build"), "refusing to delete refs/heads/gpt/build")

    def test_subject_needs_an_active_item_or_ops(self) -> None:
        r = self.repo
        self.items(item_line("M1-01", "queued"))
        r.must_commit("chore(plan): add M1-01 [ops]")
        r.write("plan/n.md", "n\n")
        self.refused(r.commit("docs: no tag"), "the subject must end with [ITEM-ID]")
        self.refused(r.commit("docs: queued item [M1-01]"), "an active item")
        self.items(item_line("M1-01", "active", baseline="1/9"))
        self.assertEqual(r.commit("docs: active item [M1-01]").returncode, 0)

    def test_ops_commit_may_not_change_product_code(self) -> None:
        r = self.repo
        r.write("crates/x/src/lib.rs", "fn a() {}\n")
        self.refused(r.commit("feat: product code [ops]"), "needs a work item")
        self.items(item_line("M1-01", "active", baseline="1/9"))
        self.assertEqual(r.commit("feat: product code [M1-01]").returncode, 0)
        r.write("tools/t.py", "x = 1\n")
        r.write("plan/p.md", "p\n")
        self.assertEqual(r.commit("chore: tools and plan [ops]").returncode, 0)

    def evidence(self, name: str, manifest: dict | None = None) -> str:
        """The evidence file of a run; with `manifest`, the run.json of a completed run next to it."""
        folder = self.repo.gpt / "runs" / "measure" / name
        folder.mkdir(parents=True, exist_ok=True)
        if manifest is not None:
            (folder / "run.json").write_text(json.dumps({"end": "2026-11-17T23:00:00Z", **manifest}))
        return str(folder / "harness.json")

    def full_row(self, utc: str, commit: str, evidence: str, item: str = "-", dirty: str = "no") -> str:
        return row(utc, commit, item, "harness.units", 5, 9, dirty=dirty).replace("/runs/x", evidence)

    def completed(self, name: str, utc: str, commit: str, rows: int = 1,
                  suites: tuple[str, ...] = ("harness", "corpus")) -> str:
        return self.evidence(name, {"utc": utc, "commit": commit, "rows": rows, "suites": list(suites)})

    def two_suites(self) -> str:
        """Commit a registry with the active suites harness and corpus; returns that commit."""
        self.repo.write("tools/autobuild/metrics.toml", REGISTRY + REGISTRY.replace("harness", "corpus"))
        return self.repo.must_commit("chore(tools): two suites [ops]", ["tools"])

    def test_ten_commits_after_the_measured_commit(self) -> None:
        r = self.repo
        measured = self.two_suites()
        utc = "2026-11-17T10:00:00Z"
        r.append("plan/scoreboard/build.tsv", self.full_row(utc, measured, self.completed("a", utc, measured)))
        r.must_commit("chore(plan): rows [ops]")
        for n in range(9):
            r.write(f"plan/n{n}.md", f"{n}\n")
            r.must_commit(f"docs: note {n} [ops]")
        r.write("plan/late.md", "late\n")
        self.refused(r.commit("docs: one too many [ops]"), "10 commits since the last measured commit")
        head, old = r.head(), r.out("rev-parse", "HEAD~3")
        utc = "2026-11-17T11:00:00Z"
        for wrong in (
            self.full_row(utc, head, self.completed("b", utc, head), item="M1-01"),  # an item run
            self.full_row(utc, head, self.completed("b", utc, head), dirty="yes"),  # a dirty tree
            self.full_row(utc, old, self.completed("o", utc, old)),  # not HEAD or its parent
            self.full_row(utc, head, "/nowhere/measure/x/harness.json"),  # no evidence
            self.full_row(utc, head, self.evidence("bare")),  # a folder with no run.json: the run did not end
            self.full_row(utc, head, self.completed("short", utc, head, rows=2)),  # rows of the run are missing
            self.full_row(utc, head, self.completed("other", "2026-11-17T09:00:00Z", head)),  # another run
            self.full_row(utc, head, self.completed("one", utc, head, suites=("harness",))),  # an active suite is missing
        ):
            text = r.read("plan/scoreboard/build.tsv")
            r.append("plan/scoreboard/build.tsv", wrong)
            self.refused(r.commit("chore(plan): rows [ops]"), "10 commits since the last measured commit")
            r.write("plan/scoreboard/build.tsv", text)
        utc = "2026-11-17T12:00:00Z"
        r.append("plan/scoreboard/build.tsv", self.full_row(utc, head, self.completed("c", utc, head)))
        self.assertEqual(r.commit("chore(plan): new rows [ops]").returncode, 0)

    def test_committed_rows_of_a_run_that_misses_an_active_suite_never_reset_the_counter(self) -> None:
        r = self.repo
        measured = self.two_suites()
        r.append("plan/scoreboard/build.tsv", r.full_run("2026-11-17T10:00:00Z", measured, {"harness.units": (5, 9)},
                                                         suites=("harness", "corpus")))
        r.must_commit("chore(plan): rows [ops]")
        for n in range(4):
            r.write(f"plan/n{n}.md", f"{n}\n")
            r.must_commit(f"docs: note {n} [ops]")
        r.append("plan/scoreboard/build.tsv", r.full_run("2026-11-17T11:00:00Z", r.head(), {"harness.units": (5, 9)},
                                                         suites=("harness",)))
        r.must_commit("chore(plan): rows of one suite [ops]")
        for n in range(4, 8):
            r.write(f"plan/n{n}.md", f"{n}\n")
            r.must_commit(f"docs: note {n} [ops]")
        r.write("plan/late.md", "late\n")
        self.refused(r.commit("docs: one too many [ops]"), "10 commits since the last measured commit")

    def test_three_experiments_with_no_gain_block_each_commit(self) -> None:
        r = self.repo
        self.items(item_line("M1-01", "active", baseline="5/9"))
        for n in range(3):
            r.append("plan/scoreboard/build.tsv", row(f"2026-11-17T1{n}:00:00Z", r.head(), "M1-01", "harness.units", 5, 9))
        r.write("plan/x.md", "x\n")
        self.refused(r.commit("docs: x [M1-01]"), "used 3 experiments with no gain")
        r.append("plan/scoreboard/build.tsv", row("2026-11-17T14:00:00Z", r.head(), "M1-01", "harness.units", 6, 9))
        self.assertEqual(r.commit("docs: x [M1-01]").returncode, 0)

    def test_the_gain_is_that_of_the_latest_row_and_the_failing_count_must_not_rise(self) -> None:
        r = self.repo
        self.items(item_line("M1-01", "active", baseline="5/9"))
        r.append("plan/scoreboard/build.tsv", row("2026-11-17T10:00:00Z", r.head(), "M1-01", "harness.units", 7, 9))
        r.append("plan/scoreboard/build.tsv", row("2026-11-17T11:00:00Z", r.head(), "M1-01", "harness.units", 5, 9))
        r.append("plan/scoreboard/build.tsv", row("2026-11-17T12:00:00Z", r.head(), "M1-01", "harness.units", 6, 11))
        r.write("plan/x.md", "x\n")
        self.refused(r.commit("docs: x [M1-01]"), "used 3 experiments with no gain")
        r.append("plan/scoreboard/build.tsv", row("2026-11-17T13:00:00Z", r.head(), "M1-01", "harness.units", 8, 11))
        self.assertEqual(r.commit("docs: x [M1-01]").returncode, 0)

    def test_rows_below_the_coverage_rule_of_the_item_are_no_gain(self) -> None:
        r = self.repo
        self.items(item_line("M1-01", "active", metric="gate.steps", baseline="0/0", accept="all-pass"))
        for n in range(3):
            r.append("plan/scoreboard/build.tsv", row(f"2026-11-17T1{n}:00:00Z", r.head(), "M1-01", "gate.steps", 0, 8))
        r.write("plan/x.md", "x\n")
        self.refused(r.commit("docs: x [M1-01]"), "used 3 experiments with no gain on gate.steps")
        r.append("plan/scoreboard/build.tsv", row("2026-11-17T14:00:00Z", r.head(), "M1-01", "gate.steps", 8, 8))
        self.assertEqual(r.commit("docs: x [M1-01]").returncode, 0)

    def test_attempt_records_count_as_experiments(self) -> None:
        r = self.repo
        self.items(item_line("M1-01", "active", baseline="5/9"))
        attempts = r.gpt / "runs" / "attempts.tsv"
        attempts.write_text("utc\tcommit\titem\tsuites\tpid\n" + "".join(
            f"2026-11-17T1{n}:00:00Z\t{r.head()}\tM1-01\tharness\t1\n" for n in range(3)))
        r.write("plan/x.md", "x\n")
        self.refused(r.commit("docs: x [M1-01]"), "used 3 experiments with no gain")
        r.append("plan/scoreboard/build.tsv", row("2026-11-17T14:00:00Z", r.head(), "M1-01", "harness.units", 6, 9))
        self.assertEqual(r.commit("docs: x [M1-01]").returncode, 0)

    def test_a_row_older_than_a_correction_of_its_metric_is_no_gain(self) -> None:
        r = self.repo
        self.items(item_line("M1-01", "active", baseline="5/9"))
        for n in range(3):
            r.append("plan/scoreboard/build.tsv", row(f"2026-11-17T1{n}:00:00Z", r.head(), "M1-01", "harness.units", 7, 9))
        r.write("plan/x.md", "x\n")
        self.assertEqual(r.commit("docs: x [M1-01]").returncode, 0)
        r.write("plan/corrections.tsv", "utc\tmetric\tcommit\titem\treason\n"
                f"2026-11-17T15:00:00Z\tharness.units\t{r.head()}\tM1-01\tthe units are formed in another way\n")
        self.refused(r.commit("chore(plan): correction [M1-01]"), "used 3 experiments with no gain")

    def test_the_cut_commit_may_carry_the_tag_of_the_item_that_it_cuts(self) -> None:
        r = self.repo
        self.items(item_line("M1-01", "active", baseline="5/9"), item_line("M1-02", "queued"))
        r.must_commit("chore(plan): start M1-01 [ops]")
        r.write("crates/x/src/lib.rs", "fn a() {}\n")
        r.must_commit("feat: a [M1-01]")
        for n in range(3):
            r.append("plan/scoreboard/build.tsv", row(f"2026-11-17T1{n}:00:00Z", r.head(), "M1-01", "harness.units", 5, 9))
        self.refused(r.commit("chore(plan): rows [ops]"), "used 3 experiments with no gain")
        r.git("rm", "-q", "crates/x/src/lib.rs")
        self.items(item_line("M1-01", "cut", baseline="5/9"), item_line("M1-02", "cut"))
        self.refused(r.commit("chore(plan): cut a queued item with code [M1-02]"), "an active item")
        self.assertEqual(r.commit("chore(plan): cut item M1-01 and revert its code [M1-01]").returncode, 0)
        r.write("crates/x/src/lib.rs", "fn b() {}\n")
        self.refused(r.commit("feat: more code under a cut item [M1-01]"), "an active item")

    def test_a_cut_does_not_wait_for_another_exhausted_item_or_for_the_commit_limit(self) -> None:
        r = self.repo
        r.append("plan/scoreboard/build.tsv", r.full_run("2026-11-17T09:00:00Z", r.head(), {"harness.units": (5, 9)}))
        self.items(item_line("M1-01", "active", baseline="5/9"), item_line("M1-02", "active", baseline="5/9"))
        r.must_commit("chore(plan): start M1-01 and M1-02 [ops]")
        for n in range(9):
            r.write(f"plan/n{n}.md", f"{n}\n")
            r.must_commit(f"docs: note {n} [ops]")
        (r.gpt / "runs" / "attempts.tsv").write_text("utc\tcommit\titem\tsuites\tpid\n" + "".join(
            f"2026-11-17T1{n}:00:00Z\t{r.head()}\t{item}\tharness\t1\n" for n in range(3) for item in ("M1-01", "M1-02")))
        r.write("plan/x.md", "x\n")
        self.refused(r.commit("docs: x [ops]"), "10 commits since the last measured commit")
        r.git("rm", "-q", "--cached", "plan/x.md")
        self.items(item_line("M1-01", "cut", baseline="5/9"), item_line("M1-02", "active", baseline="5/9"))
        self.assertEqual(r.commit("chore(plan): cut item M1-01 [ops]", ["plan/ITEMS.tsv"]).returncode, 0)
        self.items(item_line("M1-01", "cut", baseline="5/9"), item_line("M1-02", "cut", baseline="5/9"))
        self.assertEqual(r.commit("chore(plan): cut item M1-02 [ops]", ["plan/ITEMS.tsv"]).returncode, 0)

    def test_a_cut_commit_needs_origin_rows_except_for_files_that_its_revert_brings_back(self) -> None:
        r = self.repo
        seed = b"\0seed"
        (r.root / "plan/seed.bin").write_bytes(seed)
        r.write(PROVENANCE, PROVENANCE_HEADER + f"plan/seed.bin\t{hashlib.sha256(seed).hexdigest()}\tmade by the test\tMIT\n")
        self.items(item_line("M1-01", "active", baseline="5/9"))
        r.must_commit("test: a seed and an item [ops]")
        r.git("rm", "-q", "plan/seed.bin")
        r.write(PROVENANCE, PROVENANCE_HEADER)
        r.must_commit("chore: remove the seed and its row [M1-01]")
        r.git("checkout", "HEAD~1", "--", "plan/seed.bin")
        self.items(item_line("M1-01", "cut", baseline="5/9"))
        (r.root / "plan/new.bin").write_bytes(b"\0new")
        self.refused(r.commit("chore(plan): cut item M1-01 and revert its code [M1-01]"),
                     "plan/new.bin is a binary file or a file above 5 MB")
        r.git("rm", "-q", "--cached", "plan/new.bin")
        (r.root / "plan/new.bin").unlink()
        self.assertEqual(r.commit("chore(plan): cut item M1-01 and revert its code [M1-01]").returncode, 0,
                         "the revert brings back a file of the history with no row")

    def test_banned_phrases(self) -> None:
        r = self.repo
        r.write("plan/a.md", "We put this on HOLD.\n")
        self.refused(r.commit("docs: a [ops]"), "banned phrase")
        r.write("plan/a.md", "We hold the lock.\n")
        r.must_commit("docs: a [ops]")
        r.write("plan/b.md", "b\n")
        self.refused(r.commit("docs: b, then a Fresh Complete Review [ops]"), "banned phrase")

    def test_banned_phrases_in_each_staged_text_file_except_the_files_that_ban_them(self) -> None:
        r = self.repo
        r.write("tools/x.py", "state = 'HOLD'\n")
        self.refused(r.commit("chore(tools): x [ops]"), "banned phrase")
        r.write("tools/x.py", "state = 'wait'\n")
        r.write("tools/autobuild/tests/test_guard.py", "TEXT = 'put it on HOLD'\n")
        r.write("AGENTS.md", r.read("AGENTS.md") + "Never write HOLD.\n")
        blob = b"\0\1HOLD\0"
        (r.root / "plan/blob.bin").write_bytes(blob)
        r.write(PROVENANCE, PROVENANCE_HEADER + f"plan/blob.bin\t{hashlib.sha256(blob).hexdigest()}\twritten by this test\tMIT\n")
        self.assertEqual(r.commit("chore(tools): x [ops]").returncode, 0, "binary files and the phrase files pass")

    def test_a_binary_or_large_file_needs_a_row_of_its_origin(self) -> None:
        r = self.repo
        data = b"\x89PNG\r\n\0\1"
        sha = hashlib.sha256(data).hexdigest()
        (r.root / "conformance/cases").mkdir(parents=True)
        (r.root / "conformance/cases/a.png").write_bytes(data)
        message = "conformance/cases/a.png is a binary file or a file above 5 MB"
        self.refused(r.commit("test: a case [ops]"), message)
        for wrong in (f"conformance/cases/a.png\t{'0' * 64}\tdrawn by the test\tMIT\n",  # another content
                      f"conformance/cases/a.png\t{sha}\t-\tMIT\n",  # no origin
                      f"conformance/cases/a.png\t{sha}\tdrawn by the test\t\n",  # no licence
                      f"conformance/cases/b.png\t{sha}\tdrawn by the test\tMIT\n"):  # another path
            r.write(PROVENANCE, PROVENANCE_HEADER + wrong)
            self.refused(r.commit("test: a case [ops]"), message)
        r.write(PROVENANCE, PROVENANCE_HEADER + f"conformance/cases/a.png\t{sha}\tdrawn by the test\tMIT\n")
        self.assertEqual(r.commit("test: a case [ops]").returncode, 0)
        r.write("plan/big.txt", "a" * (5 * 1024 * 1024 + 1))
        self.refused(r.commit("docs: big [ops]"), "plan/big.txt is a binary file or a file above 5 MB")
        r.write("plan/big.txt", "a" * (5 * 1024 * 1024))
        self.assertEqual(r.commit("docs: big [ops]").returncode, 0, "a text file of 5 MB passes")

    def test_attribution_lines_are_removed(self) -> None:
        r = self.repo
        r.write("plan/a.md", "a\n")
        r.git("add", "plan/a.md")
        result = r.git("commit", "-q", "-m", "docs: a [ops]", "-m", "Co-Authored-By: Someone <s@example.invalid>")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("Co-Authored-By", r.out("log", "-1", "--format=%B"))

    def test_companion_files_are_refused(self) -> None:
        r = self.repo
        r.write("plan/._a.md", "x\n")
        r.git("add", "-f", "plan/._a.md")
        self.refused(r.git("commit", "-q", "-m", "docs: a [ops]"), "macOS companion file")

    def test_merge_commits_need_no_tag(self) -> None:
        r = self.repo
        self.items(item_line("M1-01", "active", baseline="1/9"))
        r.must_commit("chore(plan): start M1-01 [ops]")
        r.git("branch", "gpt/w/M1-01")
        r.git("checkout", "-q", "gpt/w/M1-01")
        r.write("plan/w.md", "w\n")
        r.must_commit("docs: w [ops]")
        r.git("checkout", "-q", "gpt/build")
        r.write("plan/b.md", "b\n")
        r.must_commit("docs: b [ops]")
        result = r.git("merge", "--no-ff", "-q", "-m", "Merge the worker", "gpt/w/M1-01")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_pushed_history_of_gpt_build_is_not_rewritten(self) -> None:
        r = self.repo
        r.write("plan/a.md", "a\n")
        r.must_commit("docs: a [ops]")
        r.git("update-ref", "refs/remotes/origin/gpt/build", r.head())
        self.refused(r.git("reset", "-q", "--hard", "HEAD~1"), "refusing to rewrite pushed history")
        r.write("plan/b.md", "b\n")
        r.must_commit("docs: b [ops]")
        self.assertEqual(r.git("reset", "-q", "--hard", "HEAD~1").returncode, 0)


if __name__ == "__main__":
    unittest.main()
