"""Tool corrections: plan/corrections.tsv and `ops correction add`.

A correction records that a tool now forms or counts the units of a metric in
another way (plan/SPEC.md 3.13). The rows of the metric that are older than
the correction no longer count for gains, regressions or milestone checks
(hooks/experiments.py has the rule). The item that makes the correction gets
the baseline 0/0 on its own metric (ROADMAP rule 7), so its first row with a
TOTAL above 0 is its gain. `ops item cut` reverts the tool change of the item
and withdraws each correction that the item made (a row whose `withdraws` is
the utc of that correction).
"""

from __future__ import annotations

import argparse
from dataclasses import replace
from pathlib import Path

import measure

from .core import Context, Refusal, check_line, utc_now
from .items import METRIC_KEY, require_lead
from .notes import log_entry
from .plan import ItemsTable, commit_transition

experiments = measure.experiments


def path(ctx: Context) -> Path:
    return ctx.root / experiments.CORRECTIONS_FILE


def load(ctx: Context) -> list[dict[str, str]]:
    return experiments.read_tsv(path(ctx))


def with_rows(ctx: Context, rows: list[tuple[str, ...]]) -> str:
    """The text of plan/corrections.tsv with `rows` appended, under the header CORRECTION_FIELDS.
    An older header without the field `withdraws` is replaced; its rows read that field as empty."""
    file = path(ctx)
    lines = file.read_text(encoding="utf-8").splitlines() if file.exists() else []
    header = "\t".join(experiments.CORRECTION_FIELDS)
    at = next((i for i, line in enumerate(lines) if line.strip() and not line.startswith("#")), None)
    if at is None:
        lines.append(header)
    else:
        lines[at] = header
    return "\n".join(lines + ["\t".join(row) for row in rows]) + "\n"


def withdrawals(ctx: Context, item_id: str) -> tuple[str, list[dict[str, str]]] | None:
    """The text of plan/corrections.tsv with a withdrawal of each correction in force that `item_id`
    made, and those corrections; None when there is none."""
    rows = load(ctx)
    gone = {(r.get("metric"), experiments.withdrawal_of(r)) for r in rows if experiments.withdrawal_of(r)}
    mine = [r for r in rows if r.get("item") == item_id and not experiments.withdrawal_of(r)
            and (r.get("metric"), r.get("utc")) not in gone]
    if not mine:
        return None
    utc, head = utc_now(), ctx.git.rev("HEAD") or "-"
    reason = f"withdrawn: item {item_id} is cut, so its tool change is reverted"
    return with_rows(ctx, [(utc, r["metric"], head, item_id, reason, r["utc"]) for r in mine]), mine


def add(ctx: Context, args: argparse.Namespace) -> int:
    require_lead(ctx)
    if not METRIC_KEY.match(args.metric):
        raise Refusal(f"{args.metric!r} is not a metric key", "use one key of tools/autobuild/metrics.toml")
    reason = check_line(ctx, args.reason, "--reason")
    table = ItemsTable.load(ctx.items_file)
    item = table.need(args.item)
    if item.status != "active":
        raise Refusal(
            f"item {item.id} is {item.status}; a correction belongs to the active item that changes the tool",
            f"start the item first (`ops item start {item.id}`), or name the active item with --item",
        )
    utc = utc_now()
    head = ctx.git.rev("HEAD") or "-"
    file = path(ctx)
    text = with_rows(ctx, [(utc, args.metric, head, item.id, reason, "-")])
    note = (f"Tool correction of {args.metric} by {item.id}: {reason.rstrip('.')}. The rows of {args.metric} before "
            f"{utc} no longer count for gains, regressions or milestone checks.")
    if item.metric == args.metric:
        table.put(replace(item, baseline="0/0"))
        note += f" The baseline of {item.id} is now 0/0: its first row with a TOTAL above 0 is its gain."
    log = (ctx.log_file.read_text(encoding="utf-8") if ctx.log_file.exists() else "") \
        + log_entry(ctx, "decision", item.id, note)
    commit_transition(ctx, table.items(), f"chore(plan): correct the metric {args.metric} [ops]", note,
                      {file: text, ctx.items_file: table.text(), ctx.log_file: log})
    print(note)
    return 0
