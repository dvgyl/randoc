# The document corpus

The corpus is a set of real documents with open licences. It shows how Randoc
works on real input, which small conformance cases do not show. A tool
converts each document with Pandoc 3.13 (the oracle) and with Randoc, and
compares the results.

The documents are not in the repository. `tools/corpus/manifest.toml` lists
them, and `tools/corpus/fetch.py` downloads them to `$RANDOC_CORPUS`.

## Files

| File | Purpose |
| --- | --- |
| `tools/corpus/manifest.toml` | One entry for each document: name, format, URL, licence, source, SHA-256 hash, size. |
| `tools/corpus/fetch.py` | Downloads the documents and checks each hash. |
| `tools/corpus/run.py` | Converts each document with two programs and compares the results. |
| `tools/corpus/corpuslib.py` | Reads, checks and writes the manifest. |
| `tools/corpus/test_corpus.py` | The unit tests of the tools. |
| `spec/corpus.json` | The result of the last Randoc run, with the path and the SHA-256 hash of the tested program and the hash of the oracle. |

The tools use Python 3.11 or later and only the standard library.

## How to run

Run these commands from the root of the repository.

```sh
. "/Volumes/Extreme Pro/Randoc/env/env.sh"

# 1. Download the documents. A second run skips each file that has the correct hash.
python3 tools/corpus/fetch.py

# 2. Check the tool: the oracle against itself must give 100% equal results.
python3 tools/corpus/run.py --pandoc "$RANDOC_ORACLE_PANDOC" --oracle "$RANDOC_ORACLE_PANDOC" --expect-all-pass

# 3. Compare Randoc with the oracle and write the result.
cargo build -p randoc-cli
python3 tools/corpus/run.py --pandoc target/debug/randoc --oracle "$RANDOC_ORACLE_PANDOC" --out spec/corpus.json

# The unit tests of the tools.
python3 -m unittest discover -s tools/corpus
```

Useful options of `run.py`:

- `--only TEXT`: use only the documents whose name contains `TEXT`.
- `--target FORMAT`: use only one target format (`json`, `html` or `markdown`). Repeat for more.
- `--jobs N`: the number of parallel conversions. The default is 4.
- `--timeout S`: the time limit of one conversion in seconds. The default is 60. A manifest entry can set its own limit with `timeout`. `jupyter-running-code` has `timeout = 300`, because Pandoc needs about 40 to 105 s of wall time for it.

## What `run.py` compares

For each document, `run.py` runs `pandoc -f FORMAT -t TARGET FILE` for the
targets `json` (the document tree), `html` and `markdown`. One document and
one target make one unit. A unit passes when the two programs give the same
exit code, the same standard error output and the same output. The tool
compares the `json` output by JSON equality and the other outputs by bytes.

Each program runs under the name `pandoc`, in the corpus directory, with the
fixed environment of the conformance harness: `PATH=/usr/bin:/bin:/usr/sbin:/sbin`,
`HOME=/var/empty`, `LANG=en_US.UTF-8`, `LC_ALL=en_US.UTF-8`, `TZ=UTC`
and `SOURCE_DATE_EPOCH=1700000000`.

The tool stores the oracle results in `$RANDOC_CORPUS/.oracle-cache`, one
SQLite file. The key of a result is the SHA-256 hash of the oracle program file,
the hash of the document file and the arguments. Before a run, `run.py`
checks the size and the hash of each document file against the manifest and
stops when one differs. A second run starts only the
program under test. A result after a timeout is not stored.

The status of a unit in `spec/corpus.json` is one of these:

| Status | Meaning |
| --- | --- |
| `pass` | Exit code, standard error and output are equal. |
| `fail PARTS` | `PARTS` names the parts that differ: `exit`, `stderr`, `output`. |
| `timeout` | The program under test did not stop in the time limit. |
| `oracle timeout` | The oracle did not stop in the time limit. |
| `error TEXT` | The tool failed on this unit, for example on a Python error. The unit counts as a failure. |

Each program runs in its own process session. At the time limit, `run.py`
stops the whole process group. It keeps at most 64 MB of standard output
and 64 MB of standard error output for one run.

## Rules for the manifest

- A URL points to a fixed version where the host allows it: a Git commit for
  GitHub files, a revision identifier for Wikipedia. The hash detects a change
  of the other sources.
- Each document has an SPDX licence expression. `LicenseRef-Public-Domain`
  marks a document in the public domain. `LicenseRef-Public-Domain-US AND
  CC0-1.0` marks an ebook whose text is in the public domain in the United
  States and whose contributor work is under CC0.
- A file from a collection of test files can have its own rights statement.
  Read the statement inside the file (for example `dc:rights` in an EPUB
  package file). Do not take the licence of the repository for such a file.
  Leave out a file whose content comes from a third party without a clear
  licence.
- One document is 2 MB or less. The corpus is 300 MB or less. `corpuslib.check`
  enforces both limits.
- The drive is exFAT, and each file uses 2 MB or more there. Each document is
  one file. The tools do not unpack archives.
- `fetch.py` sends one request at a time, waits 0.5 s between two requests to
  the same host, and sends the user agent `randoc-corpus/1.0`.

To add documents, add entries with `sha256 = ""` and `size = 0`, then run
`python3 tools/corpus/fetch.py --pin`. The tool downloads the new documents
and writes their hashes and sizes into the manifest.

## Sources and licences

`python3 tools/corpus/corpuslib.py` prints the two tables below. A unit test checks
that they match the manifest.

| Source | Licence | Formats | Documents |
| --- | --- | --- | --- |
| 01mf02/jaq | MIT | djot | 5 |
| adamgreen/CrashCatcher | Apache-2.0 | creole | 1 |
| adamgreen/mri | Apache-2.0 | creole | 2 |
| alexott/muse | GPL-3.0-or-later | muse | 2 |
| apache/tika | Apache-2.0; LicenseRef-Public-Domain-US AND CC0-1.0 | docx, epub, odt, pptx, rtf, xlsx | 39 |
| asciidoctor/asciidoctor | MIT | asciidoc | 8 |
| astral-sh/ruff | MIT | gfm | 1 |
| BurntSushi/ripgrep | Unlicense OR MIT | gfm | 1 |
| CDCgov/influenza-generation_time-us | Apache-2.0 | ris | 1 |
| datasets/co2-ppm | ODC-PDDL-1.0 | csv | 2 |
| datasets/gdp | CC-BY-4.0 | csv | 2 |
| denoland/deno | MIT | gfm | 1 |
| doomemacs/doomemacs | MIT | org | 9 |
| en.wikipedia.org | CC-BY-SA-4.0 | html, mediawiki | 24 |
| europepmc.org | CC-BY-4.0 | jats | 4 |
| facebook/react | MIT | gfm | 1 |
| facebook/zstd | BSD-3-Clause OR GPL-2.0-only | man | 1 |
| github/docs | CC-BY-4.0 | markdown | 6 |
| golang/go | BSD-3-Clause | html | 3 |
| greenelab/deep-review | CC-BY-4.0 | csljson, markdown | 10 |
| hacketyhack/hacketyhack | MIT | textile | 1 |
| inbo/tutorials | CC-BY-4.0 | bibtex, csljson | 5 |
| jgm/pandoc-citeproc | BSD-3-Clause | man | 1 |
| jqlang/jq | CC-BY-3.0 | man | 1 |
| jupyter/notebook | BSD-3-Clause | ipynb | 8 |
| kubernetes/kubernetes | Apache-2.0 | gfm | 1 |
| latex3/latex2e | LPPL-1.3c | latex | 8 |
| lz4/lz4 | GPL-2.0-or-later | man | 1 |
| mdn/content | CC-BY-SA-2.5 | commonmark | 10 |
| microsoft/vscode | MIT | gfm | 1 |
| openbsd/src | BSD-2-Clause AND LicenseRef-SSH-Ylonen; ISC; LicenseRef-Public-Domain | mdoc | 6 |
| pallets/flask | BSD-3-Clause | gfm | 1 |
| Perl/perl5 | Artistic-1.0-Perl OR GPL-1.0-or-later | pod | 5 |
| plk/biblatex | LPPL-1.3c | biblatex | 1 |
| psf/requests | Apache-2.0 | gfm | 1 |
| python/peps | LicenseRef-Public-Domain; LicenseRef-Public-Domain OR CC0-1.0 | rst | 8 |
| RailsApps/rails_layout | MIT | textile | 1 |
| rust-lang/book | MIT OR Apache-2.0 | markdown | 14 |
| rust-lang/cargo | MIT OR Apache-2.0 | gfm | 1 |
| scripting/concord | GPL-3.0-or-later | opml | 2 |
| Sephiroth87/ODRefreshControl | MIT | textile | 1 |
| sharkdp/bat | MIT OR Apache-2.0 | gfm | 1 |
| sphinx-doc/sphinx | BSD-2-Clause | rst | 8 |
| standardebooks.org | LicenseRef-Public-Domain-US AND CC0-1.0 | epub | 5 |
| systemd/systemd | LGPL-2.1-or-later | docbook | 8 |
| tokio-rs/tokio | MIT | gfm | 1 |
| typst/templates | MIT-0 | typst | 4 |
| ufal/udpipe | MPL-2.0 | t2t | 3 |
| vuejs/core | MIT | gfm | 1 |
| xyfeng/XYPieChart | MIT | textile | 1 |

## Counts for each format

| Format | Documents | Bytes |
| --- | --- | --- |
| asciidoc | 8 | 95329 |
| biblatex | 1 | 68013 |
| bibtex | 2 | 83249 |
| commonmark | 10 | 82691 |
| creole | 3 | 34402 |
| csljson | 5 | 909802 |
| csv | 4 | 592251 |
| djot | 5 | 92461 |
| docbook | 8 | 214704 |
| docx | 12 | 199713 |
| epub | 7 | 3567105 |
| gfm | 12 | 117005 |
| html | 11 | 3797438 |
| ipynb | 8 | 138916 |
| jats | 4 | 216938 |
| latex | 8 | 309365 |
| man | 4 | 192038 |
| markdown | 28 | 433860 |
| mdoc | 6 | 479239 |
| mediawiki | 16 | 1676361 |
| muse | 2 | 19188 |
| odt | 8 | 126404 |
| opml | 2 | 39392 |
| org | 9 | 178162 |
| pod | 5 | 256312 |
| pptx | 5 | 220314 |
| ris | 1 | 3096 |
| rst | 16 | 518846 |
| rtf | 8 | 218956 |
| t2t | 3 | 39673 |
| textile | 4 | 29884 |
| typst | 4 | 18471 |
| xlsx | 4 | 36560 |
| all | 233 | 15006138 |

## Documents that Pandoc rejects

Pandoc 3.13 rejects 15 documents. They stay in the corpus, because Randoc
must give the same exit code and the same error message.

- Pandoc exits with code 64 (a parse error) for 13 of them, for example
  `latex2e-cfgguide` (`unexpected @`), `wp-haskell` (an unclosed `<ref>`),
  `openbsd-ksh` (the `Xo` macro) and `deep-review-manual-references` (a CSL
  JSON type error).
- `systemd-systemd-vmspawn` exits with code 44: the XML has an entity that is
  not resolved (`&fedora_latest_version;`).
- `asciidoctor-manpage-backend-index` exits with code 1: it includes a file
  that is not in the corpus.

These two documents test error handling for missing resources. They do not
test the reader on a complete document.

The corpus has no Typst document that imports a package (`#import "@...`).
Pandoc looks for the package under `HOME` and fails with a message that
contains that path. Such a document tests the environment, not the reader.
A unit test enforces this rule when `$RANDOC_CORPUS` is present.
