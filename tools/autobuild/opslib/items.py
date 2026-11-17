"""The item commands: add, start, cut, done and list."""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

from . import fleet
from .core import BUILD_BRANCH, ITEM_ID, Context, OpsError, Refusal, check_line, check_words, stage
from .notes import log_entry
from .plan import (
    Item, ItemsTable, commit_transition, lead_active, moved, newest_row, predict_commit, progress,
    read_rows, scoreboard_changes,
)

METRIC_KEY = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._+-]*$")


def require_lead(ctx: Context) -> None:
    branch = ctx.git.branch()
    if branch != BUILD_BRANCH:
        raise Refusal(
            f"only the lead changes plan/ITEMS.tsv, and only on {BUILD_BRANCH} (this is {branch or 'a detached HEAD'})",
            f"run the command in the lead's clone on {BUILD_BRANCH}"
            + ("; a worker writes its result in its final message" if branch.startswith("gpt/w/") else ""),
        )


def worker_ids(ctx: Context) -> set[str]:
    return {w.id for w in fleet.list_workers(ctx) if w.present}


def add(ctx: Context, args: argparse.Namespace) -> int:
    require_lead(ctx)
    match = ITEM_ID.match(args.id)
    if not match:
        raise Refusal(f"{args.id!r} is not an item id", "use M<milestone>-<nn>, for example M1-07")
    milestone = "M" + args.milestone.lstrip("Mm")
    if milestone != f"M{match.group(1)}":
        raise Refusal(f"item {args.id} does not belong to milestone {milestone}", f"use --milestone M{match.group(1)}")
    if not METRIC_KEY.match(args.metric):
        raise Refusal(f"{args.metric!r} is not a metric key", "use one key of plan/SPEC.md, for example harness.units")
    if not re.match(r"^\+\d+$", args.target):
        raise Refusal(f"{args.target!r} is not a target", "give the expected gain in PASS units, for example +12")
    title = check_line(ctx, args.title, "the title")
    table = ItemsTable.load(ctx.items_file)
    if table.get(args.id):
        raise Refusal(f"item {args.id} exists; an id is never used again", "choose the next free number")
    table.add(Item(args.id, milestone, "queued", args.metric, "-", args.target, title))
    ctx.items_file.parent.mkdir(parents=True, exist_ok=True)
    ctx.items_file.write_text(table.text(), encoding="utf-8")
    stage(ctx, ctx.items_file)
    print(f"added {args.id} (queued): {args.metric} {args.target} {title}")
    print(f"plan/ITEMS.tsv is staged; `tools/autobuild/ops item start {args.id}` commits it")
    return 0


def start_item(ctx: Context, table: ItemsTable, item: Item) -> Item:
    """Make the item active with its baseline and commit plan/ITEMS.tsv."""
    row = newest_row(read_rows(ctx), item.metric, stream="build")
    if row is None or int(row["total"]) == 0:
        baseline, note = "0/0", f"Baseline: 0/0 of {item.metric}: the metric has no row yet (ROADMAP rule 7)."
    else:
        baseline = f"{row['pass']}/{row['total']}"
        note = f"Baseline: {baseline} of {item.metric}, measured at {row['commit']} ({row['utc']})."
    started = moved(item, "start", baseline=baseline)
    table.put(started)
    commit_transition(ctx, table.items(), f"chore(plan): start item {item.id} [ops]", note, {ctx.items_file: table.text()})
    print(f"started {item.id}: {note}")
    return started


def start(ctx: Context, args: argparse.Namespace) -> int:
    require_lead(ctx)
    table = ItemsTable.load(ctx.items_file)
    item = table.need(args.id)
    others = [i for i in lead_active(ctx, table.items(), worker_ids(ctx)) if i.id != item.id]
    if others:
        raise Refusal(
            f"the lead already has an active item on this branch ({others[0].id})",
            f"finish it (`ops item done {others[0].id} --weak-spots TEXT`) or cut it "
            f"(`ops item cut {others[0].id} --reason TEXT`), or give {item.id} to a worker",
        )
    start_item(ctx, table, item)
    print(f"next: change the code, commit with [{item.id}], then `tools/autobuild/ops measure --item {item.id} --quick`")
    return 0


def done(ctx: Context, args: argparse.Namespace) -> int:
    require_lead(ctx)
    table = ItemsTable.load(ctx.items_file)
    item = table.need(args.id)
    finished = moved(item, "done")
    weak = args.weak_spots.strip()
    check_words(ctx, weak, "--weak-spots")
    state = progress(ctx, item, read_rows(ctx))
    if not state.gained:
        worker = item.id in worker_ids(ctx)
        raise Refusal(
            f"item {item.id} shows no gain on {item.metric} (baseline {item.baseline}); done needs a gain",
            f"merge its worker first (`ops worker merge {item.id}`)" if worker else
            f"run the next experiment (`ops measure --item {item.id} --quick`) or cut it (`ops item cut {item.id} --reason TEXT`)",
        )
    table.put(finished)
    gain = f"+{state.gain}" if item.base() != (0, 0) else f"the metric now has rows (best PASS {state.gain})"
    text = f"Item {item.id} done. Gain on {item.metric}: {gain} (baseline {item.baseline}).\n\nWeak spots: {weak or 'none known'}"
    log = (ctx.log_file.read_text(encoding="utf-8") if ctx.log_file.exists() else "") + log_entry(ctx, "weak-spot", item.id, text)
    commit_transition(ctx, table.items(), f"chore(plan): finish item {item.id} [ops]", text,
                      {ctx.items_file: table.text(), ctx.log_file: log})
    print(f"done {item.id}: gain {gain}")
    print("next: run the gate (`tools/autobuild/ops gate`), then push gpt/build")
    return 0


def item_commits(ctx: Context, item_id: str) -> list[tuple[str, bool, str]]:
    """Commits on the first-parent history of HEAD with the tag [ID] that change more than plan/."""
    out = ctx.git.run("log", "--first-parent", "--format=%H%x1f%P%x1f%s", "HEAD", check=False)
    found = []
    for line in out.splitlines():
        sha, parents, subject = line.split("\x1f", 2)
        if item_id not in ctx.guard.TAG.findall(subject):
            continue
        first = parents.split()[0] if parents else None
        names = ctx.git.run(*(["diff", "--name-only", first, sha] if first else ["show", "--format=", "--name-only", sha])).split()
        if names and all(n.startswith("plan/") for n in names):
            continue
        found.append((sha, len(parents.split()) > 1, subject))
    return found


def cut(ctx: Context, args: argparse.Namespace) -> int:
    reason = check_line(ctx, args.reason, "--reason")
    worker_item = ctx.worker_item()
    if worker_item is not None:
        if args.id != worker_item:
            raise Refusal(f"a worker cuts only its own item ({worker_item})", f"run `ops item cut {worker_item} --reason TEXT`")
        fleet.mark_abandoned(ctx, args.id, f"cut by the worker: {reason}")
        print(f"branch gpt/w/{args.id} is abandoned; the lead never merges it")
        print("next: stop now and write the reason and what you learned in your final message")
        return 0
    require_lead(ctx)
    table = ItemsTable.load(ctx.items_file)
    item = table.need(args.id)
    state = progress(ctx, item, read_rows(ctx))
    table.put(moved(item, "cut"))
    board = scoreboard_changes(ctx)
    predict_commit(ctx, table.items(), touches_scoreboard=bool(board))
    commits = item_commits(ctx, args.id)
    listed = "".join(f"\n- {sha[:12]} {subject}" for sha, _, subject in commits)
    text = (f"Item {item.id} cut after {state.experiments} experiments with no gain on {item.metric}. "
            f"Reason: {reason}" + (f"\n\nReverted commits:{listed}" if commits else ""))
    log = (ctx.log_file.read_text(encoding="utf-8") if ctx.log_file.exists() else "") + log_entry(ctx, "cut", item.id, text)
    if not commits:
        commit_transition(ctx, table.items(), f"chore(plan): cut item {item.id} [ops]", text,
                          {ctx.items_file: table.text(), ctx.log_file: log})
    else:
        dirty = [p for p in ctx.git.dirty_tracked() if not p.startswith("plan/")]
        if dirty:
            raise Refusal(
                f"tracked files have changes ({', '.join(dirty[:3])}); the revert needs a clean tree",
                f"commit them with [{item.id}], or restore them with `git restore PATH`",
            )
        revert_and_cut(ctx, item.id, commits, text, {ctx.items_file: table.text(), ctx.log_file: log, **board})
    if args.id in worker_ids(ctx):
        fleet.mark_abandoned(ctx, args.id, f"cut by the lead: {reason}")
    print(f"cut {item.id}: " + (f"reverted {len(commits)} commits in one commit{listed}" if commits
                                else "no commit of the item changes code; plan/ITEMS.tsv is committed"))
    return 0


def revert_and_cut(ctx: Context, item_id: str, commits: list[tuple[str, bool, str]], text: str,
                   plan_files: dict[Path, str]) -> None:
    """One commit: the reverts of the item's commits (newest first) and the new plan files.

    The plan files keep their working state (append-only notes and rows); only code is reverted."""
    out = ctx.git.run("status", "--porcelain", "--untracked-files=all", "--", "plan/", check=False)
    kept = {ctx.root / line[3:]: (ctx.root / line[3:]).read_bytes() for line in out.splitlines()
            if (ctx.root / line[3:]).is_file() and not Path(line[3:]).name.startswith("._")}

    def put_back_plan() -> None:
        for path, data in kept.items():
            path.write_bytes(data)
        if kept:
            stage(ctx, *kept)

    try:
        for sha, is_merge, _subject in commits:
            ctx.git.run("revert", "--no-commit", *(["-m", "1"] if is_merge else []), sha)
        ctx.git.run("restore", "--source=HEAD", "--staged", "--worktree", "--", "plan/")
        put_back_plan()
        for path, content in plan_files.items():
            path.write_text(content, encoding="utf-8")
        stage(ctx, *plan_files)
        ctx.git.run("commit", "-q", "-m", f"chore(plan): cut item {item_id} and revert its code [{item_id}]", "-m", text)
    except OpsError as error:
        ctx.git.run("reset", "-q", "--hard", "HEAD", check=False)
        ctx.git.run("revert", "--quit", check=False)
        put_back_plan()
        raise Refusal(
            f"the revert of item {item_id} failed, nothing changed: {error}",
            "revert the item's commits by hand (`git revert --no-commit SHA`, newest first), then run "
            "`ops item cut` again",
        ) from error


def remeasure_item(ctx: Context, item_id: str) -> bool:
    """Measure the item's metric on the checked-out tree and judge the gain with the guard's rule.

    `ops worker merge` calls it on the detached merge commit after the gate
    passed, so that the gain shows on the exact tree that gpt/build gets. The
    run measures only the suite of the metric, with `--item ID --stream build`:
    it is one experiment of the item, and its rows stay in
    plan/scoreboard/build.tsv for the next commit of the lead.
    """
    import measure

    from . import corrections

    item = ItemsTable.load(ctx.items_file).need(item_id)
    try:
        metric = measure.tree_registry(ctx.autobuild).get(item.metric)
    except measure.ToolError as error:
        raise OpsError(f"the metric registry cannot be read: {error}") from error
    if metric is None or metric.status != "active":
        print(f"re-measure of {item_id}: {item.metric} is not an active metric of this tree; no gain can be shown")
        return False
    head = ctx.git.rev("HEAD") or ""
    argv = [sys.executable, str(ctx.autobuild / "measure.py"), "--suite", metric.suite, "--item", item_id,
            "--stream", "build"]
    print(f"re-measure of {item_id} on {head[:12]}: {' '.join(argv[2:])}", flush=True)
    code = subprocess.run(argv, cwd=ctx.root).returncode
    if code == 2:
        print(f"re-measure of {item_id}: measure.py was refused or failed (exit 2); no gain can be shown")
        return False
    rows = [r for r in read_rows(ctx) if r["stream"] == "build" and r.get("commit") == head
            and r.get("item") == item_id and r.get("metric") == item.metric]
    if not rows:
        print(f"re-measure of {item_id}: the run wrote no row of {item.metric}; no gain can be shown")
        return False
    newest = rows[-1]
    gained = measure.experiments.gained([newest], item_id, item.metric, item.base(), corrections.load(ctx))
    print(f"re-measure of {item_id}: {item.metric} {newest['pass']}/{newest['total']} against the baseline "
          f"{item.baseline}: {'gain' if gained else 'no gain'}")
    return gained


def list_items(ctx: Context, args: argparse.Namespace) -> int:
    table = ItemsTable.load(ctx.items_file)
    rows = read_rows(ctx)
    items = table.items()
    if not items:
        print("no items in plan/ITEMS.tsv")
        return 0
    for item in items:
        line = f"{item.id}  {item.status:<7}  {item.metric}  baseline {item.baseline}  target {item.target}"
        if item.status == "active":
            p = progress(ctx, item, rows)
            gain = "none" if p.gain is None else f"{p.gain:+d}"
            line += f"  experiments {p.experiments}/{ctx.guard.MAX_TRIES}  gain {gain}"
        print(f"{line}  {item.title}")
    return 0

