# Blocked on owner

Blockers that only the owner can remove. `tools/autobuild/ops blocked "TEXT"`
writes each entry. The team continues with other work.

Entry heading: `## UTC open`. When the blocker is gone, the owner changes
`open` to `closed`. Each entry says what is blocked, what the owner must do,
and what the team does in the meantime.

## 2026-11-17T22:24:12Z open

- Blocked: how users get the data pack: the reference documents, `epub.css`, the translations, `default.csl`, the CSL locales, the Lua data files and 132 syntax definitions (`plan/ARCHITECTURE.md` 4.7). Their licences keep them out of the repository. The final goal needs `install.units` at TOTAL/TOTAL with the distributed package (`plan/SPEC.md` section 7, item 9), so the package needs a lawful way to deliver these files.
- The owner must: decide how a user gets these files (for example a separate data package with its own licences, or a tool that makes the pack from the user's own Pandoc installation), and where the program finds them.
- Meanwhile the team: measures with the data pack that `tools/autobuild/datapack` makes from the oracle. The repository holds none of these files. Item M8-09 builds the install measurement with that data pack.

## 2026-11-17T22:24:12Z open

- Blocked: the installation of Quarto, R with `rmarkdown`, and the real PDF engines. Neither Quarto nor R is installed. `downstream.quarto` and `downstream.rmarkdown` cannot pass, and `pdf.units` has real-engine units only for installed engines (today only `tectonic`). The final goal needs all 15 PDF engines as real engines.
- The owner must: install a pinned Quarto, a pinned R with `rmarkdown`, and each PDF engine that the build sandbox cannot install, or permit the team to install them. The versions and SHA-256 values go to `spec/downstream.toml` and `spec/pdf-engines.toml`.
- Meanwhile the team: writes the `downstream.*` rows with each (project, output format) unit failing (0/TOTAL with TOTAL above 0, never 0/0), and measures the PDF routes with the recording stand-ins.

## 2026-11-17T22:24:12Z open

- Blocked: the licence decisions D1 to D6 of `PROGRESS.md` (section on licence decisions). D1: the GPL data files of Pandoc (reference documents, translations, `epub.css`, Lua files, 98 copyleft syntax definitions). D2: `default.csl` (CC BY-SA 3.0). D3: the abbreviation list and the DocBook entity list. D4: 34 syntax definitions with no licence grant. D5: test inputs from copyleft documents (the examples of the CommonMark specification, example text of the Pandoc manual). D6: Pandoc's completion scripts in the history of `main` (see the entry on old commits).
- The owner must: decide each of D1 to D6.
- Meanwhile the team: follows the working rules of `PROGRESS.md` and `plan/SPEC.md` section 8. None of these files enters the repository. The abbreviation list comes from tests of the oracle; the DocBook entity table comes from the caller. Syntax definitions with no grant load at run time from a user installation, so byte-equal highlighting of those languages stays a gap. The team writes its own test inputs.

## 2026-11-17T22:24:12Z open

- Blocked: old commits of `main` hold GPL text of Pandoc: the completion scripts (from commit `6089e1f`), FODT results of the suite `templates`, three table inputs of the suite `native`, and the two scripts of decision D7 in stored results (`PROGRESS.md`, D6 and D7). The current files of `main` do not hold them. A removal needs a rewrite of published history and a forced push.
- The owner must: decide whether the history of `main` stays as it is or is rewritten outside the build.
- Meanwhile the team: never rewrites history, and never merges, fetches or pushes the old `ws/*` and `wip/*` branches. `licence_scan.py scan --all` marks a hit that `origin/main` already has as `inherited`.

## 2026-11-17T22:24:12Z open

- Blocked: nothing stops; this entry is an optional hardening. GitHub cannot limit the owner's credential to one branch. The broker pushes with that credential; the broker's code, the Git guard and the GitHub ruleset (no forced push, no deletion of `main` and `gpt/build`) limit what it pushes.
- The owner must (optional): give the broker a separate GitHub identity, for example a deploy key and a ruleset that lets only that key push `gpt/build`, with an admin bypass for the owner.
- Meanwhile the team: has no GitHub credential and pushes only with `tools/autobuild/ops push`. The broker pushes only `gpt/build` to `gpt/build`, fast-forward, after the licence scan of each commit.

## 2026-11-19T10:20:54Z open

- Blocked: M1-12 left the M1 gate without a lua.api row. The worker recorded 0/468 on its branch, which meets the coverage rule for a new metric. The lead guard counted all three attempts and had no scored row on gpt/build. The item was cut and its branch gpt/w/M1-12 stays at 375d659. The measured row predates the final probe fix. AGENTS.md sections 6 and 9 prevent a repeat item for the same cause or a merge of the cut branch.
- The owner must: Specify a recovery path for the cut Lua measurement under AGENTS.md sections 6 and 9.
- Meanwhile the team: Complete the reader measurement and the other independent M1 items.
