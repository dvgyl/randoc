"""The worker commands: start, run, merge, drop and list."""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
import time
from pathlib import Path

from . import codex, fleet
from .core import BUILD_BRANCH, WORKER_PREFIX, Context, OpsError, Refusal, remove_tree
from .items import remeasure_item, require_lead, start_item
from .plan import ItemsTable, progress, read_rows


def start(ctx: Context, args: argparse.Namespace) -> int:
    require_lead(ctx)
    table = ItemsTable.load(ctx.items_file)
    item = table.need(args.id)
    workers = [w for w in fleet.list_workers(ctx) if w.present]
    if any(w.id == item.id for w in workers):
        print(f"worker {item.id} exists: {ctx.wt / item.id}")
        return 0
    if len(workers) >= fleet.MAX_WORKERS:
        raise Refusal(
            f"{len(workers)} workers exist (limit {fleet.MAX_WORKERS})",
            "merge or drop a worker first (`ops worker list`)",
        )
    free = shutil.disk_usage(ctx.gpt).free
    if free < fleet.MIN_FREE_BYTES:
        raise Refusal(
            f"{free / 1e9:.0f} GB free on the drive (a new worker needs {fleet.MIN_FREE_BYTES / 1e9:.0f} GB)",
            "drop a finished worker (`ops worker drop ID`); if that is not enough, write it with `ops blocked`",
        )
    if item.status != "active":
        start_item(ctx, table, item)
    path = ctx.wt / item.id
    branch = WORKER_PREFIX + item.id
    ctx.wt.mkdir(parents=True, exist_ok=True)
    ctx.git.run("worktree", "prune")
    if ctx.git.rev(f"refs/heads/{branch}"):
        ctx.git.run("worktree", "add", "-q", str(path), branch)
    else:
        ctx.git.run("worktree", "add", "-q", "-b", branch, str(path), BUILD_BRANCH)
    print(f"worker {item.id}: worktree {path} on {branch}")
    print(f"next: `tools/autobuild/ops worker run {item.id}`")
    return 0


def run(ctx: Context, args: argparse.Namespace) -> int:
    require_lead(ctx)
    worker = fleet.find(ctx, args.id)
    if not worker.present:
        raise Refusal(f"the worktree {worker.path} is missing", f"drop it (`ops worker drop {args.id}`) and start it again")
    if worker.abandoned:
        raise Refusal(f"worker {args.id} is abandoned ({worker.abandoned})", f"drop it: `ops worker drop {args.id}`")
    if worker.run.name == "running":
        raise Refusal(f"worker {args.id} runs ({worker.run.detail})", "wait; `ops worker list` shows the state")
    item = ItemsTable.load(ctx.items_file).need(args.id)
    values = {
        "item": item.id, "title": item.title, "metric": item.metric, "baseline": item.baseline,
        "target": item.target, "worktree": worker.path, "branch": WORKER_PREFIX + item.id,
    }
    prompt = codex.render(ctx, "worker", values)
    setting = codex.worker_setting(ctx, getattr(args, "effort", None))
    record = codex.RunDir.new(fleet.records(ctx, item.id))
    argv = codex.worker_command(worker.path, record.final, setting)
    record.prepare(argv, prompt)
    pid = codex.start_background(argv, record, worker.path)
    print(f"worker {item.id} runs in the background: pid {pid}, {setting[0]} at effort {setting[1]}")
    print(f"record: {record.path}")
    print("next: continue your own item; `ops worker list` shows when the worker ends")
    return 0


def worker_rows(ctx: Context, item_id: str) -> list[dict[str, str]]:
    """The committed rows of the worker branch, with the rows of this branch for the baseline."""
    text = ctx.git.run("show", f"{WORKER_PREFIX}{item_id}:plan/scoreboard/{item_id}.tsv", check=False)
    rows = ctx.guard.tsv_rows(text)
    for row in rows:
        row["stream"] = item_id
    return sorted(read_rows(ctx) + rows, key=lambda r: r.get("utc", ""))


def worktree_changes(ctx: Context, worker: fleet.Worker) -> list[str]:
    """Tracked and untracked changes in the worker's worktree (macOS companion files do not count)."""
    out = ctx.git.run("-C", str(worker.path), "status", "--porcelain=v1", "-z", "--untracked-files=all", check=False)
    return [entry[3:] for entry in out.split("\0") if len(entry) > 3 and not Path(entry[3:]).name.startswith("._")]


def registered(ctx: Context, item_id: str) -> fleet.Worker:
    """The worktree gpt/wt/ID that Git has registered on branch gpt/w/ID, or a refusal.

    ops removes no other folder: a folder that Git does not know, or a worktree on
    another branch, stays for a person to look at."""
    path = ctx.wt / item_id
    worker = next((w for w in fleet.list_workers(ctx) if w.id == item_id), None)
    if worker is None or worker.path.resolve() != path.resolve():
        raise Refusal(
            f"{path} is not a registered worktree on branch {WORKER_PREFIX}{item_id}; ops does not remove it",
            "look at the folder (`git worktree list`); remove it by hand only when nothing in it is needed",
        )
    return worker


def check_removable(ctx: Context, item_id: str) -> fleet.Worker:
    """A registered worktree with no tracked or untracked changes, or a refusal (uncommitted work stays)."""
    worker = registered(ctx, item_id)
    changes = worktree_changes(ctx, worker) if worker.present else []
    if changes:
        raise Refusal(
            f"the worktree {worker.path} has changes that are not committed ({', '.join(changes[:3])})",
            f"commit them on {WORKER_PREFIX}{item_id} (with [{item_id}]) or remove them, then run the command again",
        )
    return worker


def merge(ctx: Context, args: argparse.Namespace) -> int:
    require_lead(ctx)
    worker = fleet.find(ctx, args.id)
    if worker.abandoned:
        raise Refusal(
            f"worker {args.id} is abandoned ({worker.abandoned}); the lead never merges it",
            f"`ops worker drop {args.id}`, then `ops item cut {args.id} --reason TEXT`",
        )
    if worker.run.name == "running":
        raise Refusal(f"worker {args.id} still runs ({worker.run.detail})", "wait until `ops worker list` shows it finished")
    check_removable(ctx, args.id)
    dirty = ctx.git.dirty_tracked()
    if dirty:
        raise Refusal(f"tracked files have changes ({', '.join(dirty[:3])})", "commit or restore them first")
    item = ItemsTable.load(ctx.items_file).need(args.id)
    state = progress(ctx, item, worker_rows(ctx, args.id))
    if not state.gained:
        raise Refusal(
            f"the committed rows of gpt/w/{args.id} show no gain on {item.metric} (baseline {item.baseline})",
            f"`ops worker drop {args.id}`, then `ops item cut {args.id} --reason TEXT`",
        )
    gate = ctx.autobuild / "gate.py"
    if not gate.is_file():
        raise OpsError(f"{ctx.rel(gate)} does not exist yet, so the merged tree cannot be checked")
    base = ctx.git.run("rev-parse", "HEAD").strip()
    tip = ctx.git.run("rev-parse", WORKER_PREFIX + args.id).strip()
    merged = subprocess.run(
        ["git", "merge-tree", "--write-tree", "--name-only", base, tip], cwd=ctx.root, capture_output=True, text=True
    )
    if merged.returncode != 0:
        conflicts = [l for l in merged.stdout.splitlines()[1:] if l.strip()][:5]
        raise Refusal(
            f"gpt/w/{args.id} conflicts with {BUILD_BRANCH} ({', '.join(conflicts) or merged.stderr.strip()})",
            f"in the worker worktree, merge {BUILD_BRANCH} and resolve the conflicts, then run `ops worker merge {args.id}` again",
        )
    tree = merged.stdout.split()[0]
    subject = f"chore(merge): merge gpt/w/{args.id}: {item.title} [{args.id}]"
    body = f"Gain on {item.metric}: {state.gain:+d} (baseline {item.baseline})." if state.gain is not None else ""
    commit = ctx.git.run("commit-tree", tree, "-p", base, "-p", tip, "-m", subject, "-m", body or "Worker merge.").strip()
    ctx.git.run("checkout", "-q", "--detach", commit)
    try:
        print(f"gate on the merged tree {commit[:12]} (base {base[:12]})", flush=True)
        code = subprocess.run([sys.executable, str(gate), "--base", base], cwd=ctx.root).returncode
        gained = code == 0 and remeasure_item(ctx, args.id)
    finally:
        ctx.git.run("checkout", "-q", BUILD_BRANCH)
    if code != 0:
        raise Refusal(
            f"the gate failed on the merged tree (exit {code}); {BUILD_BRANCH} did not change",
            f"fix the cause on gpt/w/{args.id} (`ops worker run {args.id}` or by hand), then merge again",
        )
    if not gained:
        raise Refusal(
            f"the merged tree shows no gain on {item.metric} (baseline {item.baseline}); {BUILD_BRANCH} did not change",
            f"commit the new rows of plan/scoreboard/build.tsv, then fix gpt/w/{args.id} or drop and cut it",
        )
    ctx.git.run("merge", "-q", "--ff-only", commit)
    check_removable(ctx, args.id)
    remove_tree(ctx, worker.path)
    ctx.git.run("worktree", "prune")
    print(f"merged gpt/w/{args.id} into {BUILD_BRANCH} with --no-ff ({commit[:12]}); the worktree is removed")
    print(f"next: `tools/autobuild/ops item done {args.id} --weak-spots TEXT`")
    return 0


def drop(ctx: Context, args: argparse.Namespace) -> int:
    require_lead(ctx)
    if not (ctx.wt / args.id).exists() and not any(w.id == args.id for w in fleet.list_workers(ctx)):
        ctx.git.run("worktree", "prune")
        print(f"worker {args.id} does not exist; nothing to drop")
        return 0
    worker = registered(ctx, args.id)
    if worker.run.name == "running":
        if not args.stop:
            raise Refusal(
                f"worker {args.id} runs ({worker.run.detail})",
                f"wait, or stop it and drop it: `ops worker drop {args.id} --stop`",
            )
        run = fleet.newest_run(ctx, args.id)
        if run is None or not codex.stop(run, worker.run.pid or 0):
            raise Refusal(
                f"the pid {worker.run.pid} cannot be shown to be the worker's process (no matching start time)",
                "stop the worker by hand, then drop it",
            )
        for _ in range(20):
            if codex.state(run).name != "running":
                break
            time.sleep(0.5)
        else:
            raise Refusal(f"worker {args.id} still runs 10 s after the stop signal; the worktree stays",
                          f"wait, then run `ops worker drop {args.id}` again")
    check_removable(ctx, args.id)
    remove_tree(ctx, worker.path)
    ctx.git.run("worktree", "prune")
    print(f"dropped worker {args.id}; the branch gpt/w/{args.id} stays for the review")
    return 0


def list_cmd(ctx: Context, args: argparse.Namespace) -> int:
    workers = fleet.list_workers(ctx)
    print(f"workers: {sum(w.present for w in workers)}/{fleet.MAX_WORKERS}")
    for worker in workers:
        print(f"  {worker.line()}")
    return 0
