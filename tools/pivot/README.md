# Pivot set

The pivot set contains the 400 document trees that the format suites give
to each writer. The first tree is the same input that the written-file
cases use. The other trees cover document forms, syntax highlighting, TeX
math and seeded combinations of document elements.

Run the generator from the repository root. Give it a new output path.

```sh
. "${RANDOC_ENV:-../env}/env.sh"
python3 -B tools/pivot/generate.py --out "$TMPDIR/pivot.jsonl"
```

The generator checks each tree with `pandoc -f json -t json`. It writes one
JSON record per line with the fields `id` and `tree`. It checks the SHA-256
of the complete list against `spec/pivot.sha256`.

The `writers` suite uses each tree with each of the 76 non-PDF writers. Run
a diagnostic for the first tree with this command.

```sh
python3 tools/measure/measure.py --suite writers --filter forms/sample-cell
```

The suite checks the oracle checksum and records each new oracle result
with two runs in separate work folders. The cache at
`$RANDOC_CACHE/fmt/writers.sqlite` (default `$RANDOC_ENV/cache/fmt/writers.sqlite`) holds raw oracle output. The evidence
folder holds the result of each comparison and the SHA-256 of its oracle
output. The score for each writer uses `fmt.write.F`, where `F` is the
format name.

At each milestone, run the writer tool with `--verify-oracle` to check
each saved oracle result in a new work folder. A later difference keeps
the original result and records the parts that changed. An oracle run that
hit a limit gets another pair of runs on the next measurement. If an
earlier result exists, the tool compares both new results with it and
keeps its known differences.
