"""A temporary repository with the Git guard installed, for the guard and ops tests."""

from __future__ import annotations

import contextlib
import io
import os
import shutil
import stat
import subprocess
import sys
import tempfile
from pathlib import Path
from unittest import mock

AUTOBUILD = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(AUTOBUILD))

ITEMS_HEADER = "id\tmilestone\tstatus\tmetric\tbaseline\ttarget\ttitle\n"
ROWS_HEADER = "utc\tcommit\tdirty\titem\tmetric\tpass\ttotal\ttool\tevidence\n"
HOOKS = ("guard.py", "pre-commit", "commit-msg", "pre-push", "reference-transaction")

FAKE_CODEX = """#!/usr/bin/env python3
import json, os, sys, time
args = sys.argv[1:]
prompt = sys.stdin.read()
with open(os.environ["FAKE_CODEX_LOG"], "a") as log:
    log.write(json.dumps({"argv": args, "prompt": prompt, "cwd": os.getcwd()}) + "\\n")
time.sleep(float(os.environ.get("FAKE_CODEX_SLEEP", "0")))
with open(args[args.index("-o") + 1], "w") as out:
    out.write(os.environ.get("FAKE_CODEX_FINAL", "done\\n"))
print(json.dumps({"type": "thread.started", "thread_id": "thread-123"}))
sys.exit(int(os.environ.get("FAKE_CODEX_EXIT", "0")))
"""


def fast_temp() -> str | None:
    """The per-user temporary folder of macOS (internal disk; the sandbox allows it), else TMPDIR.

    TMPDIR of the build is on the exFAT drive, where each Git command of these tests is slow."""
    try:
        folder = subprocess.run(["getconf", "DARWIN_USER_TEMP_DIR"], capture_output=True, text=True).stdout.strip()
    except OSError:
        return None
    return folder if folder and os.access(folder, os.W_OK) else None


def git(cwd: Path, *args: str, env: dict | None = None) -> subprocess.CompletedProcess:
    return subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True,
                          env=None if env is None else {**os.environ, **env})


def row(utc: str, commit: str, item: str, metric: str, passed: int, total: int, dirty: str = "no") -> str:
    return f"{utc}\t{commit}\t{dirty}\t{item}\t{metric}\t{passed}\t{total}\ttest@1\t/runs/x\n"


def item_line(item_id: str, status: str, metric: str = "harness.units", baseline: str = "-", target: str = "+2") -> str:
    return f"{item_id}\tM1\t{status}\t{metric}\t{baseline}\t{target}\ttitle of {item_id}\n"


class TempRepo:
    """gpt/build in a temporary folder, with the guard of this source tree as core.hooksPath."""

    def __init__(self) -> None:
        self.tmp = Path(tempfile.mkdtemp(prefix="ops-test-", dir=fast_temp())).resolve()
        self.root = self.tmp / "repo"
        self.gpt = self.tmp / "gpt"
        (self.gpt / "wt").mkdir(parents=True)
        (self.gpt / "runs").mkdir()
        hooks = self.tmp / "hooks"
        hooks.mkdir()
        for name in HOOKS:
            shutil.copy(AUTOBUILD / "hooks" / name, hooks / name)
            (hooks / name).chmod(0o755)
        self.root.mkdir()
        self.git("init", "-q", "-b", "gpt/build")
        self.git("config", "user.name", "Test")
        self.git("config", "user.email", "test@example.invalid")
        self.git("config", "commit.gpgsign", "false")
        self.git("config", "core.hooksPath", str(hooks))
        self.write(".gitignore", "._*\n__pycache__/\n")
        self.write("plan/ITEMS.tsv", ITEMS_HEADER)
        self.write("plan/scoreboard/build.tsv", ROWS_HEADER)
        shutil.copy(AUTOBUILD.parents[1] / "plan" / ".gitattributes", self.root / "plan" / ".gitattributes")
        for name in ("worker.md", "review.md", "adviser.md"):
            self.write(f"tools/autobuild/prompts/{name}", (AUTOBUILD / "prompts" / name).read_text())
        self.commit("chore: start the test repository [ops]", [".gitignore", "plan", "tools"])
        self.bin = self.tmp / "bin"
        self.bin.mkdir()
        codex = self.bin / "codex"
        codex.write_text(FAKE_CODEX)
        codex.chmod(codex.stat().st_mode | stat.S_IXUSR)
        self.codex_log = self.tmp / "codex.log"

    def cleanup(self) -> None:
        shutil.rmtree(self.tmp, ignore_errors=True)

    def git(self, *args: str, env: dict | None = None, cwd: Path | None = None) -> subprocess.CompletedProcess:
        return git(cwd or self.root, *args, env=env)

    def out(self, *args: str) -> str:
        result = self.git(*args)
        assert result.returncode == 0, result.stderr
        return result.stdout.strip()

    def head(self) -> str:
        return self.out("rev-parse", "HEAD")

    def write(self, rel: str, text: str, cwd: Path | None = None) -> None:
        path = (cwd or self.root) / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)

    def append(self, rel: str, text: str, cwd: Path | None = None) -> None:
        with ((cwd or self.root) / rel).open("a") as handle:
            handle.write(text)

    def read(self, rel: str) -> str:
        return (self.root / rel).read_text()

    def commit(self, subject: str, paths: list[str] | None = None, cwd: Path | None = None,
               env: dict | None = None) -> subprocess.CompletedProcess:
        self.git("add", "--", *(paths or ["."]), cwd=cwd)
        return self.git("commit", "-q", "-m", subject, cwd=cwd, env=env)

    def must_commit(self, subject: str, paths: list[str] | None = None, cwd: Path | None = None) -> str:
        result = self.commit(subject, paths, cwd)
        assert result.returncode == 0, f"commit refused: {result.stderr}"
        return self.out("rev-parse", "HEAD") if cwd is None else git(cwd, "rev-parse", "HEAD").stdout.strip()

    def env(self, **extra: str) -> dict[str, str]:
        return {
            "RANDOC_GPT": str(self.gpt), "RANDOC_RUNS": str(self.gpt / "runs"),
            "PATH": f"{self.bin}{os.pathsep}{os.environ.get('PATH', '')}",
            "FAKE_CODEX_LOG": str(self.codex_log), **extra,
        }

    def ops(self, *args: str, cwd: Path | None = None, **env: str) -> tuple[int, str, str]:
        """Run ops in this process, in the repository (or a worktree), with the test environment."""
        from opslib import cli

        out, err = io.StringIO(), io.StringIO()
        old = os.getcwd()
        os.chdir(cwd or self.root)
        try:
            with mock.patch.dict(os.environ, self.env(**env)), contextlib.redirect_stdout(out), \
                    contextlib.redirect_stderr(err):
                code = cli.main(list(args))
        finally:
            os.chdir(old)
        return code, out.getvalue(), err.getvalue()

    def codex_calls(self) -> list[dict]:
        import json

        if not self.codex_log.exists():
            return []
        return [json.loads(line) for line in self.codex_log.read_text().splitlines()]
