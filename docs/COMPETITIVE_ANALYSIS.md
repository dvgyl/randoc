# Competitive analysis

This document compares Randoc with other projects that reimplement Pandoc or
claim compatibility with it. Each number comes from a command in
`docs/research-data/commands.md`. The raw results are in
`docs/research-data/results.tsv`. The probe program is
`docs/research-data/probe.py`. The oracle is Pandoc 3.13.

## Summary

1. **No project gives byte-identical output for Pandoc 3.13.** On the 12
   Pandoc documentation files, carta and ferrodoc each gave identical
   Pandoc-Markdown-to-HTML output for 2 of 12 files, and identical
   standalone HTML for 0 of 12. Over all 6 conversions of these files, carta
   passed 29% and ferrodoc 24%. Each project pins an older Pandoc: carta pins 3.10,
   ferrodoc pins 3.8.2.1. Randoc is the only project that targets 3.13.
2. **carta** (Rust, `MIT OR Apache-2.0`, 202 000 lines, last push 2026-11-09) is
   the strongest project. Its document model is `pandoc-types` 1.23.1.2, its
   command line copies Pandoc's, and its TeX math output was identical to the
   oracle for one test document in four output formats. It does not use Pandoc's templates, so standalone
   output is never identical. It does not have citations or Lua filters.
3. **ferrodoc** (Rust, `MIT OR Apache-2.0`, 44 000 lines) is a library for a
   small set of formats. Its Markdown reader is built on `comrak`, so Pandoc
   Markdown extensions such as title blocks and fenced divs are missing. Its
   README claims 47 of 48 real command lines identical against Pandoc
   3.8.2.1. Against Pandoc 3.13 the same script gives 22 of 48.
4. **rescribe** (Rust, `MIT OR Apache-2.0`, 754 000 lines) is not a Pandoc
   replacement. It has its own document model and its own command line. It
   claims 54 readers and 64 writers but no identical output, and it passed
   11 of the 201 probe units that it could run.
5. **Neither carta nor ferrodoc copies Pandoc's exit codes.** For an unknown
   format, a JSON parse error or an unknown option, Pandoc exits with 21, 22,
   64 or 6. Both tools exit with 1 or 2. Both accept a JSON document with
   API version 1.22, which Pandoc rejects.
6. **The licence split of syntax definitions has a visible cost.** carta
   compiles only the permissively licensed KDE definitions. YAML, JSON,
   Haskell and JavaScript code blocks are not highlighted in its default
   build, and the Pandoc MANUAL is different for this reason.
7. **What Randoc can reuse**: the template approach of ferrodoc (Pandoc's
   templates under their BSD-3-Clause option), the design of carta's
   conformance suite, ferrodoc's corpus of real command lines, and Rust
   libraries for XML, ZIP, YAML, Unicode, Lua and image size. Randoc must
   write its own Pandoc Markdown reader, template and layout engines, TeX
   math converter, CSL processor and tagsoup-like HTML reader.
8. **Two lessons for the Randoc harness.** Two oracle runs on the same input
   gave different bytes for 9 of 76 writers: `docx`, `pptx`, `odt`, `fodt`
   and `chunkedhtml` (time stamps), `epub`, `epub2`, `epub3` (a random
   `urn:uuid` identifier and a date) and `ipynb` (a random cell `id`). With
   `SOURCE_DATE_EPOCH` set, only `epub` and `ipynb` still differ. The harness
   must set `SOURCE_DATE_EPOCH` and normalize the other two. Pandoc changes its
   output often between minor versions (RTF in 3.10.1, the HTML template
   after 3.8.2.1), so each oracle result must name its Pandoc version.

## Method

- Each project was cloned at its newest commit (shallow) and built in the
  debug profile. The standing orders forbid release builds except for a
  benchmark, so the speed numbers below are rough.
- The probe set has 22 Markdown documents, 18 HTML documents and the 18
  conformance cases of this repository. `docs/research-data/commands.md`
  lists them. 12 of the Markdown documents come from the Pandoc
  documentation (`MANUAL.txt` and `doc/*.md`); they favour no project. The
  HTML documents and 7 Markdown documents come from the test corpora of carta
  and ferrodoc; they favour those projects.
- Each Markdown document ran through 6 conversions (`markdown`,
  `commonmark` and `gfm` to HTML, `markdown` to JSON, LaTeX and standalone
  HTML). Each HTML document ran through 2 (`html` to Markdown and JSON). Each
  JSON conformance case ran through 5 writers. A second run converted the
  Markdown documents to 16 more text formats.
- A unit is "identical" when the standard output and the exit code are equal
  to the oracle's. For JSON output, a unit is also counted as a pass
  ("ast-equal") when the JSON values are equal after the
  `pandoc-api-version` field is removed. This is fair to ferrodoc, which
  writes API version `1.23.1` for Pandoc 3.8.

## Results of the probe set

Pass means identical or ast-equal.

| Input group | Units for each tool | carta pass | ferrodoc pass |
| --- | ---: | ---: | ---: |
| Pandoc documentation (12 Markdown files, 6 conversions) | 72 | 21 (29%) | 17 (24%) |
| ferrodoc corpus (7 Markdown and 10 HTML files) | 62 | 26 (42%) | 45 (73%) |
| carta corpus (8 HTML files) | 22 | 15 (68%) | 16 (73%) |
| README files of 3 projects | 12 | 5 (42%) | 6 (50%) |
| Conformance cases of this repository | 62 | 29 (47%) | 18 (29%) |

By conversion (all inputs):

| Conversion | Units | carta identical | ferrodoc identical or ast-equal |
| --- | ---: | ---: | ---: |
| `markdown` to `html` | 22 | 6 | 10 |
| `markdown` to `json` | 22 | 12 | 13 (ast-equal) |
| `markdown` to `latex` | 22 | 5 | 7 |
| `markdown` to `html -s` | 22 | 0 | 0 |
| `commonmark` to `html` | 22 | 12 | 15 |
| `gfm` to `html` | 22 | 12 | 11 |
| `html` to `markdown` | 18 | 9 | 12 |
| `html` to `json` | 18 | 11 | 16 (ast-equal) |
| JSON cases to 5 writers | 55 | 25 | 15 |
| Command-line cases | 7 | 4 | 3 |

Markdown (`-f markdown`) to 16 more writers, 22 documents each. "No
writer" means that the tool refused the format.

| Writer | carta identical | ferrodoc identical |
| --- | ---: | ---: |
| `asciidoc` | 7 | 11 |
| `commonmark` | 11 | 10 |
| `gfm` | 13 | 10 |
| `plain` | 11 | 11 |
| `rst` | 10 | 10 |
| `mediawiki` | 13 | no writer |
| `opml` | 14 | no writer |
| `native` | 12 | no writer |
| `dokuwiki` | 11 | no writer |
| `jira` | 10 | no writer |
| `docbook5` | 7 | no writer |
| `man` | 6 | no writer |
| `org` | 4 | no writer |
| `typst` | 4 | no writer |
| `rtf` | 0 | no writer |
| `ipynb` | 0 | 0 |

Two results need an explanation:

- `ipynb`: Pandoc writes a random cell `id` (a UUID) in each run, so no
  tool can give identical bytes. Randoc's harness needs a rule for this
  format (for example, compare with the `id` values removed).
- `rtf`: Pandoc 3.10.1 changed the RTF writer: each paragraph has a font
  size (`\fs24`), and lists use a real list table (`changelog.md` lines 1495
  to 1503). carta targets Pandoc 3.10 and writes the older form.

### Kinds of differences

These kinds come from `diff` of the outputs of the oracle and the tool for
the failing units (commands in `docs/research-data/commands.md`).

| Kind | carta | ferrodoc |
| --- | --- | --- |
| Standalone page: a different template | Yes. carta writes its own templates. Its `docs/STATUS.md` says: "Each writer's scaffold (CSS, preamble) is authored independently and is not byte-identical across tools." | Yes. ferrodoc uses the Pandoc 3.8.2.1 templates. Pandoc changed the HTML template after 3.8.2.1 (for example `light-dark()` colours and `<meta name="generator" content="pandoc 3.13">`). |
| Syntax highlighting missing for some languages | Yes: YAML, JSON, Haskell, JavaScript and other languages with non-permissive definitions. They are in `crates/carta-highlight/data/syntax-copyleft/` and are not compiled in by default. | Yes, for each language. The default build writes plain `<pre class="haskell"><code>` without highlighting (test in `commands.md`). |
| Pandoc Markdown extensions | Small differences (a space before `</a>` in a wrapped link). | Missing title block (`% title` becomes a paragraph) and missing fenced divs (`::: {.class}` stays text). The reader uses `comrak`. |
| Exit codes and error messages | Exit 1 or 2 instead of 21, 22, 23, 64 or 6. | The same. |
| Old JSON API versions | Accepted (Pandoc exits with 64). | Accepted. |
| Crash | None in the probe set. | One panic: `numbers#json>markdown` (an integer overflow in a sum, `core/src/iter/traits/accum.rs`). |
| `native` writer | Present. 6 of the 7 valid JSON cases identical; `numbers` differs. | Not present ("unknown format \"native\""). |

Pandoc itself exited with code 251 on the `numbers` case converted to
Markdown. carta exited with 0. This is a Pandoc failure, not a carta failure.

### Speed

All tools ran on a machine that other agents used at the same time. carta and
ferrodoc were debug builds; Pandoc is an optimized build. The numbers are
therefore only an upper limit for the two Rust tools.

| Tool | Median time for one document unit | Sum for 168 units |
| --- | ---: | ---: |
| Pandoc 3.13 | 0.803 s | 494.5 s |
| carta (debug) | 0.073 s | 59.7 s |
| ferrodoc (debug) | 0.081 s | 130.1 s |

Even unoptimized, both Rust tools are about 10 times faster for each
document, because Pandoc's start-up time dominates small documents.
carta's own benchmark (`docs/BENCHMARKS.md`, release build, Linux) claims
14 to 32 times. ferrodoc's README claims 72 times for an in-process library
call. These claims were not measured here.

## The projects

### carta

| Item | Value | Proof |
| --- | --- | --- |
| Repository | <https://github.com/mfkrause/carta> | |
| Licence | `MIT OR Apache-2.0` | `Cargo.toml` line 10; `LICENSE-MIT`, `LICENSE-APACHE`. Early crates.io versions (0.0.x) were described as AGPL-3.0 on docs.rs; the current version is permissive. |
| Size | 201 925 lines of Rust, 11 299 files, 3 075 `#[test]` functions, 47 fuzz targets | Commands in `commands.md` |
| Activity | Created 2026-06-06, last push 2026-11-09, 13 releases (0.0.1 to 0.0.13), 60 stars | `gh repo view`, `CHANGELOG.md` |
| Target | Pandoc 3.10 | `docs/STATUS.md` line 5 |
| Architecture | Crates `carta-ast` (the `pandoc-types` model, API version `[1, 23, 1, 2]`, `crates/carta-ast/src/lib.rs` line 32), `carta-core`, `carta-readers`, `carta-writers`, `carta-highlight` (a KDE syntax engine with `fancy-regex`), and `carta` (library and CLI). The CLI copies Pandoc's options. Few dependencies: `quick-xml`, `fancy-regex`, `miniz_oxide`, `granit-parser` (YAML), `clap`. | `Cargo.toml` files |
| Formats claimed | 36 readers and 33 writers marked usable (✅), including DOCX, ODT, EPUB, LaTeX, Typst, RST, Org, AsciiDoc, MediaWiki, DocBook, man, RTF, ipynb | `docs/STATUS.md` |
| Missing | Citations (`--citeproc`), Lua filters, PPTX, JATS, Djot, the bibliography formats, Texinfo, ConTeXt, many wiki formats | `docs/STATUS.md`, section "Cross-cutting features" |
| Tests | Differential tests against the oracle (`tools/conformance-suite/`), `insta` snapshots (about 9 000 files under `tests/`), CommonMark spec, fuzzing in CI (`.github/workflows/fuzz.yml`). It fetches the Pandoc test files at test time (`tools/fetch-pandoc-tests.sh`) and does not commit them. A list of "controls" records cases where carta deliberately differs because Pandoc is wrong (`tools/conformance-suite/correctness.py`). | Files named |

What Randoc can learn or reuse:

- The conformance suite design: "surfaces" (reader, writer, end-to-end,
  round trip) that take their format lists from `--list-input-formats` of
  both programs, a per-case `repro:` line in each failure log, and a list of
  deliberate differences with a reason for each. Randoc's harness has the
  same idea; the `repro:` line and the automatic format list are worth
  copying.
- The licence split of KDE syntax definitions
  (`vendor/syntax-highlighting/ATTRIBUTION.md`): permissive files compiled
  in, the other files loaded at run time. `docs/DATA_FILES_LICENCES.md`
  recommends the same split.
- `carta-highlight` (published on crates.io, `MIT OR Apache-2.0`) is the
  only Rust engine for KDE syntax definitions found. Randoc can test it
  against the oracle before it writes its own.
- carta's TeX math output was identical to the oracle for MathML, Typst,
  native and RTF on a small test with fractions, roots, sums, `\mathbb`, a
  `pmatrix` and an integral (`commands.md`). carta does not state where its
  symbol table (`crates/carta-writers/src/math/symbols/table.rs`) comes from.
  `texmath` is GPL-2. Randoc must not copy that table without a provenance
  record; it can use carta as a second black-box oracle.

What it does not do: identical standalone output, citations, Lua, PPTX,
Pandoc's exit codes, rejection of old JSON API versions.

### ferrodoc

| Item | Value | Proof |
| --- | --- | --- |
| Repository | <https://github.com/alexgaoth/ferrodoc> | |
| Licence | `MIT OR Apache-2.0`; three BSD-3-Clause files, including the Pandoc templates under their BSD option | `LICENSE-MIT`, `LICENSE-APACHE`, `THIRD-PARTY.md` lines 3 to 21 |
| Size | 44 420 lines of Rust, 391 files, 307 `#[test]` functions, 0 fuzz targets (a `corpus/bombs` generator and `scripts/hostile.sh` instead) | Commands in `commands.md` |
| Activity | Created 2026-08-15, last push 2026-10-06, version 1.0.1 (2026-09-02), 12 stars | `gh repo view`, `gh release view` |
| Target | Pandoc 3.8.2.1 | `COMPATIBILITY.md` line 1, `scripts/dropin.sh` (`PANDOC_PINNED=3.8.2.1`) |
| Architecture | One crate for each format (`ferrodoc-markdown`, `-html`, `-docx`, `-odt`, `-epub`, `-ipynb`, `-latex`, `-rst`, `-asciidoc`, `-text`), `ferrodoc-ast` (the `pandoc-types` model, API version `[1, 23, 1]`), a harness crate. Readers use `comrak` (Markdown) and `html5ever` (HTML). Bindings for Python, JavaScript (wasm32) and C. | `crates/`, `bindings/`, `Cargo.toml` files |
| Formats claimed | Read: Markdown, CommonMark, GFM, HTML, DOCX, ODT, EPUB, ipynb, JSON. Write: the same plus LaTeX, RST, AsciiDoc, plain text | `COMPATIBILITY.md`, section "Formats" |
| Missing | Org, presentations, citations, filters, `native`, most of Pandoc's formats | `COMPATIBILITY.md` |
| Tests | Harness commands with `--fail-under` thresholds for each surface (`COMPATIBILITY.md`); a corpus of 48 real Pandoc command lines from public Makefiles and CI files (`dropin/commands.tsv`); scripts for drift, hostile input and round trips. The README states its limits honestly ("It is not a drop-in `pandoc`"). | Files named |

ferrodoc's claim "47/48 identical" is true only for Pandoc 3.8.2.1. With the
oracle, the same script gives `22/48 command lines identical (45.8%)`. 20 of
the 25 "fixable" misses are HTML files, most of them standalone pages. This
shows how fast Pandoc's standalone output changes between versions.

What Randoc can learn or reuse:

- The corpus of real command lines (`dropin/commands.tsv`, MIT OR
  Apache-2.0) and the method to collect it with `gh search code`. Randoc can
  build the same kind of corpus as `cmd` cases.
- The use of Pandoc's templates under their BSD-3-Clause option, with the
  notice in `THIRD-PARTY.md`.
- The in-process library with bindings. Randoc's architecture already
  separates the library from the CLI; Python and wasm bindings are a product
  advantage over Pandoc for batch work.

What it does not do: Pandoc Markdown in full, most formats, exit codes,
`native`, fuzzing.

### rescribe

| Item | Value | Proof |
| --- | --- | --- |
| Repository | <https://github.com/rhi-zone/rescribe> | |
| Licence | `MIT OR Apache-2.0` | `Cargo.toml` (`license = "MIT OR Apache-2.0"`), `LICENSE-MIT`, `LICENSE-APACHE` |
| Size | 753 620 lines of Rust, 7 441 files, 3 807 `#[test]` functions, 128 fuzz targets | Commands in `commands.md` |
| Activity | Created 2026-01-09, last push 2026-08-29, version 0.1.0, 1 star | `gh repo view`, `Cargo.toml` |
| Target | No Pandoc version. "pandoc-inspired" | `README.md` line 3 |
| Architecture | An open document model ("open node kinds and property bags"), not the Pandoc model, with "fidelity tracking" warnings. More than 50 format crates. Pandoc JSON is a bridge format. The CLI is `rescribe convert IN --from F --to T`; it has no `-s` and no Pandoc options. It embeds `jaq` for queries. | `README.md`, `crates/rescribe-cli/src/main.rs` |
| Formats claimed | 54 readers and 64 writers | `crates/rescribe-cli/src/main.rs` line 3, `FORMATS.md` |
| Tests | Unit tests, round-trip tests, 128 fuzz targets. No differential test against Pandoc output. | `TESTING.md`, `fuzz/` |

Probe results (debug build, 49 minutes on the shared machine). rescribe has
no standalone option and no Pandoc options, so those units did not run.

| Conversion | Units | Identical or ast-equal |
| --- | ---: | ---: |
| `markdown`, `commonmark`, `gfm` to `html` | 66 | 0 (1 differs only in white space) |
| `markdown` to `json`, `latex` | 44 | 0 |
| `html` to `markdown` | 18 | 4 |
| `html` to `json` | 18 | 0 |
| JSON cases to 5 writers | 55 | 7 |

The differences are basic: for `# Hi *there*` rescribe writes
`<h1>Hi <em>there</em></h1>` without the identifier `id="hi-there"` and
without a final newline. There were no crashes and no time-outs. The median
time for one document was 0.022 s.

What Randoc can learn: the large set of fuzz targets, and the idea of
fidelity warnings for lossy conversions. rescribe is not a competitor for
drop-in compatibility. Its format crates are permissively licensed, but they
use a different document model, so Randoc cannot reuse them directly.

### Other projects

| Project | Language | Licence | State | Why it is not tested here |
| --- | --- | --- | --- | --- |
| zenfmt (<https://github.com/insanai/zenfmt>) | Zig | Not stated in the announcement | Announced 2026-08-12 | It converts 16 input formats to Markdown only. It does not claim Pandoc output compatibility (announcement on ziggit.dev). |
| panduck (crates `panduck`, `panduck-core`) | Rust | See crates.io | Last releases 2020 to 2022 | Dormant. "Inspired by pandoc", no compatibility claim (docs.rs). |
| `pandoc` crate | Rust | MIT OR Apache-2.0 (crates.io) | 0.8.11, 2023 | A wrapper that calls the Pandoc 2.x program. Not a reimplementation. |
| `pandoc-wasm-wrapper` | Rust | See crates.io | | Runs `pandoc.wasm` (Pandoc itself, GPL) in wasmer. Not a reimplementation. |
| `pandoc_types` | Rust | See crates.io | | The `pandoc-types` model only. |

Search queries used: crates.io search API (`pandoc`, `texmath`,
`latex to mathml`, `csl processor`, `kde syntax highlighting`,
`skylighting`, `doctemplates`), web search ("Rust pandoc compatible document
converter reimplementation 2026", "pandoc rewrite in Rust OR Go OR Zig drop-in
replacement"), and `gh search repos`.

## Reusable Rust libraries

The data comes from the crates.io API on 2026-11-09
(`https://crates.io/api/v1/crates/NAME`): the latest version, the licence of
that version, the date of the last update and the downloads of the last 90
days. "Fit" says how well the library matches what Randoc needs for
byte-identical Pandoc output.

| Need | Library | Version | Licence | Last update | Recent downloads | Fit for Randoc |
| --- | --- | --- | --- | --- | ---: | --- |
| YAML | `serde_yaml` | 0.9.34+deprecated | MIT OR Apache-2.0 | 2024-03-25 | 99.8 M | Do not use. The author marked it deprecated. |
| YAML | `serde_yml` | 0.0.13 | MIT OR Apache-2.0 | 2026-05-27 | 4.6 M | Do not use. RustSec advisory RUSTSEC-2025-0068 says that it is unsound and unmaintained, and that its repository is archived. |
| YAML | `yaml-rust2` | 0.13.0 | MIT OR Apache-2.0 | 2026-09-11 | 22.6 M | Good. Pure Rust YAML 1.2 parser with an event API. Randoc needs the event API: Pandoc parses metadata values as Markdown, so a typed deserializer is not sufficient. |
| YAML | `saphyr` | 0.3.3 | MIT OR Apache-2.0 | 2026-10-27 | 1.2 M | Good. Successor of `yaml-rust2` by the same group, with spans for error messages. Recommended first choice. |
| XML | `quick-xml` | 0.42.0 | MIT | 2026-08-22 | 169 M | Good for streaming read and write (DOCX, ODT, EPUB, DocBook, JATS, FB2). Fast. Randoc must handle entity and namespace details itself. |
| XML | `roxmltree` | 0.21.2 | MIT OR Apache-2.0 | 2026-10-24 | 38.5 M | Good for read-only trees (reference documents, OPML, small XML formats). No writer. |
| XML | `xml-rs` | 1.0.0 | MIT | 2026-08-06 | 30.8 M | Usable, slower than `quick-xml`. |
| ZIP | `zip` | 9.5.0 | MIT | 2026-11-09 | 102 M | Good. Read and write; Randoc must set the entry order, the time stamps and the compression to match Pandoc. Turn off the default features to keep the dependency tree small. |
| ZIP | `rc-zip` | 5.4.1 | Apache-2.0 OR MIT | 2025-11-19 | 0.7 M | Read only, sans-I/O. A second choice for the reader side. |
| HTML parsing | `html5ever` | 0.40.1 | MIT OR Apache-2.0 | 2026-09-14 | 55.5 M | Spec-conformant HTML5 tree building. Pandoc uses `tagsoup`, which is not an HTML5 parser, so html5ever gives a different tree for broken HTML. Use it only as a tokenizer, or write a tagsoup-like tokenizer. |
| HTML parsing | `html5gum` | 0.9.1 | MIT | 2026-11-04 | 0.7 M | HTML5 tokenizer only. A good base for a tagsoup-like reader. |
| HTML parsing | `lol_html` | 3.0.3 | BSD-3-Clause | 2026-10-28 | 2.5 M | Streaming rewriter. No fit for a reader. |
| HTML parsing | `scraper` | 0.29.0 | ISC | 2026-10-21 | 10.8 M | A wrapper of html5ever with CSS selectors. Not necessary. |
| HTML parsing | `tl` | 0.7.8 | MIT | 2024-01-29 | 0.7 M | Not maintained since 2024. Do not use. |
| Lua 5.4 | `mlua` | 0.12.2 | MIT | 2026-10-03 | 2.7 M | The only good choice. Feature `lua54` with `vendored` builds Lua 5.4 from C source (MIT). It needs `unsafe` code inside the library and a C compiler. |
| Lua 5.4 | `rlua` | 0.20.1 | MIT | 2024-07-20 | 0.04 M | Deprecated in favour of `mlua`. |
| Lua 5.4 | `piccolo` | 0.3.3 | MIT | 2024-06-16 | 0.05 M | A pure Rust interpreter, but not complete Lua 5.4 and not maintained. No fit. |
| Syntax highlighting | `syntect` | 5.3.0 | MIT | 2025-09-27 | 16.7 M | Sublime Text grammars, not KDE grammars. Token classes are different from skylighting, so output is never identical. No fit. |
| Syntax highlighting | `tree-sitter-highlight` | 0.27.2 | MIT | 2026-11-07 | 0.8 M | Different grammars and token classes. No fit. |
| Syntax highlighting | `carta-highlight` | 0.0.13 | MIT OR Apache-2.0 | 2026-10-30 | 0.002 M | A KDE syntax-definition engine in Rust, written for Pandoc compatibility. It is the only Rust engine for KDE definitions that this research found. Young (version 0.0.x), one author. Study it or depend on it after a test against the oracle. |
| Syntax highlighting | `fancy-regex` | 0.20.1 | MIT | 2026-11-09 | 100 M | KDE definitions use PCRE-style regular expressions with look-around and back-references. `fancy-regex` (on top of `regex`) supports them. `onig` (MIT, C library) is the alternative. carta uses `fancy-regex`. |
| TeX math | `math-core` | 0.9.0 | MIT | 2026-10-12 | 0.04 M | LaTeX to MathML Core. Active. A parser to study; its MathML is not the texmath MathML. |
| TeX math | `pulldown-latex` | 0.8.0 | MIT | 2026-07-28 | 0.07 M | LaTeX to MathML pull parser. Same remark. |
| TeX math | `latex2mathml` | 0.2.3 | MIT | 2020-04-27 | 0.04 M | Not maintained. |
| TeX math | `tex2math` | 2.4.1 | LGPL-3.0-only | 2026-08-26 | <0.001 M | Licence not permitted. |
| CSL citations | `citationberg` | 0.7.0 | MIT OR Apache-2.0 | 2026-06-05 | 2.0 M | Parses CSL styles and locales (from the Typst project). Good for the parse step. |
| CSL citations | `hayagriva` | 0.10.1 | MIT OR Apache-2.0 | 2026-06-14 | 2.0 M | A CSL processor. Its output and its disambiguation rules are not the same as Haskell `citeproc`, which Pandoc uses. Use it as a reference; port `citeproc` (BSD-2-Clause) for identical output. |
| CSL citations | `csl-legacy` | 0.84.0 | MIT OR Apache-2.0 | 2026-10-30 | <0.001 M | CSL 1.0 and CSL-JSON parsers of the Citum project. Young. |
| BibTeX and BibLaTeX | `biblatex` | 0.12.0 | MIT OR Apache-2.0 | 2026-06-05 | 2.3 M | A BibLaTeX parser from the Typst project. Its field normalization is different from Pandoc's; usable as a lexer. |
| Typst | `typst-syntax` | 0.15.1 | Apache-2.0 | 2026-07-17 | 2.1 M | The official Typst parser. Good for the Typst reader. Pandoc's reader (`typst-hs`, BSD-3-Clause) evaluates a subset of Typst; Randoc must port that evaluation. Apache-2.0 only is compatible with `MIT OR Apache-2.0` distribution. |
| Image size | `imagesize` | 0.15.0 | MIT | 2026-07-09 | 19.9 M | Good. Reads the header only. Pandoc also reads the DPI (from JFIF, PNG `pHYs` and EXIF); check that `imagesize` gives it, or add a small reader. |
| Image size | `image` | 0.25.10 | MIT OR Apache-2.0 | 2026-03-10 | 78.4 M | Too large for this need. |
| Unicode width | `unicode-width` | 0.2.3 | MIT OR Apache-2.0 | 2026-10-28 | 221 M | Pandoc uses `doclayout`, which has its own width table (`EastAsianWidth.txt` and its own rules for emoji). Values differ for some characters. Port the `doclayout` table (BSD-3-Clause). |
| Unicode segmentation | `unicode-segmentation` | 1.13.3 | MIT OR Apache-2.0 | 2026-06-01 | 175 M | Good. |
| Unicode normalization | `unicode-normalization` | 0.1.26 | (MIT OR Apache-2.0) AND Unicode-3.0 | 2026-10-16 | 144 M | Good. |
| Unicode properties and collation | `icu_properties`, `icu_collator` | 2.4.0 | Unicode-3.0 | 2026-10-22 | 259 M, 7.2 M | Good. Pandoc sorts citations with Unicode collation (`citeproc` uses `unicode-collation`); `icu_collator` gives the same algorithm, but the table version must match. |
| Emoji | `emojis` | 0.9.1 | (MIT OR Apache-2.0) AND Unicode-3.0 | 2026-10-17 | 2.0 M | Its shortcode set comes from `gemoji` like the Haskell `emojis` package, but the versions differ. Generate a table from the Haskell `emojis` 0.1.5 data (BSD-3-Clause) instead. |
| HTML entities | `entities` | 1.0.1 | MIT | 2023-04-10 | 3.7 M | The full WHATWG table. Stable data. Usable. |
| HTML entities | `htmlize` | 1.1.0 | MIT OR Apache-2.0 | 2026-04-13 | 1.7 M | Encode and decode with the WHATWG table. Usable. |
| CommonMark reference | `pulldown-cmark`, `comrak`, `markdown` | 0.13.6, 0.59.0, 1.0.0 | MIT, BSD-2-Clause, MIT | 2026 | 69.5 M, 3.4 M, 4.5 M | Useful as CommonMark spec references and for the spec test data. Pandoc's `commonmark` reader gives a Pandoc AST with Pandoc-specific source positions and extensions. Port `commonmark-hs` (BSD-3-Clause) instead of a wrapper. |
| JSON | `serde_json` | 1.0.154 | MIT OR Apache-2.0 | 2026-10-11 | 419 M | Good. Pandoc's JSON number format (for example `1.0e-3`) needs a custom writer. |

### Where no library fits

Randoc must write its own code for these parts:

1. **The Pandoc Markdown reader and writer.** No Rust crate parses Pandoc's
   Markdown dialect with its extensions.
2. **The template engine and the layout engine.** No Rust port of
   `doctemplates` or `doclayout` exists. Both are BSD-3-Clause and can be
   ported.
3. **TeX math conversion.** `texmath` is GPL-2. The MIT crates give different
   MathML. A new implementation tested against the oracle is necessary.
4. **A CSL processor with identical output.** Port `citeproc` (BSD-2-Clause).
5. **A tagsoup-like HTML tokenizer.** HTML5 tree building gives different
   trees for broken input.
6. **KDE syntax highlighting.** Only `carta-highlight` exists, and it is
   young. Either depend on it after tests, or port `skylighting-core`
   (BSD-3-Clause).
7. **The DOCX, ODT, PPTX and EPUB writers and readers.** Rust crates for
   OOXML exist (for example the `ooxml-*` crates that rescribe uses), but none
   gives Pandoc's element order and attributes. Use `quick-xml` and `zip`
   directly.
