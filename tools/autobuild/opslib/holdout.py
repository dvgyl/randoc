"""The held-out queue: requests for one evaluation, results with totals only, the heartbeat."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import shutil
import time
from pathlib import Path

from .core import BUILD_BRANCH, Context, Refusal, append_text, stage, utc_now, utc_stamp
from .plan import ROW_FIELDS

HEARTBEAT_LIMIT_S = 600
SETS = ("corpus", "generated")


def default_binary(ctx: Context) -> Path | None:
    target = Path(os.environ.get("CARGO_TARGET_DIR") or ctx.root / "target")
    found = [p for p in (target / "release" / "randoc", target / "debug" / "randoc") if p.is_file()]
    return max(found, key=lambda p: p.stat().st_mtime) if found else None


def heartbeat_age(ctx: Context) -> float | None:
    beat = ctx.queue / "heartbeat.json"
    return time.time() - beat.stat().st_mtime if beat.exists() else None


def heartbeat_line(ctx: Context) -> str:
    age = heartbeat_age(ctx)
    if age is None:
        return "evaluator: no heartbeat file; the evaluator does not run"
    state = "runs" if age <= HEARTBEAT_LIMIT_S else "does not run (write it with `ops blocked`)"
    return f"evaluator: heartbeat {age / 60:.0f} min old; the evaluator {state}"


def request(ctx: Context, args: argparse.Namespace) -> int:
    head = ctx.git.rev("HEAD")
    dirty = ctx.git.dirty_tracked()
    if head is None or dirty:
        raise Refusal(
            f"the binary must belong to one commit; tracked files have changes ({', '.join(dirty[:3])})",
            "commit or restore them, build again, then request",
        )
    binary = Path(args.binary).resolve() if args.binary else default_binary(ctx)
    if binary is None or not binary.is_file():
        raise Refusal("no Randoc binary was found", "build it (`cargo build -p randoc-cli`) or give `--binary PATH`")
    requests = ctx.queue / "requests"
    requests.mkdir(parents=True, exist_ok=True)
    for old in sorted(requests.glob("*.json")):
        try:
            if json.loads(old.read_text()).get("commit") == head:
                print(f"a request for {head[:12]} waits already: {old}")
                return 0
        except ValueError:
            continue
    built = dt.datetime.fromtimestamp(binary.stat().st_mtime, dt.timezone.utc)
    committed = dt.datetime.fromtimestamp(int(ctx.git.run("log", "-1", "--format=%ct").strip()), dt.timezone.utc)
    stamp = f"{utc_stamp()}-{head[:12]}"
    copy = ctx.runs / "holdout" / stamp / "randoc"
    copy.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(binary, copy)
    temporary = requests / f".{stamp}.json.part"
    temporary.write_text(json.dumps({"commit": head, "binary": str(copy)}) + "\n", encoding="utf-8")
    temporary.rename(requests / f"{stamp}.json")
    print(f"requested one held-out evaluation of {head[:12]} with {copy}")
    if built < committed:
        print(f"warning: the binary ({built:%Y-%m-%d %H:%M}Z) is older than HEAD ({committed:%Y-%m-%d %H:%M}Z); "
              "check that it holds the code of HEAD")
    print(heartbeat_line(ctx))
    print("next: continue; `ops holdout results` shows the totals when the evaluator is done")
    return 0


def totals(node: object) -> tuple[int, int] | None:
    """PASS and TOTAL of a set: its own numbers, or the sum of its targets or families."""
    if not isinstance(node, dict):
        return None
    if isinstance(node.get("pass"), int) and isinstance(node.get("total"), int):
        return node["pass"], node["total"]
    parts = [t for t in (totals(v) for v in node.values()) if t]
    return (sum(p for p, _ in parts), sum(t for _, t in parts)) if parts else None


def load_results(ctx: Context) -> list[tuple[Path, dict]]:
    folder = ctx.queue / "results"
    found = []
    for path in sorted(folder.glob("*.json")) if folder.is_dir() else []:
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


def is_deferred(data: dict) -> bool:
    return "deferred" in (str(data.get("status", "")), str(data.get("state", ""))) or data.get("deferred") is True


def set_totals(data: dict) -> dict[str, tuple[int, int]]:
    sets = data.get("sets") if isinstance(data.get("sets"), dict) else data
    return {name: t for name in SETS if (t := totals(sets.get(name)))}


def newest(ctx: Context) -> tuple[Path, dict] | None:
    done = [pair for pair in load_results(ctx) if not is_deferred(pair[1])]
    return done[-1] if done else None


def summary(ctx: Context) -> str:
    found = newest(ctx)
    if found is None:
        return "held-out: no result yet"
    path, data = found
    parts = ", ".join(f"{name} {p}/{t}" for name, (p, t) in set_totals(data).items()) or "no totals"
    return f"held-out: {parts} at {str(data.get('commit', '?'))[:12]} ({data.get('utc', path.name)})"


def results(ctx: Context, args: argparse.Namespace) -> int:
    print(heartbeat_line(ctx))
    every = load_results(ctx)
    if every and is_deferred(every[-1][1]):
        data = every[-1][1]
        print(f"newest request: deferred (next run allowed: {data.get('next_run') or data.get('next_allowed') or '?'})")
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
    tool = "holdout@" + str(data.get("binary_sha256") or data.get("binary_hash") or hashlib.sha256(path.read_bytes()).hexdigest())[:12]
    for name, (p, t) in set_totals(data).items():
        row = [str(data.get("utc") or utc_now()), str(data.get("commit", "-")), "no", "-", f"holdout.{name}", str(p), str(t), tool, str(path)]
        append_text(board, "\t".join(row) + "\n")
    stage(ctx, board)
    print("rows written to plan/scoreboard/build.tsv (staged)")
    return 0
