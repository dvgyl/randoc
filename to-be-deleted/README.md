# Files staged for deletion

This folder holds obsolete and historical files from earlier development
sessions (Claude sessions 1–6 and one-time research snapshots) so they do not
clutter active repository navigation.

Once reviewed, this entire folder can be removed in a single commit.

| Path | Original location | Why it is staged for deletion |
| --- | --- | --- |
| `MERGE_LOG.md` | `docs/MERGE_LOG.md` | 235 KB merge transcript from sessions 1–6 (superseded by git history on `main`). |
| `OWNER_INSTRUCTIONS.md` | `docs/OWNER_INSTRUCTIONS.md` | Historical session 1–6 prompts and temporary rate-limit rules (superseded by `AGENTS.md` and `docs/MODEL_ROUTING.md`). |
| `WORKER_STANDING_ORDERS.md` | `docs/WORKER_STANDING_ORDERS.md` | Old multi-worktree worker standing orders referencing `ws/*`, `wip/*`, and `env/store/`. |
| `ORIGINAL_SPEC.md` | `docs/ORIGINAL_SPEC.md` | Raw Nov 9 bootstrap prompt (distilled requirements live in `README.md`, `AGENTS.md`, and `docs/ARCHITECTURE.md`). |
| `PROGRESS-sessions-1-to-6.md` | `PROGRESS.md` | Session 6 handoff snapshot (replaced by the current `PROGRESS.md`). |
| `COMPETITIVE_ANALYSIS.md` | `docs/COMPETITIVE_ANALYSIS.md` | One-time Nov 10 comparative benchmark survey. |
| `research-data/` | `docs/research-data/` | Raw TSV/scripts backing `COMPETITIVE_ANALYSIS.md`. |
