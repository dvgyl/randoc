"""The workers: one worktree and one branch gpt/w/ID per item, and the state of its runs."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from . import codex
from .core import BUILD_BRANCH, WORKER_PREFIX, Context, Refusal

MAX_WORKERS = 4
MIN_FREE_BYTES = 25 * 10**9


@dataclass(frozen=True)
class Worker:
    id: str
    path: Path
    present: bool
    run: codex.RunState
    abandoned: str
    own: int
    behind: int

    @property
    def phase(self) -> str:
        return "abandoned" if self.abandoned else {"none": "idle"}.get(self.run.name, self.run.name)

    def line(self) -> str:
        where = str(self.path) if self.present else f"{self.path} (folder missing)"
        detail = self.abandoned if self.abandoned else self.run.detail
        return (
            f"{self.id}: {self.phase} ({detail}); {self.own} own commits; "
            f"gpt/build moved {self.behind} commits since its start; {where}"
        )


def records(ctx: Context, item_id: str) -> Path:
    return ctx.runs / "workers" / item_id


def newest_run(ctx: Context, item_id: str) -> codex.RunDir | None:
    folder = records(ctx, item_id)
    runs = sorted(p for p in folder.iterdir() if p.is_dir()) if folder.is_dir() else []
    return codex.RunDir(runs[-1]) if runs else None


def abandoned_reason(ctx: Context, item_id: str) -> str:
    marker = records(ctx, item_id) / "abandoned"
    return marker.read_text(encoding="utf-8").strip() if marker.exists() else ""


def mark_abandoned(ctx: Context, item_id: str, text: str) -> None:
    folder = records(ctx, item_id)
    folder.mkdir(parents=True, exist_ok=True)
    (folder / "abandoned").write_text(text.strip() + "\n", encoding="utf-8")


def list_workers(ctx: Context) -> list[Worker]:
    out = ctx.git.run("worktree", "list", "--porcelain", check=False)
    workers = []
    for block in out.split("\n\n"):
        fields = dict(line.split(" ", 1) for line in block.splitlines() if " " in line)
        branch = fields.get("branch", "")
        prefix = "refs/heads/" + WORKER_PREFIX
        if not branch.startswith(prefix):
            continue
        item_id = branch[len(prefix):]
        path = Path(fields["worktree"])
        fork = ctx.git.run("merge-base", BUILD_BRANCH, branch, check=False).strip()
        own = ctx.git.count(fork, branch) if fork else 0
        behind = ctx.git.count(fork, BUILD_BRANCH) if fork else 0
        workers.append(Worker(
            item_id, path, path.is_dir(), codex.state(newest_run(ctx, item_id)),
            abandoned_reason(ctx, item_id), own, behind,
        ))
    return workers


def find(ctx: Context, item_id: str) -> Worker:
    worker = next((w for w in list_workers(ctx) if w.id == item_id), None)
    if worker is None:
        raise Refusal(
            f"item {item_id} has no worker worktree",
            f"make it with `tools/autobuild/ops worker start {item_id}`",
        )
    return worker
