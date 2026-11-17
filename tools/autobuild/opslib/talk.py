"""The note and model commands: log, blocked, ask and review."""

from __future__ import annotations

import argparse

from . import codex, notes
from .core import WORKER_PREFIX, Context, OpsError, Refusal, append_text, check_words, stage, utc_now
from .items import item_commits, require_lead, worker_ids
from .plan import ItemsTable, commit_transition, lead_active, moved, progress, read_rows


def default_item(ctx: Context) -> str:
    """The item of this worker branch, or the one active item of the lead, or '-'."""
    own = ctx.worker_item()
    if own:
        return own
    active = lead_active(ctx, ItemsTable.load(ctx.items_file).items(), worker_ids(ctx))
    return active[0].id if len(active) == 1 else "-"


def scrub(ctx: Context, text: str) -> str:
    """Model output goes into plan files; the guard refuses a banned phrase there."""
    for pattern in ctx.guard.BANNED:
        text = pattern.sub("(banned phrase removed)", text)
    return text


def log(ctx: Context, args: argparse.Namespace) -> int:
    item = args.item or default_item(ctx)
    notes.write_log(ctx, args.kind, item, args.text)
    print(f"logged {args.kind} for {item} in plan/REVIEW_LOG.md (staged; it goes with your next commit)")
    return 0


def blocked(ctx: Context, args: argparse.Namespace) -> int:
    entry = notes.blocked_entry(ctx, args.text, args.owner_must or "", args.meanwhile or "")
    log_text = notes.log_entry(ctx, "blocked-on-owner", args.item or default_item(ctx), args.text)
    if args.item:
        require_lead(ctx)
        table = ItemsTable.load(ctx.items_file)
        table.put(moved(table.need(args.item), "block"))
        read = lambda p: p.read_text(encoding="utf-8") if p.exists() else ""
        commit_transition(ctx, table.items(), f"chore(plan): block item {args.item} [ops]", args.text, {
            ctx.items_file: table.text(),
            ctx.blocked_file: read(ctx.blocked_file) + entry,
            ctx.log_file: read(ctx.log_file) + log_text,
        })
        print(f"item {args.item} is blocked; the entry is committed in plan/BLOCKED_ON_OWNER.md")
    else:
        append_text(ctx.blocked_file, entry)
        append_text(ctx.log_file, log_text)
        stage(ctx, ctx.blocked_file, ctx.log_file)
        print("the entry is in plan/BLOCKED_ON_OWNER.md and plan/REVIEW_LOG.md (staged)")
    print("next: continue with other work")
    return 0


def ask(ctx: Context, args: argparse.Namespace) -> int:
    entries = notes.advice_entries(ctx)
    if args.decision:
        if not entries or entries[-1].decided:
            raise Refusal("no advice entry waits for a decision", "ask a question first, when you are stuck")
        check_words(ctx, args.decision, "the decision")
        append_text(ctx.advice_file, args.decision.strip() + "\n")
        stage(ctx, ctx.advice_file)
        print(f"decision written for the question of {entries[-1].utc} (plan/ADVICE.md, staged)")
        return 0
    if not args.reason or not args.question:
        raise Refusal("a question needs --reason KIND and the question text", 'run `ops ask --reason KIND "QUESTION"`')
    if entries and not entries[-1].decided:
        raise Refusal(
            f"the advice of {entries[-1].utc} has no decision yet",
            'write the decision first: `ops ask --decision "TEXT"`',
        )
    check_words(ctx, args.question, "the question")
    item_id = args.item or default_item(ctx)
    item = ItemsTable.load(ctx.items_file).get(item_id) if item_id != "-" else None
    state = progress(ctx, item, read_rows(ctx)) if item else None
    if args.reason == "two-rows-no-gain" and not (state and state.stuck):
        raise Refusal(
            f"'two-rows-no-gain' needs the stuck state of an active item; {item_id} does not show it in `ops status`",
            "continue the work; ask only for a stuck state that is true now",
        )
    head = (ctx.git.rev("HEAD") or "-")[:12]
    event = notes.stuck_event(item_id, state.experiments if state else 0, head)
    if any(e.event == event for e in entries):
        raise Refusal(
            f"stuck event {event} already has its one question (plan/ADVICE.md)",
            "decide with that answer and continue; the next experiment makes a new stuck event",
        )
    values = {
        "reason": args.reason, "item": item_id, "title": item.title if item else "-",
        "metric": item.metric if item else "-", "baseline": item.baseline if item else "-",
        "experiments": state.experiments if state else 0, "question": args.question.strip(),
    }
    prompt = codex.render(ctx, "adviser", values)
    record = codex.RunDir.new(ctx.runs / "advice", item_id)
    argv = codex.command("adviser", ctx.root, record.final)
    record.prepare(argv, prompt)
    print(f"asking the adviser model (record: {record.path}) ...", flush=True)
    code = codex.run_sync(argv, record, ctx.root)
    answer = record.final_text()
    if code != 0 or not answer.strip():
        raise OpsError(
            f"the adviser run failed (exit {code}; see {record.stderr}); nothing was recorded, so you may ask again"
        )
    section = notes.advice_section(
        event, args.reason, record.session(), scrub(ctx, args.question), scrub(ctx, notes.parse_summary(answer)),
        str(record.final),
    )
    append_text(ctx.advice_file, section)
    stage(ctx, ctx.advice_file)
    print(answer.rstrip())
    print(f'\nnext: decide, then `tools/autobuild/ops ask --decision "TEXT"`, and continue')
    return 0


def review_commits(ctx: Context, item_id: str) -> list[tuple[str, str]]:
    """The worker branch commits that HEAD does not have, else the item's commits on HEAD."""
    branch = WORKER_PREFIX + item_id
    if ctx.git.rev(f"refs/heads/{branch}") and not ctx.git.is_ancestor(branch, "HEAD"):
        out = ctx.git.run("log", "--format=%H%x1f%s", f"HEAD..{branch}")
        return [tuple(line.split("\x1f", 1)) for line in out.splitlines()]  # type: ignore[misc]
    return [(sha, subject) for sha, _, subject in item_commits(ctx, item_id)]


def review(ctx: Context, args: argparse.Namespace) -> int:
    item = ItemsTable.load(ctx.items_file).need(args.id)
    rounds = notes.review_rows(ctx, item.id)
    if len(rounds) >= 2:
        raise Refusal(
            f"item {item.id} had 2 review rounds; a third round is not allowed",
            'decide with the evidence and write it: `ops log decision "TEXT"`',
        )
    commit = ctx.git.rev(args.commit or "HEAD")
    if commit is None:
        raise Refusal(f"{args.commit} is not a commit", "give --commit SHA, or leave it out for HEAD")
    commits = review_commits(ctx, item.id)
    if not commits:
        raise Refusal(f"item {item.id} has no commits to review", f"commit the change with [{item.id}] first")
    round_no = len(rounds) + 1
    values = {
        "item": item.id, "title": item.title, "round": round_no, "commit": commit,
        "commits": "\n".join(f"- {sha} {subject}" for sha, subject in commits),
    }
    prompt = codex.render(ctx, "review", values)
    record = codex.RunDir.new(ctx.runs / "review", f"{item.id}-r{round_no}")
    argv = codex.command("review", ctx.root, record.final)
    record.prepare(argv, prompt)
    print(f"review round {round_no} of {item.id} in a new session (record: {record.path}) ...", flush=True)
    code = codex.run_sync(argv, record, ctx.root)
    text = record.final_text()
    verdict, evidence = notes.parse_verdict(text)
    if code != 0 or verdict is None:
        raise OpsError(
            f"the review run gave no verdict (exit {code}; see {record.path}); no round was used, so you may run it again"
        )
    note = ""
    if verdict == "block" and not evidence:
        verdict, note = "approve", "the reviewer blocked with no failing test or command; that is not a valid block, so it counts as approve"
    row = {
        "utc": utc_now(), "item": item.id, "round": round_no, "commit": commit, "session": record.session(),
        "verdict": verdict, "evidence": scrub(ctx, evidence) if verdict == "block" else str(record.final),
    }
    notes.append_review(ctx, row)
    print(text.rstrip())
    print(f"\nround {round_no}: {verdict}" + (f" (evidence: {evidence})" if verdict == "block" else ""))
    if note:
        print(f"note: {note}")
    print("next: " + (
        "fix the defect that the evidence shows, then run the second round" if verdict == "block" and round_no == 1
        else 'write each finding that is not a block as `ops log weak-spot "TEXT"`, and continue'
    ))
    return 0
