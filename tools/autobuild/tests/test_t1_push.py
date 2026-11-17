"""The push rules of the Git guard (pre-push and the broker's `guard.py outgoing`) and its role table."""

from __future__ import annotations

import hashlib
import json
import subprocess
import sys
import unittest

from fixture import AUTOBUILD, ROLE_TABLE, TempRepo

import licence_scan

# Invented text that stands for copyleft reference text in the test index.
REFERENCE = ("the lantern keeper counted every copper bell along the northern harbour wall "
             "before the evening tide returned with seven grey boats and a quiet song\n")
ZERO = "0" * 40


class PushTest(unittest.TestCase):
    def setUp(self) -> None:
        self.repo = r = TempRepo()
        self.remote = r.tmp / "remote.git"
        r.git("init", "-q", "--bare", str(self.remote), cwd=r.tmp)
        r.git("remote", "add", "origin", str(self.remote))
        self.base = r.head()
        r.git("update-ref", "refs/remotes/origin/main", self.base)
        index = r.tmp / "index"
        licence_scan.write_index(index, "reference", licence_scan.shingle_hashes(REFERENCE))
        (r.hooks / "guard.json").write_text(json.dumps(
            {"PUSH_URL": str(self.remote), "BRANCH_BASE": self.base, "LICENCE_INDEX": str(index)}))

    def tearDown(self) -> None:
        self.repo.cleanup()

    def refused(self, result, words: str) -> None:
        self.assertNotEqual(result.returncode, 0, "Git accepted a push that the guard must refuse")
        self.assertIn(words, result.stderr)

    def push(self, *refspec: str, url: str = "origin") -> subprocess.CompletedProcess:
        return self.repo.git("push", "-q", url, *refspec)

    def commit_with_message(self, message: str) -> None:
        """A commit on gpt/build that the commit hooks did not see (as a commit made elsewhere)."""
        r = self.repo
        tree = r.out("rev-parse", "HEAD^{tree}")
        sha = r.out("commit-tree", tree, "-p", "HEAD", "-m", message)
        self.assertEqual(r.git("update-ref", "refs/heads/gpt/build", sha).returncode, 0)

    def outgoing(self, local: str, remote: str) -> subprocess.CompletedProcess:
        return subprocess.run([sys.executable, "-I", str(self.repo.hooks / "guard.py"), "outgoing", local, remote],
                              cwd=self.repo.root, capture_output=True, text=True)

    def test_only_local_gpt_build_to_remote_gpt_build_of_the_one_remote(self) -> None:
        r = self.repo
        r.write("plan/a.md", "a\n")
        r.must_commit("docs: a [ops]")
        self.assertEqual(self.push("gpt/build").returncode, 0)
        self.refused(self.push("gpt/build:refs/heads/main"), "only refs/heads/gpt/build to refs/heads/gpt/build")
        self.refused(self.push("gpt/build:refs/heads/gpt/x"), "only refs/heads/gpt/build to refs/heads/gpt/build")
        tip = r.out("commit-tree", r.out("rev-parse", "HEAD^{tree}"), "-p", "HEAD", "-m", "w [ops]")
        self.assertEqual(r.git("update-ref", "refs/heads/gpt/w/M1-01", tip).returncode, 0)
        self.refused(self.push("gpt/w/M1-01:refs/heads/gpt/build"), "only refs/heads/gpt/build")
        self.refused(self.push(":gpt/build"), "refused")
        other = r.tmp / "other.git"
        r.git("init", "-q", "--bare", str(other), cwd=r.tmp)
        self.refused(self.push("gpt/build", url=str(other)), "the only remote is")

    def test_outgoing_refuses_a_push_that_is_not_a_fast_forward(self) -> None:
        r = self.repo
        r.write("plan/a.md", "a\n")
        first = r.must_commit("docs: a [ops]")
        r.write("plan/b.md", "b\n")
        second = r.must_commit("docs: b [ops]")
        self.assertEqual(self.outgoing(second, first).returncode, 0)
        result = self.outgoing(first, second)
        self.assertEqual(result.returncode, 1)
        self.assertIn("not a fast-forward", result.stderr)
        self.assertEqual(self.outgoing(second, ZERO).returncode, 0)

    def test_a_commit_older_than_the_branch_is_refused(self) -> None:
        r = self.repo
        r.write("plan/old.md", "old\n")
        self.assertEqual(r.commit("docs: old [ops]", env={"GIT_COMMITTER_DATE": "2026-01-01T00:00:00Z"}).returncode, 0)
        self.refused(self.push("gpt/build"), "has a committer date before the start of this branch")

    def test_history_that_does_not_descend_from_the_branch_base_is_refused(self) -> None:
        r = self.repo
        empty = r.out("hash-object", "-t", "tree", "-w", "/dev/null")
        orphan = r.out("commit-tree", empty, "-m", "an old local branch")
        tree = r.out("rev-parse", "HEAD^{tree}")
        merged = r.out("commit-tree", tree, "-p", "HEAD", "-p", orphan, "-m", "Merge an old branch")
        self.assertEqual(r.git("update-ref", "refs/heads/gpt/build", merged).returncode, 0)
        self.refused(self.push("gpt/build"), f"commit {orphan[:12]} is not a descendant of the branch base")

    def test_attribution_lines_and_banned_phrases_in_a_message_are_refused(self) -> None:
        self.commit_with_message("docs: x [ops]\n\nGenerated with a tool")
        self.refused(self.push("gpt/build"), "holds an attribution line")
        self.repo.git("reset", "-q", "--hard", self.base)
        self.commit_with_message("docs: y [ops]\n\nPut it on HOLD.")
        self.refused(self.push("gpt/build"), "uses a banned phrase")

    def test_each_outgoing_commit_gets_the_licence_scan(self) -> None:
        r = self.repo
        r.write("docs/a.md", "Notes.\n" + REFERENCE)
        added = r.must_commit("docs: a [ops]")
        r.git("rm", "-q", "docs/a.md")
        r.must_commit("docs: remove a [ops]")
        result = self.push("gpt/build")
        self.refused(result, f"licence scan: {added[:12]}\tdocs/a.md\treference")
        digest = hashlib.sha256(("Notes.\n" + REFERENCE).encode()).hexdigest()
        r.write("tools/autobuild/licence-allow.tsv",
                f"path\tpart\tsha256\treason\ndocs/a.md\treference\t{digest}\tthe text is our own; reviewed\n")
        r.must_commit("chore(licence): allow docs/a.md [ops]")
        self.assertEqual(self.push("gpt/build").returncode, 0, "an allow row with the SHA-256 of the content allows it")
        r.write("docs/a.md", "Other notes.\n" + REFERENCE)
        changed = r.must_commit("docs: a again [ops]")
        self.refused(self.push("gpt/build"), f"licence scan: {changed[:12]}\tdocs/a.md")

    def test_the_push_needs_the_licence_scanner_next_to_the_guard(self) -> None:
        r = self.repo
        (r.hooks / "licence_scan.py").unlink()
        r.write("plan/a.md", "a\n")
        r.must_commit("docs: a [ops]")
        self.refused(self.push("gpt/build"), "licence_scan.py is missing")


class RoleTableTest(unittest.TestCase):
    def roles(self, text: str) -> subprocess.CompletedProcess:
        return subprocess.run([sys.executable, "-I", str(AUTOBUILD / "hooks" / "guard.py"), "roles"],
                              input=text, capture_output=True, text=True)

    def test_the_roles_of_section_3_with_their_models_and_efforts(self) -> None:
        result = self.roles(ROLE_TABLE)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), {
            "lead": {"model": "test-lead", "efforts": ["high"]},
            "worker": {"model": "test-worker", "efforts": ["high", "low"]},
            "reviewer": {"model": "test-reviewer", "efforts": ["high"]},
            "adviser": {"model": "test-adviser", "efforts": ["xhigh"]},
        })

    def test_a_table_outside_section_3_is_not_the_role_table(self) -> None:
        text = ROLE_TABLE.replace("## 3. Models and effort", "## 2. Other")
        result = self.roles(text)
        self.assertEqual(result.returncode, 1)
        self.assertIn("no role table", result.stderr)

    def test_the_real_manual_has_every_role(self) -> None:
        result = self.roles((AUTOBUILD.parents[1] / "AGENTS.md").read_text())
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(sorted(json.loads(result.stdout)), ["adviser", "lead", "reviewer", "worker"])


if __name__ == "__main__":
    unittest.main()
