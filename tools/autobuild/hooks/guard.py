"""Git guard for the autonomous build on the gpt/* branches.

The active copy lives outside the build folder, where the build cannot change
it. tools/autobuild/install-guard copies this file, experiments.py,
licence_scan.py and the hook shims there and points core.hooksPath at that
copy. A change to this source file has no effect until the owner runs
install-guard again.

Usage (from the hook shims): guard.py HOOK [ARGS...]

The broker (the owner's support daemon) also runs the installed copy, with
GIT_DIR set to its own repository:

    guard.py outgoing LOCAL_OID REMOTE_OID   the checks of a push of gpt/build
    guard.py roles < AGENTS.md               the role table of AGENTS.md as JSON
"""

from __future__ import annotations

import datetime as dt
import hashlib
import importlib.util
import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
# No __pycache__ in the protected folder (the broker and the owner's Git run this copy).
sys.dont_write_bytecode = True
MEASURE_LIMIT = 10
BUILD_BRANCH = "refs/heads/gpt/build"
WORKER_BRANCH = re.compile(r"^refs/heads/gpt/w/(M\d+-\d{2,})$")
# Commits on the gpt/* branches are newer than this. An older commit that is
# not on main comes from an old local branch whose history must stay private.
BRANCH_EPOCH = "2026-11-17T00:00:00Z"
# A test points the push rules at a local remote with guard.json next to this
# file; the installed copy has no guard.json.
SETTINGS = {
    "PUSH_URL": "https://github.com/dvgyl/randoc.git",
    "BRANCH_BASE": "d40a641d3b76d2270e420b9d8a48c2b9962f189f",
    "LICENCE_INDEX": "/Volumes/Extreme Pro/Randoc/env/licence-index",
}
if (HERE / "guard.json").is_file():
    SETTINGS.update(json.loads((HERE / "guard.json").read_text(encoding="utf-8")))
BANNED = (re.compile(r"\bHOLD\b"), re.compile(r"fresh\s+complete\s+review", re.IGNORECASE))
# The files that name the banned phrases in order to ban them: the manual, this guard and its tests.
PHRASE_FILES = (
    "AGENTS.md", "tools/autobuild/hooks/guard.py", "tools/autobuild/tests/test_guard.py",
    "tools/autobuild/tests/test_t1_push.py", "tools/autobuild/tests/test_ops.py",
)
ATTRIBUTION = re.compile(
    r"^\s*(co-authored-by:|claude-session:)|generated with", re.IGNORECASE
)
TAG = re.compile(r"\[([A-Za-z0-9][A-Za-z0-9._-]*)\]")
SCOREBOARD_DIR = "plan/scoreboard/"
ITEMS_FILE = "plan/ITEMS.tsv"
ALLOW_FILE = "tools/autobuild/licence-allow.tsv"
REGISTRY_FILE = "tools/autobuild/metrics.toml"
REGISTRY_DIR = "tools/autobuild/metrics.d"
# The origin of each file that the licence scan skips (binary, or above its size limit).
PROVENANCE_FILE = "tools/autobuild/provenance.tsv"
# Files that a commit tagged [ops] may change. Product code needs an item.
OPS_PATHS = (
    "plan/", "tools/", "spec/", "docs/", "conformance/", "benchmarks/", "scripts/",
    "crates/randoc-harness/", ".github/", "AGENTS.md", "README.md", "PROGRESS.md",
    "init.sh", ".gitignore",
)


def sibling(name: str):
    """A module next to this file. The hooks run with python3 -I, so the folder is not on sys.path."""
    path = HERE / f"{name}.py"
    spec = importlib.util.spec_from_file_location(f"guard_{name}", path)
    if not path.is_file() or spec is None or spec.loader is None:
        raise Refused(f"{path} is missing; the owner must run tools/autobuild/install-guard")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class Refused(Exception):
    pass


experiments = sibling("experiments")
MAX_TRIES = experiments.MAX_TRIES
tsv_rows = experiments.tsv_rows
baseline = experiments.baseline


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


def is_full_run_row(row: dict[str, str]) -> bool:
    """A row of a clean measurement of the whole branch: no item, no dirty tree, not an imported held-out total."""
    return (row.get("dirty") == "no" and row.get("item") == "-"
            and not row.get("metric", "").startswith("holdout."))


def current_branch() -> str:
    return git("symbolic-ref", "-q", "HEAD", check=False).strip()


def active_ids(items_text: str) -> set[str]:
    return {r.get("id", "") for r in tsv_rows(items_text) if r.get("status") == "active"}


def full_runs(rows: list[dict[str, str]]) -> dict[tuple[str, str], list[dict[str, str]]]:
    """The full-run rows of `rows`, grouped by run (utc and commit)."""
    runs: dict[tuple[str, str], list[dict[str, str]]] = {}
    for row in rows:
        if is_full_run_row(row):
            runs.setdefault((row.get("utc", ""), row.get("commit", "")), []).append(row)
    return runs


def commits_since_measurement(rows: list[dict[str, str]]) -> int:
    """Commits after the newest commit that has a completed full run (completed_run) among `rows`."""
    head = rev_ok("HEAD")
    if head is None:
        return 0
    since: dict[str, int | None] = {}
    for commit in {commit for _utc, commit in full_runs(rows)}:
        since[commit] = (int(git("rev-list", "--count", f"{commit}..{head}").strip())
                         if commit and rev_ok(commit) and is_ancestor(commit, head) else None)
    # Newest commit first: the manifest and registry reads stop at the first completed run.
    for count, run in sorted(((since[commit], run) for (_utc, commit), run in full_runs(rows).items()
                              if since[commit] is not None), key=lambda pair: pair[0]):
        if completed_run(run):
            return count
    base = rev_ok("refs/remotes/origin/main")
    if base is None:
        return 0
    merge_base = git("merge-base", base, head, check=False).strip()
    if not merge_base:
        return 0
    return int(git("rev-list", "--count", f"{merge_base}..{head}").strip())


def new_full_run(staged: list[dict[str, str]], committed: list[dict[str, str]]) -> bool:
    """The staged rows add a full run of HEAD or of HEAD's parent that measure.py completed."""
    measured = {rev_ok(rev) for rev in ("HEAD", "HEAD^")} - {None}
    old = {tuple(sorted(r.items())) for r in committed}
    runs: dict[tuple[str, str], list[dict[str, str]]] = {}
    for row in staged:
        if tuple(sorted(row.items())) not in old:
            runs.setdefault((row.get("utc", ""), row.get("commit", "")), []).append(row)
    return any(commit in measured and all(is_full_run_row(r) for r in rows) and completed_run(rows)
               for (_utc, commit), rows in runs.items())


ACTIVE_SUITES: dict[str, frozenset[str] | None] = {}


def active_suites_at(commit: str) -> frozenset[str] | None:
    """The suites with an active metric in the registry of `commit` (metrics.toml, then each
    metrics.d/*.toml, a later entry of a key in place of an earlier one, as measure.py reads them),
    or None when the registry is missing or not valid."""
    if commit not in ACTIVE_SUITES:
        overlays = sorted(n for n in git("ls-tree", "-z", "--name-only", f"{commit}:{REGISTRY_DIR}",
                                         check=False).split("\0")
                          if n.endswith(".toml") and not n.startswith(("._", "_")))
        entries: dict[str, dict] = {}
        try:
            for path in [REGISTRY_FILE, *(f"{REGISTRY_DIR}/{name}" for name in overlays)]:
                for entry in tomllib.loads(git("show", f"{commit}:{path}"))["metric"]:
                    entries[entry["key"]] = entry
            ACTIVE_SUITES[commit] = frozenset(str(e["suite"]) for e in entries.values() if e.get("status") == "active")
        except (Refused, tomllib.TOMLDecodeError, KeyError, TypeError):
            ACTIVE_SUITES[commit] = None
    return ACTIVE_SUITES[commit]


def completed_run(rows: list[dict[str, str]]) -> bool:
    """The rows share one evidence folder, and its run.json (measure.py writes it at the end of a
    successful run) names their utc and commit, an end time, their number of rows, and suites that
    hold each active suite of the registry of that commit."""
    folders = {Path(r.get("evidence", "")).parent for r in rows}
    folder = folders.pop() if len(folders) == 1 else None
    if folder is None or not folder.is_absolute():
        return False
    try:
        manifest = json.loads((folder / "run.json").read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return False
    if not (isinstance(manifest, dict) and bool(manifest.get("end"))
            and manifest.get("utc") == rows[0].get("utc") and manifest.get("commit") == rows[0].get("commit")
            and manifest.get("rows") == len(rows) and isinstance(manifest.get("suites"), list)):
        return False
    required = active_suites_at(rows[0].get("commit", ""))
    return required is not None and required <= {str(s) for s in manifest["suites"]}


def cuts_an_item() -> bool:
    if not rev_ok("HEAD"):
        return False
    cut = {r.get("id", "") for r in tsv_rows(staged_file(ITEMS_FILE)) if r.get("status") == "cut"}
    return bool(cut & active_ids(committed_file("HEAD", ITEMS_FILE)))


def check_branch(branch: str) -> None:
    if branch == BUILD_BRANCH:
        return
    worker = WORKER_BRANCH.match(branch)
    if worker is None:
        raise Refused(
            f"commits are allowed only on gpt/build and on gpt/w/ID, not on {branch or 'a detached HEAD'}"
        )
    # The lead's table decides: a worker branch stays open only while the lead keeps its item active.
    if worker.group(1) not in active_ids(committed_file(BUILD_BRANCH, ITEMS_FILE)):
        raise Refused(f"item {worker.group(1)} is not active in gpt/build:{ITEMS_FILE}; its worker branch is closed")


def staged_blobs() -> list[tuple[str, str]]:
    """(path, blob id) of each file that the commit adds or changes. A submodule has no blob."""
    fields = git("diff", "--cached", "--raw", "-z", "--no-renames", "--no-abbrev", "--diff-filter=d").split("\0")
    blobs = []
    for meta, path in zip(fields[0::2], fields[1::2]):
        parts = meta.split()
        if len(parts) >= 4 and parts[1] != "160000":
            blobs.append((path, parts[3]))
    return blobs


def blob_facts(oid: str, sniff: int) -> tuple[int, bytes, str]:
    """The size, the first `sniff` bytes and the SHA-256 of a blob, read in blocks."""
    digest, size, head = hashlib.sha256(), 0, b""
    with subprocess.Popen(["git", "cat-file", "blob", oid], stdout=subprocess.PIPE) as proc:
        for block in iter(lambda: proc.stdout.read(1 << 20), b""):
            head += block[: sniff - len(head)]
            digest.update(block)
            size += len(block)
    if proc.returncode != 0:
        raise Refused(f"git cat-file blob {oid} failed")
    return size, head, digest.hexdigest()


def in_history(path: str, oid: str) -> bool:
    """The history of HEAD holds the blob `oid` at `path`."""
    return bool(git("log", "-1", "--format=%H", f"--find-object={oid}", "HEAD", "--", path, check=False).strip())


def files_without_origin(cutting: bool) -> list[str]:
    """The staged files that the licence scan skips (a NUL byte in the first SNIFF_BYTES, or more
    than MAX_BYTES) with no row of their path and SHA-256 in the staged PROVENANCE_FILE that names
    an origin and a licence. In a commit that cuts an item, a file that the history of HEAD holds
    at the same path needs no row: the revert of the item brings it back."""
    scan = sibling("licence_scan")
    recorded = {(r.get("path"), r.get("sha256")) for r in tsv_rows(staged_file(PROVENANCE_FILE))
                if all((r.get(f) or "").strip() not in ("", "-") for f in ("origin", "licence"))}
    missing = []
    for path, oid in staged_blobs():
        size, head, sha = blob_facts(oid, scan.SNIFF_BYTES)
        if ((size > scan.MAX_BYTES or b"\0" in head) and (path, sha) not in recorded
                and not (cutting and in_history(path, oid))):
            missing.append(path)
    return missing


def added_lines() -> list[str]:
    """The added lines of each staged text file except PHRASE_FILES (Git shows no lines of a binary file)."""
    excluded = [f":(exclude){name}" for name in PHRASE_FILES]
    lines, in_hunk = [], False
    for line in git("diff", "--cached", "-U0", "--no-color", "--no-ext-diff", "--no-textconv", "--",
                    ":/", *excluded).splitlines():
        if line.startswith("diff --git "):
            in_hunk = False
        elif line.startswith("@@"):
            in_hunk = True
        elif in_hunk and line.startswith("+"):
            lines.append(line[1:])
    return lines


def pre_commit() -> None:
    check_branch(current_branch())
    staged = [n for n in git("diff", "--cached", "--name-only", "-z").split("\0") if n]
    for name in staged:
        if Path(name).name.startswith("._"):
            raise Refused(f"{name} is a macOS companion file; unstage it")
    for line in added_lines():
        hits = banned_hits(line)
        if hits:
            raise Refused(f"a staged line uses a banned phrase ({', '.join(hits)}): {line.strip()[:120]}")

    cutting = cuts_an_item()
    unrecorded = files_without_origin(cutting)
    if unrecorded:
        raise Refused(
            f"{unrecorded[0]} is a binary file or a file above 5 MB, which the licence scan skips"
            + (f" ({len(unrecorded) - 1} more)" if len(unrecorded) > 1 else "")
            + f". Add a row to {PROVENANCE_FILE} in the same commit: path, sha256 (of the staged content), "
            "origin, licence"
        )
    # The commit that cuts an item (ops item cut: the reverts and the item's rows) passes the limit and
    # the check of the other items: an exhausted item refuses every other commit, so the limit, or a
    # second exhausted item, would otherwise lock the branch.
    if cutting:
        return
    head_rows = scoreboard_rows(lambda n: committed_file("HEAD", n)) if rev_ok("HEAD") else []
    rows = scoreboard_rows(staged_file)
    since = commits_since_measurement(head_rows)
    if since >= MEASURE_LIMIT and not new_full_run(rows, head_rows):
        raise Refused(
            f"{since} commits since the last measured commit (limit {MEASURE_LIMIT}). "
            "Run `tools/autobuild/ops measure` (a full run, no --item) and commit its new scoreboard rows first."
        )

    attempts = experiments.read_tsv(experiments.attempts_file())
    corrections = tsv_rows(staged_file(experiments.CORRECTIONS_FILE))
    for item in tsv_rows(staged_file(ITEMS_FILE)):
        if item.get("status") != "active":
            continue
        item_id, metric = item.get("id", ""), item.get("metric", "")
        if experiments.exhausted(rows, attempts, item_id, metric, baseline(item.get("baseline", "")), corrections,
                                 item.get("accept")):
            raise Refused(
                f"item {item_id} used {experiments.experiments(rows, attempts, item_id)} experiments "
                f"with no gain on {metric}. "
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
        was_active = active_ids(committed_file("HEAD", ITEMS_FILE))
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


def outgoing_problems(local: str, remote: str) -> list[str]:
    """Why a push of `local` onto the remote gpt/build at `remote` (zeros: absent) is refused."""
    if not is_zero(remote):
        if rev_ok(remote) is None:
            return ["the remote gpt/build has commits that are not here: fetch and merge first"]
        if not is_ancestor(remote, local):
            return ["the push is not a fast-forward: force pushes are refused"]
    main = rev_ok("refs/remotes/origin/main")
    base = rev_ok(SETTINGS["BRANCH_BASE"])
    if main is None or base is None:
        return ["refs/remotes/origin/main or the branch base is missing: fetch main first"]
    exclude = ["--not", main] + ([] if is_zero(remote) else [remote])
    new = git("rev-list", local, *exclude).split()
    if not new:
        return []
    from_base = set(git("rev-list", "--ancestry-path", f"{base}..{local}").split()) | {base}
    epoch = int(dt.datetime.strptime(BRANCH_EPOCH, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=dt.timezone.utc).timestamp())
    problems: list[str] = []
    foreign: list[str] = []
    for record in git("log", "--format=%H%x1f%ct%x1f%B%x00", local, *exclude).split("\0"):
        if not record.strip():
            continue
        sha, stamp, message = record.strip("\n").split("\x1f", 2)
        if sha not in from_base:
            foreign.append(f"commit {sha[:12]} is not a descendant of the branch base {base[:12]}")
        if int(stamp) <= epoch:
            foreign.append(f"commit {sha[:12]} has a committer date before the start of this branch ({BRANCH_EPOCH})")
        if any(ATTRIBUTION.search(line) for line in message.splitlines()):
            problems.append(f"commit {sha[:12]} holds an attribution line")
        if banned_hits(message):
            problems.append(f"commit {sha[:12]} uses a banned phrase")
    if foreign:
        problems += foreign + ["history of an old local branch must never be published; import its files as a new commit"]
    licence = sibling("licence_scan")
    hits = licence.commit_hits(new, licence.load_index(Path(SETTINGS["LICENCE_INDEX"])),
                               licence.allowed(committed_file(local, ALLOW_FILE)))
    problems += [f"licence scan: {hit}" for hit in hits]
    return problems


def pre_push(url: str, lines: list[str]) -> None:
    if url != SETTINGS["PUSH_URL"]:
        raise Refused(f"push to {url} refused: the only remote is {SETTINGS['PUSH_URL']}")
    for line in lines:
        parts = line.split()
        if len(parts) != 4:
            continue
        local_ref, local_oid, remote_ref, remote_oid = parts
        if local_ref != BUILD_BRANCH or remote_ref != BUILD_BRANCH:
            raise Refused(f"push of {local_ref} to {remote_ref} refused: only {BUILD_BRANCH} to {BUILD_BRANCH}")
        if is_zero(local_oid):
            raise Refused(f"deletion of {remote_ref} refused")
        problems = outgoing_problems(local_oid, remote_oid)
        if problems:
            raise Refused("; ".join(problems))


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
        if not ref.startswith("refs/heads/"):
            continue
        if ref != BUILD_BRANCH and not WORKER_BRANCH.match(ref):
            raise Refused(f"refusing to change {ref}: only gpt/build and gpt/w/ID branches may change")
        if is_zero(new):
            raise Refused(f"refusing to delete {ref}: each branch stays for the review")
        if ref != BUILD_BRANCH:
            continue
        current = rev_ok(ref)
        pushed = rev_ok("refs/remotes/origin/gpt/build")
        if current and not is_ancestor(current, new) and pushed and is_ancestor(current, pushed):
            raise Refused(
                "refusing to rewrite pushed history of gpt/build; undo a change with `git revert`"
            )


def role_table(text: str) -> dict[str, dict[str, object]]:
    """The table of AGENTS.md section 3: the first word of each role, its model id and its efforts (default first)."""
    roles: dict[str, dict[str, object]] = {}
    inside = False
    for line in text.splitlines():
        if line.startswith("## "):
            inside = line.startswith("## 3.")
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")] if inside and line.startswith("|") else []
        if len(cells) < 3 or not cells[0] or cells[0] == "Role" or set(cells[0]) <= set("-: "):
            continue
        model = re.findall(r"`([^`]+)`", cells[1])
        efforts = re.findall(r"`([^`]+)`", cells[2])
        if model and efforts:
            roles[cells[0].split()[0].lower()] = {"model": model[0], "efforts": efforts}
    return roles


def main(argv: list[str]) -> int:
    hook = argv[1] if len(argv) > 1 else ""
    try:
        if hook == "pre-commit":
            pre_commit()
        elif hook == "commit-msg":
            commit_msg(argv[2])
        elif hook == "pre-push":
            pre_push(argv[3] if len(argv) > 3 else "", sys.stdin.read().splitlines())
        elif hook == "reference-transaction":
            reference_transaction(argv[2], sys.stdin.read().splitlines())
        elif hook == "outgoing":
            problems = outgoing_problems(argv[2], argv[3])
            if problems:
                raise Refused("; ".join(problems))
            print(f"guard: {argv[2][:12]} may be pushed onto {argv[3][:12]}")
        elif hook == "roles":
            roles = role_table(sys.stdin.read())
            if not roles:
                raise Refused("AGENTS.md has no role table in section 3")
            print(json.dumps(roles, indent=1, sort_keys=True))
        else:
            print(f"guard: unknown hook {hook!r}", file=sys.stderr)
            return 2
    except Refused as refused:
        print(f"guard ({hook}): {refused}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
