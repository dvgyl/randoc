"""Tool corrections: plan/corrections.tsv and `ops correction add`.

A correction records that a tool now forms or counts the units of a metric in
another way (plan/SPEC.md 3.13). The rows of the metric that are older than
the correction no longer count for gains, regressions or milestone checks
(hooks/experiments.py has the rule). The item that makes the correction gets
the baseline 0/0 on its own metric (ROADMAP rule 7), so its first row with a
TOTAL above 0 is its gain.
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
    old = file.read_text(encoding="utf-8") if file.exists() else ""
    if not old.strip():
        old = "\t".join(experiments.CORRECTION_FIELDS) + "\n"
    text = old + ("" if old.endswith("\n") else "\n") + "\t".join((utc, args.metric, head, item.id, reason)) + "\n"
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
