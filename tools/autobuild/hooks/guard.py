"""Git guard for the autonomous build on the gpt/* branches.

The active copy lives outside the build folder, where the build cannot change
it. tools/autobuild/install-guard copies this file and the hook shims there and
points core.hooksPath at that copy. A change to this source file has no effect
until the owner runs install-guard again.

Usage (from the hook shims): guard.py HOOK [ARGS...]
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

MEASURE_LIMIT = 10
MAX_TRIES = 3
BUILD_BRANCH = "refs/heads/gpt/build"
# Commits on the gpt/* branches are newer than this. An older commit that is
# not on main comes from an old local branch whose history must stay private.
BRANCH_EPOCH = "2026-11-17T00:00:00Z"
BANNED = (re.compile(r"\bHOLD\b"), re.compile(r"fresh\s+complete\s+review", re.IGNORECASE))
ATTRIBUTION = re.compile(
    r"^\s*(co-authored-by:|claude-session:)|generated with", re.IGNORECASE
)
TAG = re.compile(r"\[([A-Za-z0-9][A-Za-z0-9._-]*)\]")
SCOREBOARD_DIR = "plan/scoreboard/"
ITEMS_FILE = "plan/ITEMS.tsv"
# Files that a commit tagged [ops] may change. Product code needs an item.
OPS_PATHS = (
    "plan/", "tools/", "spec/", "docs/", "conformance/", "benchmarks/", "scripts/",
    "crates/randoc-harness/", ".github/", "AGENTS.md", "README.md", "PROGRESS.md",
    "init.sh", ".gitignore",
)


class Refused(Exception):
    pass


def git(*args: str, check: bool = True, stdin: str | None = None) -> str:
    result = subprocess.run(
        ["git", *args], input=stdin, capture_output=True, text=True, check=False
    )
    if check and result.returncode != 0:
        raise Refused(f"git {' '.join(args)} failed: {result.stderr.strip()}")
    return result.stdout


def is_zero(oid: str) -> bool:
    return set(oid) <= {"0"}


def rev_ok(rev: str) -> str | None:
    out = git("rev-parse", "-q", "--verify", f"{rev}^{{commit}}", check=False).strip()
    return out or None


def is_ancestor(old: str, new: str) -> bool:
    return (
        subprocess.run(
            ["git", "merge-base", "--is-ancestor", old, new], capture_output=True
        ).returncode
        == 0
    )


def banned_hits(text: str) -> list[str]:
    return [p.pattern for p in BANNED if p.search(text)]


def tsv_rows(text: str) -> list[dict[str, str]]:
    lines = [l for l in text.splitlines() if l.strip() and not l.startswith("#")]
    if not lines:
        return []
    header = lines[0].split("\t")
    return [dict(zip(header, l.split("\t"))) for l in lines[1:]]


def staged_file(path: str) -> str:
    return git("show", f":{path}", check=False)


def committed_file(rev: str, path: str) -> str:
    return git("show", f"{rev}:{path}", check=False)


def scoreboard_rows(read) -> list[dict[str, str]]:
    names = [
        n
        for n in git("ls-files", "--", SCOREBOARD_DIR).split()
        if n.endswith(".tsv")
    ]
    rows: list[dict[str, str]] = []
    for name in names:
        rows.extend(tsv_rows(read(name)))
    return rows


def item_tries(rows: list[dict[str, str]], item: str, metric: str) -> tuple[int, int | None, int]:
    """Experiments of the item, best PASS on its metric, largest TOTAL seen."""
    runs = {(r.get("utc"), r.get("commit")) for r in rows if r.get("item") == item}
    best = None
    widest = 0
    for r in rows:
        if r.get("item") == item and r.get("metric") == metric:
            try:
                value, total = int(r.get("pass", "")), int(r.get("total", ""))
            except ValueError:
                continue
            best = value if best is None else max(best, value)
            widest = max(widest, total)
    return len(runs), best, widest


def baseline(value: str) -> tuple[int, int] | None:
    match = re.match(r"\s*(\d+)\s*/\s*(\d+)\s*$", value or "")
    return (int(match.group(1)), int(match.group(2))) if match else None


def current_branch() -> str:
    return git("symbolic-ref", "-q", "HEAD", check=False).strip()


def commits_since_measurement(rows: list[dict[str, str]]) -> int:
    head = rev_ok("HEAD")
    if head is None:
        return 0
    counts = []
    for commit in {r.get("commit", "") for r in rows}:
        if commit and rev_ok(commit) and is_ancestor(commit, head):
            counts.append(int(git("rev-list", "--count", f"{commit}..{head}").strip()))
    if counts:
        return min(counts)
    base = rev_ok("refs/remotes/origin/main")
    if base is None:
        return 0
    merge_base = git("merge-base", base, head, check=False).strip()
    if not merge_base:
        return 0
    return int(git("rev-list", "--count", f"{merge_base}..{head}").strip())


def pre_commit() -> None:
    branch = current_branch()
    if not branch.startswith("refs/heads/gpt/"):
        raise Refused(f"commits are allowed only on gpt/* branches, not on {branch or 'a detached HEAD'}")
    staged = [n for n in git("diff", "--cached", "--name-only", "-z").split("\0") if n]
    for name in staged:
        if Path(name).name.startswith("._"):
            raise Refused(f"{name} is a macOS companion file; unstage it")
    added = [
        line[1:]
        for line in git("diff", "--cached", "-U0", "--", "*.md", "*.tsv", "*.txt").splitlines()
        if line.startswith("+") and not line.startswith("+++")
    ]
    for line in added:
        hits = banned_hits(line)
        if hits:
            raise Refused(f"a staged line uses a banned phrase ({', '.join(hits)}): {line.strip()[:120]}")

    head_rows = scoreboard_rows(lambda n: committed_file("HEAD", n)) if rev_ok("HEAD") else []
    since = commits_since_measurement(head_rows)
    touches_scoreboard = any(n.startswith(SCOREBOARD_DIR) for n in staged)
    if since >= MEASURE_LIMIT and not touches_scoreboard:
        raise Refused(
            f"{since} commits since the last measured commit (limit {MEASURE_LIMIT}). "
            "Run `tools/autobuild/ops measure` and commit the new scoreboard rows first."
        )

    rows = scoreboard_rows(staged_file)
    for item in tsv_rows(staged_file(ITEMS_FILE)):
        if item.get("status") != "active":
            continue
        tries, best, widest = item_tries(rows, item.get("id", ""), item.get("metric", ""))
        base = baseline(item.get("baseline", ""))
        # Baseline 0/0: the item builds a new metric; its first row with a
        # TOTAL above 0 is the gain.
        gained = base is not None and (
            (base[1] == 0 and widest > 0) or (best is not None and best > base[0])
        )
        if tries >= MAX_TRIES and not gained:
            raise Refused(
                f"item {item.get('id')} used {tries} experiments with no gain on {item.get('metric')}. "
                "Cut it: `tools/autobuild/ops item cut ID --reason TEXT`, then revert its code."
            )


def commit_msg(path: str) -> None:
    text = Path(path).read_text(encoding="utf-8")
    kept = [l for l in text.splitlines() if not ATTRIBUTION.search(l)]
    while kept and not kept[-1].strip():
        kept.pop()
    Path(path).write_text("\n".join(kept) + "\n", encoding="utf-8")
    body = "\n".join(l for l in kept if not l.startswith("#"))
    hits = banned_hits(body)
    if hits:
        raise Refused(f"the commit message uses a banned phrase ({', '.join(hits)})")
    git_dir = Path(git("rev-parse", "--git-dir").strip())
    if (git_dir / "MERGE_HEAD").exists():
        return
    subject = next((l for l in kept if l.strip() and not l.startswith("#")), "")
    staged_items = tsv_rows(staged_file(ITEMS_FILE))
    ids = {r.get("id") for r in staged_items if r.get("status") == "active"}
    # The commit that cuts an item also reverts its code (ops item cut), so it
    # may carry the tag of the item that it moves from active to cut.
    if rev_ok("HEAD"):
        was_active = {r.get("id") for r in tsv_rows(committed_file("HEAD", ITEMS_FILE)) if r.get("status") == "active"}
        ids |= {r.get("id") for r in staged_items if r.get("status") == "cut"} & was_active
    tags = TAG.findall(subject)
    if not any(t == "ops" or t in ids for t in tags):
        raise Refused(
            "the subject must end with [ITEM-ID] (an active item in plan/ITEMS.tsv) or [ops]; "
            f"subject was: {subject[:120]}"
        )
    if not any(t in ids for t in tags):
        staged = [n for n in git("diff", "--cached", "--name-only", "-z").split("\0") if n]
        product = [n for n in staged if not n.startswith(OPS_PATHS)]
        if product:
            raise Refused(
                "an [ops] commit may change only measurement, plan and tool files; "
                f"{product[0]} needs a work item: `tools/autobuild/ops item add` and start it"
            )


def pre_push(lines: list[str]) -> None:
    for line in lines:
        parts = line.split()
        if len(parts) != 4:
            continue
        local_ref, local_oid, remote_ref, remote_oid = parts
        if not remote_ref.startswith("refs/heads/gpt/"):
            raise Refused(f"push to {remote_ref} refused: only refs/heads/gpt/* may be pushed")
        if is_zero(local_oid):
            raise Refused(f"deletion of {remote_ref} refused")
        if not is_zero(remote_oid):
            if rev_ok(remote_oid) is None:
                raise Refused(f"the remote {remote_ref} has commits that are not here: run git fetch and merge first")
            if not is_ancestor(remote_oid, local_oid):
                raise Refused(f"push to {remote_ref} is not a fast-forward: force pushes are refused")
            span = f"{remote_oid}..{local_oid}"
        else:
            span = local_oid
        old = git(
            "rev-list", f"--before={BRANCH_EPOCH}", span, "--not", "refs/remotes/origin/main", check=False
        ).split()
        if old:
            raise Refused(
                f"commit {old[0][:12]} is older than this branch and not on main: history of an "
                "old local branch must never be published; import its files as a new commit"
            )
        for message in git("log", "--format=%B%x00", span).split("\0"):
            if any(ATTRIBUTION.search(l) for l in message.splitlines()):
                raise Refused("a commit to push holds an attribution line")
            if banned_hits(message):
                raise Refused("a commit to push uses a banned phrase")


def reference_transaction(state: str, lines: list[str]) -> None:
    if state == "committed":
        pack = Path(git("rev-parse", "--git-common-dir").strip()) / "objects" / "pack"
        for companion in pack.glob("._*"):
            try:
                companion.unlink()
            except OSError:
                pass
        return
    if state != "prepared":
        return
    for line in lines:
        parts = line.split()
        if len(parts) != 3:
            continue
        _old, new, ref = parts
        if ref.startswith("refs/tags/"):
            raise Refused(f"tags are not used on this build: {ref}")
        if ref.startswith("refs/heads/") and not ref.startswith("refs/heads/gpt/"):
            raise Refused(f"refusing to change {ref}: only refs/heads/gpt/* may change")
        if ref != BUILD_BRANCH:
            continue
        if is_zero(new):
            raise Refused("refusing to delete gpt/build")
        current = rev_ok(ref)
        pushed = rev_ok("refs/remotes/origin/gpt/build")
        if current and not is_ancestor(current, new) and pushed and is_ancestor(current, pushed):
            raise Refused(
                "refusing to rewrite pushed history of gpt/build; undo a change with `git revert`"
            )


def main(argv: list[str]) -> int:
    hook = argv[1] if len(argv) > 1 else ""
    try:
        if hook == "pre-commit":
            pre_commit()
        elif hook == "commit-msg":
            commit_msg(argv[2])
        elif hook == "pre-push":
            pre_push(sys.stdin.read().splitlines())
        elif hook == "reference-transaction":
            reference_transaction(argv[2], sys.stdin.read().splitlines())
        else:
            print(f"guard: unknown hook {hook!r}", file=sys.stderr)
            return 2
    except Refused as refused:
        print(f"guard ({hook}): {refused}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
