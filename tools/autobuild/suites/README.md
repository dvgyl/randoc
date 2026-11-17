# Suites of the measurement

One file for each suite, so that parallel workers add files and do not edit
one shared file.

- `tools/autobuild/suites/NAME.py` defines `make_runner(measure)` and returns
  `measure.Runner(parse, filter_option, tool_dir)`. `parse(evidence, log)`
  reads the raw output of the suite command from the evidence folder and
  returns `measure.Counts.of(units, metric_keys)`; each unit is
  `measure.Unit(id, cls, metric_keys)` with `cls` one of `measure.PASS`,
  `measure.FAIL`, `measure.UNSTABLE`, `measure.ORACLE_LIMIT`,
  `measure.NO_INPUT` (plan/SPEC.md 3.10 and 3.11).
- `tools/autobuild/metrics.d/NAME.toml` holds the `[[metric]]` entries of the
  suite, in the form of `tools/autobuild/metrics.toml`, with `suite = "NAME"`.
  An entry here may replace a `to-build` entry of `metrics.toml` with the
  same key; any other repeated key is an error.
- Add unit tests in `tools/autobuild/tests/test_suite_NAME.py`.
