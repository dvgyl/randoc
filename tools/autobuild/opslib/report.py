"""The read commands: status (counters and the next required action), next and milestone check."""

from __future__ import annotations

import argparse
import json
import re
import shutil
import tomllib
from collections import defaultdict
from dataclasses import dataclass

import measure

from . import corrections, fleet, holdout, notes
from .corrections import experiments
from .core import BUILD_BRANCH, Context, OpsError, Refusal, guard_paths, utc_stamp
from .plan import (
    Item, ItemsTable, Progress, build_runs, commits_since_measured, cut_streak, lead_active,
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
    if f.advice and not f.advice[-1].decided:
        return 'write the decision for the newest advice: `tools/autobuild/ops ask --decision "TEXT"`'
    asked = {a.event for a in f.advice}
    for item_id, p in mine.items():
        if p.stuck and notes.stuck_event(item_id, "two-rows-no-gain") not in asked:
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
    """The newest evidence file of each suite that is not a filtered run: {suite: (run folder name, data)}."""
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
            if isinstance(data, dict) and isinstance(data.get("failing"), list) and not data.get("filter"):
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


UNKNOWN_METRIC = "unknown"


def unit_metrics(data: dict) -> dict[str, str]:
    """The metric of each failing unit: of the unit's metrics in `failing_by_metric`, the one with
    the smallest TOTAL of the run (the most specific one; on a tie, the first name)."""
    by_metric = data.get("failing_by_metric")
    if not isinstance(by_metric, dict):
        return {}
    totals = {str(r.get("metric")): r.get("total") for r in data.get("rows") or [] if isinstance(r, dict)}

    def rank(metric: str) -> tuple[int, str]:
        total = totals.get(metric)
        return (total if isinstance(total, int) else 1 << 62, metric)

    chosen: dict[str, str] = {}
    for metric, units in by_metric.items():
        for unit in map(str, units if isinstance(units, list) else []):
            if unit not in chosen or rank(metric) < rank(chosen[unit]):
                chosen[unit] = metric
    return chosen


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
        chosen = unit_metrics(data)
        groups: dict[str, list[str]] = defaultdict(list)
        for unit in failing:
            groups[chosen.get(unit, UNKNOWN_METRIC)].append(unit)
        for metric, units in groups.items():
            for prefix, members in clusters(units):
                found.append((len(members), suite, metric, run, prefix, members))
    found.sort(key=lambda c: (-c[0], c[1], c[4]))
    out = ctx.runs / "next" / utc_stamp()
    out.mkdir(parents=True, exist_ok=True)
    for suite in sorted(s for s, (_, data) in evidence.items() if not isinstance(data.get("failing_by_metric"), dict)):
        print(f"note: the evidence of {suite} has no failing_by_metric (an older measure.py), so its metric is "
              "unknown; `tools/autobuild/ops measure` writes it")
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

KINDS = ("exists", "min_pass", "min_fraction", "min_total", "exact_total")
HELD_OUT = "holdout."


@dataclass(frozen=True)
class Candidate:
    """The run that a milestone check judges: one utc, one commit, one row for each metric."""

    utc: str
    commit: str
    rows: dict[str, dict[str, str]]


def candidate_run(rows: list[dict[str, str]], active: set[str]) -> Candidate | None:
    """The newest full, clean `build` run: its rows share utc and commit, have dirty `no` and
    item `-`, and hold a row for each active metric. Imported held-out rows are not part of a run."""
    runs: dict[tuple[str, str], list[dict[str, str]]] = defaultdict(list)
    for row in rows:
        if row.get("stream") == "build" and not row.get("metric", "").startswith(HELD_OUT):
            runs[(row.get("utc", ""), row.get("commit", ""))].append(row)
    for (utc, commit), group in sorted(runs.items(), reverse=True):
        if all(r.get("dirty") == "no" and r.get("item") == "-" for r in group):
            by_metric = {r.get("metric", ""): r for r in group}
            if active <= by_metric.keys():
                return Candidate(utc, commit, by_metric)
    return None


def requirement_checks(req: dict, row: dict[str, str] | None, cut: dict[str, str]) -> list[tuple[bool, str]]:
    """The checks of one [[milestone.require]] entry against the candidate row of its metric."""
    metric = str(req.get("metric", ""))
    pair = experiments.numbers(row) if row else None
    if row is None:
        value = "no row"
    elif pair is None:
        value = f"{row.get('pass')}/{row.get('total')}, not 0 <= PASS <= TOTAL"
    elif not experiments.counts(row, cut):
        value = f"{row['pass']}/{row['total']}, older than the correction of {cut[metric]}"
        pair = None
    else:
        value = f"{row['pass']}/{row['total']}"
    passed, total = pair if pair else (-1, -1)
    checks = []
    if req.get("exists"):
        checks.append((total > 0, f"{metric} has a row with TOTAL above 0 ({value})"))
    if "min_pass" in req:
        checks.append((pair is not None and passed >= int(req["min_pass"]), f"{metric} PASS >= {req['min_pass']} ({value})"))
    if "min_fraction" in req:
        ok = total > 0 and passed / total >= float(req["min_fraction"])
        checks.append((ok, f"{metric} PASS/TOTAL >= {req['min_fraction']} ({value})"))
    if "min_total" in req:
        checks.append((total >= int(req["min_total"]), f"{metric} TOTAL >= {req['min_total']} ({value})"))
    if "exact_total" in req:
        checks.append((total == int(req["exact_total"]), f"{metric} TOTAL = {req['exact_total']} ({value})"))
    unknown = sorted(set(req) - {"metric", *KINDS})
    if unknown or not checks:
        checks.append((False, f"{metric}: the entry needs one of {', '.join(KINDS)}"
                       + (f"; unknown fields {', '.join(unknown)}" if unknown else "")))
    return checks


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
    try:
        active = {k for k, m in measure.tree_registry(ctx.autobuild).items() if m.status == "active"}
    except measure.ToolError as error:
        raise OpsError(f"the metric registry cannot be read: {error}") from error
    rows = read_rows(ctx)
    cut = experiments.cutoffs(corrections.load(ctx))
    found = candidate_run(rows, active)
    print(f"milestone {args.id}: {milestone.get('title', '')}")
    if found is None:
        print(f"  candidate run: none (no clean full {BUILD_BRANCH} run with item - and a row for each of the "
              f"{len(active)} active metrics); run `tools/autobuild/ops measure` on a clean tree")
    else:
        print(f"  candidate run: {found.commit[:12]} at {found.utc} ({len(found.rows)} rows)")
    checks: list[tuple[bool, str]] = []
    for req in milestone.get("require", []):
        row = found.rows.get(str(req.get("metric", ""))) if found else None
        checks += requirement_checks(req, row, cut)
    checks += implicit_checks(ctx, rows, found, cut)
    for ok, text in checks:
        print(f"  {'pass' if ok else 'FAIL'}  {text}")
    failed = sum(not ok for ok, _ in checks)
    print(f"milestone {args.id}: {'passes' if not failed else f'{failed} of {len(checks)} requirements fail'}")
    return 0 if not failed else 1


def implicit_checks(ctx: Context, rows: list[dict[str, str]], found: Candidate | None,
                    cut: dict[str, str]) -> list[tuple[bool, str]]:
    """Valid counts, no regression against earlier full measurements, and a held-out result for the candidate."""
    if found is None:
        return [(False, f"a clean full measurement of {BUILD_BRANCH} exists (none)")]
    invalid = [f"{m} {r.get('pass')}/{r.get('total')}" for m, r in sorted(found.rows.items()) if experiments.numbers(r) is None]
    checks = [(not invalid, "each row of the candidate run has 0 <= PASS <= TOTAL"
               + (f": {'; '.join(invalid[:5])}" if invalid else ""))]
    best: dict[tuple[str, int], int] = {}
    for row in rows:
        pair = experiments.numbers(row)
        if (row.get("stream") == "build" and row.get("dirty") == "no" and row.get("item") == "-"
                and row.get("utc", "") < found.utc and pair and experiments.counts(row, cut)
                and not row.get("metric", "").startswith(HELD_OUT)):
            key = (row.get("metric", ""), pair[1])
            best[key] = max(pair[0], best.get(key, pair[0]))
    fallen = []
    for metric, row in sorted(found.rows.items()):
        pair = experiments.numbers(row)
        if pair and experiments.counts(row, cut) and pair[0] < best.get((metric, pair[1]), pair[0]):
            fallen.append(f"{metric} {pair[0]}/{pair[1]} < {best[(metric, pair[1])]}")
    checks.append((not fallen, f"no metric of the candidate run is below its best earlier value with the same TOTAL"
                   + (f": {'; '.join(fallen[:5])}" if fallen else "")))
    newest = holdout.newest(ctx)
    held = str(newest[1].get("commit", "")) if newest else ""
    checks.append((held_out_matches(ctx, found.commit, held),
                   f"the newest held-out result measures the candidate commit, or a later commit that changes "
                   f"only plan/ (held-out {held[:12] or 'none'}, candidate {found.commit[:12]})"))
    return checks


def held_out_matches(ctx: Context, candidate: str, held: str) -> bool:
    if not held or not ctx.git.rev(held) or not ctx.git.rev(candidate) or not ctx.git.is_ancestor(held, "HEAD"):
        return False
    if ctx.git.rev(held) == ctx.git.rev(candidate):
        return True
    if not ctx.git.is_ancestor(candidate, held):
        return False
    changed = ctx.git.run("diff", "--name-only", candidate, held).split("\n")
    return all(name.startswith("plan/") for name in changed if name)
