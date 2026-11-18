"""The note and model commands: log, blocked, ask and review.

ask and review send the prompt to the broker, which runs a new top-level
session with read-only access and returns the final message."""

from __future__ import annotations

import argparse
from pathlib import Path

from . import broker, codex, notes
from .core import WORKER_PREFIX, Context, OpsError, Refusal, append_text, check_words, stage, utc_now

# The stuck reasons that only the lead can see; each question with one of them carries its record.
EVIDENCE_REASONS = ("two-fixes-failed", "acceptance-failed", "no-next-step")
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


def lead_tree(ctx: Context) -> tuple[str, frozenset[str]]:
    """HEAD and the `git status` entries of the lead tree (macOS companion files do not count)."""
    out = ctx.git.run("status", "--porcelain=v1", "-z", "--untracked-files=all")
    entries = frozenset(e for e in out.split("\0") if e and not Path(e[3:]).name.startswith("._"))
    return ctx.git.rev("HEAD") or "-", entries


def tree_change(ctx: Context, before: tuple[str, frozenset[str]], role: str, item: str) -> str | None:
    """None when the lead tree is as it was before the read-only session. Else the change, which also
    goes to plan/REVIEW_LOG.md and plan/BLOCKED_ON_OWNER.md: the session was not read-only."""
    after = lead_tree(ctx)
    if after == before:
        return None
    parts = ([f"HEAD {before[0][:12]} -> {after[0][:12]}"] if after[0] != before[0] else []) \
        + sorted(before[1] ^ after[1])[:5]
    change = "; ".join(parts)
    text = (f"The lead tree changed while the read-only {role} session ran ({change}). ops did not use the "
            "answer. The broker's profile randoc-review must keep each session read-only.")
    notes.write_log(ctx, "blocked-on-owner", item, text)
    append_text(ctx.blocked_file, notes.blocked_entry(
        ctx, text, "check the profile randoc-review of the broker", "the lead works without that answer"))
    stage(ctx, ctx.blocked_file)
    return change


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
        append_text(ctx.advice_file, f"\nDecision: {args.decision.strip()}\n")
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
    evidence = (getattr(args, "evidence", None) or "").strip()
    if args.reason in EVIDENCE_REASONS and not evidence:
        raise Refusal(
            f"the reason {args.reason} needs --evidence: the record of the stuck state",
            "give the failing case, the commands and their output, and the fixes that failed: "
            f'`ops ask --reason {args.reason} --evidence "TEXT" "QUESTION"`',
        )
    check_words(ctx, evidence, "the evidence")
    item_id = args.item or default_item(ctx)
    item = ItemsTable.load(ctx.items_file).get(item_id) if item_id != "-" else None
    state = progress(ctx, item, read_rows(ctx)) if item else None
    if args.reason == "two-rows-no-gain" and not (state and state.stuck):
        raise Refusal(
            f"'two-rows-no-gain' needs the stuck state of an active item; {item_id} does not show it in `ops status`",
            "continue the work; ask only for a stuck state that is true now",
        )
    head = (ctx.git.rev("HEAD") or "-")
    event = notes.stuck_event(item_id, args.reason, head[:12])
    if any(e.event == event for e in entries):
        raise Refusal(
            f"stuck event {event} already has its one question (plan/ADVICE.md)",
            "decide with that answer and continue; there is one question for each item and reason",
        )
    question = args.question.strip() + (f"\n\nThe evidence of the lead:\n\n{evidence}" if evidence else "")
    values = {
        "reason": args.reason, "item": item_id, "title": item.title if item else "-",
        "metric": item.metric if item else "-", "baseline": item.baseline if item else "-",
        "experiments": state.experiments if state else 0, "question": question,
    }
    prompt = codex.render(ctx, "adviser", values)
    broker.require_running()
    request_id = broker.new_request_id("advice")

    def record(part: str) -> None:
        append_text(ctx.advice_file, scrub(ctx, part))
        stage(ctx, ctx.advice_file)

    # The question is used when it is sent: its section comes first, so a failed request asks no second time.
    record(notes.advice_question(event, args.reason, request_id, args.question, evidence))
    before = lead_tree(ctx)
    try:
        broker.send(ctx, "advice", {"commit": head, "item": item_id, "prompt": prompt}, request_id)
        print(f"request {request_id} sent to the broker; waiting for the answer in {broker.OUT} ...", flush=True)
        answer = broker.wait(broker.OUT, request_id, broker.WAIT_SECONDS["advice"])
    except OpsError as error:
        record(notes.advice_no_answer(str(error)))
        raise
    change = tree_change(ctx, before, "adviser", item_id)
    if change:
        record(notes.advice_no_answer(f"not used: the lead tree changed during the session: {change}"))
        raise Refusal(f"the lead tree changed during the adviser session ({change}); the answer is not used",
                      "look at `git status` and `git log -1` and keep no change that you did not make; then "
                      'decide without the answer: `ops ask --decision "TEXT"`')
    text = str(answer.get("final") or "")
    if answer.get("status") != "done" or not text.strip():
        record(notes.advice_no_answer(f"{answer.get('reason') or answer.get('status')}"))
        raise OpsError(
            f"the adviser session gave no answer ({answer.get('reason') or answer.get('status')}); the question "
            f'of {item_id}/{args.reason} is used: decide without an answer (`ops ask --decision "TEXT"`)'
        )
    record(notes.advice_answer(str(answer.get("session") or "-"), notes.parse_summary(text),
                               str(broker.OUT / f"{answer.get('request')}.json")))
    print(text.rstrip())
    print('\nnext: decide, then `tools/autobuild/ops ask --decision "TEXT"`, and continue')
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
    broker.require_running()
    row = {"utc": utc_now(), "item": item.id, "round": round_no, "commit": commit, "session": "-",
           "verdict": "started", "evidence": "-"}
    notes.append_review(ctx, row)  # the round is used now, also when the session fails
    print(f"review round {round_no} of {item.id} in a new session of the broker ...", flush=True)
    before = lead_tree(ctx)
    answer = broker.ask(ctx, "review", {"commit": commit, "item": item.id, "round": round_no, "prompt": prompt})
    change = tree_change(ctx, before, "review", item.id)
    if change:
        notes.append_review(ctx, {**row, "utc": utc_now(), "session": answer.get("session") or "-",
                                  "verdict": "failed", "evidence": scrub(ctx, f"the lead tree changed: {change}")})
        raise Refusal(f"the lead tree changed during the review session ({change}); round {round_no} is used "
                      "and its answer is not used",
                      "look at `git status` and `git log -1` and keep no change that you did not make; the entry "
                      "is in plan/REVIEW_LOG.md")
    text = str(answer.get("final") or "")
    verdict, evidence, output = notes.parse_verdict(text) if answer.get("status") == "done" else (None, "", "")
    note = ""
    if verdict is None:
        verdict, evidence = "failed", f"no valid verdict ({answer.get('reason') or answer.get('status')})"
    elif verdict == "block" and not (evidence and output):
        verdict, note = "approve", ("the reviewer blocked with no test or command and its output; "
                                    "that is not a valid block, so it counts as approve")
    shown = f"{evidence} => {output}"[:400] if verdict == "block" else evidence or str(broker.OUT / f"{answer.get('request')}.json")
    notes.append_review(ctx, {**row, "utc": utc_now(), "session": answer.get("session") or "-", "verdict": verdict,
                              "evidence": scrub(ctx, shown)})
    print(text.rstrip())
    print(f"\nround {round_no}: {verdict}" + (f" (evidence: {evidence})" if verdict in ("block", "failed") else ""))
    if note:
        print(f"note: {note}")
    if verdict == "failed":
        print(f"next: round {round_no} is used; decide with the evidence, or run the last round")
        return 1
    print("next: " + (
        "fix the defect that the evidence shows, then run the second round" if verdict == "block" and round_no == 1
        else 'write each finding that is not a block as `ops log weak-spot "TEXT"`, and continue'
    ))
    return 0
