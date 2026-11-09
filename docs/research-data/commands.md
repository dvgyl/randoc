# Commands and raw numbers of the competitive analysis

This file records the exact commands of the research for
`docs/COMPETITIVE_ANALYSIS.md`. The raw results are in `results.tsv` in this
folder. The probe program is `probe.py` in this folder.

Date: 2026-11-09. Machine: Apple silicon (arm64), macOS, 8 cores shared with
other agents. Oracle: Pandoc 3.13 (`$RANDOC_ORACLE_PANDOC`).

## Environment

Each command started with:

```sh
. "/Volumes/Extreme Pro/Randoc/env/env.sh"
export RUSTUP_TOOLCHAIN=1.99.1-aarch64-apple-darwin   # do not download the toolchains that the projects pin
```

## Clones

```sh
cd "$RANDOC_ENV/research"
git clone -q --depth 1 https://github.com/mfkrause/carta carta
git clone -q --depth 1 https://github.com/alexgaoth/ferrodoc ferrodoc
git clone -q --depth 1 https://github.com/rhi-zone/rescribe rescribe
git clone -q --depth 1 https://github.com/jgm/skylighting skylighting
```

| Project | Commit | Commit date |
| --- | --- | --- |
| carta | `9796ffed1ace6beed70cb64af8db29667030f788` | 2026-11-09 |
| ferrodoc | `38615d9217ac024ae1914b677f413906ac44c6a0` | 2026-10-05 |
| rescribe | `44357d5fa75f943acaa475f6122c8315e356811f` | 2026-08-29 |
| skylighting | `9a414fca3ac94dee81fb76e0ef4f88c9e6037618` | 2026-11-09 |

## Size

```sh
find . -name '*.rs' -not -path './target/*' -print0 | xargs -0 cat | wc -l
find . -type f -not -path './.git/*' | wc -l
grep -r --include='*.rs' -h '#\[test\]' . | wc -l
ls fuzz/fuzz_targets | wc -l
```

| Project | Rust lines | Files | `#[test]` functions | Fuzz targets |
| --- | ---: | ---: | ---: | ---: |
| carta | 201 925 | 11 299 | 3 075 | 47 |
| ferrodoc | 44 420 | 391 | 307 | 0 |
| rescribe | 753 620 | 7 441 | 3 807 | 128 |

GitHub metadata (`gh repo view OWNER/NAME --json stargazerCount,createdAt,pushedAt,licenseInfo`):

| Project | Created | Last push | Stars |
| --- | --- | --- | ---: |
| carta | 2026-06-06 | 2026-11-09 | 60 |
| ferrodoc | 2026-08-15 | 2026-10-06 | 12 |
| rescribe | 2026-01-09 | 2026-08-29 | 1 |

Release archives (`gh release view -R OWNER/NAME --json assets`): carta
v0.0.13 `carta-aarch64-apple-darwin.tar.gz` 6 205 602 bytes; ferrodoc v1.0.1
`ferrodoc-v1.0.1-aarch64-apple-darwin.tar.gz` 2 365 688 bytes,
`ferrodoc.wasm` 2 088 117 bytes. The oracle program is 195 370 176 bytes.

## Builds (debug profile, 4 jobs)

```sh
(cd ferrodoc && /usr/bin/time -p cargo build -p ferrodoc --jobs 4)
(cd carta    && /usr/bin/time -p cargo build -p carta --bin carta --jobs 4)
(cd rescribe && /usr/bin/time -p cargo build -p rescribe-cli --jobs 4)
```

| Project | Result | Wall time | Debug binary |
| --- | --- | ---: | ---: |
| ferrodoc 1.0.1 | exit 0 | 176 s | 20 168 136 bytes |
| carta 0.0.13 | exit 0 | 1 261 s | 47 634 072 bytes |
| rescribe 0.1.0 | RESCRIBE-BUILD | | |

The wall times are long because other agents used the same 8 cores and the
drive uses exFAT.

## Probe set

The probe inputs are outside the repository, in `$RANDOC_ENV/research/probe`:

- `md/` (22 files): `MANUAL.txt` and 11 files of `doc/` of the Pandoc source
  (documentation, not tests), 7 Markdown files of `ferrodoc/corpus`, and the
  README files of carta, rescribe and skylighting.
- `html/` (18 files): 10 HTML files of `ferrodoc/corpus` and 8 of
  `carta/corpus/text/html`.
- The 18 cases of `conformance/cases/json/roundtrip.toml` and
  `conformance/cases/cli/formats.toml` of this repository.

The HTML files come from the test corpora of two of the tested projects.
They favour those projects. The Pandoc documents favour no project.

```sh
R="$RANDOC_ENV/research"
export PROBE_TOOLS="carta=${R}/carta/target/debug/carta:ferrodoc=${R}/ferrodoc/target/debug/ferrodoc:rescribe=${R}/rescribe/target/debug/rescribe"
python3 -I docs/research-data/probe.py "$R/probe" "$RANDOC_WT/research" "$R/probe.tsv"
PROBE_WRITERS=rst,org,asciidoc,mediawiki,docbook5,man,typst,rtf,ipynb,dokuwiki,jira,opml,plain,commonmark,gfm,native \
  python3 -I docs/research-data/probe.py "$R/probe" "$RANDOC_WT/research" "$R/probe-writers.tsv"
```

`results.tsv` holds both runs. Its first column is `main` or `writers`. The
summary that `probe.py` printed for the `main` run:

```text
carta      cli                          n=  7 exit-differ=3 identical=4
carta      commonmark>html              n= 22 differ=10 identical=12
carta      gfm>html                     n= 22 differ=10 identical=12
carta      html>json                    n= 18 differ=7 identical=11
carta      html>markdown                n= 18 differ=9 identical=9
carta      json                         n= 55 ast-equal=4 differ=9 exit-differ=21 identical=21
carta      markdown>html                n= 22 differ=16 identical=6
carta      markdown>html-s              n= 22 differ=22
carta      markdown>json                n= 22 differ=10 identical=12
carta      markdown>latex               n= 22 differ=16 identical=5 ws-only=1
ferrodoc   cli                          n=  7 exit-differ=4 identical=3
ferrodoc   commonmark>html              n= 22 differ=6 identical=15 ws-only=1
ferrodoc   gfm>html                     n= 22 differ=11 identical=11
ferrodoc   html>json                    n= 18 ast-equal=16 differ=2
ferrodoc   html>markdown                n= 18 differ=5 identical=12 ws-only=1
ferrodoc   json                         n= 55 ast-equal=4 crash=1 differ=10 exit-differ=20 identical=11 unsupported=7 ws-only=2
ferrodoc   markdown>html                n= 22 differ=12 identical=10
ferrodoc   markdown>html-s              n= 22 differ=22
ferrodoc   markdown>json                n= 22 ast-equal=13 differ=9
ferrodoc   markdown>latex               n= 22 differ=14 identical=7 ws-only=1
```

The summary of the `writers` run:

```text
carta      markdown>asciidoc            n= 22 differ=14 identical=7 ws-only=1
carta      markdown>commonmark          n= 22 differ=10 identical=11 ws-only=1
carta      markdown>docbook5            n= 22 differ=15 identical=7
carta      markdown>dokuwiki            n= 22 differ=10 identical=11 ws-only=1
carta      markdown>gfm                 n= 22 differ=8 identical=13 ws-only=1
carta      markdown>ipynb               n= 22 differ=19 exit-differ=3
carta      markdown>jira                n= 22 differ=10 identical=10 ws-only=2
carta      markdown>man                 n= 22 differ=15 identical=6 ws-only=1
carta      markdown>mediawiki           n= 22 differ=8 identical=13 ws-only=1
carta      markdown>native              n= 22 differ=10 identical=12
carta      markdown>opml                n= 22 differ=8 identical=14
carta      markdown>org                 n= 22 differ=18 identical=4
carta      markdown>plain               n= 22 differ=9 identical=11 ws-only=2
carta      markdown>rst                 n= 22 differ=10 identical=10 ws-only=2
carta      markdown>rtf                 n= 22 differ=22
carta      markdown>typst               n= 22 differ=18 identical=4
ferrodoc   markdown>asciidoc            n= 22 differ=11 identical=11
ferrodoc   markdown>commonmark          n= 22 differ=11 identical=10 ws-only=1
ferrodoc   markdown>docbook5            n= 22 unsupported=22
ferrodoc   markdown>dokuwiki            n= 22 unsupported=22
ferrodoc   markdown>gfm                 n= 22 differ=10 identical=10 ws-only=2
ferrodoc   markdown>ipynb               n= 22 differ=19 exit-differ=3
ferrodoc   markdown>jira                n= 22 unsupported=22
ferrodoc   markdown>man                 n= 22 unsupported=22
ferrodoc   markdown>mediawiki           n= 22 unsupported=22
ferrodoc   markdown>native              n= 22 unsupported=22
ferrodoc   markdown>opml                n= 22 unsupported=22
ferrodoc   markdown>org                 n= 22 unsupported=22
ferrodoc   markdown>plain               n= 22 differ=9 identical=11 ws-only=2
ferrodoc   markdown>rst                 n= 22 differ=10 identical=10 ws-only=2
ferrodoc   markdown>rtf                 n= 22 unsupported=22
ferrodoc   markdown>typst               n= 22 unsupported=22
```

Per-source pass counts (identical or ast-equal) come from a short Python
script over `results.tsv` that groups the `unit` column by the prefix of
the file name (`pandoc-`, `ferrodoc-`, `carta-`, the README files, and
`conformance/`).

## Spot checks

TeX math (carta), with the file `math.md`:

```markdown
$\frac{a}{b}+\sqrt{x^2}$ and $\alpha_i \leq \sum_{k=1}^n k$ and $\mathbb{R}^{n\times m}$

$$\begin{pmatrix}1&2\\3&4\end{pmatrix}$$

$$\int_0^\infty e^{-x^2}\,dx = \frac{\sqrt{\pi}}{2}$$
```

```sh
for t in "--mathml -t html" "-t typst" "-t native" "-t rtf"; do
  a=$($RANDOC_ORACLE_PANDOC -f markdown $t math.md | shasum); b=$(carta -f markdown $t math.md | shasum)
  [ "$a" = "$b" ] && echo "same: $t" || echo "DIFF: $t"
done
```

Result: `same` for all four.

Highlighting (ferrodoc): a Haskell and a YAML code block gave
`<pre class="haskell"><code>main :: IO ()</code></pre>`, without
highlighting. carta keeps `haskell.xml`, `json.xml`, `javascript.xml` and
`yaml.xml` in `crates/carta-highlight/data/syntax-copyleft/`.

## ferrodoc drop-in check against Pandoc 3.13

ferrodoc's `scripts/dropin.sh` refuses a Pandoc other than 3.8.2.1. A copy
with one changed line ran it against the oracle:

```sh
cd ferrodoc
sed 's/^PANDOC_PINNED=.*/PANDOC_PINNED=3.13/' scripts/dropin.sh > scripts/dropin-313.sh
ln -sf "$RANDOC_ORACLE_PANDOC" "$TMPDIR/oraclebin/pandoc"
PATH="$TMPDIR/oraclebin:$PATH" FERRODOC=./target/debug/ferrodoc ./scripts/dropin-313.sh
```

Result: `22/48 command lines identical (45.8%), 0 refused for a missing flag`.
The README claims 47/48 against Pandoc 3.8.2.1. Of the 26 misses, 1 is
"deliberate" (`dropin-006`) and 25 are "fixable"; 20 of the 25 are HTML files.

## crates.io data

```sh
curl -s -A 'randoc-research' "https://crates.io/api/v1/crates/NAME"
```

The fields used: `max_stable_version`, `versions[0].license`, `updated_at`,
`recent_downloads`, `repository`.
