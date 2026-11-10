# Prior art and reuse

Randoc does not have to build each part from zero. Much of what Randoc needs
exists in open source. This file is the record of that material: what Randoc
uses now, and what a workstream can study or use next.

## The rule

The owner gave this rule on 2026-11-10. It applies to each workstream.

1. **Study first.** Before you write a subsystem, look for open-source code
   that does the same work. Build on it where that is faster or more reliable.
2. **Record before reuse.** Before you reuse anything, add a row to this file
   and write a short decision record in [`ADR.md`](ADR.md) that says why you
   chose it.
3. **Check the licence.** The repository is public, and its licence is
   `MIT OR Apache-2.0`. The licence of the material must be compatible with
   that. Keep the attribution in [`THIRD_PARTY_NOTICES.md`](../THIRD_PARTY_NOTICES.md)
   and the full licence text in `third_party/licences/`.
4. **Depend, do not copy.** Prefer a dependency on a pinned upstream release
   (an exact version in `Cargo.toml` and `Cargo.lock`) to a copy of the code.
5. **Fork or vendor only for a change.** Copy code into the repository only
   when Randoc must change it. Keep the changes small, describe them in the
   decision record and in the `NOTICE.md` of the crate, and cover them with
   Randoc tests.
6. **Same quality bar.** Reused code must pass the same merge gate as code
   that a workstream writes: oracle-derived tests, limits for untrusted input
   (nesting depth, size, time), fuzzing of each parser path that input can
   reach, and two reviews.

Licences that are compatible: MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause,
ISC, Zlib, Unicode-3.0, CC0-1.0, Unlicense. Check each other licence with
`docs/DATA_FILES_LICENCES.md` and record the result here.

Material that Randoc cannot reuse:

- **Pandoc itself** (GPL-2.0-or-later): its Haskell source, its tests and its
  data files without a permissive option. Pandoc 3.13 stays the oracle: a
  black box that only tests run. Randoc never calls it.
- **`texmath`** (GPL-2.0-or-later), the TeX math library of Pandoc.
- **`pandoc-lua-engine`, `pandoc-server`, `skylighting`** (the package with
  all syntax definitions; GPL-2.0-or-later). `skylighting-core` is
  BSD-3-Clause and is permitted.
- Each file with a copyleft licence or with no licence grant
  (`docs/DATA_FILES_LICENCES.md`, decisions D1 to D5 in `PROGRESS.md`).

One fact makes reuse harder than in most projects: the output of Randoc must
be byte-identical to the output of Pandoc. A general-purpose library for the
same format (for example another CommonMark parser) gives a different tree
for edge cases. So the best sources are the libraries that Pandoc itself is
built from, where their licence permits a port, and Rust crates for the
infrastructure below the format rules (archives, XML, compression, Unicode
data, Lua, network).

## Material in use on `main`

"Port" means that a workstream translated the upstream code to Rust. "Data"
means tables or files. "Observed" means that no upstream code or data is in
the repository; the behaviour was found with the oracle.

| Material | Upstream and version | Licence | Use | Where | Decision |
| --- | --- | --- | --- | --- | --- |
| Document model: type names and JSON form | `pandoc-types` 1.23.1.2 | BSD-3-Clause | Reimplemented from its documented interface | `crates/randoc-ast` | ADR 2 |
| Layout engine | `doclayout` 0.6 | BSD-3-Clause | Port | `crates/randoc-layout` | ADR 3 |
| Template language | `doctemplates` 0.11.1 | BSD-3-Clause | Port | `crates/randoc-templates` | ADR 4 |
| Error texts of the template parser | `parsec` 3.1.17.0 | BSD-2-Clause | Port of the error-merge rules | `crates/randoc-templates/src/parsec.rs` | ADR 4 |
| Default templates and the dzslides template | Pandoc 3.13 `data/templates`, `data/dzslides` | BSD-3-Clause option; WTFPL | Data, unchanged | `crates/randoc-data` | ADR 5 |
| Number formatting of `show` for `Double`, `Char`, `String` | GHC 9.12.2 (`GHC.Internal.Float`) | BSD-3-Clause (GHC licence) | Port | `crates/randoc-ast/src/json/haskell.rs` | ADR 6 |
| HTML entity table | `commonmark` 0.3.1 of `commonmark-hs` | BSD-3-Clause | Data | `crates/randoc-core/src/shared/tables.rs` | ADR 7 |
| ASCII table of CommonMark identifiers | `commonmark-extensions` 0.2.7.3 | BSD-3-Clause | Data | same file | ADR 7 |
| Emoji tables | `emojis` 0.1.5; GitHub `gemoji` data | BSD-3-Clause; MIT | Data | same file; `crates/randoc-layout/src/tables.rs` | ADR 7 |
| Character classes, decompositions, NFC tables | Unicode Character Database 15.1.0, 16.0.0, 18.0.0 | Unicode-3.0 | Generated tables | `crates/randoc-core/src/shared/`, `crates/randoc-ast/src/json/categories.rs` | ADR 7 |
| JSON error texts | `aeson` 2.2 | BSD-3-Clause | Observed | `crates/randoc-ast/src/json/` | none (no reuse) |
| Layout rules of the native writer | `pretty-show` 1.10, `pretty` 1.1.3 | MIT; BSD-3-Clause | Observed | `crates/randoc-native` | none (no reuse) |
| Default abbreviation list | none: 76 entries found with 3,926,072 oracle probes | not applicable | Observed | `crates/randoc-core/src/shared/` | none (no reuse) |

Rust dependencies on `main`, pinned by `Cargo.lock`:

| Crate | Version | Licence | Used by |
| --- | --- | --- | --- |
| `serde`, `serde_json` | 1.0.229, 1.0.154 | MIT OR Apache-2.0 | The harness (a test tool), and tests of `randoc-core`, `randoc-native`, `randoc-templates` |
| `toml` | 1.2.1 | MIT OR Apache-2.0 | The harness, tests of `randoc-native` |
| `zip` | 9.5.0 | MIT | The harness |
| `nix` | 0.31.3 | MIT | Time tests of `randoc-native` on Unix |

The library and the program on `main` have no external dependency at run
time. That was a result of the first sessions, not a goal. The rule above
replaces it.

## Material in use on branches that are not merged

| Material | Upstream and version | Licence | Use | Branch |
| --- | --- | --- | --- | --- |
| Syntax highlighting engine | `skylighting-core` 0.15 | BSD-3-Clause | Port | `ws/highlight` |
| 63 KDE syntax definitions with a permissive licence | `skylighting-core` 0.15 `xml/` (from KDE syntax-highlighting) | MIT, BSD, Apache-2.0 and others; one row for each file on the branch | Data | `ws/highlight` |
| YAML event parser | `saphyr-parser` (version in the branch `Cargo.lock`) | MIT OR Apache-2.0 | Dependency, until the reader moves to `randoc-yaml` | `ws/markdown-reader` |
| CommonMark specification examples | `commonmark-spec` 0.31.2 | CC-BY-SA-4.0 | Not in the repository: `tools/commonmark-spec/run.py` downloads the file and stores only numbers | `ws/commonmark-reader` |

## Candidates to study

No workstream uses these yet. The licence in the table is the licence that
the coordinator remembers. **It is not checked.** Check the licence file of
the exact release before you reuse a candidate, then move its row to a table
above.

Libraries that Pandoc is built from (a port keeps the behaviour of the
oracle):

| Need | Candidate | Licence to check | Note |
| --- | --- | --- | --- |
| CommonMark and GFM reader | `commonmark`, `commonmark-extensions`, `commonmark-pandoc` (`commonmark-hs`) | BSD-3-Clause | The parser that the oracle uses for `commonmark`, `gfm` and `commonmark_x`. `ws/commonmark-reader` was written from black-box tests; a comparison with this source can close its open rows. |
| Citations (`--citeproc`) | `citeproc` | BSD-2-Clause | Needs CSL styles and locales; their licences are in `docs/DATA_FILES_LICENCES.md` (D2). |
| Sort order for citations | `unicode-collation` | BSD-2-Clause | |
| Djot reader and writer | `djot` (`djoths`) | MIT | |
| Typst reader | `typst-hs` | BSD-3-Clause | |
| Grid tables | `gridtables` | MIT | |
| Jira reader and writer | `jira-wiki-markup` | MIT | |
| Jupyter notebooks | `ipynb` | BSD-3-Clause | |
| Containers of `docx`, `odt`, `epub` | `zip-archive` | BSD-3-Clause | For the byte layout of the archives that the oracle writes. |
| Lua interface | `hslua` and its packages | MIT | The Lua API of Pandoc itself is in GPL code: implement that part from the manual and from oracle behaviour. |

Rust crates for infrastructure (pin the exact version):

| Need | Candidate | Licence to check | Note |
| --- | --- | --- | --- |
| ZIP containers | `zip` (in use in the harness) | MIT | |
| Compression | `flate2`, `miniz_oxide` | MIT OR Apache-2.0, Zlib | Compressed bytes can differ from the oracle; binary formats are compared after unpacking. |
| XML reading (`docx`, `odt`, `epub`, `jats`, `docbook`, `opml`, `fb2`, `endnotexml`) | `quick-xml`, `roxmltree` | MIT; MIT OR Apache-2.0 | Check entity handling and limits for untrusted input. |
| Lua 5.4 for filters and custom readers and writers | `mlua` with vendored Lua 5.4 | MIT | The oracle embeds Lua 5.4. |
| HTTP for `--embed-resources` and remote input | `ureq` | MIT OR Apache-2.0 | Only through the `Context` of `randoc-core`, so that `--sandbox` holds. |
| Hashes and encodings | `sha1`, `sha2`, `base64`, `percent-encoding` | MIT OR Apache-2.0 | |
| CSV reader | `csv` | MIT OR Unlicense | Compare the quoting rules with the oracle. |
| Image size (`docx`, `odt`, HTML dimensions) | `imagesize` | MIT | Pandoc has its own rules; compare with the oracle. |
| BibTeX and BibLaTeX input | `biblatex` | MIT OR Apache-2.0 | |
| CSL processing | `hayagriva`, `citationberg` | MIT OR Apache-2.0 | Output must be equal to the output of `citeproc`; study before a port of `citeproc`. |
| Typst syntax | `typst-syntax` | Apache-2.0 | |
| Djot parser | `jotdown` | MIT | |
| Unicode normalization | `unicode-normalization` | MIT OR Apache-2.0 | The oracle normalizes with Unicode 18.0 data; the shared module has its own tables for that reason. Use the crate only if its Unicode version is the same. |

TeX math (the workstream `math`; `texmath` is not permitted):

| Need | Candidate | Licence to check | Note |
| --- | --- | --- | --- |
| Parser structure and symbol tables for TeX math | KaTeX, Temml | MIT | JavaScript; a source for command lists and Unicode mappings. |
| TeX math to MathML in Rust | `pulldown-latex`, `latex2mathml` | MIT | The MathML text must be equal to the oracle text, so use these as a base for the parser and check each rule with the oracle. |
| Symbol names | `unicode-math` table | LPPL-1.3c | Check whether LPPL permits the use as a data source. |

Parsers that are useful for differential tests and for study, but whose
trees differ from the oracle: `pulldown-cmark` (MIT), `comrak`
(BSD-2-Clause), `markdown-rs` (MIT), `html5ever` (MIT OR Apache-2.0),
`orgize` (MIT), `syntect` (MIT; it reads Sublime definitions, not KDE
definitions).

Test suites: the CommonMark and GFM specifications (CC-BY-SA-4.0) and the
Pandoc tests (GPL) stay outside the repository; a tool downloads them and
stores only numbers (`tools/pandoc-testsuite`, `tools/commonmark-spec` on its
branch). A test suite with a permissive licence can go into the repository
with its row in this file.
