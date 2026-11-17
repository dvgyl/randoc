"""licence_scan.py: the scan of each commit, the whole-tree scan with inherited hits, and allow rows bound to content."""

from __future__ import annotations

import hashlib
import subprocess
import sys
import unittest

from fixture import AUTOBUILD, TempRepo

import licence_scan

REFERENCE = ("the lantern keeper counted every copper bell along the northern harbour wall "
             "before the evening tide returned with seven grey boats and a quiet song\n")


class LicenceScanTest(unittest.TestCase):
    def setUp(self) -> None:
        self.repo = r = TempRepo()
        self.index = r.tmp / "index"
        licence_scan.write_index(self.index, "reference", licence_scan.shingle_hashes(REFERENCE))
        r.write("docs/old.md", "Inherited.\n" + REFERENCE)
        r.must_commit("docs: text that main has [ops]")
        r.git("update-ref", "refs/remotes/origin/main", r.head())

    def tearDown(self) -> None:
        self.repo.cleanup()

    def scan(self, *args: str) -> subprocess.CompletedProcess:
        return subprocess.run([sys.executable, "-I", str(AUTOBUILD / "licence_scan.py"), "scan", "--index",
                               str(self.index), *args], cwd=self.repo.root, capture_output=True, text=True)

    def allow(self, path: str, content: str) -> None:
        digest = hashlib.sha256(content.encode()).hexdigest()
        self.repo.write("tools/autobuild/licence-allow.tsv",
                        f"path\tpart\tsha256\treason\n{path}\treference\t{digest}\treviewed: our own text\n")

    def test_text_that_one_commit_adds_and_a_later_commit_removes_is_a_hit(self) -> None:
        r = self.repo
        self.assertEqual(self.scan().returncode, 0)
        r.write("docs/new.md", REFERENCE)
        added = r.must_commit("docs: new [ops]")
        r.git("rm", "-q", "docs/new.md")
        r.must_commit("docs: remove new [ops]")
        result = self.scan()
        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertIn(f"HIT\t{added[:12]}\tdocs/new.md\treference", result.stdout)
        self.assertEqual(self.scan("--base", "HEAD~1").returncode, 0, "the commits after --base only")

    def test_moved_text_of_a_parent_is_no_hit_but_a_copy_into_another_file_is(self) -> None:
        r = self.repo
        r.write("docs/old.md", "Inherited, with a new first line.\n" + REFERENCE)
        r.must_commit("docs: edit old [ops]")
        self.assertEqual(self.scan().returncode, 0, "the parent had these shingles")
        r.write("docs/copy.md", REFERENCE)
        r.must_commit("docs: copy [ops]")
        self.assertEqual(self.scan().returncode, 1)

    def test_an_allow_row_binds_to_the_sha256_of_the_content(self) -> None:
        r = self.repo
        half = " ".join(REFERENCE.split()[:14]) + "\n"
        r.write("docs/new.md", half)
        self.allow("docs/new.md", half)
        r.must_commit("docs: new, reviewed [ops]")
        self.assertEqual(self.scan().returncode, 0)
        r.write("docs/new.md", REFERENCE)
        r.must_commit("docs: more text in new [ops]")
        result = self.scan()
        self.assertEqual(result.returncode, 1, "the row allows the reviewed content only, not later additions")
        self.assertIn("docs/new.md", result.stdout)

    def test_the_whole_tree_scan_marks_text_that_main_has_as_inherited(self) -> None:
        r = self.repo
        result = self.scan("--all")
        self.assertEqual(result.returncode, 0, result.stdout)
        self.assertIn("inherited\tdocs/old.md\treference", result.stdout)
        r.write("docs/new.md", REFERENCE)
        r.must_commit("docs: new [ops]")
        result = self.scan("--all")
        self.assertEqual(result.returncode, 1)
        self.assertIn("HIT\tdocs/new.md\treference", result.stdout)
        self.allow("docs/new.md", REFERENCE)
        r.must_commit("chore(licence): allow new [ops]")
        result = self.scan("--all")
        self.assertEqual(result.returncode, 0)
        self.assertIn("allowed\tdocs/new.md\treference", result.stdout)
        r.write("docs/new.md", "Changed.\n" + REFERENCE)
        r.must_commit("docs: change new [ops]")
        self.assertEqual(self.scan("--all").returncode, 1, "a changed file needs a new review of its row")

    def test_no_origin_main_is_a_tool_error(self) -> None:
        self.repo.git("update-ref", "-d", "refs/remotes/origin/main")
        self.assertEqual(self.scan().returncode, 2)


if __name__ == "__main__":
    unittest.main()
