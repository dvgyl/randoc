# Standing orders for each Randoc agent

Randoc is a Rust reimplementation of Pandoc 3.13. The goal is a complete
drop-in replacement: the same output bytes, warnings, errors and exit codes.
The owner is away. Work without questions to the owner.

## 1. Where you work

1. Your brief names your worktree `"/Volumes/Extreme Pro/Randoc/wt/NAME"` and your branch `ws/NAME`.
2. Change files only in your worktree. The folder `"/Volumes/Extreme Pro/Randoc/randoc"` is the main checkout. It belongs to the coordinator. Do not edit it and do not commit there.
3. Start each shell command with: `. "/Volumes/Extreme Pro/Randoc/env/env.sh" && cd "$RANDOC_WT/NAME" &&`. The paths contain a space. Always quote them.
4. Read these files in your worktree first: `docs/ARCHITECTURE.md`, `docs/CONTRIBUTING.md`, `conformance/README.md`.

## 2. The oracle and the licence boundary

5. `$RANDOC_ORACLE_PANDOC` is Pandoc 3.13. Its behaviour is the definition of correct. Use it as a black box as often as you need. Randoc code must never call it.
6. Pandoc is GPL. Do not open the Haskell source files of Pandoc (`$RANDOC_PANDOC_SRC/src`, `pandoc-lua-engine`, `pandoc-cli`, `pandoc-server`, `citeproc`). Do not copy Pandoc test files into the repository. You can read `$RANDOC_PANDOC_SRC/MANUAL.txt`, `doc/`, `changelog.md` and `data/`.
7. You can study and port libraries with a permissive licence (for example the BSD libraries `doclayout`, `doctemplates`, `commonmark-hs`, `skylighting-core`, `citeproc`). Check the licence file first. Add a row to `THIRD_PARTY.md`, and put the full upstream licence file (copyright notice, conditions, disclaimer) in `third_party/licences/NAME.txt`. A licence name alone is not sufficient for ported code. Name the exact upstream version.
7c. Reuse rule of the owner (2026-11-10). Before you write a subsystem, look for open-source code that does the same work, and build on it where that is faster or more reliable. Start with `docs/PRIOR_ART.md` (the candidates) and `docs/ADR.md` (ADR 1). Before you reuse anything: (a) check its licence file at the exact release (compatible with `MIT OR Apache-2.0`; Pandoc itself and `texmath` are GPL and are not permitted); (b) add a row to `docs/PRIOR_ART.md`; (c) write a short record in `docs/ADR.md` that says why you chose it; (d) keep the attribution in `THIRD_PARTY_NOTICES.md` (the new name of `THIRD_PARTY.md` on `main`; a branch from before the rename keeps the old name until it merges `main`) and the licence text in `third_party/licences/`. Prefer a dependency on a pinned upstream release (exact version) to a copy. Fork or vendor only when you must change the code; keep the changes small, documented and covered by Randoc tests. Reused code passes the same gate as your own code: oracle-derived tests, limits for untrusted input, fuzzing of the paths that input reaches. A new dependency is a change to the root `Cargo.toml`: tell the coordinator in your report.
7a. A test input is a data file too. Write your own inputs for conformance cases. Do not copy example text from the Pandoc manual (GPL), from the CommonMark or GFM specification (CC-BY-SA), or from another document that has no permissive licence. For a public test suite with such a licence, write a tool under `tools/` that downloads the suite at a pinned version with a SHA-256 hash to a folder outside the repository, runs both programs, and stores only numbers and case identifiers in `spec/` (see `tools/pandoc-testsuite` and `tools/corpus`). Inputs from a test file with a permissive licence (MIT, BSD, Apache-2.0) are permitted with a row in `THIRD_PARTY.md`.
7b. A crate that holds ported code or copied data from another project carries the notice inside the crate: one file `crates/NAME/NOTICE.md` with a short list of the material and its source, then the full licence texts (see `crates/randoc-layout/NOTICE.md` on `main`). Keep the copy in `third_party/licences/` too. A source package of one crate does not include `third_party/`.
8a. Identifiers in `docs/ISSUES.md` carry the name of your workstream, so that two branches never use one identifier: `I-LAYOUT-01`, `I-NATIVE-01`, `I-JSON-01`, `I-TPL-01`, `I-LATEX-01`, `I-MD-01`, `I-CM-01`, `I-CLI-01`, `I-HTML-01`. Do not use a plain number such as `I-005`.
8b. An in-place mutation run changes source files in your worktree while it runs. Reviewers read and build your worktree. Thus do not start `cargo mutants --in-place` while a review of your branch is in progress. Run it before you report, or after the coordinator tells you that both reviews ended.
8. Implement general logic. Do not add a special case for one test. Do not edit an expected result to make a test pass. If an oracle result looks wrong, keep it and record it in `docs/ISSUES.md`.

## 3. Shared contracts

9. Do not change `crates/randoc-ast`, `crates/randoc-core`, `crates/randoc-harness`, `scripts/`, `spec/`, `PROGRESS.md`, `docs/MERGE_LOG.md`, `docs/ARCHITECTURE.md`, `docs/CONTRIBUTING.md`, or a crate that a different workstream owns.
10. If you need a change to a shared contract, do not work around it with a copy. Send the request to the coordinator with `SendMessage` to `main` (one clear sentence first, then the exact change). Continue with the other parts of your work.
11. You can add lines to these shared files, and you must not change the lines that exist: the root `Cargo.toml` (`members`, `exclude`, `[workspace.dependencies]`), `crates/randoc/Cargo.toml` and `crates/randoc/src/formats.rs` (your registration), `docs/ISSUES.md` (your known divergences), `THIRD_PARTY.md`.
12. Keep dependencies few. Each new dependency needs a reason in your report. Use only MIT, Apache-2.0, BSD, ISC, Zlib or Unicode licences.

## 4. The drive is exFAT

13. Each file uses 2 MB of disk space or more. Keep the number of files low. Put many conformance cases in one case file. Use at most 15 case files for one workstream.
14. macOS writes a `._NAME` companion file next to each file. Never commit one (`.gitignore` covers them). Code that lists a directory must skip names that start with `._`.
15. Do not make release builds, except for a benchmark. Do not turn on incremental compilation.
16. Run fuzzing only with `scripts/fuzz.sh`. It keeps the corpus in memory.
17. For mutation tests use `cargo mutants --in-place` (without `--jobs`; the two options do not combine). After the run, `git status` must show no change to source files.
    The conformance suite is a test of the program (`crates/randoc-cli/tests/conformance.rs`, on `main` since the commit "test(harness): run the conformance suite as a test of the program"). It checks `spec/baseline.json` and, when it exists, `target/harness/local-baseline.json`. Write the local file for your branch with `target/debug/randoc-harness run --baseline target/harness/local-baseline.json --update-baseline`. Then run the mutation tool with `--test-package YOUR_CRATE --test-package randoc-cli`, so that the conformance units judge each mutant too.
    Mutation tests have a time budget of 30 minutes for one workstream, because each mutant needs a full build. A new crate has more mutants than the budget permits. Then test a sample: use `--shard 1/N` with N chosen so that the run fits in the budget, and add `--timeout 60`. Kill or explain each survivor in the sample. Report the number of mutants in the crate (`cargo mutants --list | wc -l`), the sample size and the result. The coordinator records the sample as a limit of gate 3.
18. The machine has 8 cores for all agents. Run one heavy command at a time. Run a command that needs more than 2 minutes in the background.
18a. MEMORY. On 2026-11-10 at 04:40 UTC the computer stopped, because the memory ran out (16 GB, many agents). Rules since then: (a) the `cargo` wrapper makes each heavy cargo command wait for one of 3 slots and for 20% free memory; it prints `randoc: cargo waits for a build slot` while it waits. This is normal: do not stop it and do not start a second command. Run each cargo command that can wait in the background and wait for its end. (b) Never run two heavy commands at the same time (a build, a test run, a fuzz run, a mutation run, a coverage run, a run of many oracle processes). (c) Run the oracle in one process for many inputs where you can, and never more than 2 oracle processes at the same time. (d) A fuzz run uses much memory: run it alone, and only when a parser or a decoder changed since the last run. (e) Keep a mutation sample to 15 minutes in a correction round. (f) Commit often: work that is not committed can be lost.
18b. THE ORACLE AND MEMORY. On 2026-11-10 one oracle process used 22 GB of memory for one repeated-pattern input and filled the swap space. `env.sh` now sets `GHCRTS=-M3g`, which limits the heap of each oracle process to 3 GB. A run that reaches the limit ends with exit code 251 and the text `Heap exhausted`: record it as "the oracle did not end", not as a result of Pandoc. Never unset or raise this limit. Run each oracle process with a time limit of at most 60 seconds, and source `env.sh` in the same shell that starts the oracle. Your own test programs must also not use much memory: before you run an input that can make a large tree or a large output (deep nesting, alias expansion, repeated patterns above 100,000 items), start with a small size and raise it in steps. A watchdog stops each Randoc process that uses more than 5 GB.
19. Put temporary files in `$TMPDIR` and delete them when you finish. All agents share `$TMPDIR`: put the name of your workstream in each file name (for example `$TMPDIR/cli-mutants.log`), or make your own folder `$TMPDIR/NAME/`.
19a. DELETING FILES. On 2026-11-10 a reviewer deleted each file of the folder `env/store/briefs` by mistake: its command was `cd SCRATCH && ls ...; python3 DELETE`, the `cd` failed, and the delete ran in the folder where the shell started. Rules since then: (a) delete only files that you created, and only under your own scratch folder; (b) give the delete command the full absolute path of that folder (`rm -rf "$TMPDIR/NAME-scratch"`), and run `ls` on the same path first; (c) never write a delete command that depends on the current folder, and never join it to a `cd` with `;`; (d) never delete, move or overwrite a file under `env/store`, in the main checkout, or in the worktree of a different workstream; your report file is the one file that you write there. If you delete something by mistake, stop, do not try to repair it, and say so at the top of your report.

## 5. Tests and the gate

20. Each behaviour needs conformance cases in `conformance/cases/SUITE/` with oracle results (`randoc-harness record`). Take edge cases from the Pandoc manual and from your own experiments with the oracle.
21. Unit tests go in the source file (`#[cfg(test)]`). Expected values come from the oracle.
21a. A time test that runs in the gate must not fail on correct code when the machine has a high load. Prefer a deterministic measure: count the steps of the algorithm (a counter that exists only in test builds) and compare the count with a linear bound. If you measure time, measure 2 sizes, take the minimum of 5 runs for each size, and compare the ratio with a wide margin (a factor of 10 for a size factor of 4). Do not use one run and do not use a tight absolute limit. Keep each such test below 20 seconds on the loaded machine; put a larger version behind `#[ignore]`.
21b. Depth. The reviews found the same defect in five crates: a helper that recurses or copies a subtree before the depth check (plain text of inlines, a search of a subtree, a section tree built from a flat list of headings, a clone of a list item, the drop of an own tree type). Each crate that walks a document tree or a nested input has two tests that run on a thread with a 2 MB stack in a debug build: (1) each container kind, and each helper that walks a tree, at 10,000 and 100,000 nested levels with the default limit gives the depth error or a correct result, with no stack overflow; (2) the default limit of 128 itself is safe for each container kind. A flat input that makes a deep structure (heading levels 1 to 10,000) is one of the cases.
21c. Time. The reviews found quadratic time in six crates. Do not use `contains`, `position`, `iter().any`, `remove(0)`, `insert(0, ..)`, a copy of the rest of the input, or a scan to the end of the input inside a loop over the same input. Use a set, an index, a cursor, or a memory of failed searches. Before you report, measure each input class that repeats one construct 50,000 times (each delimiter, each list or table part, each attribute list, each kind of nesting) at 2 sizes, and put the numbers in your report.
    Harness rules: a stored oracle result belongs to one command line, one standard input and one set of files. After you change a case, run `randoc-harness record` again; a result for the old input counts as "not recorded". The `args` of a case replace the `args` of the case file. A run that the harness stops at the time limit is "no result" and never passes.
22. Before you report, all of these must hold in your worktree:
    - `scripts/gate.sh` passes (format, clippy with `-D warnings`, docs, tests, harness baseline).
    - `cargo mutants` on your diff: each surviving mutant is killed by a new test or explained in your report.
    - Coverage: 85% line coverage or more for your crate, or a written reason. On this drive use `randoc-cov --package YOUR_CRATE` (it is on the `PATH` after `env.sh`). Plain `cargo llvm-cov` fails here with "no profile can be merged", because macOS adds `._` files next to the profile files.
    - Each parser or archive reader has a fuzz target in `crates/YOUR_CRATE/fuzz/` and 900 seconds of fuzzing with no crash.
    - No `unwrap()`, `expect()`, `todo!()`, `unimplemented!()`, `panic!()` or `unsafe` in code that input can reach. Each public item has documentation.
    - Recursion has a depth limit from `context.limits.max_nesting_depth`.

## 6. Git

23. Commit to your branch often, with conventional commit messages (`feat(markdown): add pipe tables`). A commit message has no co-author line and no tool attribution.
24. Do not push. Do not rebase. Do not change `main`. Do not delete a file that you did not create. Do not merge a branch of a different workstream. Merge `main` into your branch only when the coordinator tells you to.

## 7. Writing

25. Write all prose (documentation, comments, commit messages, your report) in simple technical English: short sentences, active voice, one term for one concept, no idioms.
26. Keep a code comment only when it gives a reason that the code cannot show.

## 8. Your report

27. Commit your work in a state that passes `scripts/gate.sh`, even if the work is not complete. Partial work that passes the gate can merge.
28. Write your report to `"/Volumes/Extreme Pro/Randoc/env/store/reports/NAME.md"` and give the same text as your final answer. Include:
    - Status: complete, partial or blocked. Branch and HEAD commit.
    - Harness numbers for your suites: units, pass, fail. The output of the last `randoc-harness run`.
    - Mutation tests: caught, missed, unviable, timeouts. The reason for each survivor.
    - Coverage: the line percentage of your crate.
    - Fuzzing: each target, the seconds, the number of runs, the crashes.
    - Known divergences from Pandoc, each with its reason. Add them to `docs/ISSUES.md` also.
    - Requests for changes to shared contracts.
    - The next steps that you recommend.
29. Report only the checks that you ran. If a check failed or did not run, say so.
