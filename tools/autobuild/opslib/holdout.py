"""The held-out evaluation: a request names a commit of gpt/build; the broker builds and evaluates it.

The broker writes each result to broker.RESULTS, which the build can read but
not write. A result counts only when it is complete: status done, the full
commit id, the full SHA-256 of the binary that the broker evaluated, and both
sets with 0 <= PASS <= TOTAL and TOTAL > 0. It counts for a milestone gate only
when the broker built the binary itself (binary_provenance "built"); an older
result with a supplied binary stays for the history. The totals are those of the
development part; the broker keeps the reserved FINAL part for the later review.
"""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

from . import broker
from .core import BUILD_BRANCH, Context, Refusal, append_text, stage, utc_now
from .plan import ROW_FIELDS

SETS = ("corpus", "generated")
SHA1 = re.compile(r"^[0-9a-f]{40}$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")


def heartbeat_line(ctx: Context) -> str:
    return broker.heartbeat_line()


def request(ctx: Context, args: argparse.Namespace) -> int:
    """One evaluation of HEAD, or of --commit SHA (for example the candidate of a deferred request)."""
    if args.commit:
        head = ctx.git.rev(args.commit)
        if head is None:
            raise Refusal(f"{args.commit} is not a commit", f"give --commit SHA of a commit of {BUILD_BRANCH}")
    else:
        head = ctx.git.rev("HEAD")
        dirty = ctx.git.dirty_tracked()
        if head is None or dirty:
            raise Refusal(
                f"a request names one commit; tracked files have changes ({', '.join(dirty[:3])})",
                "commit or restore them, then request; or name a commit with --commit SHA",
            )
    if not ctx.git.is_ancestor(head, f"refs/heads/{BUILD_BRANCH}"):
        raise Refusal(f"{head[:12]} is not on {BUILD_BRANCH}; the broker builds only commits of {BUILD_BRANCH}",
                      f"name a commit of {BUILD_BRANCH}")
    broker.require_running()
    folder = broker.inbox(ctx)
    for old in sorted(folder.glob("*.json")) if folder.is_dir() else []:
        try:
            if json.loads(old.read_text(encoding="utf-8")).get("commit") == head:
                print(f"a request for {head[:12]} waits already: {old}")
                return 0
        except ValueError:
            continue
    request_id = broker.send(ctx, "evaluate", {"commit": head})
    print(f"requested one held-out evaluation of {head[:12]} (request {request_id}); the broker builds it first")
    print(broker.heartbeat_line())
    print("next: continue; `ops holdout results` shows the answer (done, deferred or rejected)")
    return 0


def problems(data: dict) -> list[str]:
    """Why a result does not count. An empty list: it counts."""
    found = []
    if data.get("status") != "done":
        found.append(f"status {data.get('status')!r}")
    if not SHA1.match(str(data.get("commit", ""))):
        found.append("no full commit id")
    if not SHA256.match(str(data.get("binary_sha256", ""))):
        found.append("no full binary SHA-256")
    sets = data.get("sets") if isinstance(data.get("sets"), dict) else {}
    for name in SETS:
        node = sets.get(name) if isinstance(sets.get(name), dict) else {}
        passed, total = node.get("pass"), node.get("total")
        if not (type(passed) is int and type(total) is int and 0 <= passed <= total and total > 0):
            found.append(f"no valid counts for the set {name}")
    return found


def load_results() -> list[tuple[Path, dict]]:
    found = []
    for path in sorted(broker.RESULTS.glob("*.json")) if broker.RESULTS.is_dir() else []:
        if path.name.startswith("._"):
            continue
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
        except ValueError:
            continue
        if isinstance(data, dict):
            found.append((path, data))
    found.sort(key=lambda pair: (str(pair[1].get("utc", "")), pair[0].name))
    return found


def set_totals(data: dict) -> dict[str, tuple[int, int]]:
    return {name: (data["sets"][name]["pass"], data["sets"][name]["total"]) for name in SETS}


def newest(ctx: Context) -> tuple[Path, dict] | None:
    done = [pair for pair in load_results() if not problems(pair[1])]
    return done[-1] if done else None


def counts_for_gate(data: dict) -> bool:
    return not problems(data) and data.get("binary_provenance") == "built"


def matching(ctx: Context, candidate: str) -> tuple[Path, dict] | None:
    """The newest result that counts for a gate and measures the candidate commit, or a later commit of
    HEAD whose diff from the candidate changes only plan/ (plan/SPEC.md 4.1)."""
    found = None
    for path, data in load_results():
        commit = str(data.get("commit", ""))
        if (counts_for_gate(data) and ctx.git.rev(commit) and ctx.git.is_ancestor(commit, "HEAD")
                and ctx.git.only_plan_changes(candidate, commit)):
            found = (path, data)
    return found


def summary(ctx: Context) -> str:
    found = newest(ctx)
    if found is None:
        return "held-out: no complete result yet"
    path, data = found
    parts = ", ".join(f"{name} {p}/{t}" for name, (p, t) in set_totals(data).items())
    provenance = "" if counts_for_gate(data) else \
        f", binary {data.get('binary_provenance', 'of unknown origin')}: history only, not valid for a gate"
    return f"held-out: {parts} at {data['commit'][:12]} ({data.get('utc', path.name)}{provenance})"


def results(ctx: Context, args: argparse.Namespace) -> int:
    print(broker.heartbeat_line())
    every = load_results()
    if every and every[-1][1].get("status") != "done":
        data = every[-1][1]
        print(f"newest answer: {data.get('status')} ({data.get('reason') or data.get('next_run') or '-'})")
    for path, data in every:
        if data.get("status") == "done" and problems(data):
            print(f"not counted: {path.name} ({'; '.join(problems(data))})")
    print(summary(ctx))
    found = newest(ctx)
    if found is None or not args.record:
        return 0
    if ctx.git.branch() != BUILD_BRANCH:
        raise Refusal(f"held-out rows go to plan/scoreboard/build.tsv, only on {BUILD_BRANCH}", f"run it on {BUILD_BRANCH}")
    path, data = found
    board = ctx.scoreboard / "build.tsv"
    if board.exists() and str(path) in board.read_text(encoding="utf-8"):
        print(f"the rows of {path.name} are in plan/scoreboard/build.tsv already")
        return 0
    if not board.exists() or not board.read_text(encoding="utf-8").strip():
        append_text(board, "\t".join(ROW_FIELDS) + "\n")
    tool = f"holdout@{data['binary_sha256'][:12]}"
    for name, (p, t) in set_totals(data).items():
        row = [str(data.get("utc") or utc_now()), data["commit"], "no", "-", f"holdout.{name}", str(p), str(t), tool, str(path)]
        append_text(board, "\t".join(row) + "\n")
    stage(ctx, board)
    print("rows written to plan/scoreboard/build.tsv (staged)")
    return 0
