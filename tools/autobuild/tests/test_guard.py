"""The Git guard with real Git commands: each rule refuses and allows as the data model says."""

from __future__ import annotations

import unittest

from fixture import ITEMS_HEADER, ROWS_HEADER, TempRepo, item_line, row


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

    def test_commits_only_on_gpt_branches(self) -> None:
        r = self.repo
        r.write("plan/a.md", "a\n")
        self.assertEqual(r.commit("docs: add a [ops]").returncode, 0)
        r.git("checkout", "-q", "--detach")
        r.write("plan/b.md", "b\n")
        self.refused(r.commit("docs: add b [ops]"), "only on gpt/* branches")

    def test_refs_outside_gpt_and_tags_are_refused(self) -> None:
        r = self.repo
        self.refused(r.git("branch", "feature"), "only refs/heads/gpt/* may change")
        self.refused(r.git("tag", "v1"), "tags are not used")
        self.assertEqual(r.git("branch", "gpt/x").returncode, 0)
        r.git("checkout", "-q", "gpt/x")
        self.refused(r.git("branch", "-D", "gpt/build"), "refusing to delete gpt/build")

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

    def test_ten_commits_after_the_measured_commit(self) -> None:
        r = self.repo
        measured = r.head()
        r.append("plan/scoreboard/build.tsv", row("2026-11-17T10:00:00Z", measured, "-", "harness.units", 5, 9))
        r.must_commit("chore(plan): rows [ops]")
        for n in range(9):
            r.write(f"plan/n{n}.md", f"{n}\n")
            r.must_commit(f"docs: note {n} [ops]")
        r.write("plan/late.md", "late\n")
        self.refused(r.commit("docs: one too many [ops]"), "10 commits since the last measured commit")
        r.append("plan/scoreboard/build.tsv", row("2026-11-17T11:00:00Z", r.head(), "-", "harness.units", 5, 9))
        self.assertEqual(r.commit("chore(plan): new rows [ops]").returncode, 0)

    def test_three_experiments_with_no_gain_block_each_commit(self) -> None:
        r = self.repo
        self.items(item_line("M1-01", "active", baseline="5/9"))
        for n in range(3):
            r.append("plan/scoreboard/build.tsv", row(f"2026-11-17T1{n}:00:00Z", r.head(), "M1-01", "harness.units", 5, 9))
        r.write("plan/x.md", "x\n")
        self.refused(r.commit("docs: x [M1-01]"), "used 3 experiments with no gain")
        r.append("plan/scoreboard/build.tsv", row("2026-11-17T14:00:00Z", r.head(), "M1-01", "harness.units", 6, 9))
        self.assertEqual(r.commit("docs: x [M1-01]").returncode, 0)

    def test_baseline_zero_counts_the_first_row_with_a_total_as_the_gain(self) -> None:
        r = self.repo
        self.items(item_line("M1-01", "active", metric="lua.api", baseline="0/0"))
        for n in range(3):
            r.append("plan/scoreboard/build.tsv", row(f"2026-11-17T1{n}:00:00Z", r.head(), "M1-01", "lua.api", 0, 0))
        r.write("plan/x.md", "x\n")
        self.refused(r.commit("docs: x [M1-01]"), "used 3 experiments with no gain")
        r.append("plan/scoreboard/build.tsv", row("2026-11-17T14:00:00Z", r.head(), "M1-01", "lua.api", 0, 40))
        self.assertEqual(r.commit("docs: x [M1-01]").returncode, 0)

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

    def test_banned_phrases(self) -> None:
        r = self.repo
        r.write("plan/a.md", "We put this on HOLD.\n")
        self.refused(r.commit("docs: a [ops]"), "banned phrase")
        r.write("plan/a.md", "We hold the lock.\n")
        r.must_commit("docs: a [ops]")
        r.write("plan/b.md", "b\n")
        self.refused(r.commit("docs: b, then a Fresh Complete Review [ops]"), "banned phrase")

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
        r.git("branch", "gpt/w/M1-01")
        r.git("checkout", "-q", "gpt/w/M1-01")
        r.write("plan/w.md", "w\n")
        r.must_commit("docs: w [ops]")
        r.git("checkout", "-q", "gpt/build")
        r.write("plan/b.md", "b\n")
        r.must_commit("docs: b [ops]")
        result = r.git("merge", "--no-ff", "-q", "-m", "Merge the worker", "gpt/w/M1-01")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_push_rules(self) -> None:
        r = self.repo
        remote = r.tmp / "remote.git"
        r.git("init", "-q", "--bare", str(remote), cwd=r.tmp)
        r.git("remote", "add", "origin", str(remote))
        r.git("update-ref", "refs/remotes/origin/main", r.head())
        r.write("plan/a.md", "a\n")
        r.must_commit("docs: a [ops]")
        self.assertEqual(r.git("push", "-q", "origin", "gpt/build").returncode, 0)
        self.refused(r.git("push", "-q", "origin", "gpt/build:refs/heads/main"), "only refs/heads/gpt/* may be pushed")
        self.refused(r.git("reset", "-q", "--hard", "HEAD~1"), "refusing to rewrite pushed history")
        r.git("branch", "gpt/x")
        r.git("checkout", "-q", "gpt/x")
        r.write("plan/x.md", "x\n")
        r.must_commit("docs: x [ops]")
        self.assertEqual(r.git("push", "-q", "origin", "gpt/x").returncode, 0)
        self.assertEqual(r.git("reset", "-q", "--hard", "HEAD~1").returncode, 0)
        r.write("plan/y.md", "y\n")
        r.must_commit("docs: y [ops]")
        self.refused(r.git("push", "-q", "--force", "origin", "gpt/x"), "not a fast-forward")
        self.refused(r.git("push", "-q", "origin", ":gpt/x"), "deletion of refs/heads/gpt/x refused")
        r.write("plan/old.md", "old\n")
        old = r.commit("docs: old [ops]", env={"GIT_COMMITTER_DATE": "2026-01-01T00:00:00Z"})
        self.assertEqual(old.returncode, 0, old.stderr)
        r.git("branch", "gpt/old")
        self.refused(r.git("push", "-q", "origin", "gpt/old"), "older than this branch")


if __name__ == "__main__":
    unittest.main()
