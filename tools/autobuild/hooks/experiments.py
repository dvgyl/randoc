"""The experiment count and the gain of a work item, and the cut-off of tool corrections.

One rule for the Git guard (guard.py), ops (opslib/plan.py) and measure.py.
Standard library only, with no import of the other tool files: the guard
loads this file by its path next to its protected copy.

- Rows: scoreboard rows (plan/scoreboard/*.tsv) as dicts.
- Attempts: rows of $RANDOC_RUNS/attempts.tsv (utc, commit, item, suites,
  pid). measure.py writes one before each run that measures an item, so a run
  that fails before it writes rows still counts as an experiment.
- Corrections: rows of plan/corrections.tsv (utc, metric, commit, item,
  reason). The rows of a metric that are older than its newest correction
  do not count for gains, regressions or milestone checks.
"""

from __future__ import annotations

import os
from pathlib import Path

MAX_TRIES = 3
ATTEMPT_FIELDS = ("utc", "commit", "item", "suites", "pid")
CORRECTION_FIELDS = ("utc", "metric", "commit", "item", "reason")
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


def cutoffs(corrections: list[dict[str, str]]) -> dict[str, str]:
    """The UTC time of the newest correction of each metric."""
    found: dict[str, str] = {}
    for correction in corrections:
        metric, utc = correction.get("metric", ""), correction.get("utc", "")
        if metric and utc > found.get(metric, ""):
            found[metric] = utc
    return found


def counts(row: dict[str, str], cut: dict[str, str]) -> bool:
    """False for a row that is older than the newest correction of its metric."""
    return row.get("utc", "") >= cut.get(row.get("metric", ""), "")


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


def gained(rows: list[dict[str, str]], item: str, metric: str, base: tuple[int, int] | None,
           corrections: list[dict[str, str]] = ()) -> bool:
    """The gain rule: the LATEST row of the item's metric has a PASS above the baseline PASS,
    and its failing count (TOTAL - PASS) is not above the baseline failing count.

    Baseline 0/0 (an item that makes or corrects a metric, ROADMAP rule 7): the
    latest row has a TOTAL above 0."""
    row = latest(rows, item, metric, corrections)
    if base is None or row is None:
        return False
    passed, total = numbers(row)
    base_pass, base_total = base
    if base_total == 0:
        return total > 0
    return passed > base_pass and total - passed <= base_total - base_pass


def exhausted(rows: list[dict[str, str]], attempts: list[dict[str, str]], item: str, metric: str,
              base: tuple[int, int] | None, corrections: list[dict[str, str]] = ()) -> bool:
    """True when the item used MAX_TRIES experiments and shows no gain: only a cut is allowed."""
    return experiments(rows, attempts, item) >= MAX_TRIES and not gained(rows, item, metric, base, corrections)
