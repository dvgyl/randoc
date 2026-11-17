"""Shared parts of ops: paths, Git, refusals, time stamps, text checks and safe deletes."""

from __future__ import annotations

import datetime as dt
import importlib.util
import os
import re
import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path
from types import ModuleType

GPT_DEFAULT = "/Volumes/Extreme Pro/Randoc/gpt"
BUILD_BRANCH = "gpt/build"
WORKER_PREFIX = "gpt/w/"
ITEM_ID = re.compile(r"^M(\d+)-(\d{2,})$")


class Refusal(Exception):
    """A rule stops the command. It names the rule and the next allowed action."""

    def __init__(self, rule: str, next_action: str):
        super().__init__(rule)
        self.rule = rule
        self.next_action = next_action


class OpsError(Exception):
    """The command cannot run: a missing file or a failed Git command."""


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def utc_stamp() -> str:
    """A UTC time for file names. exFAT does not allow ':' in a name."""
    return dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ")


class Git:
    def __init__(self, root: Path):
        self.root = root

    def run(self, *args: str, check: bool = True, stdin: str | None = None) -> str:
        result = subprocess.run(
            ["git", *args], cwd=self.root, input=stdin, capture_output=True, text=True
        )
        if check and result.returncode != 0:
            message = (result.stderr.strip() or result.stdout.strip()).splitlines()
            raise OpsError(f"git {' '.join(args)} failed: {' / '.join(message[-6:])}")
        return result.stdout

    def ok(self, *args: str) -> bool:
        return subprocess.run(["git", *args], cwd=self.root, capture_output=True).returncode == 0

    def rev(self, rev: str) -> str | None:
        out = self.run("rev-parse", "-q", "--verify", f"{rev}^{{commit}}", check=False).strip()
        return out or None

    def branch(self) -> str:
        """The short branch name, or '' for a detached HEAD."""
        return self.run("symbolic-ref", "-q", "--short", "HEAD", check=False).strip()

    def dirty_tracked(self) -> list[str]:
        out = self.run("status", "--porcelain", "--untracked-files=no", "-z", check=False)
        return [entry[3:] for entry in out.split("\0") if len(entry) > 3]

    def is_ancestor(self, old: str, new: str) -> bool:
        return self.ok("merge-base", "--is-ancestor", old, new)

    def count(self, old: str, new: str) -> int:
        return int(self.run("rev-list", "--count", f"{old}..{new}").strip())


@dataclass(frozen=True)
class Context:
    root: Path
    gpt: Path
    runs: Path
    git: Git
    guard: ModuleType

    @property
    def plan(self) -> Path:
        return self.root / "plan"

    @property
    def items_file(self) -> Path:
        return self.plan / "ITEMS.tsv"

    @property
    def scoreboard(self) -> Path:
        return self.plan / "scoreboard"

    @property
    def reviews_file(self) -> Path:
        return self.plan / "REVIEWS.tsv"

    @property
    def advice_file(self) -> Path:
        return self.plan / "ADVICE.md"

    @property
    def log_file(self) -> Path:
        return self.plan / "REVIEW_LOG.md"

    @property
    def blocked_file(self) -> Path:
        return self.plan / "BLOCKED_ON_OWNER.md"

    @property
    def milestones_file(self) -> Path:
        return self.plan / "milestones.toml"

    @property
    def autobuild(self) -> Path:
        return self.root / "tools" / "autobuild"

    @property
    def wt(self) -> Path:
        return self.gpt / "wt"

    @property
    def queue(self) -> Path:
        return self.gpt / "holdout-queue"

    def rel(self, path: Path) -> str:
        return str(path.relative_to(self.root))

    def worker_item(self) -> str | None:
        """The item of a worker branch (gpt/w/ID), or None on any other branch."""
        branch = self.git.branch()
        return branch[len(WORKER_PREFIX):] if branch.startswith(WORKER_PREFIX) else None


def load_context(cwd: Path | None = None) -> Context:
    top = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"], cwd=cwd, capture_output=True, text=True
    )
    if top.returncode != 0:
        raise OpsError("run ops inside the build clone or inside a worker worktree")
    root = Path(top.stdout.strip())
    gpt = Path(os.environ.get("RANDOC_GPT") or GPT_DEFAULT)
    runs = Path(os.environ.get("RANDOC_RUNS") or gpt / "runs")
    git = Git(root)
    return Context(root, gpt, runs, git, load_guard(root, git))


def guard_paths(root: Path, git: Git) -> tuple[Path | None, Path]:
    """The active guard (from core.hooksPath) and the source copy in the repository."""
    hooks = git.run("config", "--get", "core.hooksPath", check=False).strip()
    active = root / Path(hooks).expanduser() / "guard.py" if hooks else None
    return (active if active and active.is_file() else None), root / "tools/autobuild/hooks/guard.py"


def load_guard(root: Path, git: Git) -> ModuleType:
    """Load the guard that Git runs, so that the counters of ops agree with the hooks."""
    active, source = guard_paths(root, git)
    path = active or source
    spec = importlib.util.spec_from_file_location("ops_guard", path)
    if spec is None or spec.loader is None or not path.is_file():
        raise OpsError(f"the Git guard {path} is missing")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def check_line(ctx: Context, text: str, what: str) -> str:
    """Refuse text that would break a TSV row or that the guard refuses."""
    text = text.strip()
    if not text:
        raise Refusal(f"{what} is empty", f"give {what} as one line of text")
    if "\t" in text or "\n" in text:
        raise Refusal(f"{what} must be one line with no tab", f"write {what} again as one line")
    check_words(ctx, text, what)
    return text


def check_words(ctx: Context, text: str, what: str) -> None:
    hits = ctx.guard.banned_hits(text)
    if hits:
        raise Refusal(
            f"{what} uses a banned phrase ({', '.join(hits)}); the guard refuses it",
            f"write {what} again without that phrase",
        )


def append_text(path: Path, text: str) -> None:
    """Append text to a file. Add a line end first when the file does not end with one."""
    path.parent.mkdir(parents=True, exist_ok=True)
    old = path.read_text(encoding="utf-8") if path.exists() else ""
    prefix = "" if not old or old.endswith("\n") else "\n"
    with path.open("a", encoding="utf-8") as handle:
        handle.write(prefix + text)


def stage(ctx: Context, *paths: Path) -> None:
    ctx.git.run("add", "--", *(ctx.rel(p) for p in paths))


def remove_tree(ctx: Context, path: Path) -> bool:
    """Delete a folder or file, only under gpt/wt/ or gpt/runs/ (full resolved path)."""
    target = Path(path).resolve()
    roots = [(ctx.gpt / "wt").resolve(), (ctx.gpt / "runs").resolve()]
    if not any(target != root and target.is_relative_to(root) for root in roots):
        raise Refusal(
            f"delete refused: {target} is not under {roots[0]}/ or {roots[1]}/",
            "ops deletes only worker and run folders; leave this path as it is",
        )
    if not os.path.lexists(target):
        return False
    if target.is_dir():
        shutil.rmtree(target)
    else:
        target.unlink()
    return True
