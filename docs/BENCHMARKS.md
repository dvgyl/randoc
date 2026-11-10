# Benchmarks

The benchmark tool measures how much CPU time a Randoc program needs. It also
compares two programs, for example a branch and `main`. Gate 6 of
`docs/CONTRIBUTING.md` uses it: the benchmarks of the changed paths must not be
more than 5% slower than `main`.

## Files

| Path | Purpose |
| --- | --- |
| `tools/bench/run.py` | Runs the benchmarks, writes the raw data, compares two programs. |
| `tools/bench/make_inputs.py` | Writes the files in `benchmarks/inputs/`. |
| `tools/bench/make_slow.py` | Makes a wrapper program that is slower by a known factor, to test the tool. |
| `tools/bench/test_run.py` | The unit tests. |
| `benchmarks/benchmarks.toml` | The benchmarks. |
| `benchmarks/inputs/` | The inputs: one Markdown section repeated 1, 10 and 100 times, and its JSON tree from the oracle. |
| `benchmarks/results/` | The first baselines: `main.json` (Randoc) and `oracle.json` (Pandoc 3.13). |

The tools use Python 3.11 or later and only the standard library. `make_slow.py`
also needs a C compiler (`cc`).

## Commands

Build the release program one time. A benchmark is the only permitted reason
for a release build.

```sh
cargo build --release -p randoc-cli
```

Measure one program:

```sh
python3 tools/bench/run.py --program target/release/randoc --out "$TMPDIR/bench.json"
```

Compare the program under test with a reference program:

```sh
python3 tools/bench/run.py --program target/release/randoc \
    --compare "$TMPDIR/randoc-main" --filter json --runs 30 --max-runs 480 \
    --out "$TMPDIR/bench.json"
```

| Option | Meaning |
| --- | --- |
| `--program PATH` | The program under test. |
| `--compare PATH` | The reference program. Without it, the tool measures one program. |
| `--filter TEXT` | Run only the benchmarks whose name contains TEXT. The tool also runs the startup benchmark of each selected benchmark. |
| `--runs N` | The number of timed runs of each benchmark for each program. The default is 30. The minimum is 8. |
| `--max-runs M` | With `--compare`: give an inconclusive benchmark more runs, up to M (see "More runs and the error allowance"). The default is N. |
| `--alpha A` | With `--compare`: the error allowance of this invocation. The default is 0.01. |
| `--timeout S` | The time limit of one run in seconds. The default is 60. At the limit, the tool kills the process group of the run and reports an error. |
| `--benchmarks FILE` | Another benchmarks file. The default is `benchmarks/benchmarks.toml`. |
| `--out FILE` | The JSON file for the raw data. |

| Exit code | Meaning |
| --- | --- |
| 0 | Each comparison is `same`, `faster`, `new` or `not-supported`. |
| 1 | A benchmark is `slower`, or it is `lost-support`. Gate 6 fails. |
| 2 | A run failed: an exit code that is not 0, 21 or 22, or the time limit. |
| 3 | A comparison is `inconclusive`. The data cannot show that the slowdown is 5% or less. Gate 6 is not decided. |

Run the unit tests:

```sh
python3 -m unittest discover -s tools/bench
```

Write the inputs again after a change to the section text in
`make_inputs.py`:

```sh
python3 tools/bench/make_inputs.py --oracle "$RANDOC_ORACLE_PANDOC"
```

## The benchmarks file

`[[benchmark]]` is a name, a command line and a standard input. `[[series]]`
expands to one benchmark for each scale, with the name `NAME-xSCALE`, and to
one startup benchmark, `NAME-startup`. The startup benchmark uses the same
arguments as the series, with an empty document (`startup_stdin`) as input.
Thus the startup cost and the conversion use the same reader and writer. The
comments at the top of `benchmarks/benchmarks.toml` describe each key.

The tool gives each input on the standard input, not as a file argument. Then
the command line is the same for each scale.

A program that exits with 21 (unknown input format) or 22 (unknown output
format) does not support the benchmark. The tool reports the benchmark as
`not-supported` and does not time it. Today Randoc supports only the `json`
reader and the `json` writer.

## The method

### What the tool measures

The machine runs many other jobs, so wall time is not stable. The tool measures
the CPU time of the child process: user time plus system time, from
`os.wait4`. It also records the wall time and the peak memory (`ru_maxrss`, in
bytes). For a ratio, a CPU time of 0 counts as 1 microsecond, the
resolution of the CPU time.

For each program and each benchmark, the tool does these steps:

1. It runs the command one time and keeps the output. This run classifies the
   result (`ok`, `not-supported` or `error`). It records the exit code, the
   standard error, and the size and SHA-256 hash of the output. This run is also
   the warm-up run. Its time is not used.
2. It runs the command N times with the output sent to `/dev/null`.

The timed runs go in rounds. In each round, the tool runs each benchmark one
time for each program. The order of the two programs changes in each round.
Thus a change of machine load affects the two programs equally. Run i of the
program under test and run i of the reference are a pair.

Each run is in its own process group. At the time limit, the tool kills the
group, so a child process of the program also stops. A run counts as a
time-out only when SIGKILL ended it. A run that exits just before the limit is
a normal run. The standard error that the tool records has the repository root
replaced by `<repo>` and the home folder replaced by `~`.

### Startup and conversion

`startup-version` runs `--version`. Each series has its startup benchmark,
`NAME-startup`. The tool gives two derived values for each program in
`derived`:

- `conversion_cpu_seconds`: the minimum CPU time of a benchmark minus the
  minimum CPU time of its startup benchmark.
- `slope_cpu_seconds_per_section`: the least-squares slope of the minimum CPU
  time over the scales 1, 10 and 100. It is the CPU time for one more copy of
  the section, without the startup cost.

### The comparison rule

The data must prove the result. A missing proof is not a pass.

1. For each pair, the tool calculates the ratio `program time / reference time`.
2. The quantity that the gate limits is the median of this ratio. The
   estimate is the median of the measured ratios.
3. The tool calculates a lower bound and an upper bound of the median ratio
   with the exact sign test. With the sorted ratios r(1) <= ... <= r(n), the
   lower bound is r(k) and the upper bound is r(n + 1 - k). k is the largest
   number with P(B <= k - 1) <= alpha, for B with a binomial distribution of n
   trials and probability 1/2. Each bound has a one-sided confidence of
   1 - alpha, 99% by default.

The sign test needs independent pairs. It needs no symmetry and no other form
of the distribution. An earlier version used the Wilcoxon signed-rank method.
That method also needs a symmetric distribution of the differences, and a
review showed an asymmetric case where its 99% claim failed. The test
`test_bounds_keep_the_confidence_for_an_asymmetric_distribution` checks that
case with the sign test.

| Verdict | Condition |
| --- | --- |
| `slower` | The lower bound is above 1.05. The data show a slowdown of more than 5%. |
| `same` | The upper bound is 1.05 or less. The data show that a slowdown is 5% or less. |
| `faster` | The upper bound is below 1/1.05. |
| `inconclusive` | Each other case. |
| `lost-support` | The reference runs the benchmark, and the program under test exits with 21 or 22. |
| `new` | The program under test runs the benchmark, and the reference exits with 21 or 22. |
| `not-supported` | Both programs exit with 21 or 22. |
| `error` | A run of one program failed. |

Why pairs: the two runs of a pair are close in time, so a slow change of the
machine load affects both runs. A few very slow runs do not change the median
and the bounds much.

The table also shows the minimum CPU times. They are for information. The
verdict does not use them, because the minimum of 30 runs changed by up to
20% between two runs of the same program on this machine.

### More runs and the error allowance

The error allowance `alpha` (option `--alpha`, default 0.01) is the
probability of a false verdict for one benchmark in one direction: a false
`same` when the median ratio is above 1.05, or a false `slower` when it is
1.05 or less. It is not a probability for the whole suite.

One invocation of the tool is one measurement plan with one allowance. With
`--max-runs M`, the tool looks at the data after N runs, then after 2N, 4N and
so on, up to M. After each look, only the `inconclusive` benchmarks get more
runs. Each look uses alpha/K, with K the number of looks, so the error rate of
the whole invocation stays at alpha or below.

A new invocation after an `inconclusive` result gets a new allowance. Thus
repeated tries raise the probability of a false pass above alpha. Example from
the review: with log ratios that are symmetric around ln(1.06) and 8 pairs,
three tries at 1% each give a false-pass probability of 1.13%. Therefore the
gate procedure permits one invocation for each merge (see "How the
coordinator uses the tool in the merge gate"). If more invocations are
necessary, declare the split before the first one: for example, three
invocations with `--alpha 0.0033` each. The test
`test_declared_split_keeps_the_allowance_over_tries` checks this rule.

### Measured behaviour of the rule

These results are from an Apple M1 (4 performance cores and 4 efficiency
cores) with a load average of 490 to 690.

- Randoc compared with itself, sign test, `--filter json-to-json --runs 30
  --max-runs 480` (load average about 500): no `slower` result.
  `json-to-json-x100` was `same` after 120 pairs, `json-to-json-x10` after 240
  pairs and `json-to-json-x1` after 480 pairs. `json-to-json-startup` was
  still `inconclusive` after 480 pairs, with an upper bound of 1.060. Exit
  code 3. The comparison took about 70 seconds.
- The same comparison with the earlier signed-rank method (load average about
  660): 2 benchmarks `same`, 2 `inconclusive`, no `slower`.
- Randoc compared with a wrapper from `make_slow.py --factor 1.3`, calibrated
  on `json-to-json-x100`, `--runs 30 --max-runs 240`, with the earlier
  signed-rank method: `slower` for each of the 4 benchmarks after 30 pairs.
  For `json-to-json-x100`, the bounds were 1.263 to 1.495. Exit code 1. A
  calibration for the factor 1.2 was refused, because the median cost of the
  wrapper alone (4.5 ms) was more than 20% of the program (18.1 ms).
- A proportional slowdown, resampled from the 30 samples of
  `json-to-json-x100` in `main.json`, 200 trials for each row, with the sign
  test. The samples of the two programs are independent here, so the pairs
  give no advantage:

| Pairs | Factor | `same` | `inconclusive` | `slower` |
| --- | --- | --- | --- | --- |
| 30 | 1.00 | 10% | 90% | 0% |
| 30 | 1.06 | 0.5% | 97% | 2.5% |
| 30 | 1.10 | 0% | 81.5% | 18.5% |
| 30 | 1.20 | 0% | 14% | 86% |
| 120 | 1.00 | 60% | 40% | 0% |
| 120 | 1.06 | 0% | 95.5% | 4.5% |
| 120 | 1.10 | 0% | 46.5% | 53.5% |
| 120 | 1.20 | 0% | 0% | 100% |
| 480 | 1.00 | 100% | 0% | 0% |
| 480 | 1.06 | 0% | 69% | 31% |
| 480 | 1.10 | 0% | 0% | 100% |
| 480 | 1.20 | 0% | 0% | 100% |

A slowdown of 10% or more did not pass in any trial. One trial of 200 at 30
pairs gave `same` for a 6% slowdown. That is within the allowance of 1%. The
sign test needs more pairs than the signed-rank method: at 120 pairs and no
slowdown, it gave `same` in 60% of the trials, and the signed-rank method in
80.5%. At 480 pairs, it gave `same` in each trial.

## The raw data

The JSON file has these keys. It contains no local path: the programs are
`PROGRAM` and `REFERENCE` in each `argv`, and the benchmarks file is relative
to the repository root.

| Key | Content |
| --- | --- |
| `schema` | The version of this format. Now 1. |
| `started`, `finished` | UTC time stamps. |
| `settings` | The runs, the maximum runs, the time limit, the filter, the benchmarks file, the threshold, the error allowance (`alpha`) and, with `--compare`, the method and the level of each look (`alpha_per_look`). |
| `environment` | OS, CPU model, core counts (also performance and efficiency cores on macOS), memory, Python version, load average at the start and at the end. |
| `programs` | For `program` and `reference`: the file name, the `--version` lines, the time of the last change of the program file, and the Git state (see below). |
| `benchmarks` | Each benchmark: name, arguments, standard input (a file or a text), series, scale, startup benchmark. |
| `results` | For each program and benchmark: status, exit code, standard error, output size and hash, the command (`argv`), each timed run (`cpu`, `user`, `sys`, `wall` in seconds, `maxrss` in bytes) and a summary (minimum and median). |
| `derived` | Conversion times and slopes for each program. |
| `comparison` | With `--compare`: one row for each benchmark with the estimate and the bounds of the ratio, the ratios of the minimums and the medians, the number of runs, the verdict and `same_output`. |

The `--version` lines do not contain a line that starts with
`User data directory:`, and the tool replaces the home folder with `~`.

The Git state is the state of the Git work tree that contains the program,
when the tool runs: `worktree_commit`, `worktree_commit_time` and
`worktree_dirty` (true when `git status --porcelain` shows a change, also an
untracked file). It is not a proven build commit. A program file that is older
than `worktree_commit_time`, or a work tree with changes, can come from a
different commit.

`same_output` is false when the two programs write different bytes. The table
then shows `(output differs)`. The conformance harness, not this tool, decides
whether an output is correct.

## How the coordinator uses the tool in the merge gate

1. Build the release program of `main` one time for each merge round. Copy it
   out of the target folder, for example to `"$TMPDIR/randoc-main"`.
2. Build the release program of the branch.
3. Run the comparison with a filter for the changed paths. Use the format name,
   for example `--filter markdown`. Use no filter when the change is in a
   shared crate.

   ```sh
   python3 tools/bench/run.py --program "$RANDOC_WT/NAME/target/release/randoc" \
       --compare "$TMPDIR/randoc-main" --filter markdown --runs 30 --max-runs 480 \
       --out "$TMPDIR/gate6-NAME.json"
   ```

4. Read the exit code:
   - 0: gate 6 passes.
   - 1: gate 6 fails.
   - 2: a run failed. Find the cause with the conformance harness.
   - 3: gate 6 is not decided. Record the result as "not decided". Do not run
     the tool again for this merge, except under a split of the allowance
     that you declared before the first invocation (see "More runs and the
     error allowance"). Gate 6 passes only with exit code 0.
5. One invocation with `--runs` and `--max-runs` is the whole gate measurement
   for one merge. Choose `--max-runs` before you start. Do not run a
   comparison again to remove a `slower` result. Keep each JSON file with the
   merge record.
6. A benchmark with the verdict `new` has no reference. Measure it with one
   program and keep the result as the first baseline of that benchmark.

## Limits

- **Load.** CPU time is more stable than wall time, but it is not constant. On
  a loaded machine, cache sharing and memory bandwidth change the CPU time.
  Under heavy load, a comparison needs hundreds of pairs before it can show
  that a slowdown is 5% or less (see "Measured behaviour of the rule").
- **Independent pairs.** The bounds assume that the pairs are independent and
  have the same distribution. When the load changes faster than one pair, this
  is not fully true, and the bounds can be too narrow.
- **Performance and efficiency cores.** macOS can run a process on an
  efficiency core, where the same work needs more CPU time. The tool cannot
  select the core type. The `taskpolicy -b` command keeps a process on the
  efficiency cores. Under this load, one comparison with it did not finish in
  3 minutes. The same comparison without it takes about 3 seconds. Thus the
  tool does not use it.
- **Short commands.** The startup benchmarks take 2 to 3 ms. The resolution of
  the CPU time is 1 microsecond, but the runs of one command spread over
  more than 1 ms.
- **Live comparison only.** The tool compares two programs in the same run. It
  does not compare a program with a stored JSON file, because the machine load
  is different at a different time. The files in `benchmarks/results/` are a
  record, not a gate input.
- **The slow wrapper.** `make_slow.py` adds a fixed CPU cost, not a cost in
  proportion to the work. The calibration uses medians of interleaved runs and
  refuses a calibration with a cost that is not positive. The real factor of a
  wrapper changes with the load. Use the wrapper to check the tool end to end.
  The unit tests check the detection of a proportional slowdown with resampled
  data from `main.json`.
- **Inputs.** The inputs are one generated document. They do not cover each
  reader and writer path. Add a series to `benchmarks.toml` when a new format
  needs a benchmark. Keep at most 6 files in `benchmarks/inputs/`.
- **Small differences.** The conversion time is a difference of two minimums.
  When the conversion is small, the noise can be larger than the conversion,
  and the value can be negative. Use the slope for small inputs.
- **Peak memory.** `ru_maxrss` is the peak resident memory. On macOS it
  includes shared libraries. The tool records it but does not compare it.
