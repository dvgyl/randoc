# Original task text of the owner

This is the first message of the owner, unchanged (2026-11-09).

````text


<pasted_content id="760e">
# Build Randoc: a complete drop-in replacement for Pandoc

## Goal
Randoc is a native Rust reimplementation of Pandoc 3.13, built to be a complete drop-in replacement. Drop-in means a user can put Randoc on their PATH under the name `pandoc`, and everything they already rely on keeps working unchanged:

- every reader, writer, extension and command-line option, with the same defaults and exit codes
- the same output: byte-identical for text formats, and identical document content for ZIP-based formats such as DOCX and EPUB
- the same warnings and error messages
- defaults files, the user data directory, templates, reference documents, CSL styles and bibliography files
- JSON filters, Lua filters, custom Lua readers and writers, and the full `pandoc` Lua module with its global variables
- citation processing, syntax highlighting, math conversion (such as TeX to MathML and to Word equations), and PDF output through the same external engines Pandoc uses
- Pandoc's `lua` and `server` modes

When invoked as `pandoc`, `--version` prints the Pandoc version that Randoc matches, in Pandoc's format, so tools that check the version keep working. A following line names Randoc and its own version.

Randoc should also be faster, lighter, and usable as a Rust library, including in WASM. Compatibility comes first: never accept a difference in behaviour in exchange for speed.

Repository: https://github.com/dvgyl/randoc (currently empty). Licence: MIT OR Apache-2.0.

This codebase is yours. You decide how it is built, and you will keep building on it across many sessions.

I am away for this whole run and will read your handoff when I return. Work autonomously, and finish as fast as possible without lowering the merge gate below.

## Ground truth
Pandoc 3.13 is the oracle. Install exactly that version, record its build details, and treat its behaviour as the definition of correct. It is a test tool only: Randoc must never call it.

Pandoc is GPL-2.0-or-later. Reimplement its behaviour from the manual, its documented interfaces and black-box oracle output. Do not copy Pandoc source code or test files into this repository. Two cases need care:

- Pandoc's own test suite is a strong compatibility check. Clone Pandoc's repository outside the Randoc repository and run its test cases against Randoc as an external check. Never commit those files.
- Byte-identical standalone output needs Pandoc's default templates and data files. Check the licence of each file. Include the ones Randoc's licence allows, with attribution. Record the rest as gaps in the capability matrix.

## Existing work
Before building, study these projects and search for newer ones:
- Carta: https://github.com/mfkrause/carta
- Ferrodoc: https://github.com/alexgaoth/ferrodoc
- Rescribe: https://github.com/rhi-zone/rescribe

Test their claims instead of trusting READMEs, and write `docs/COMPETITIVE_ANALYSIS.md`. Reuse permissively licensed libraries where they genuinely help, with attribution. Randoc must earn its place through complete verified compatibility, a reproducible conformance suite and a clean library API, not by repackaging another converter.

## First session: foundations before features
1. Set up the empty repository: a Cargo workspace with crates named `randoc-*` and a `randoc` binary, CI, licence files and a README.
2. Create `init.sh` to build, test and run the harness in one command. It must also install a `commit-msg` git hook that removes any `Co-Authored-By:`, `Claude-Session:` or "Generated with" attribution lines from commit messages.
3. Build a differential harness that runs the same input and options through Pandoc and through Randoc. Compare exit codes, warnings and errors, the AST (via JSON) and writer output. Track byte-identical and content-identical results separately. For ZIP formats, compare normalized contents and ignore timestamps and file order.
4. Set up the external run of Pandoc's own test suite against Randoc.
5. Generate the inventory of formats, extensions, options, highlighting languages and styles from `pandoc --list-*` and `pandoc --help`.
6. Write results to a machine-readable capability matrix in `spec/`, with a status per format, extension and option: not started, partial, passing, failing, or not applicable. All coverage numbers come from test runs.
7. Create `PROGRESS.md` and `spec/status.json` to hold project state.

Then build the first vertical slice: the AST with Pandoc-compatible JSON, and Markdown to HTML passing against the oracle.

## Parallel work
Speed comes from parallelism, so run as many workstreams at once as the work allows. Quality comes from the merge gate below, which no workstream skips.

The shared contracts are the critical path: the AST and its JSON, the reader and writer traits, the error types and the harness. Build these yourself first and freeze them. Then fan out. Each reader, each writer and each subsystem can be its own Opus 5.5 subagent, in its own git worktree and branch. The subsystems include the CLI and defaults files, templates, JSON filters, the Lua runtime and `pandoc` module, citations, syntax highlighting, math conversion, archive handling, PDF engine integration, the `server` mode, fuzzing and benchmarks. When a workstream finishes, start the next one immediately. Only you change shared contracts. If a subagent needs a change, it tells you, and you make the change once for everyone.

Brief each subagent with the files it owns, the interfaces it must not change, and the harness tests that define done.

When choosing what to start next, prefer the most-used formats first: Markdown variants, HTML, LaTeX, Typst, JSON, DOCX, EPUB and ODT. Then every remaining format, performance work and WASM builds. If an area is blocked, record the blocker and give that capacity to another workstream.

## Review and merge gate
The bar for merging is deliberately high. Randoc is only useful if users can trust that it behaves exactly like Pandoc. A workstream merges into `main` only when all of these hold:

1. **No regressions.** The harness keeps a record of every case that passes on `main`. Any case that moves from passing to failing blocks the merge.
2. **Measured progress.** The work raises the verified pass count on the harness, on Pandoc's own test suite, and on a corpus of real, openly licensed documents, and the capability matrix shows it. Record every remaining difference from Pandoc as a known divergence with its reason.
3. **Tests that catch real bugs.** Every new behaviour has tests built from oracle output, including the edge cases the Pandoc manual describes for that format and its extensions. Run `cargo mutants` on the changed code: a surviving mutant means a missing test, so add tests until every survivor is killed or explained. Report coverage of new code with `cargo llvm-cov`; anything below 85% needs a written reason.
4. **No panics on any input.** Fuzz every new or changed parser and archive reader for at least 15 minutes with no crashes. Code that input can reach contains no `todo!()`, `unimplemented!()`, `unwrap()` or `expect()`.
5. **Two independent reviews, run in parallel.** One comes from GPT-6.1 Sol in Fast mode through the Codex CLI, called non-interactively, for example `codex exec -m gpt-6.1-sol -c service_tier="fast" "<review brief>"` (confirm the exact flags with `codex exec --help` first). The other comes from a fresh-context Opus 5.5 subagent that did not work on the implementation. Ask both to break the work: find counterexamples against Pandoc, write failing tests, and check parser and archive security, `unsafe` code and misleading coverage claims. Fix every critical, high and medium finding and retest; record low findings in `docs/ISSUES.md`. Merge only when both reviewers approve the fixed version. If Codex is unavailable, record that and use a second fresh-context Opus 5.5 reviewer.
6. **No slowdowns.** Benchmarks on the affected paths are no more than 5% slower than `main`.
7. **Clean build.** `cargo fmt --all -- --check`, clippy with `-D warnings` and `cargo doc` all pass with no warnings, and every public item is documented.

Run these checks and reviews while other workstreams keep building, so the pipeline never sits idle waiting for a gate. If a gate truly cannot apply to a specific case, such as fuzzing a format with no parser, write down why in `PROGRESS.md` and continue. Never lower the bar without recording it.

Report only reviews and delegation that actually happened.

## Quality bar
- Implement general logic, never special cases for specific tests. If a test or an oracle output looks wrong, record it; never edit an expected result to make a test pass.
- Treat all input as untrusted: limit nesting depth, decompressed size, archive paths and external fetches. Keep Pandoc's default behaviour, and add a restricted mode for server use.
- Write safe Rust. Any `unsafe` block needs a written safety argument and a focused test.
- Keep benchmarks in `benchmarks/` with raw data, environment details and exact commands. Measure process startup separately from conversion time.

## Definition of done
Randoc is done when every format, extension and option in the inventory passes the harness (byte-identical for text formats, content-identical for ZIP formats and PDF), Pandoc's own test suite passes against Randoc, and real downstream tools such as Quarto and R Markdown give the same results when configured to use Randoc in place of Pandoc. Until all of that holds, there is always a next task. Report compatibility only from measured results, and never claim more than the tests show.

## Git
Commit and push to `main` after every major block, as soon as it passes the merge gate. Never batch several blocks into one commit. Use conventional commit messages such as `feat(readers): add docx reader`.

Commits have no co-author or AI attribution: never add `Co-Authored-By:` trailers, `Claude-Session:` lines or "Generated with" footers, and keep the commit-msg hook installed in every session.

These steps need no approval: commits, merges into `main`, pushes to `main`, new branches and worktrees, dependency and library choices, architecture decisions, and retrying failed commands.

Never force-push, rewrite published history, delete files you did not create, or expose credentials. If a push fails, keep the local commits and record why.

## Keep going
This job spans many sessions, and your context will be compacted. Keep `PROGRESS.md` and `spec/status.json` current enough to resume from them and the git log alone. At the start of each session, read them, run `init.sh`, and confirm the harness passes before starting new work.

Finishing a milestone is not a reason to stop or offer to continue: pick the next item and do it. When you meet ordinary obstacles such as errors, timeouts or flaky tools, diagnose them and retry, fix the call, or find another route. Stop work on an area only for a deliberate blocker, such as withheld access: record it and move to other work.

Before writing any status, check each claim against a command result from this session. If something failed or is unverified, say so.

## Handoff
When you finish or run out of budget, leave the repository compiling, tested, committed and pushed. Write a handoff at the top of `PROGRESS.md` for a reader who has not seen your work: what works (with harness and Pandoc test-suite numbers), what does not, open issues, and the next three tasks. Put the key points first, use plain full sentences, and avoid shorthand you invented during the work.
</pasted_content id="760e">

I have enabled bypass permissions and please dont ask me for anything and please sotre everything in the external drive (Extreme Pro) since the laptop's internal drive doesnt have much space
````
