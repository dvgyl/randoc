"""Nested Codex runs of the workers: the role table, the command line, the prompts and the run records.

Reviews and adviser questions do not run here: the broker starts them as
top-level read-only sessions (opslib/broker.py). A worker must write, so it
stays a nested run inside the lead's sandbox.
"""

from __future__ import annotations

import ctypes
import ctypes.util
import json
import os
import shutil
import signal
import string
import subprocess
from dataclasses import dataclass
from pathlib import Path

from .core import Context, OpsError, Refusal, utc_stamp

TEMPLATES = {"worker": "worker.md", "review": "review.md", "adviser": "adviser.md"}


@dataclass(frozen=True)
class Role:
    model: str
    efforts: tuple[str, ...]  # the default first


def role(ctx: Context, name: str) -> Role:
    """A row of the table in AGENTS.md section 3, the only place with model names."""
    manual = ctx.root / "AGENTS.md"
    table = ctx.guard.role_table(manual.read_text(encoding="utf-8")) if manual.is_file() else {}
    row = table.get(name)
    if not row:
        raise OpsError(f"{manual} has no row for the role {name!r} in the table of section 3")
    return Role(str(row["model"]), tuple(row["efforts"]))


def worker_setting(ctx: Context, effort: str | None = None) -> tuple[str, str]:
    """The model and the effort of a worker run: the worker row of AGENTS.md, its first effort by default."""
    worker = role(ctx, "worker")
    effort = effort or worker.efforts[0]
    if effort not in worker.efforts:
        raise Refusal(
            f"the effort {effort!r} is not in the worker row of AGENTS.md ({', '.join(worker.efforts)})",
            f"use one of: {', '.join(worker.efforts)}",
        )
    return worker.model, effort


def worker_command(workdir: Path, final: Path, setting: tuple[str, str]) -> list[str]:
    """The nested call. The outer sandbox of the lead still applies to it and to its children."""
    codex = shutil.which("codex")
    if codex is None:
        raise OpsError("the codex command is not on PATH")
    model, effort = setting
    return [
        codex, "exec",
        "--dangerously-bypass-approvals-and-sandbox",
        "-m", model,
        "-c", f"model_reasoning_effort={effort}",
        "-c", "service_tier=default",
        "-C", str(workdir),
        "-o", str(final),
        "--json",
        "-",
    ]


def render(ctx: Context, template: str, values: dict[str, object]) -> str:
    path = ctx.autobuild / "prompts" / TEMPLATES[template]
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
    started = property(lambda self: self.path / "started")
    exit = property(lambda self: self.path / "exit")

    def prepare(self, argv: list[str], prompt: str) -> None:
        self.prompt.write_text(prompt, encoding="utf-8")
        self.argv.write_text(json.dumps(argv, indent=1) + "\n", encoding="utf-8")


def start_background(argv: list[str], run: RunDir, cwd: Path) -> int:
    """Start in a new session and return at once. A shell records the exit code at the end."""
    env = dict(os.environ, OPS_EXIT_FILE=str(run.exit))
    wrapper = ["/bin/sh", "-c", '"$@"; echo $? > "$OPS_EXIT_FILE"', "ops-run", *argv]
    with run.prompt.open("rb") as prompt, run.events.open("wb") as out, run.stderr.open("wb") as err:
        child = subprocess.Popen(
            wrapper, stdin=prompt, stdout=out, stderr=err, cwd=cwd, env=env, start_new_session=True
        )
    run.pid.write_text(f"{child.pid}\n", encoding="utf-8")
    started = process_start(child.pid)
    if started is not None:
        run.started.write_text(f"{started}\n", encoding="utf-8")
    return child.pid


def process_times(pid: int) -> tuple[int, int] | None:
    """The start and exit time of a process (proc_pid_rusage, macOS), or None when they cannot be read.

    The exit time is above 0 for a zombie that its parent did not collect yet. ps is
    refused in the sandbox; proc_pid_rusage works on the user's own processes."""
    name = ctypes.util.find_library("proc")
    if pid <= 0 or name is None:
        return None
    try:
        info = ctypes.create_string_buffer(256)
        if ctypes.CDLL(name, use_errno=True).proc_pid_rusage(pid, 0, info) != 0:
            return None
    except (OSError, AttributeError):
        return None
    # rusage_info_v0: ri_proc_start_abstime at byte 80, ri_proc_exit_abstime at byte 88.
    return int.from_bytes(info.raw[80:88], "little"), int.from_bytes(info.raw[88:96], "little")


def process_start(pid: int) -> int | None:
    times = process_times(pid)
    return times[0] if times else None


def alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def recorded_start(run: RunDir) -> int | None:
    text = run.started.read_text().strip() if run.started.exists() else ""
    return int(text) if text.isdigit() else None


def same_process(run: RunDir, pid: int) -> bool | None:
    """True: `pid` is the running process of the run. False: it is not. None: it cannot be told."""
    if not alive(pid):
        return False
    started, times = recorded_start(run), process_times(pid)
    if started is None or times is None:
        return None
    return times == (started, 0)


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
    # A pid that another process took after the run ended is not the run.
    if pid is not None and same_process(run, pid) is not False:
        return RunState("running", f"pid {pid}", pid=pid)
    return RunState("lost", "the process ended with no exit code")


def stop(run: RunDir, pid: int) -> bool:
    """Stop the process group of a background run (the shell and its Codex child).

    It sends the signal only when the pid and the start time are those of the run."""
    if same_process(run, pid) is not True:
        return False
    try:
        os.killpg(pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    return True
