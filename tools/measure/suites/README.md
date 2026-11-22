# Suites of the measurement

One file for each suite, so that parallel workers add files and do not edit
one shared file.

- `tools/measure/suites/NAME.py` defines `make_runner(measure)` and returns
  `measure.Runner(parse, filter_option, tool_dir)`. `parse(evidence, log)`
  reads the raw output of the suite command from the evidence folder and
  returns `measure.Counts.of(units, metric_keys)`; each unit is
  `measure.Unit(id, cls, metric_keys)` with `cls` one of `measure.PASS`,
  `measure.FAIL`, `measure.UNSTABLE`, `measure.ORACLE_LIMIT`,
  `measure.NO_INPUT`.
- `measure.Runner(..., exits=(0, 1))` makes `measure.py` read the report also
  after the exit code 1 (default: `(0,)`). Each other exit code is a tool
  error. The suite of `gate.steps` uses it: its command is
  `gate.py --report FILE`, which exits 1 when a step fails, and the gate
  never runs that suite itself.
- `tools/measure/metrics.d/NAME.toml` holds the `[[metric]]` entries of the
  suite, in the form of `tools/measure/metrics.toml`, with `suite = "NAME"`.
  An entry here may replace a `to-build` entry of `metrics.toml` with the
  same key; any other repeated key is an error.
- Add unit tests in `tools/measure/tests/test_suite_NAME.py`.
