"""The read commands: status (counters and the next required action), next and milestone check."""

from __future__ import annotations

import argparse
import json
import re
import shutil
import tomllib
from collections import defaultdict
from dataclasses import dataclass

from . import fleet, holdout, notes
from .core import BUILD_BRANCH, Context, OpsError, Refusal, guard_paths, utc_stamp
from .plan import (
    Item, ItemsTable, Progress, build_runs, commits_since_measured, cut_streak, lead_active, number,
    plan_dirty, progress, read_rows,
)

NO_GAIN_RUNS_LIMIT = 3
CUT_STREAK_LIMIT = 3


@dataclass(frozen=True)
class Facts:
    branch: str
    since: int
    items: list[Item]
    progress: dict[str, Progress]
    workers: list[fleet.Worker]
    advice: list[notes.Advice]
    plan_dirty: list[str]
    no_gain_runs: int
    cut_streak: int
    cut_area: str


def gather(ctx: Context) -> Facts:
    rows = read_rows(ctx)
    items = ItemsTable.load(ctx.items_file).items()
    runs = [r for r in build_runs(rows) if r.item == "-"]
    no_gain = 0
    for run in reversed(runs):
        if run.gained:
            break
        no_gain += 1
    streak, area = cut_streak(items)
    return Facts(
        ctx.git.branch(), commits_since_measured(ctx), items,
        {i.id: progress(ctx, i, rows) for i in items if i.status == "active"},
        fleet.list_workers(ctx), notes.advice_entries(ctx), plan_dirty(ctx), no_gain, streak, area,
    )


def next_action(ctx: Context, f: Facts) -> str:
    """The first rule that applies, in this order of priority."""
    guard = ctx.guard
    own = ctx.worker_item()
    if not f.branch.startswith("gpt/"):
        return f"`git checkout {BUILD_BRANCH}` (work happens only on gpt/* branches)"
    for item_id, p in f.progress.items():
        if (own is None or own == item_id) and p.experiments >= guard.MAX_TRIES and not p.gained:
            return (f"`tools/autobuild/ops item cut {item_id} --reason TEXT` ({p.experiments} experiments, no gain; "
                    "the cut commit also commits the new scoreboard rows)")
    scoreboard = [p for p in f.plan_dirty if p.startswith("plan/scoreboard/")]
    if scoreboard:
        return "commit the new scoreboard rows: `git add plan/scoreboard && git commit -m \"chore(plan): add scoreboard rows [ops]\"`"
    if f.since >= guard.MEASURE_LIMIT:
        return "`tools/autobuild/ops measure`, then commit the new scoreboard rows (the guard refuses other commits)"
    mine = {k: v for k, v in f.progress.items() if own is None or own == k}
    for item_id, p in mine.items():
        if p.total_changed and not p.gained:
            return (f"the TOTAL of the metric of {item_id} changed: measure its baseline commit again with the "
                    f"new TOTAL (the body of `git log -1 -F --grep 'start item {item_id} '` names it); ops then uses that row")
    if f.advice and not f.advice[-1].decided:
        return 'write the decision for the newest advice: `tools/autobuild/ops ask --decision "TEXT"`'
    asked = {a.event for a in f.advice}
    for item_id, p in mine.items():
        if p.stuck and notes.stuck_event(item_id, p.experiments, "") not in asked:
            return f'stuck (two rows, no gain): `tools/autobuild/ops ask --reason two-rows-no-gain --item {item_id} "QUESTION"`'
    if own is not None:
        p = f.progress.get(own)
        if p and p.gained:
            return "the metric rose: commit the rows, then stop and write the final message"
        return f"change, commit with [{own}], then `tools/autobuild/ops measure --item {own} --quick`"
    for w in f.workers:
        if w.abandoned:
            return f"`tools/autobuild/ops worker drop {w.id}`, then `tools/autobuild/ops item cut {w.id} --reason TEXT`"
        if w.run.name in ("finished", "lost"):
            return f"worker {w.id} ended ({w.run.detail}): `tools/autobuild/ops worker merge {w.id}`, or drop it"
    worker_ids = {w.id for w in f.workers if w.present}
    lead = lead_active(ctx, f.items, worker_ids)
    for item in lead:
        p = f.progress[item.id]
        # A 0/0 item makes its metric (ROADMAP rule 7): its first row with a TOTAL is the whole gain.
        if p.gained and (item.base() == (0, 0) or (p.gain or 0) >= item.target_gain()):
            return f"`tools/autobuild/ops item done {item.id} --weak-spots TEXT` (gain {p.gain:+d}, target {item.target})"
    if f.no_gain_runs >= NO_GAIN_RUNS_LIMIT:
        return f"{f.no_gain_runs} full measurements with no gain: stop this approach; `tools/autobuild/ops next` and take a cluster in another area"
    if f.cut_streak >= CUT_STREAK_LIMIT:
        return (f"{f.cut_streak} items cut in a row in {f.cut_area}: park the area (`ops log weak-spot`), ask one "
                "strategy question (`ops ask --reason no-next-step`), and move to another area")
    if lead:
        item = lead[0]
        return f"continue {item.id}: change, commit with [{item.id}], then `tools/autobuild/ops measure --item {item.id} --quick`"
    queued = [i for i in f.items if i.status == "queued"]
    if queued:
        return f"`tools/autobuild/ops item start {queued[0].id}`"
    return "`tools/autobuild/ops next`, then `tools/autobuild/ops item add ...` for the largest cluster"


def status(ctx: Context, args: argparse.Namespace) -> int:
    f = gather(ctx)
    guard = ctx.guard
    head = ctx.git.rev("HEAD")
    dirty = ctx.git.dirty_tracked()
    print(f"branch: {f.branch or '(detached HEAD)'} at {head[:12] if head else '(no commit)'}"
          + (f", {len(dirty)} changed tracked files" if dirty else ", clean"))
    print(f"measurement: {f.since} commits since the newest measured commit (limit {guard.MEASURE_LIMIT})")
    if f.plan_dirty:
        print(f"plan files not committed: {', '.join(f.plan_dirty)}")
    rows = read_rows(ctx)
    runs = build_runs(rows)
    gains = [r for r in runs if r.gained]
    build = ctx.git.rev(BUILD_BRANCH)
    if gains and build and ctx.git.rev(gains[-1].commit) and ctx.git.is_ancestor(gains[-1].commit, build):
        print(f"last gain on {BUILD_BRANCH}: {ctx.git.count(gains[-1].commit, build)} commits ago "
              f"({', '.join(gains[-1].gained[:4])})")
    else:
        print(f"last gain on {BUILD_BRANCH}: none recorded")
    print(f"full {BUILD_BRANCH} measurements with no gain in a row: {f.no_gain_runs} (limit {NO_GAIN_RUNS_LIMIT})")
    print(f"items cut in a row in one area: {f.cut_streak}{' (' + f.cut_area + ')' if f.cut_area else ''} (limit {CUT_STREAK_LIMIT})")
    worker_ids = {w.id for w in f.workers if w.present}
    active = [i for i in f.items if i.status == "active"]
    queued = sum(i.status == "queued" for i in f.items)
    print(f"items: {len(active)} active, {queued} queued, {len(f.items)} in all")
    for item in active:
        p = f.progress[item.id]
        owner = "worker" if item.id in worker_ids else "lead"
        gain = "none" if p.gain is None else f"{p.gain:+d}"
        flags = [s for s, on in (("stuck: two rows with no gain", p.stuck), ("TOTAL changed", p.total_changed)) if on]
        print(f"  {item.id} ({owner}) {item.metric} baseline {item.baseline} target {item.target}: "
              f"experiments {p.experiments}/{guard.MAX_TRIES}, gain {gain}, "
              f"reviews {len(notes.review_rows(ctx, item.id))}/2" + (f"; {'; '.join(flags)}" if flags else ""))
    print(f"workers: {len(worker_ids)}/{fleet.MAX_WORKERS}")
    for w in f.workers:
        print(f"  {w.line()}")
    free = shutil.disk_usage(ctx.gpt if ctx.gpt.exists() else ctx.root).free
    print(f"disk: {free / 1e9:.0f} GB free (a new worker needs {fleet.MIN_FREE_BYTES / 1e9:.0f} GB)")
    print(f"blocked on owner: {len(notes.open_blockers(ctx))} open entries (plan/BLOCKED_ON_OWNER.md)")
    print(holdout.summary(ctx))
    print(holdout.heartbeat_line(ctx))
    active_guard, source = guard_paths(ctx.root, ctx.git)
    if active_guard is None:
        print("guard: NOT INSTALLED (core.hooksPath has no guard.py); the owner runs tools/autobuild/install-guard")
    elif source.is_file() and active_guard.read_bytes() != source.read_bytes():
        print(f"guard: the active copy differs from {ctx.rel(source)}; the owner runs tools/autobuild/install-guard")
    print(f"next: {next_action(ctx, f)}")
    return 0


# ---- ops next -------------------------------------------------------------

CLUSTER_LIMIT = 40
MIN_GROUP = 3
SPLIT = re.compile(r"/|::|:|#|->|\s+")


def newest_evidence(ctx: Context) -> dict[str, tuple[str, dict]]:
    """The newest evidence file of each suite: {suite: (run folder name, data)}."""
    base = ctx.runs / "measure"
    found: dict[str, tuple[str, dict]] = {}
    for run in sorted((p for p in base.iterdir() if p.is_dir()), reverse=True) if base.is_dir() else []:
        for path in sorted(run.glob("*.json")):
            if path.name.startswith(".") or path.stem in found:
                continue
            try:
                data = json.loads(path.read_text(encoding="utf-8"))
            except ValueError:
                continue
            if isinstance(data, dict) and isinstance(data.get("failing"), list):
                found[path.stem] = (run.name, data)
    return found


def clusters(units: list[str], depth: int = 0, prefix: tuple[str, ...] = ()) -> list[tuple[tuple[str, ...], list[str]]]:
    """Group unit ids by the parts of their names, until each group has at most CLUSTER_LIMIT units."""
    if len(units) <= CLUSTER_LIMIT:
        return [(prefix, sorted(units))]
    groups: dict[str, list[str]] = defaultdict(list)
    rest: list[str] = []
    for unit in units:
        parts = [p for p in SPLIT.split(unit) if p]
        (groups[parts[depth]] if len(parts) > depth else rest).append(unit)
    result = []
    for key, members in groups.items():
        if len(members) >= MIN_GROUP or len(groups) == 1:
            result += clusters(members, depth + 1, prefix + (key,))
        else:
            rest += members
    rest.sort()
    pieces = [rest[i:i + CLUSTER_LIMIT] for i in range(0, len(rest), CLUSTER_LIMIT)]
    for index, piece in enumerate(pieces, 1):
        label = "(other)" if len(pieces) == 1 else f"(other {index}/{len(pieces)})"
        result.append((prefix + (label,), piece))
    return result


def next_items(ctx: Context, args: argparse.Namespace) -> int:
    evidence = newest_evidence(ctx)
    if not evidence:
        raise Refusal(
            f"no evidence file with failing units under {ctx.runs / 'measure'}",
            "run `tools/autobuild/ops measure` first",
        )
    found = []
    for suite, (run, data) in evidence.items():
        unstable = set(map(str, data.get("unstable") or []))
        failing = sorted({str(u) for u in data["failing"]} - unstable)
        rows = [r for r in data.get("rows") or [] if isinstance(r, dict)]
        metric = str(rows[0].get("metric")) if rows and rows[0].get("metric") else suite
        for prefix, members in clusters(failing):
            found.append((len(members), suite, metric, run, prefix, members))
    found.sort(key=lambda c: (-c[0], c[1], c[4]))
    out = ctx.runs / "next" / utc_stamp()
    out.mkdir(parents=True, exist_ok=True)
    print(f"failing units by cluster, largest first (lists in {out}):")
    for index, (size, suite, metric, run, prefix, members) in enumerate(found[: args.limit], 1):
        name = "/".join(prefix) or "(all)"
        listing = out / f"{index:02d}.txt"
        listing.write_text("\n".join(members) + "\n", encoding="utf-8")
        print(f"{index:2d}. {suite} {name}: {size} units (metric {metric}; run {run})")
        print(f"    for example: {', '.join(members[:3])}")
        print(f"    units: {listing}")
    print('next: `tools/autobuild/ops item add M<n>-<nn> --milestone M<n> --metric KEY --target +N --title "..."` '
          "for one cluster; name its units in the title")
    return 0


# ---- ops milestone check --------------------------------------------------

def milestone_check(ctx: Context, args: argparse.Namespace) -> int:
    if not ctx.milestones_file.exists():
        raise OpsError(f"{ctx.rel(ctx.milestones_file)} does not exist")
    try:
        data = tomllib.loads(ctx.milestones_file.read_text(encoding="utf-8"))
    except tomllib.TOMLDecodeError as error:
        raise OpsError(f"{ctx.rel(ctx.milestones_file)} is not valid TOML: {error}") from error
    milestone = next((m for m in data.get("milestone", []) if m.get("id") == args.id), None)
    if milestone is None:
        known = ", ".join(str(m.get("id")) for m in data.get("milestone", []))
        raise Refusal(f"milestone {args.id} is not in plan/milestones.toml", f"use one of: {known}")
    rows = [r for r in read_rows(ctx) if r["stream"] == "build"]
    newest: dict[str, dict[str, str]] = {}
    for row in rows:
        if number(row, "pass") is not None and number(row, "total") is not None:
            newest[row.get("metric", "")] = row
    checks: list[tuple[bool, str]] = []
    for req in milestone.get("require", []):
        metric = str(req.get("metric", ""))
        row = newest.get(metric)
        value = f"{row['pass']}/{row['total']}" if row else "no row"
        if req.get("exists"):
            checks.append((row is not None, f"{metric} has a row ({value})"))
        if "min_pass" in req:
            ok = row is not None and int(row["pass"]) >= int(req["min_pass"])
            checks.append((ok, f"{metric} PASS >= {req['min_pass']} ({value})"))
        if "min_fraction" in req:
            ok = row is not None and int(row["total"]) > 0 and int(row["pass"]) / int(row["total"]) >= float(req["min_fraction"])
            checks.append((ok, f"{metric} PASS/TOTAL >= {req['min_fraction']} ({value})"))
    checks += implicit_checks(ctx, rows)
    print(f"milestone {args.id}: {milestone.get('title', '')}")
    for ok, text in checks:
        print(f"  {'pass' if ok else 'FAIL'}  {text}")
    failed = sum(not ok for ok, _ in checks)
    print(f"milestone {args.id}: {'passes' if not failed else f'{failed} of {len(checks)} requirements fail'}")
    return 0 if not failed else 1


def implicit_checks(ctx: Context, rows: list[dict[str, str]]) -> list[tuple[bool, str]]:
    """No regression in the newest full measurement, and a held-out result for that code."""
    checks = []
    runs = [r for r in build_runs(rows) if r.item == "-"]
    if not runs:
        return [(False, f"a full measurement of {BUILD_BRANCH} exists (none)")]
    last = runs[-1]
    best: dict[tuple[str, int], int] = {}
    for row in rows:
        if (row.get("utc"), row.get("commit")) == (last.utc, last.commit):
            continue
        value, total = number(row, "pass"), number(row, "total")
        if value is not None and total is not None:
            key = (row.get("metric", ""), total)
            best[key] = max(value, best.get(key, value))
    fallen = [
        f"{r['metric']} {r['pass']}/{r['total']} < {best[(r['metric'], int(r['total']))]}"
        for r in rows
        if (r.get("utc"), r.get("commit")) == (last.utc, last.commit)
        and number(r, "pass") is not None and number(r, "total") is not None
        and (r.get("metric", ""), int(r["total"])) in best and int(r["pass"]) < best[(r["metric"], int(r["total"]))]
    ]
    checks.append((not fallen, f"no metric of the newest full measurement ({last.commit[:12]}) is below its best "
                   f"earlier value with the same TOTAL" + (f": {'; '.join(fallen[:5])}" if fallen else "")))
    found = holdout.newest(ctx)
    held = str(found[1].get("commit", "")) if found else ""
    fresh = bool(held) and bool(ctx.git.rev(held)) and ctx.git.is_ancestor(held, "HEAD") \
        and bool(ctx.git.rev(last.commit)) and ctx.git.is_ancestor(last.commit, held)
    checks.append((fresh, f"the newest held-out result measures the code of the newest full measurement or newer "
                   f"(held-out {held[:12] or 'none'}, measurement {last.commit[:12]})"))
    return checks
