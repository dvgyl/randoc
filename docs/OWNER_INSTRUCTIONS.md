# Instructions of the owner

Read this file at the start of each session, before any work. It holds each
instruction that the owner gave. The words of the owner are quoted without
changes. A later instruction replaces an earlier one where they differ.

## 1. The task (2026-11-09)

The full task text is in [`ORIGINAL_SPEC.md`](ORIGINAL_SPEC.md). Read it
completely. Its main rules:

- Randoc is a native Rust reimplementation of Pandoc 3.13 and a complete
  drop-in replacement: each reader, writer, extension and option;
  byte-identical text output; the same warnings, errors and exit codes.
- Pandoc 3.13 is the oracle and a test tool only. "Randoc must never call it."
- "Pandoc is GPL-2.0-or-later. Reimplement its behaviour from the manual, its
  documented interfaces and black-box oracle output. Do not copy Pandoc
  source code or test files into this repository." For external Pandoc
  tests: "Never commit those files."
- Each subsystem is an Opus subagent in its own worktree and branch. "Only
  you change shared contracts."
- The merge gate has 7 points (`CONTRIBUTING.md`). "Merge only when both
  reviewers approve the fixed version." "Report only reviews and delegation
  that actually happened."
- "Treat all input as untrusted: limit nesting depth, decompressed size,
  archive paths and external fetches."
- "Commits have no co-author or AI attribution: never add `Co-Authored-By:`
  trailers, `Claude-Session:` lines or "Generated with" footers, and keep the
  commit-msg hook installed in every session."
- "Never force-push, rewrite published history, delete files you did not
  create, or expose credentials. If a push fails, keep the local commits and
  record why."

## 2. Later instructions, in the order that the owner gave them

1. "I have enabled bypass permissions and please dont ask me for anything and
   please sotre everything in the external drive (Extreme Pro) since the
   laptop's internal drive doesnt have much space"
2. "just a note - please dont write the handoff before 10 am UTC. Keep working
   in this thread till then" (This applied to the first session only. The
   owner ended that session earlier; see instruction 7.)
3. "use /pstack:poteto-mode as the general engineering skill for all the code
   written"
4. "dont make randoc a drive / just put everything in the randoc folder in the
   Extreme Pro/Randoc"
5. "we are aiming fo a full compatibility here (just to clear it up) Every
   format, extension and option passes the harness, Pandoc's own test suite
   passes, and Quarto and R Markdown give the same results."
6. "Follow-up guidance: you don't have to build everything from scratch. Much
   of what Randoc needs already exists in open source, so study it and build
   on it wherever that's faster or more reliable. Before reusing anything,
   record it in docs/PRIOR_ART.md, write a short ADR saying why you chose it,
   and check its licence: the repo is public, so the licence must be
   compatible, and attribution must be preserved in a THIRD_PARTY_NOTICES
   file. Prefer depending on a pinned upstream release over copying code.
   Fork or vendor only when you need to change it, and keep those changes
   small, documented and covered by our tests. Hold reused code to the same
   quality bar as our own code."
7. "also you can prepare the handoff now, do it now and pause all the
   background processes safely, ill start you up in a new threads"
8. "just please giveme a prompt to paste into a new thread and save every
   instruction and everything in the repo and ask the handoff to read that"
9. The start message of the second session (2026-11-10): "Continue the
   Randoc project as coordinator. [...] Then do the start steps of the
   handoff (env.sh, init.sh, the watchdog, the memory limits) and continue
   with its "Next three tasks", in order. Work autonomously and do not ask me
   questions. Keep everything on the external drive. Use
   /pstack:poteto-mode for all code. Apply the reuse rule: study and reuse
   open source before you write new code. Commit and push to main after each
   block that passes the merge gate, with no co-author or AI attribution."
10. "now please do not handoff before 10 PM UTC and keep working in this
    thread" (Given in the second session, 2026-11-10. The time is 22:00 UTC
    on 2026-11-10.)
11. "Please use fable 5.2 extensively for the next few tasks or do the fable
    specific tasks first" (Given in the second session at about 10:40 UTC.
    How the coordinator applies it: from this time, a new worker is an
    agent on the model Fable 5.2, and the work that needs the strongest
    model starts first: a port from upstream source, an engine design, a
    new subsystem. The two independent reviewers of gate 5 stay as the task
    text defines them, GPT-6.1 Sol through the Codex CLI and an Opus 5.5
    agent with a new context, so that a reviewer is not the model that
    wrote the code.)
12. "Please at the next natural stopping point, prepare the handoff the
    fable limit is at 96%" (Given in the second session at about 12:20 UTC.
    It replaces instruction 10. The coordinator told each worker to commit
    its state and to stop, and wrote the handoff.)

## 3. Writing style

The owner's global rule: write all prose (answers, documents, comments,
commit messages) in ASD-STE100 Simplified Technical English. Use short,
direct sentences, the active voice and one term for one concept. Explain a
technical term that a reader possibly does not know. Technical accuracy has
priority.

## 4. Where the rest is

| File | Content |
| --- | --- |
| `PROGRESS.md`, section "Handoff" | The state of `main` and of each branch, the open issues, the next three tasks |
| `spec/status.json` | The same state for programs |
| `docs/CONTRIBUTING.md` | The merge gate, the severity levels, the reuse procedure |
| `docs/PRIOR_ART.md`, `docs/ADR.md` | Material in use, candidates, decision records |
| `docs/WORKER_STANDING_ORDERS.md` | The rules that each worker agent gets (a copy of `env/store/standing-orders.md`, which is the file that workers read) |
| `docs/MERGE_LOG.md`, `docs/ISSUES.md` | The evidence of each merge; the open differences |

Not in the repository, only on the drive under
`/Volumes/Extreme Pro/Randoc/env/store/`: the briefs, the worker reports,
the review reports, the decision log (`decisions.tsv`) and the branches
`ws/*`. Some of these files quote test inputs with a copyleft licence, so
they must not go into the public repository.
