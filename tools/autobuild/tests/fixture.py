"""A temporary repository with the Git guard installed, for the guard and ops tests."""

from __future__ import annotations

import contextlib
import io
import json
import os
import shutil
import stat
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path
from unittest import mock

AUTOBUILD = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(AUTOBUILD))

ITEMS_HEADER = "id\tmilestone\tstatus\tmetric\tbaseline\ttarget\ttitle\taccept\n"
ROWS_HEADER = "utc\tcommit\tdirty\titem\tmetric\tpass\ttotal\ttool\tevidence\n"
# The registry of the test repository: one active suite, so a full run lists ["harness"].
REGISTRY = """[[metric]]
key = "harness.units"
suite = "harness"
status = "active"
command = "c"
pass = "p"
total = "t"
"""
HOOKS = ("guard.py", "experiments.py", "pre-commit", "commit-msg", "pre-push", "reference-transaction")
# The role table of AGENTS.md section 3, with model ids that are not real.
ROLE_TABLE = """# Test manual

## 3. Models and effort

| Role | Model id | Reasoning effort | Use |
| --- | --- | --- | --- |
| Lead model | `test-lead` | `high` | The lead. |
| Worker | `test-worker` | `high` (`low` for routine edits) | One item. |
| Reviewer | `test-reviewer` | `high` | One review round. |
| Adviser model | `test-adviser` | `xhigh` | One question. |

## 4. Hard limits
"""

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


def item_line(item_id: str, status: str, metric: str = "harness.units", baseline: str = "-", target: str = "+2",
              accept: str = "-") -> str:
    return f"{item_id}\tM1\t{status}\t{metric}\t{baseline}\t{target}\ttitle of {item_id}\t{accept}\n"


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
        shutil.copy(AUTOBUILD / "licence_scan.py", hooks / "licence_scan.py")
        self.hooks = hooks
        self.root.mkdir()
        self.git("init", "-q", "-b", "gpt/build")
        self.git("config", "user.name", "Test")
        self.git("config", "user.email", "test@example.invalid")
        self.git("config", "commit.gpgsign", "false")
        self.git("config", "core.hooksPath", str(hooks))
        self.write(".gitignore", "._*\n__pycache__/\n")
        self.write("plan/ITEMS.tsv", ITEMS_HEADER)
        self.write("plan/scoreboard/build.tsv", ROWS_HEADER)
        self.write("AGENTS.md", ROLE_TABLE)
        self.write("tools/autobuild/metrics.toml", REGISTRY)
        shutil.copy(AUTOBUILD.parents[1] / "plan" / ".gitattributes", self.root / "plan" / ".gitattributes")
        for name in ("worker.md", "review.md", "adviser.md"):
            self.write(f"tools/autobuild/prompts/{name}", (AUTOBUILD / "prompts" / name).read_text())
        self.commit("chore: start the test repository [ops]", [".gitignore", "AGENTS.md", "plan", "tools"])
        self.bin = self.tmp / "bin"
        self.bin.mkdir()
        codex = self.bin / "codex"
        codex.write_text(FAKE_CODEX)
        codex.chmod(codex.stat().st_mode | stat.S_IXUSR)
        self.codex_log = self.tmp / "codex.log"

    def cleanup(self) -> None:
        shutil.rmtree(self.tmp, ignore_errors=True)

    def git(self, *args: str, env: dict | None = None, cwd: Path | None = None) -> subprocess.CompletedProcess:
        # The guard reads the attempt records of $RANDOC_RUNS: the test folder, never the build's.
        return git(cwd or self.root, *args, env={"RANDOC_RUNS": str(self.gpt / "runs"), **(env or {})})

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

    def full_run(self, utc: str, commit: str, counts: dict[str, tuple[int, int]],
                 suites: tuple[str, ...] = ("harness",), name: str | None = None) -> str:
        """The rows of a completed full run of `commit` (item -, clean tree), with the run.json that
        measure.py writes in its evidence folder."""
        folder = self.gpt / "runs" / "measure" / (name or f"{utc.replace(':', '')}-{commit[:12]}")
        folder.mkdir(parents=True, exist_ok=True)
        (folder / "run.json").write_text(json.dumps({
            "utc": utc, "end": utc, "commit": commit, "dirty": "no", "item": "-", "stream": "build",
            "suites": list(suites), "rows": len(counts)}))
        return "".join(row(utc, commit, "-", metric, passed, total).replace("/runs/x", str(folder / "harness.json"))
                       for metric, (passed, total) in counts.items())

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
        if not self.codex_log.exists():
            return []
        return [json.loads(line) for line in self.codex_log.read_text().splitlines()]


class FakeBroker:
    """Answers each request of the inbox with `answer(kind, request)`, as the support daemon does."""

    def __init__(self, repo: TempRepo, out: Path, results: Path, answer) -> None:
        self.inbox = repo.gpt / "broker" / "requests"
        self.out, self.results, self.answer = out, results, answer
        self.requests: list[dict] = []
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self.serve, daemon=True)
        self.thread.start()

    def serve(self) -> None:
        while not self.stop.is_set():
            for path in sorted(self.inbox.glob("*.json")) if self.inbox.is_dir() else []:
                data = json.loads(path.read_text())
                path.unlink()
                self.requests.append(data)
                reply = self.answer(data["kind"], data)
                if reply is not None:
                    folder = self.results if data["kind"] == "evaluate" else self.out
                    (folder / path.name).write_text(json.dumps({"request": path.stem, "kind": data["kind"], **reply}))
            time.sleep(0.02)

    def close(self) -> None:
        self.stop.set()
        self.thread.join()


def fake_broker(case, repo: TempRepo, answer) -> FakeBroker:
    """A FakeBroker for one test, with ops pointed at its answer folders and a fresh heartbeat."""
    from opslib import broker

    out, results = repo.tmp / "broker-out", repo.tmp / "holdout-results"
    out.mkdir()
    results.mkdir()
    (out / "heartbeat.json").write_text(json.dumps({"evaluation": {"state": "idle"}}))
    for name, value in (("OUT", out), ("RESULTS", results), ("POLL_SECONDS", 0.02)):
        patch = mock.patch.object(broker, name, value)
        patch.start()
        case.addCleanup(patch.stop)
    fake = FakeBroker(repo, out, results, answer)
    case.addCleanup(fake.close)
    return fake
