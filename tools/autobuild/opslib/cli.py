"""The ops command line."""

from __future__ import annotations

import argparse
import os
import subprocess
import sys

from . import corrections, holdout, items, report, talk, workers
from .core import Context, OpsError, Refusal, load_context
from .notes import ASK_REASONS, LOG_KINDS

PASS_THROUGH = {"measure": "measure.py", "gate": "gate.py"}


def parser() -> argparse.ArgumentParser:
    top = argparse.ArgumentParser(prog="ops", description="Operations of the build on the gpt/* branches.")
    sub = top.add_subparsers(dest="command", required=True)

    sub.add_parser("status", help="counters and the next required action").set_defaults(func=report.status)

    item = sub.add_parser("item", help="work items in plan/ITEMS.tsv").add_subparsers(dest="action", required=True)
    p = item.add_parser("add", help="add a queued item")
    p.add_argument("id")
    p.add_argument("--milestone", required=True)
    p.add_argument("--metric", required=True)
    p.add_argument("--target", required=True, help="expected gain in PASS units, for example +12")
    p.add_argument("--title", required=True)
    rule = p.add_mutually_exclusive_group()
    rule.add_argument("--all-pass", action="store_true", help="a gain needs PASS = TOTAL above 0 in the latest row")
    rule.add_argument("--min-total", type=int, metavar="N", help="a gain needs TOTAL >= N in the latest row")
    p.set_defaults(func=items.add)
    p = item.add_parser("start", help="make the item active and record its baseline")
    p.add_argument("id")
    p.set_defaults(func=items.start)
    p = item.add_parser("cut", help="cut the item and revert its commits")
    p.add_argument("id")
    p.add_argument("--reason", required=True)
    p.set_defaults(func=items.cut)
    p = item.add_parser("done", help="close an item that shows a gain, after a passing gate")
    p.add_argument("id")
    p.add_argument("--weak-spots", required=True, help="the known weak spots, or 'none known'")
    p.set_defaults(func=items.done)
    item.add_parser("list", help="all items").set_defaults(func=items.list_items)

    sub.add_parser("measure", help="run tools/autobuild/measure.py with the same arguments")
    sub.add_parser("gate", help="run tools/autobuild/gate.py with the same arguments")

    p = sub.add_parser("next", help="clusters of failing units from the newest evidence files")
    p.add_argument("--limit", type=int, default=12)
    p.set_defaults(func=report.next_items)

    worker = sub.add_parser("worker", help="worker worktrees").add_subparsers(dest="action", required=True)
    for name, func, text in (
        ("start", workers.start, "make the worktree gpt/wt/ID on branch gpt/w/ID"),
        ("run", workers.run, "start the worker model in the background"),
        ("merge", workers.merge, "gate the merged tree, then merge with --no-ff"),
        ("drop", workers.drop, "remove the worktree; the branch stays"),
    ):
        p = worker.add_parser(name, help=text)
        p.add_argument("id")
        if name == "run":
            p.add_argument("--effort", choices=("low", "high"),
                           help="reasoning effort of the worker: low for a routine item (default: the table of AGENTS.md)")
        if name == "drop":
            p.add_argument("--stop", action="store_true", help="stop a running worker first")
        p.set_defaults(func=func)
    worker.add_parser("list", help="the workers and their state").set_defaults(func=workers.list_cmd)

    p = sub.add_parser("ask", help="one question to the adviser model when stuck")
    p.add_argument("question", nargs="?")
    p.add_argument("--reason", choices=ASK_REASONS)
    p.add_argument("--item")
    p.add_argument("--evidence", help="the record of the stuck state (needed for two-fixes-failed, acceptance-failed and no-next-step)")
    p.add_argument("--decision", help="record the decision for the newest answer")
    p.set_defaults(func=talk.ask)

    p = sub.add_parser("review", help="one review round in a new session (at most 2)")
    p.add_argument("id")
    p.add_argument("--commit")
    p.set_defaults(func=talk.review)

    held = sub.add_parser("holdout", help="the held-out evaluation").add_subparsers(dest="action", required=True)
    p = held.add_parser("request", help="ask for one evaluation of HEAD or of --commit SHA")
    p.add_argument("--commit", help="a commit of gpt/build, for example the candidate of a deferred request")
    p.set_defaults(func=holdout.request)
    p = held.add_parser("results", help="the newest totals and the heartbeat")
    p.add_argument("--record", action="store_true", help="write the totals as rows of plan/scoreboard/build.tsv")
    p.set_defaults(func=holdout.results)

    p = sub.add_parser("log", help="append an entry to plan/REVIEW_LOG.md")
    p.add_argument("kind", choices=LOG_KINDS)
    p.add_argument("text")
    p.add_argument("--item")
    p.set_defaults(func=talk.log)

    p = sub.add_parser("blocked", help="append a blocker that only the owner can remove")
    p.add_argument("text", help="what is blocked")
    p.add_argument("--owner-must", help="what the owner must do")
    p.add_argument("--meanwhile", help="what the team does in the meantime")
    p.add_argument("--item", help="also set this item to blocked")
    p.set_defaults(func=talk.blocked)

    mile = sub.add_parser("milestone", help="milestone gates").add_subparsers(dest="action", required=True)
    p = mile.add_parser("check", help="compare the newest clean full build run with plan/milestones.toml")
    p.add_argument("id")
    p.set_defaults(func=report.milestone_check)

    fix = sub.add_parser("correction", help="tool corrections in plan/corrections.tsv").add_subparsers(dest="action", required=True)
    p = fix.add_parser("add", help="record that a tool now forms or counts the units of a metric in another way")
    p.add_argument("metric")
    p.add_argument("--item", required=True, help="the active item that changes the tool")
    p.add_argument("--reason", required=True)
    p.set_defaults(func=corrections.add)

    sub.add_parser("push", help="ask the broker to push gpt/build").set_defaults(func=push)
    return top


def push(ctx: Context, args: argparse.Namespace) -> int:
    try:
        from . import broker
    except ModuleNotFoundError as error:
        if error.name != f"{__package__}.broker":
            raise
        raise OpsError("opslib/broker.py is not installed in this tree, so `ops push` cannot run") from error
    return broker.push(ctx)


def pass_through(ctx: Context, name: str, rest: list[str]) -> int:
    tool = ctx.autobuild / PASS_THROUGH[name]
    if not tool.is_file():
        raise OpsError(f"{ctx.rel(tool)} does not exist yet; the {name} tool is not installed in this tree")
    return subprocess.run([sys.executable, str(tool), *rest], cwd=ctx.root).returncode


def main(argv: list[str] | None = None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    if sys.version_info < (3, 11):
        print("ops needs Python 3.11 or later", file=sys.stderr)
        return 1
    try:
        if argv and argv[0] in PASS_THROUGH:
            ctx = load_context()
            return pass_through(ctx, argv[0], argv[1:])
        args = parser().parse_args(argv)
        ctx = load_context()
        os.chdir(ctx.root)
        return args.func(ctx, args)
    except Refusal as refusal:
        print(f"refused: {refusal.rule}\nnext: {refusal.next_action}", file=sys.stderr)
        return 1
    except OpsError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
