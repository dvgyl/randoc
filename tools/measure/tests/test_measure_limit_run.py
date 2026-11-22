"""Tests of tools/measure/bin/limit-run. Run: python3 -m unittest discover -s tools/measure/tests"""

from __future__ import annotations

import contextlib
import importlib.machinery
import importlib.util
import io
import os
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

TOOL = Path(__file__).resolve().parents[1] / "bin" / "limit-run"
ALLOCATE = "b = b'x' * (300 << 20); import time; time.sleep(30)"


def limit_run(*args: str) -> tuple[subprocess.CompletedProcess, float]:
    started = time.monotonic()
    done = subprocess.run([sys.executable, str(TOOL), *args], capture_output=True, text=True, timeout=60)
    return done, time.monotonic() - started


def alive(pid: int) -> bool:
    for _ in range(40):
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            return False
        time.sleep(0.05)
    return True


class LimitRun(unittest.TestCase):
    def test_the_exit_code_and_the_output_of_the_command_pass_through(self):
        done, _ = limit_run("100", "10", "/bin/sh", "-c", "echo out; echo err >&2; exit 3")
        self.assertEqual((done.returncode, done.stdout, done.stderr), (3, "out\n", "err\n"))

    def test_memory_of_a_grandchild_counts_and_stops_the_command(self):
        done, elapsed = limit_run("100", "30", "/bin/sh", "-c", f'"{sys.executable}" -c "{ALLOCATE}"; true')
        self.assertEqual(done.returncode, 137)
        self.assertRegex(done.stderr, r"^limit-run: memory \d+ MB is above the limit of 100 MB; stopped: ")
        self.assertLess(elapsed, 20)

    def test_the_time_limit_kills_a_descendant_in_another_session(self):
        with tempfile.TemporaryDirectory() as folder:
            marker = Path(folder) / "pid"
            leaver = (f"import os, time; os.setsid(); open({str(marker)!r}, 'w').write(str(os.getpid())); "
                      "time.sleep(60)")
            script = f'"{sys.executable}" -c "{leaver}" & sleep 60'
            done, elapsed = limit_run("500", "2", "/bin/sh", "-c", script)
            pid = int(marker.read_text())
        self.assertEqual(done.returncode, 137)
        self.assertIn("limit-run: time is above the limit of 2 s", done.stderr)
        self.assertLess(elapsed, 20)
        self.assertFalse(alive(pid))

    def test_a_process_left_in_the_group_is_killed_when_the_command_ends(self):
        with tempfile.TemporaryDirectory() as folder:
            marker = Path(folder) / "pid"
            done, _ = limit_run("500", "30", "/bin/sh", "-c", f"sleep 60 & echo $! > '{marker}'")
            pid = int(marker.read_text())
        self.assertEqual(done.returncode, 0)
        self.assertFalse(alive(pid))

    def test_the_command_does_not_start_when_memory_cannot_be_read(self):
        loader = importlib.machinery.SourceFileLoader("limit_run", str(TOOL))
        module = importlib.util.module_from_spec(importlib.util.spec_from_loader("limit_run", loader))
        loader.exec_module(module)
        module.LIBPROC = "/nonexistent/libproc.dylib"
        with tempfile.TemporaryDirectory() as folder:
            marker = Path(folder) / "started"
            err = io.StringIO()
            with contextlib.redirect_stderr(err):
                code = module.main(["limit-run", "100", "10", "/usr/bin/touch", str(marker)])
            self.assertEqual(code, module.REFUSED)
            self.assertFalse(marker.exists())
        self.assertIn("cannot read process memory", err.getvalue())


if __name__ == "__main__":
    unittest.main()
