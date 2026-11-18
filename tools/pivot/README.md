# Pivot set

The pivot set contains the 400 document trees that the format suites give
to each writer. The first tree is the same input that the written-file
cases use. The other trees cover document forms, syntax highlighting, TeX
math and seeded combinations of document elements.

Run the generator from the repository root. Give it a new output path.

```sh
. tools/autobuild/env.sh
python3 -B tools/pivot/generate.py --out "$TMPDIR/pivot.jsonl"
```

The generator checks each tree with `pandoc -f json -t json`. It writes one
JSON record per line with the fields `id` and `tree`. It checks the SHA-256
of the complete list against `spec/pivot.sha256`. A change to this list
needs a tool correction under `plan/SPEC.md` section 4.2.

The `writers` suite uses each tree with each of the 76 non-PDF writers. Run
a diagnostic for the first tree with this command.

```sh
tools/autobuild/ops measure --suite writers --filter forms/sample-cell
```

The suite checks the oracle checksum and records each new oracle result
with two runs in separate work folders. The cache at
`$RANDOC_GPT/cache/fmt/writers.sqlite` holds raw oracle output. The evidence
folder holds the result of each comparison and the SHA-256 of its oracle
output. The score for each writer uses `fmt.write.F`, where `F` is the
format name.
