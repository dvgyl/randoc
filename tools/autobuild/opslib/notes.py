"""Append-only notes: the review log, owner blockers, advice and review verdicts."""

from __future__ import annotations

import re
from dataclasses import dataclass

from .core import Context, Refusal, append_text, check_words, stage, utc_now
from .plan import REVIEW_FIELDS

LOG_KINDS = ("decision", "deviation", "shortcut", "weak-spot", "cut", "blocked-on-owner", "milestone")
ASK_REASONS = ("two-fixes-failed", "acceptance-failed", "two-rows-no-gain", "no-next-step")
ADVICE_HEAD = re.compile(r"^## (\S+) event=(\S+) reason=(\S+) session=(\S+)\s*$")
BLOCKED_HEAD = re.compile(r"^## (\S+) (open|closed)\b")


def head_short(ctx: Context) -> str:
    head = ctx.git.rev("HEAD")
    return head[:12] if head else "-"


def log_entry(ctx: Context, kind: str, item: str, text: str) -> str:
    if kind not in LOG_KINDS:
        raise Refusal(f"unknown log kind {kind!r}", f"use one of: {', '.join(LOG_KINDS)}")
    text = text.strip()
    if not text:
        raise Refusal("the log text is empty", "write what happened and why, in plain sentences")
    check_words(ctx, text, "the log text")
    return f"\n## {utc_now()} {kind} {item} {head_short(ctx)}\n\n{text}\n"


def write_log(ctx: Context, kind: str, item: str, text: str) -> None:
    append_text(ctx.log_file, log_entry(ctx, kind, item, text))
    stage(ctx, ctx.log_file)


def blocked_entry(ctx: Context, what: str, owner_must: str, meanwhile: str) -> str:
    for label, value in (("the blocker", what), ("--owner-must", owner_must), ("--meanwhile", meanwhile)):
        check_words(ctx, value, label)
    return (
        f"\n## {utc_now()} open\n\n"
        f"- Blocked: {what.strip()}\n"
        f"- The owner must: {owner_must.strip() or 'see the blocked text'}\n"
        f"- Meanwhile the team: {meanwhile.strip() or 'continues with other items'}\n"
    )


def open_blockers(ctx: Context) -> list[str]:
    if not ctx.blocked_file.exists():
        return []
    heads = (BLOCKED_HEAD.match(l) for l in ctx.blocked_file.read_text(encoding="utf-8").splitlines())
    return [m.group(1) for m in heads if m and m.group(2) == "open"]


@dataclass(frozen=True)
class Advice:
    utc: str
    event: str
    reason: str
    session: str
    decided: bool


def advice_entries(ctx: Context) -> list[Advice]:
    """The sections of plan/ADVICE.md. A section is decided when text follows its 'Decision:' line."""
    if not ctx.advice_file.exists():
        return []
    entries: list[Advice] = []
    current: list[str] | None = None
    after_decision: list[str] = []
    in_decision = False

    def close() -> None:
        if current is not None:
            entries.append(Advice(*current, decided=any(l.strip() for l in after_decision)))

    for line in ctx.advice_file.read_text(encoding="utf-8").splitlines():
        match = ADVICE_HEAD.match(line)
        if match:
            close()
            current, after_decision, in_decision = list(match.groups()), [], False
        elif current is not None and line.startswith("Decision:"):
            in_decision = True
            after_decision.append(line[len("Decision:"):])
        elif in_decision:
            after_decision.append(line)
    close()
    return entries


def stuck_event(item: str, experiments: int, head: str) -> str:
    """One stuck event: (item, experiments at that time). With no item: the commit."""
    return f"{item}/{experiments}" if item != "-" else f"-/{head}"


def advice_section(event: str, reason: str, session: str, question: str, summary: str, answer_path: str) -> str:
    quoted = "\n".join("> " + line for line in question.strip().splitlines())
    return (
        f"\n## {utc_now()} event={event} reason={reason} session={session}\n\n"
        f"Question:\n\n{quoted}\n\n"
        f"Answer (summary): {summary.strip() or 'see the answer file'}\n\n"
        f"Answer (full): {answer_path}\n\n"
        "Decision:\n"
    )


def review_rows(ctx: Context, item: str) -> list[dict[str, str]]:
    if not ctx.reviews_file.exists():
        return []
    rows = ctx.guard.tsv_rows(ctx.reviews_file.read_text(encoding="utf-8"))
    return [r for r in rows if r.get("item") == item]


def append_review(ctx: Context, row: dict[str, str]) -> None:
    if not ctx.reviews_file.exists() or not ctx.reviews_file.read_text(encoding="utf-8").strip():
        append_text(ctx.reviews_file, "\t".join(REVIEW_FIELDS) + "\n")
    clean = {k: " ".join(str(row[k]).split()) or "-" for k in REVIEW_FIELDS}
    append_text(ctx.reviews_file, "\t".join(clean[k] for k in REVIEW_FIELDS) + "\n")
    stage(ctx, ctx.reviews_file)


VERDICT = re.compile(r"^\s*VERDICT:\s*(approve|block)\b", re.IGNORECASE | re.MULTILINE)
EVIDENCE = re.compile(r"^\s*EVIDENCE:\s*(.+?)\s*$", re.MULTILINE)
SUMMARY = re.compile(r"^\s*SUMMARY:\s*(.+?)\s*$", re.MULTILINE)


def parse_verdict(text: str) -> tuple[str | None, str]:
    """The last verdict line and its evidence. A block with no evidence is not a valid block."""
    verdicts = VERDICT.findall(text)
    if not verdicts:
        return None, ""
    evidence = EVIDENCE.findall(text)
    return verdicts[-1].lower(), (evidence[-1] if evidence else "")


def parse_summary(text: str) -> str:
    found = SUMMARY.findall(text)
    return found[-1] if found else ""
