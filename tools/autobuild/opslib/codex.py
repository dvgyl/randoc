"""Nested Codex runs: the role table, the command line, the prompts and the run records."""

from __future__ import annotations

import json
import os
import shutil
import signal
import string
import subprocess
from dataclasses import dataclass
from pathlib import Path

from .core import Context, OpsError, utc_stamp


@dataclass(frozen=True)
class Role:
    model: str
    effort: str
    ephemeral: bool
    template: str


# The one place in ops that names models. AGENTS.md has the same table for the readers.
ROLES = {
    "worker": Role("gpt-6.2-sol", "high", False, "worker.md"),
    "review": Role("gpt-6.2-sol", "high", True, "review.md"),
    "adviser": Role("gpt-6.2-astra", "xhigh", True, "adviser.md"),
}


def command(role_name: str, workdir: Path, final: Path) -> list[str]:
    """The nested call. The outer sandbox of the lead still applies to it and to its children."""
    role = ROLES[role_name]
    codex = shutil.which("codex")
    if codex is None:
        raise OpsError("the codex command is not on PATH")
    argv = [
        codex, "exec",
        "--dangerously-bypass-approvals-and-sandbox",
        "-m", role.model,
        "-c", f"model_reasoning_effort={role.effort}",
        "-c", "service_tier=default",
        "-C", str(workdir),
        "-o", str(final),
        "--json",
    ]
    if role.ephemeral:
        argv.append("--ephemeral")
    argv.append("-")
    return argv


def render(ctx: Context, role_name: str, values: dict[str, object]) -> str:
    path = ctx.autobuild / "prompts" / ROLES[role_name].template
    if not path.is_file():
        raise OpsError(f"the prompt template {path} is missing")
    return string.Template(path.read_text(encoding="utf-8")).substitute(
        {key: str(value) for key, value in values.items()}
    )


@dataclass(frozen=True)
class RunDir:
    """The record of one nested run, under $RANDOC_RUNS."""

    path: Path

    @classmethod
    def new(cls, parent: Path, name: str = "") -> "RunDir":
        path = parent / (utc_stamp() + (f"-{name}" if name else ""))
        suffix = 1
        while path.exists():
            suffix += 1
            path = parent / f"{utc_stamp()}-{name or 'run'}-{suffix}"
        path.mkdir(parents=True)
        return cls(path)

    prompt = property(lambda self: self.path / "prompt.md")
    events = property(lambda self: self.path / "events.jsonl")
    stderr = property(lambda self: self.path / "stderr.log")
    final = property(lambda self: self.path / "final.md")
    argv = property(lambda self: self.path / "argv.json")
    pid = property(lambda self: self.path / "pid")
    exit = property(lambda self: self.path / "exit")

    def prepare(self, argv: list[str], prompt: str) -> None:
        self.prompt.write_text(prompt, encoding="utf-8")
        self.argv.write_text(json.dumps(argv, indent=1) + "\n", encoding="utf-8")

    def session(self) -> str:
        """The thread id from the JSON events, or '-'."""
        if not self.events.exists():
            return "-"
        for line in self.events.read_text(encoding="utf-8", errors="replace").splitlines():
            try:
                event = json.loads(line)
            except ValueError:
                continue
            if isinstance(event, dict) and event.get("type") == "thread.started":
                return str(event.get("thread_id") or "-")
        return "-"

    def final_text(self) -> str:
        return self.final.read_text(encoding="utf-8", errors="replace") if self.final.exists() else ""


def run_sync(argv: list[str], run: RunDir, cwd: Path) -> int:
    """Run and wait. The events and the errors go to the run folder."""
    with run.prompt.open("rb") as prompt, run.events.open("wb") as out, run.stderr.open("wb") as err:
        code = subprocess.run(argv, stdin=prompt, stdout=out, stderr=err, cwd=cwd).returncode
    run.exit.write_text(f"{code}\n", encoding="utf-8")
    return code


def start_background(argv: list[str], run: RunDir, cwd: Path) -> int:
    """Start in a new session and return at once. A shell records the exit code at the end."""
    env = dict(os.environ, OPS_EXIT_FILE=str(run.exit))
    wrapper = ["/bin/sh", "-c", '"$@"; echo $? > "$OPS_EXIT_FILE"', "ops-run", *argv]
    with run.prompt.open("rb") as prompt, run.events.open("wb") as out, run.stderr.open("wb") as err:
        child = subprocess.Popen(
            wrapper, stdin=prompt, stdout=out, stderr=err, cwd=cwd, env=env, start_new_session=True
        )
    run.pid.write_text(f"{child.pid}\n", encoding="utf-8")
    return child.pid


def alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


@dataclass(frozen=True)
class RunState:
    name: str  # none | running | finished | lost
    detail: str
    pid: int | None = None
    code: int | None = None


def state(run: RunDir | None) -> RunState:
    if run is None:
        return RunState("none", "never run")
    if run.exit.exists():
        text = run.exit.read_text().strip()
        code = int(text) if text.lstrip("-").isdigit() else None
        return RunState("finished", f"exit {text}", code=code)
    pid_text = run.pid.read_text().strip() if run.pid.exists() else ""
    pid = int(pid_text) if pid_text.isdigit() else None
    if pid is not None and alive(pid):
        return RunState("running", f"pid {pid}", pid=pid)
    return RunState("lost", "the process ended with no exit code")


def stop(pid: int) -> None:
    """Stop the process group of a background run (the shell and its Codex child)."""
    try:
        os.killpg(pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
