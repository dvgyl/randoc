"""The broker: the owner's support daemon outside the build sandbox.

ops writes one request file into the inbox (the build may write it) and reads
the answer from a folder that only the broker writes. The broker evaluates
held-out candidates, pushes gpt/build, and runs each review and each adviser
question as a new top-level session with read-only access. The build has no
GitHub credential and no write access to the answers, so an answer and a push
come from the broker only.
"""

from __future__ import annotations

import json
import secrets
import time
from pathlib import Path

from .core import BUILD_BRANCH, Context, OpsError, Refusal, utc_stamp

OUT = Path("/Volumes/Extreme Pro/Randoc/broker-out")
RESULTS = Path("/Volumes/Extreme Pro/Randoc/holdout-results")
HEARTBEAT_LIMIT_S = 600
POLL_SECONDS = 2.0
# The longest wait for an answer: a session may run 2 hours, after one session in front of it.
WAIT_SECONDS = {"push": 1800, "review": 5 * 3600, "advice": 5 * 3600}


def inbox(ctx: Context) -> Path:
    return ctx.gpt / "broker" / "requests"


def heartbeat() -> dict:
    try:
        data = json.loads((OUT / "heartbeat.json").read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}
    return data if isinstance(data, dict) else {}


def heartbeat_age() -> float | None:
    beat = OUT / "heartbeat.json"
    return time.time() - beat.stat().st_mtime if beat.exists() else None


def heartbeat_line() -> str:
    age = heartbeat_age()
    if age is None:
        return f"broker: no heartbeat in {OUT}; the broker does not run"
    state = "runs" if age <= HEARTBEAT_LIMIT_S else "does not run (write it with `ops blocked`)"
    evaluation = heartbeat().get("evaluation") or {}
    work = f"; evaluation {evaluation.get('state', '?')}" + (
        f" {evaluation.get('done')}/{evaluation.get('total')}" if evaluation.get("state") == "running" else "")
    return f"broker: heartbeat {age / 60:.0f} min old; the broker {state}{work}"


def require_running() -> None:
    age = heartbeat_age()
    if age is None or age > HEARTBEAT_LIMIT_S:
        raise Refusal(
            f"the broker does not run ({heartbeat_line()})",
            'write it with `tools/autobuild/ops blocked "the broker does not run"` and continue with other work',
        )


def new_request_id(kind: str) -> str:
    return f"{utc_stamp()}-{kind}-{secrets.token_hex(4)}"


def send(ctx: Context, kind: str, fields: dict[str, object], request_id: str | None = None) -> str:
    """Write one request. The name is new each time, so the answer has the same name."""
    folder = inbox(ctx)
    folder.mkdir(parents=True, exist_ok=True)
    request_id = request_id or new_request_id(kind)
    temporary = folder / f".{request_id}.json.part"
    temporary.write_text(json.dumps({"kind": kind, **fields}) + "\n", encoding="utf-8")
    temporary.rename(folder / f"{request_id}.json")
    return request_id


def wait(folder: Path, request_id: str, seconds: float) -> dict:
    """The answer of one request. It stops waiting when the broker stops or after `seconds`."""
    answer = folder / f"{request_id}.json"
    deadline = time.monotonic() + seconds
    while True:
        if answer.exists():
            try:
                data = json.loads(answer.read_text(encoding="utf-8"))
            except ValueError:
                data = None
            if isinstance(data, dict):
                return data
        age = heartbeat_age()
        if age is None or age > HEARTBEAT_LIMIT_S:
            raise OpsError(f"the broker stopped while request {request_id} waited ({heartbeat_line()}); "
                           "the request stays in the inbox, so the answer comes when the broker runs again")
        if time.monotonic() > deadline:
            raise OpsError(f"no answer to request {request_id} after {seconds / 60:.0f} min; the answer, when it "
                           f"comes, is {answer}; write a blocker with `ops blocked` if the broker is stuck")
        time.sleep(POLL_SECONDS)


def ask(ctx: Context, kind: str, fields: dict[str, object]) -> dict:
    require_running()
    request_id = send(ctx, kind, fields)
    print(f"request {request_id} sent to the broker; waiting for the answer in {OUT} ...", flush=True)
    return wait(OUT, request_id, WAIT_SECONDS[kind])


def push(ctx: Context) -> int:
    tip = ctx.git.rev(f"refs/heads/{BUILD_BRANCH}")
    if tip is None:
        raise Refusal(f"there is no branch {BUILD_BRANCH}", "push only gpt/build")
    answer = ask(ctx, "push", {"commit": tip})
    status = answer.get("status")
    print(f"push of {BUILD_BRANCH} at {tip[:12]}: {status}" + (f" ({answer['reason']})" if answer.get("reason") else ""))
    if answer.get("detail"):
        print(str(answer["detail"]).rstrip())
    if status not in ("pushed", "unchanged"):
        print("next: fix the cause that the answer names, commit the fix, then `tools/autobuild/ops push` again")
        return 1
    # The local record of the pushed tip: the guard refuses to rewrite history up to it.
    ctx.git.run("update-ref", f"refs/remotes/origin/{BUILD_BRANCH}", tip)
    print("next: continue")
    return 0
