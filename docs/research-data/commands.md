# Commands and raw numbers of the competitive analysis

This file records the exact commands of the research for
`docs/COMPETITIVE_ANALYSIS.md`. The raw results are in `results.tsv` in this
folder. The probe program is `probe.py` in this folder.

Date: 2026-11-09. Machine: Apple silicon (arm64), macOS, 8 cores shared with
other agents. Oracle: Pandoc 3.13 (`$RANDOC_ORACLE_PANDOC`).

## Environment

Each command started with:

```sh
. "$RANDOC_ROOT/env/env.sh"   # RANDOC_ROOT is the folder that holds env/, wt/ and the main checkout
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

The clones were made at the commits in the table below. To get the same
files again, check out each commit:

```sh
for p in carta:9796ffed1ace6beed70cb64af8db29667030f788 ferrodoc:38615d9217ac024ae1914b677f413906ac44c6a0 \
         rescribe:44357d5fa75f943acaa475f6122c8315e356811f skylighting:9a414fca3ac94dee81fb76e0ef4f88c9e6037618; do
  n=${p%%:*}; c=${p#*:}
  git -C "$n" fetch -q --depth 1 origin "$c" && git -C "$n" checkout -q "$c"
done
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
| rescribe 0.1.0 | exit 0 | 3 306 s | 72 970 856 bytes |

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

The probe inputs are not in the repository. The manifest below names each
input: its SHA-256 hash, its name in the probe folder, and its source as
`OWNER/REPO/REF/PATH` on `raw.githubusercontent.com`. The Pandoc files come
from the tag `3.13`; their hashes are equal to the files in
`$RANDOC_PANDOC_SRC`. No file was changed: `MANUAL.txt` got the name
`pandoc-MANUAL.md`, and each other file got a prefix for its project
(`pandoc-doc-`, `ferrodoc-`, `carta-`).

```text
4cf00a087878e8a510a3a778d3a191cdd3ac4ec029f8e8e6183a1938f200d2d7 md/carta-README.md mfkrause/carta/9796ffed1ace6beed70cb64af8db29667030f788/README.md
3803d268ec9f257dc633564da75fce6ea0e62fe8eb2a9e90b605bc02636f381c html/carta-blocks.html mfkrause/carta/9796ffed1ace6beed70cb64af8db29667030f788/corpus/text/html/blocks.html
daefde528d871ea745848648c24535114bb386a0eaa9ba5b7a2bac3c77ef50a9 html/carta-citation.html mfkrause/carta/9796ffed1ace6beed70cb64af8db29667030f788/corpus/text/html/citation.html
cbadd832d9331d2758c88f3433657d51af2dfad58932bb1e976e523af0102aab html/carta-dropped-elements.html mfkrause/carta/9796ffed1ace6beed70cb64af8db29667030f788/corpus/text/html/dropped-elements.html
64dcb5302460f59fcdc48d37714d27a921043db5ecc6e8531ae836898d28fcf1 html/carta-footnotes.html mfkrause/carta/9796ffed1ace6beed70cb64af8db29667030f788/corpus/text/html/footnotes.html
56b62d5893631489cdc5cb32d6edeb55d0e9eabe1e9b0e6c045051d9a05c5c92 html/carta-inline.html mfkrause/carta/9796ffed1ace6beed70cb64af8db29667030f788/corpus/text/html/inline.html
ee7fded96b732c236dd1df4a2f4773963af6636314d233b9db8632e6c241538a html/carta-lists.html mfkrause/carta/9796ffed1ace6beed70cb64af8db29667030f788/corpus/text/html/lists.html
c1e5180d67fe8b389b23787c6eaaf2a3d127f3b12586090d28bb226e18263256 html/carta-raw-style.html mfkrause/carta/9796ffed1ace6beed70cb64af8db29667030f788/corpus/text/html/raw-style.html
b2d7c5699ae21ec4f387066d637ebccc862edeb53565d5607f9fb195d5c6667c html/carta-task-list.html mfkrause/carta/9796ffed1ace6beed70cb64af8db29667030f788/corpus/text/html/task-list.html
feefab4f84cae96d4c38f72ba2e3033c8d82c1656637dbfcc19bf11788085b91 md/ferrodoc-code-and-raw.md alexgaoth/ferrodoc/38615d9217ac024ae1914b677f413906ac44c6a0/corpus/code-and-raw.md
a937add3190de9963824f8ef9a43780e66ace2ddb202d8ad5e8e5bb58731961f md/ferrodoc-edge-cases.md alexgaoth/ferrodoc/38615d9217ac024ae1914b677f413906ac44c6a0/corpus/edge-cases.md
da7d33ba3d642a6a5bd06208e701e086cfa86a3f65cfad582662d08cf369edd0 html/ferrodoc-footnotes.html alexgaoth/ferrodoc/38615d9217ac024ae1914b677f413906ac44c6a0/corpus/footnotes.html
041f2ddf90e4b8512c694c0ec562845d5e281256a8a373174b9c3efe36ed5aa0 md/ferrodoc-headings.md alexgaoth/ferrodoc/38615d9217ac024ae1914b677f413906ac44c6a0/corpus/headings.md
9b910541f58b842da94be32a5178eb911c632874c0f3d2e05658d7480a88eb78 html/ferrodoc-hidden-content.html alexgaoth/ferrodoc/38615d9217ac024ae1914b677f413906ac44c6a0/corpus/hidden-content.html
fc1815a2a5b2d83508c71fa353623d77cbfc3a3229c152fc243c3703f1d36257 md/ferrodoc-images.md alexgaoth/ferrodoc/38615d9217ac024ae1914b677f413906ac44c6a0/corpus/images.md
06f9095c135221310ac5f361e6ab417548630ce183469bede7f95743c3493cbf html/ferrodoc-inline-edges.html alexgaoth/ferrodoc/38615d9217ac024ae1914b677f413906ac44c6a0/corpus/inline-edges.html
967909f675ed70abd81b21e237c1b9ab6ae700e51233751aa8ba9da694c4eb7c html/ferrodoc-inline-elements.html alexgaoth/ferrodoc/38615d9217ac024ae1914b677f413906ac44c6a0/corpus/inline-elements.html
99de290e2f69fca53578af56d66df37fc00e84acbd5a30511108d334ced8cb2d html/ferrodoc-inline-svg.html alexgaoth/ferrodoc/38615d9217ac024ae1914b677f413906ac44c6a0/corpus/inline-svg.html
4413cf2f8e185e5e948172b6bf1735f5bfd2a338f49226730ceba04cddbfb331 html/ferrodoc-main-content.html alexgaoth/ferrodoc/38615d9217ac024ae1914b677f413906ac44c6a0/corpus/main-content.html
f1b33561de69fac9366d8a7f67dfaf198f7e722b722969723cf8859529f09258 md/ferrodoc-nested-structures.md alexgaoth/ferrodoc/38615d9217ac024ae1914b677f413906ac44c6a0/corpus/nested-structures.md
a4fa9a11dde7098add2a7bc54b3005dc330f723b621d029b421d6307199679f9 md/ferrodoc-readme-style.md alexgaoth/ferrodoc/38615d9217ac024ae1914b677f413906ac44c6a0/corpus/readme-style.md
fb434ed48940fe2fb9820fdd783b4a24cff1cbff0e50e2b632005d387e4c163c html/ferrodoc-sectioning.html alexgaoth/ferrodoc/38615d9217ac024ae1914b677f413906ac44c6a0/corpus/sectioning.html
c360ae950fcb67af94c03ff81f30efa8a2745887222f37cea917671a0f751c94 html/ferrodoc-tables-and-attributes.html alexgaoth/ferrodoc/38615d9217ac024ae1914b677f413906ac44c6a0/corpus/tables-and-attributes.html
bd0ece79d44b3714edc88a1264870267c410d4eaaf299d5ad1c47600f2807a02 html/ferrodoc-task-lists.html alexgaoth/ferrodoc/38615d9217ac024ae1914b677f413906ac44c6a0/corpus/task-lists.html
454b8b290af0882c517e6d2ff4a97b5a2224228c6e58a17e356d76fe1e96cc30 md/ferrodoc-truncation-cases.md alexgaoth/ferrodoc/38615d9217ac024ae1914b677f413906ac44c6a0/corpus/truncation-cases.md
e4fad8f9db9d2c59428a0b51a86e43e27b54ddaa072dbc02bffb8d39886edd0c html/ferrodoc-verbatim.html alexgaoth/ferrodoc/38615d9217ac024ae1914b677f413906ac44c6a0/corpus/verbatim.html
934ebd6bc83e0881c9b5525df4d76a436aa122e5d3b4bd729186df5e32aa0dd7 md/pandoc-MANUAL.md jgm/pandoc/3.13/MANUAL.txt
3c63fa79536fe2cffe2deb120b35c72d26470f38405d84c86cef8b7c56b20c0a md/pandoc-doc-custom-readers.md jgm/pandoc/3.13/doc/custom-readers.md
e7faa457887e8d683f6d1874134bdba6926a723a458026b06f5673307fc0c9dc md/pandoc-doc-customizing-pandoc.md jgm/pandoc/3.13/doc/customizing-pandoc.md
806933ee94f46926132126e0a03cc3115773b77e1c3d646bacddcdcddbb30123 md/pandoc-doc-epub.md jgm/pandoc/3.13/doc/epub.md
1a152cee7a23655321e27f61ebdcdb4ce3adee82c00b68f76d76927aefb0619b md/pandoc-doc-faqs.md jgm/pandoc/3.13/doc/faqs.md
bf5494b79467cba892193664f28abcf83afd00033ce7e0e365259d2e4d0db6dc md/pandoc-doc-filters.md jgm/pandoc/3.13/doc/filters.md
64755259fb27dd53da77908b0069ea044ce38032e37f8d9a50932ad2f1538c54 md/pandoc-doc-getting-started.md jgm/pandoc/3.13/doc/getting-started.md
e7af5b7665541512f723e26baf0b16665a5782ada1e47b859044a089b363a549 md/pandoc-doc-jats.md jgm/pandoc/3.13/doc/jats.md
1a2aa18aa1db266eb46f9387b654b188a709715195bf067bc72b34c9872a8352 md/pandoc-doc-lua-filters.md jgm/pandoc/3.13/doc/lua-filters.md
9be4b33f9ce9f4b170a4dcca1d53b343643614dd67b086ae6588a5ecb9fd8e53 md/pandoc-doc-org.md jgm/pandoc/3.13/doc/org.md
e3dc22444e9ef172ceec7de6ae232577fdc52a62120d8e34b828c3279b7299c3 md/pandoc-doc-twenty-years-of-pandoc.md jgm/pandoc/3.13/doc/twenty-years-of-pandoc.md
6b59c0da98ca2681483dc190435ac8a26015d7c5a391a4b7b700565175146361 md/pandoc-doc-xml.md jgm/pandoc/3.13/doc/xml.md
357517ed7ec0d32cba7fd5c6b1aee2b4ed4c127a89d8a1d4102bc763cfeb7986 md/rescribe-README.md rhi-zone/rescribe/44357d5fa75f943acaa475f6122c8315e356811f/README.md
420c343870d7652105483e1f89cd83f50be9616d7c7247e0f28eec61641175c6 md/skylighting-README.md jgm/skylighting/9a414fca3ac94dee81fb76e0ef4f88c9e6037618/README.md
```

To make the probe folder again (each file is a copy, so the hashes must
match):

```sh
P="$RANDOC_ENV/research/probe"; mkdir -p "$P/md" "$P/html"
sed -n '/^[0-9a-f]\{64\} /p' docs/research-data/commands.md | while read -r hash dest src; do
  curl -sfL "https://raw.githubusercontent.com/$src" -o "$P/$dest"
  echo "$hash  $P/$dest"
done | shasum -a 256 -c
```

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
highlighting. `COMPATIBILITY.md` names five highlighted languages (C, Python,
bash, Ruby, Rust). A second test converted one short block of each with
`-f markdown -t html` and compared the bytes with the oracle:

| Language | Block | Result |
| --- | --- | --- |
| C | `#include <stdio.h>` and a `main` that calls `printf` | differs |
| Python | `def f(x):` with `return x + 1  # add` | identical |
| bash | `for i in 1 2; do echo "$i"; done` | identical |
| Ruby | `def greet(name) = puts "hi #{name}"` | identical |
| Rust | `fn main() { let x: u8 = 1; println!("{x}"); }` | identical |
 carta keeps `haskell.xml`, `json.xml`, `javascript.xml` and
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

## rescribe run

rescribe has its own command line (`rescribe convert IN --from F --to T`),
so `probe.py` runs it with that form and reads JSON input from `-`. It has
no standalone option and no Pandoc options, so the standalone units and the
command-line cases are `no-cli`. The rows are in `results.tsv` with the run
name `rescribe-run`. The oracle rows of this run are not repeated.

```sh
export PROBE_TOOLS="rescribe=${R}/rescribe/target/debug/rescribe"
python3 -I docs/research-data/probe.py "$R/probe" "$RANDOC_WT/research" "$R/probe-rescribe.tsv"
```

```text
rescribe   cli                          n=  7 no-cli=7
rescribe   commonmark>html              n= 22 differ=21 ws-only=1
rescribe   gfm>html                     n= 22 differ=22
rescribe   html>json                    n= 18 differ=18
rescribe   html>markdown                n= 18 differ=14 identical=4
rescribe   json                         n= 55 ast-equal=4 differ=20 exit-differ=21 identical=3 ws-only=7
rescribe   markdown>html                n= 22 differ=22
rescribe   markdown>html-s              n= 22 no-cli=22
rescribe   markdown>json                n= 22 differ=22
rescribe   markdown>latex               n= 22 differ=22
```

Median time for one document unit: 0.022 s (debug build, 146 units).
Example: `# Hi *there*` gives `<h1>Hi <em>there</em></h1>` without a final
newline and without the `id="hi-there"` that Pandoc writes.
