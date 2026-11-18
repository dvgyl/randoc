"""The experiment count and the gain of a work item, and the cut-off of tool corrections.

One rule for the Git guard (guard.py), ops (opslib/plan.py) and measure.py.
Standard library only, with no import of the other tool files: the guard
loads this file by its path next to its protected copy.

- Rows: scoreboard rows (plan/scoreboard/*.tsv) as dicts.
- Attempts: rows of $RANDOC_RUNS/attempts.tsv (utc, commit, item, suites,
  pid). measure.py writes one before each run that measures an item, so a run
  that fails before it writes rows still counts as an experiment.
- Corrections: rows of plan/corrections.tsv (utc, metric, commit, item,
  reason, withdraws). The rows of a metric that are older than its newest
  correction do not count for gains, regressions or milestone checks. A row
  whose `withdraws` names the utc of a correction of the same metric
  withdraws it (`ops item cut` of the item that made it): the older rows
  count again, and the rows measured while it was in force do not.
- Accept: the coverage rule of an item (column `accept` of plan/ITEMS.tsv):
  `-` (none), `all-pass` (PASS = TOTAL above 0) or `min-total=N` (TOTAL >= N).
  A latest row that does not meet it is no gain.
"""

from __future__ import annotations

import os
from pathlib import Path

MAX_TRIES = 3
ATTEMPT_FIELDS = ("utc", "commit", "item", "suites", "pid")
CORRECTION_FIELDS = ("utc", "metric", "commit", "item", "reason", "withdraws")
CORRECTIONS_FILE = "plan/corrections.tsv"
DEFAULT_RUNS = "/Volumes/Extreme Pro/Randoc/gpt/runs"


def tsv_rows(text: str) -> list[dict[str, str]]:
    lines = [line for line in text.splitlines() if line.strip() and not line.startswith("#")]
    if not lines:
        return []
    header = lines[0].split("\t")
    return [dict(zip(header, line.split("\t"))) for line in lines[1:]]


def read_tsv(path: Path) -> list[dict[str, str]]:
    try:
        return tsv_rows(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return []


def attempts_file(runs: str | None = None) -> Path:
    return Path(runs or os.environ.get("RANDOC_RUNS") or DEFAULT_RUNS) / "attempts.tsv"


def record_attempt(path: Path, utc: str, commit: str, item: str, suites: list[str], pid: int) -> None:
    """Append one attempt row with one write on an append-only descriptor."""
    path.parent.mkdir(parents=True, exist_ok=True)
    try:
        descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
    except FileExistsError:
        pass
    else:
        with os.fdopen(descriptor, "w", encoding="utf-8") as header:
            header.write("\t".join(ATTEMPT_FIELDS) + "\n")
    line = "\t".join((utc, commit, item, ",".join(suites) or "-", str(pid))) + "\n"
    descriptor = os.open(path, os.O_WRONLY | os.O_APPEND)
    try:
        os.write(descriptor, line.encode("utf-8"))
    finally:
        os.close(descriptor)


class Cutoffs(dict):
    """metric -> the UTC time of its newest correction in force. `windows`: metric -> the (start, end)
    times of each withdrawn correction, from the correction to its withdrawal."""

    def __init__(self, newest: dict[str, str], windows: dict[str, list[tuple[str, str]]]):
        super().__init__(newest)
        self.windows = windows


def withdrawal_of(correction: dict[str, str]) -> str:
    """The utc of the correction that this row withdraws, or '' for a correction."""
    value = correction.get("withdraws") or ""
    return "" if value == "-" else value


def cutoffs(corrections: list[dict[str, str]]) -> Cutoffs:
    ends = {(c.get("metric", ""), withdrawal_of(c)): c.get("utc", "") for c in corrections if withdrawal_of(c)}
    newest: dict[str, str] = {}
    windows: dict[str, list[tuple[str, str]]] = {}
    for correction in corrections:
        metric, utc = correction.get("metric", ""), correction.get("utc", "")
        if not metric or withdrawal_of(correction):
            continue
        end = ends.get((metric, utc))
        if end:
            windows.setdefault(metric, []).append((utc, end))
        elif utc > newest.get(metric, ""):
            newest[metric] = utc
    return Cutoffs(newest, windows)


def counts(row: dict[str, str], cut: dict[str, str]) -> bool:
    """False for a row that is older than the newest correction of its metric, or that was measured
    while a correction that is now withdrawn was in force."""
    utc, metric = row.get("utc", ""), row.get("metric", "")
    if utc < cut.get(metric, ""):
        return False
    return not any(start <= utc < end for start, end in getattr(cut, "windows", {}).get(metric, ()))


def numbers(row: dict[str, str]) -> tuple[int, int] | None:
    """PASS and TOTAL of a row, or None when they are not integers with 0 <= PASS <= TOTAL."""
    try:
        passed, total = int(row.get("pass", "")), int(row.get("total", ""))
    except ValueError:
        return None
    return (passed, total) if 0 <= passed <= total else None


def baseline(text: str) -> tuple[int, int] | None:
    passed, slash, total = (text or "").strip().partition("/")
    if not slash or not passed.strip().isdigit() or not total.strip().isdigit():
        return None
    return int(passed), int(total)


def experiments(rows: list[dict[str, str]], attempts: list[dict[str, str]], item: str) -> int:
    """The larger of the item's scoreboard runs (distinct utc and commit) and its attempt records."""
    runs = {(r.get("utc"), r.get("commit")) for r in rows if r.get("item") == item}
    tried = sum(1 for a in attempts if a.get("item") == item)
    return max(len(runs), tried)


def latest(rows: list[dict[str, str]], item: str, metric: str,
           corrections: list[dict[str, str]] = ()) -> dict[str, str] | None:
    """The newest valid row of the item on its metric that a correction did not make obsolete."""
    cut = cutoffs(list(corrections))
    mine = [r for r in rows if r.get("item") == item and r.get("metric") == metric
            and numbers(r) is not None and counts(r, cut)]
    return max(mine, key=lambda r: r.get("utc", ""), default=None)


def shortfall(accept: str | None, passed: int, total: int) -> str | None:
    """Why PASS/TOTAL does not meet the coverage rule `accept`, or None. An unknown rule is never met."""
    rule = accept or "-"
    if rule == "-":
        return None
    if rule == "all-pass":
        return None if 0 < total == passed else f"{passed}/{total} does not meet all-pass (PASS = TOTAL above 0)"
    name, _, value = rule.partition("=")
    if name == "min-total" and value.isdecimal():
        return None if total >= int(value) else f"{passed}/{total} does not meet {rule} (TOTAL >= {value})"
    return f"the rule {rule!r} is not -, all-pass or min-total=N"


def gained(rows: list[dict[str, str]], item: str, metric: str, base: tuple[int, int] | None,
           corrections: list[dict[str, str]] = (), accept: str | None = "-") -> bool:
    """The gain rule: the LATEST row of the item's metric has a PASS above the baseline PASS,
    its failing count (TOTAL - PASS) is not above the baseline failing count, and it meets the
    coverage rule `accept` of the item.

    Baseline 0/0 (an item that makes or corrects a metric, ROADMAP rule 7): the
    latest row has a TOTAL above 0."""
    row = latest(rows, item, metric, corrections)
    if base is None or row is None:
        return False
    passed, total = numbers(row)
    if shortfall(accept, passed, total):
        return False
    base_pass, base_total = base
    if base_total == 0:
        return total > 0
    return passed > base_pass and total - passed <= base_total - base_pass


def exhausted(rows: list[dict[str, str]], attempts: list[dict[str, str]], item: str, metric: str,
              base: tuple[int, int] | None, corrections: list[dict[str, str]] = (),
              accept: str | None = "-") -> bool:
    """True when the item used MAX_TRIES experiments and shows no gain: only a cut is allowed."""
    return (experiments(rows, attempts, item) >= MAX_TRIES
            and not gained(rows, item, metric, base, corrections, accept))
