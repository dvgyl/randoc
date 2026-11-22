import importlib.util
import json
import os
import stat
import sys
import tempfile
import unittest
import warnings
import zipfile
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import measure

source = measure.ROOT / "crates" / "randoc-harness" / "src" / "snapshot.py"
module_spec = importlib.util.spec_from_file_location("randoc_snapshot", source)
snapshot = importlib.util.module_from_spec(module_spec)
module_spec.loader.exec_module(snapshot)


class CaptureTests(unittest.TestCase):
    def test_a_link_does_not_add_the_target_to_the_snapshot(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder) / "work"
            root.mkdir()
            (root / "assets").symlink_to(Path(folder), target_is_directory=True)
            (root / "asset%20").write_bytes(b"local")
            states = snapshot.snapshot(root, set())
        self.assertEqual(set(states), {"assets", "asset%2520"})
        self.assertEqual(states["assets"]["kind"], "link")

    def test_declared_zip_files_ignore_entry_order_and_compression(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            archives = (("first.zip", ("a", "b"), zipfile.ZIP_STORED),
                        ("second.zip", ("b", "a"), zipfile.ZIP_DEFLATED))
            for name, order, method in archives:
                with zipfile.ZipFile(root / name, "w", compression=method) as archive:
                    for entry in order:
                        archive.writestr(entry, entry.encode() * 50)
            states = snapshot.snapshot(root, {b"first.zip", b"second.zip"})
        first, second = states["first.zip"], states["second.zip"]
        self.assertNotEqual(first["sha256"], second["sha256"])
        self.assertEqual(first["zip"], second["zip"])

    def test_archive_reports_if_mimetype_is_first_and_uncompressed(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            paths = set()
            for name, order, method in (
                    ("first.epub", ("mimetype", "other"), zipfile.ZIP_STORED),
                    ("last.epub", ("other", "mimetype"), zipfile.ZIP_STORED),
                    ("compressed.epub", ("mimetype", "other"), zipfile.ZIP_DEFLATED)):
                paths.add(os.fsencode(name))
                with zipfile.ZipFile(root / name, "w", compression=method) as archive:
                    for entry in order:
                        archive.writestr(entry, entry.encode())
            states = snapshot.snapshot(root, paths)
        self.assertEqual(len({entry["zip"]["sha256"] for entry in states.values()}), 1)
        self.assertTrue(states["first.epub"]["zip"]["first_stored_mimetype"])
        self.assertFalse(states["last.epub"]["zip"]["first_stored_mimetype"])
        self.assertFalse(states["compressed.epub"]["zip"]["first_stored_mimetype"])

    def test_declared_zip_comparison_ignores_explicit_directory_entries(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            for name in ("implicit.epub", "explicit.epub"):
                with zipfile.ZipFile(root / name, "w") as archive:
                    archive.writestr("mimetype", b"application/epub+zip")
                    if name == "explicit.epub":
                        archive.writestr("META-INF/", b"")
                    archive.writestr("META-INF/container.xml", b"<container/>")
            states = snapshot.snapshot(root, {b"implicit.epub", b"explicit.epub"})
        self.assertEqual(states["implicit.epub"]["zip"], states["explicit.epub"]["zip"])

    def test_declared_zip_files_refuse_duplicate_or_unsafe_members(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            for names in (("member", "member"), ("nested/../outside",)):
                with self.subTest(names=names), warnings.catch_warnings():
                    warnings.simplefilter("ignore", UserWarning)
                    with zipfile.ZipFile(root / "bad.zip", "w") as archive:
                        for name in names:
                            archive.writestr(name, b"data")
                    entry = snapshot.snapshot(root, {b"bad.zip"})["bad.zip"]
                    self.assertTrue(entry["invalid_zip"])
                    self.assertNotIn("zip", entry)

    def test_the_walk_refuses_deep_directory_trees(self):
        for depth in (10_000, 100_000):
            with self.subTest(depth=depth):
                def open_one(_name, parent=None):
                    index = 0 if parent is None else parent + 1
                    return index, (SimpleNamespace(name="child") for _ in range(index < depth))

                with mock.patch.object(snapshot, "open_directory", side_effect=open_one) as opened, \
                     mock.patch.object(os, "stat", return_value=SimpleNamespace(st_mode=stat.S_IFDIR)), \
                     mock.patch.object(os, "close"):
                    with self.assertRaisesRegex(ValueError, "directory depth limit"):
                        snapshot.snapshot("work", set())
                self.assertEqual(opened.call_count, snapshot.MAX_DEPTH + 1)


class FilesSuiteTests(unittest.TestCase):
    def parse(self, *units, media=()):
        measure.load_metrics()
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder)
            for name, results in (("files-report.json", units), ("files-media-report.json", media)):
                report = {"units": results, "summary": {"units": len(results)}}
                (path / name).write_text(json.dumps(report), encoding="utf-8")
            return measure.RUNNERS["files"].parse(path, "")

    def test_only_recorded_units_that_write_files_count_in_total(self):
        counts = self.parse(
            {"id": "written", "status": "pass", "oracle_writes_file": True},
            {"id": "missing", "status": "no_result", "oracle_writes_file": True},
            {"id": "no.input", "status": "skipped", "oracle_writes_file": None},
            {"id": "no.change", "status": "pass", "oracle_writes_file": False},
            media=({"id": "extract-docx", "status": "fail", "oracle_writes_file": True},),
        )
        self.assertEqual(counts.rows, {"files.units": (1, 3)})
        self.assertEqual(counts.no_result, ["missing"])
        self.assertEqual(counts.no_input, ["no.input"])

    def test_a_missing_oracle_capture_stops_the_measurement(self):
        with self.assertRaisesRegex(measure.ToolError, "needs a recorded oracle file result"):
            self.parse({"id": "unknown", "status": "not_recorded", "oracle_writes_file": None})
