"""The plan files: work items, scoreboard rows, item progress and the loop counters."""

from __future__ import annotations

import re
from dataclasses import dataclass, replace
from pathlib import Path

from .core import Context, OpsError, Refusal

ITEM_FIELDS = ["id", "milestone", "status", "metric", "baseline", "target", "title"]
ROW_FIELDS = ["utc", "commit", "dirty", "item", "metric", "pass", "total", "tool", "evidence"]
REVIEW_FIELDS = ["utc", "item", "round", "commit", "session", "verdict", "evidence"]
STATUSES = ("queued", "active", "done", "cut", "blocked")
# Each transition: the states that it leaves from, and the state that it makes.
TRANSITIONS = {
    "start": ({"queued", "blocked"}, "active"),
    "done": ({"active"}, "done"),
    "cut": ({"queued", "active", "blocked"}, "cut"),
    "block": ({"queued", "active"}, "blocked"),
}
FRACTION = re.compile(r"^\s*(\d+)\s*/\s*(\d+)\s*$")


@dataclass(frozen=True)
class Item:
    id: str
    milestone: str
    status: str
    metric: str
    baseline: str
    target: str
    title: str

    def base(self) -> tuple[int, int] | None:
        match = FRACTION.match(self.baseline or "")
        return (int(match.group(1)), int(match.group(2))) if match else None

    def target_gain(self) -> int:
        match = re.match(r"^\+?(\d+)$", self.target or "")
        return int(match.group(1)) if match else 0

    def line(self) -> str:
        return "\t".join(getattr(self, f) for f in ITEM_FIELDS)


def moved(item: Item, transition: str, **fields: str) -> Item:
    sources, result = TRANSITIONS[transition]
    if item.status not in sources:
        raise Refusal(
            f"item {item.id} is {item.status}; '{transition}' needs one of: {', '.join(sorted(sources))}",
            "run `tools/autobuild/ops item list` and choose an item in a valid state",
        )
    return replace(item, status=result, **fields)


class ItemsTable:
    """plan/ITEMS.tsv. Comment lines and the order of the rows stay as they are."""

    def __init__(self, path: Path, lines: list[str]):
        self.path = path
        self.lines = lines

    @classmethod
    def load(cls, path: Path) -> "ItemsTable":
        if not path.exists():
            return cls(path, ["\t".join(ITEM_FIELDS)])
        lines = path.read_text(encoding="utf-8").splitlines()
        return cls(path, lines or ["\t".join(ITEM_FIELDS)])

    def _rows(self) -> list[tuple[int, Item]]:
        header: list[str] | None = None
        rows = []
        for index, line in enumerate(self.lines):
            if not line.strip() or line.startswith("#"):
                continue
            if header is None:
                header = line.split("\t")
                continue
            values = dict(zip(header, line.split("\t")))
            rows.append((index, Item(**{f: values.get(f, "") for f in ITEM_FIELDS})))
        return rows

    def items(self) -> list[Item]:
        return [item for _, item in self._rows()]

    def get(self, item_id: str) -> Item | None:
        return next((item for item in self.items() if item.id == item_id), None)

    def need(self, item_id: str) -> Item:
        item = self.get(item_id)
        if item is None:
            raise Refusal(
                f"item {item_id} is not in plan/ITEMS.tsv",
                "add it first: `tools/autobuild/ops item add ID --milestone M --metric KEY --target +N --title TEXT`",
            )
        return item

    def add(self, item: Item) -> None:
        self.lines.append(item.line())

    def put(self, item: Item) -> None:
        for index, old in self._rows():
            if old.id == item.id:
                self.lines[index] = item.line()
                return
        raise OpsError(f"item {item.id} is not in {self.path}")

    def text(self) -> str:
        return "\n".join(self.lines) + "\n"


def read_rows(ctx: Context) -> list[dict[str, str]]:
    """All scoreboard rows of the working tree, oldest first. Each row gets its stream."""
    rows: list[dict[str, str]] = []
    if ctx.scoreboard.is_dir():
        for path in sorted(ctx.scoreboard.glob("*.tsv")):
            if path.name.startswith("._"):
                continue
            for row in ctx.guard.tsv_rows(path.read_text(encoding="utf-8")):
                row["stream"] = path.stem
                rows.append(row)
    rows.sort(key=lambda r: r.get("utc", ""))
    return rows


def number(row: dict[str, str], field: str) -> int | None:
    try:
        return int(row.get(field, ""))
    except ValueError:
        return None


def newest_row(rows: list[dict[str, str]], metric: str, stream: str | None = None) -> dict[str, str] | None:
    found = [
        r for r in rows
        if r.get("metric") == metric and (stream is None or r["stream"] == stream)
        and number(r, "pass") is not None and number(r, "total") is not None
    ]
    return found[-1] if found else None


@dataclass(frozen=True)
class Progress:
    experiments: int
    gained: bool
    gain: int | None
    newest_total: int | None
    total_changed: bool
    stuck: bool


def guard_gained(base: tuple[int, int] | None, best: int | None, widest: int) -> bool:
    """The gain test of the guard's pre-commit hook (baseline 0/0: a row with a TOTAL above 0)."""
    return base is not None and ((base[1] == 0 and widest > 0) or (best is not None and best > base[0]))


def progress(ctx: Context, item: Item, rows: list[dict[str, str]]) -> Progress:
    """Experiments (counted as the guard counts them), gain with the baseline TOTAL, and stuck state."""
    experiments, _best, _widest = ctx.guard.item_tries(rows, item.id, item.metric)
    base = item.base()
    mine = [
        r for r in rows
        if r.get("item") == item.id and r.get("metric") == item.metric
        and number(r, "pass") is not None and number(r, "total") is not None
    ]
    if base is None:
        return Progress(experiments, False, None, None, False, False)
    reference, total = base
    newest_total = number(mine[-1], "total") if mine else None
    if total == 0:
        # The item makes a new metric (ROADMAP rule 7): its first row with a TOTAL above 0 is the gain.
        made = [number(r, "pass") or 0 for r in mine if (number(r, "total") or 0) > 0]
        trailing = 0
        for r in reversed(mine):
            if (number(r, "total") or 0) > 0:
                break
            trailing += 1
        return Progress(experiments, bool(made), max(made) if made else None, newest_total, False,
                        not made and trailing >= 2)
    total_changed = newest_total is not None and newest_total != total
    if total_changed:
        # The TOTAL changed: the baseline commit must be measured again with the new TOTAL.
        base_commit = baseline_commit(ctx, item.id)
        again = [
            r for r in rows
            if base_commit and r.get("commit") == base_commit
            and r.get("metric") == item.metric and number(r, "total") == newest_total
        ]
        if not again:
            return Progress(experiments, False, None, newest_total, True, False)
        reference, total = number(again[-1], "pass") or 0, newest_total
        mine = [r for r in mine if r.get("commit") != base_commit]
    same = [number(r, "pass") or 0 for r in mine if number(r, "total") == total]
    gain = max(same) - reference if same else None
    best, no_gain_rows = reference, 0
    for value in same:
        if value > best:
            best, no_gain_rows = value, 0
        else:
            no_gain_rows += 1
    return Progress(experiments, gain is not None and gain > 0, gain, newest_total, total_changed,
                    no_gain_rows >= 2)


def baseline_commit(ctx: Context, item_id: str) -> str | None:
    """The commit of the baseline row, from the body of the commit that started the item."""
    out = ctx.git.run(
        "log", "-1", "--format=%B", "-F", f"--grep=start item {item_id} ", "HEAD", check=False
    )
    match = re.search(r"measured at ([0-9a-f]{7,40})", out)
    return match.group(1) if match else None


@dataclass(frozen=True)
class Run:
    utc: str
    commit: str
    item: str
    gained: tuple[str, ...]


def build_runs(rows: list[dict[str, str]]) -> list[Run]:
    """The measurement runs of gpt/build, oldest first, each with the metrics that rose."""
    best: dict[tuple[str, int], int] = {}
    seen: set[str] = set()
    runs: dict[tuple[str, str], Run] = {}
    for row in rows:
        if row["stream"] != "build":
            continue
        key = (row.get("utc", ""), row.get("commit", ""))
        run = runs.setdefault(key, Run(key[0], key[1], row.get("item", "-") or "-", ()))
        value, total = number(row, "pass"), number(row, "total")
        if value is None or total is None:
            continue
        slot = (row.get("metric", ""), total)
        # The first row of a metric with a TOTAL above 0 is a gain (ROADMAP rule 7).
        new_metric = slot[0] not in seen and total > 0
        seen.add(slot[0])
        if new_metric or (slot in best and value > best[slot]):
            run = runs[key]
            runs[key] = replace(run, gained=run.gained + (slot[0],))
        best[slot] = max(value, best.get(slot, value))
    return list(runs.values())


def commits_since_measured(ctx: Context) -> int:
    """The guard's counter: commits after the newest measured commit, from the rows in HEAD."""
    guard = ctx.guard
    if not ctx.git.rev("HEAD"):
        return 0
    head_rows = guard.scoreboard_rows(lambda name: guard.committed_file("HEAD", name))
    return guard.commits_since_measurement(head_rows)


def cut_streak(items: list[Item]) -> tuple[int, str]:
    """Items cut in a row in one area (the area is the metric), newest last."""
    finished = [i for i in items if i.status in ("done", "cut")]
    if not finished or finished[-1].status != "cut":
        return 0, ""
    area = finished[-1].metric
    streak = 0
    for item in reversed(finished):
        if item.status != "cut" or item.metric != area:
            break
        streak += 1
    return streak, area


def lead_active(ctx: Context, items: list[Item], worker_ids: set[str]) -> list[Item]:
    return [i for i in items if i.status == "active" and i.id not in worker_ids]


def predict_commit(ctx: Context, new_items: list[Item], touches_scoreboard: bool = False) -> None:
    """Refuse before any change when the guard would refuse the commit of a plan change."""
    guard = ctx.guard
    branch = ctx.git.branch()
    if not branch.startswith("gpt/"):
        raise Refusal(
            f"commits are allowed only on gpt/* branches, not on {branch or 'a detached HEAD'}",
            "run `git checkout gpt/build`",
        )
    since = commits_since_measured(ctx)
    if since >= guard.MEASURE_LIMIT and not touches_scoreboard:
        raise Refusal(
            f"{since} commits since the newest measured commit (limit {guard.MEASURE_LIMIT}); the guard refuses the next commit",
            "run `tools/autobuild/ops measure` and commit the new scoreboard rows first",
        )
    rows = read_rows(ctx)  # the commit stages the new rows of the working tree
    for item in new_items:
        if item.status != "active":
            continue
        tries, best, widest = guard.item_tries(rows, item.id, item.metric)
        if tries >= guard.MAX_TRIES and not guard_gained(guard.baseline(item.baseline), best, widest):
            raise Refusal(
                f"item {item.id} used {tries} experiments with no gain; the guard refuses each commit",
                f"cut it first: `tools/autobuild/ops item cut {item.id} --reason TEXT`",
            )


def commit_plan(ctx: Context, subject: str, body: str, changes: dict[Path, str]) -> str:
    """Write plan files and commit only those files. Restore them when Git refuses."""
    saved = {path: path.read_bytes() if path.exists() else None for path in changes}
    for path, text in changes.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
    names = [ctx.rel(path) for path in changes]
    try:
        ctx.git.run("add", "--", *names)
        ctx.git.run("commit", "-q", "-m", subject, "-m", body, "--", *names)
    except OpsError as error:
        for path, data in saved.items():
            if data is None:
                path.unlink(missing_ok=True)
            else:
                path.write_bytes(data)
        ctx.git.run("reset", "-q", "--", *names, check=False)
        raise Refusal(
            f"Git refused the commit, nothing changed: {error}",
            "remove the cause that the guard names, then run the same ops command again",
        ) from error
    return ctx.git.run("rev-parse", "HEAD").strip()


def scoreboard_changes(ctx: Context) -> dict[Path, str]:
    """Scoreboard files with rows that are not committed, with their text."""
    out = ctx.git.run("status", "--porcelain", "--untracked-files=all", "--", "plan/scoreboard/", check=False)
    paths = [ctx.root / line[3:] for line in out.splitlines()
             if line[3:].endswith(".tsv") and not Path(line[3:]).name.startswith("._")]
    return {path: path.read_text(encoding="utf-8") for path in paths if path.is_file()}


def commit_transition(ctx: Context, items: list[Item], subject: str, body: str, changes: dict[Path, str]) -> str:
    """Commit a change of plan/ITEMS.tsv with the uncommitted scoreboard rows.

    The rows decide the transition, and the guard sees them only when they are in the same
    commit: at the 10-commit limit, only a commit that touches the scoreboard passes."""
    board = scoreboard_changes(ctx)
    predict_commit(ctx, items, touches_scoreboard=bool(board))
    return commit_plan(ctx, subject, body, {**changes, **board})


def plan_dirty(ctx: Context) -> list[str]:
    out = ctx.git.run("status", "--porcelain", "--", "plan/", check=False)
    return [line[3:] for line in out.splitlines() if line[3:] and "/._" not in "/" + line[3:]]
