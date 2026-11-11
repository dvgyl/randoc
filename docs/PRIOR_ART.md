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
- **`pandoc-lua-marshal` 0.3.2.1: the files `src/Text/Pandoc/Lua/Walk.hs`,
  `SpliceList.hs` and `Topdown.hs`.** Each one has the header "License: GNU
  GPL, version 2 or above", although the `LICENSE` file of the package is
  MIT. The other files of the package have an MIT header or no header and
  are permitted (ADR 16).
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
| HTML tag tokenizer and tag renderer | `tagsoup` 0.14.8 (the version of the Stackage snapshot `lts-24.51` that Pandoc 3.13 builds with) | BSD-3-Clause | Port | `crates/randoc-html-reader/src/tokens.rs` | ADR 13 |
| Join rules of an inline list, grid rules of a table | `pandoc-types` 1.23.1 (`Text.Pandoc.Builder`) | BSD-3-Clause | Port | `crates/randoc-html-reader/src/text.rs`, `src/table.rs` | ADR 13 |
| Tree rules of the HTML reader | none: found with oracle probes | not applicable | Observed | `crates/randoc-html-reader/src/parse.rs`, `src/table.rs` | ADR 13 |

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
| CSL processor: types, style parser, locale parser, locale merge (later units: evaluation, output) | `citeproc` 0.14.1.1 (Hackage, SHA-256 `e5a353f975c32169979b4c9e158d3edb9e57a5f653fab8ff0505a2067e2b1b4b`) | BSD-2-Clause | Port | `ws/citeproc` (ADR 17) |
| BCP 47 language tag (`Lang`, `parseLang`, `renderLang`) | `unicode-collation` 0.1.3.7 `src/Text/Collate/Lang.hs` (code only; no data file) | BSD-2-Clause | Port | `ws/citeproc` (ADR 17) |
| XML events for CSL styles and locales | `quick-xml` 0.42.0 (`=0.42.0`, `default-features = false`), with `memchr` 2 | MIT; Unlicense OR MIT | Dependency (ADR 10) | `ws/citeproc` |
| Candidate command names for the TeX math probes (names only, 1,013 names) | KaTeX 0.16.11, `dist/katex.js` of the npm package | MIT | Data: `crates/randoc-math/tools/katex-names.txt`; the tables of `src/symbols.rs` hold oracle results for these names (ADR 14) | `ws/math` |
| CommonMark and GFM reader: delimiter run matching, the path rule of `rebase_relative_paths` with its URI scheme list, the footnote, definition list and attribute block rules | `commonmark` 0.3.1, `commonmark-extensions` 0.2.7.3, `commonmark-pandoc` 0.3.0.2 (Hackage release archives; the versions of the `stack.yaml` of Pandoc 3.13) | BSD-3-Clause | Port of rules and one data table; the list is in `crates/randoc-commonmark/NOTICE.md`; ADR 12 | `ws/commonmark-reader` |

## Candidates to study

No workstream uses these yet. The workstream `reuse` checked each row on
2026-11-10: it read the licence file of the named release (the URL in the
row). A row with the words "not checked" gives the reason. Before you reuse a
candidate, read the licence file again if you use a different version, then
move its row to a table above. Compatible means compatible with
`MIT OR Apache-2.0` under the list of licences in "The rule".

Libraries that Pandoc is built from (a port keeps the behaviour of the
oracle). The version is the version in `stack.yaml` of Pandoc 3.13 or in its
snapshot `lts-24.51`, inside the bounds of `pandoc.cabal`:

| Need | Candidate and checked release | Licence (licence file) | Compatible | Note |
| --- | --- | --- | --- | --- |
| Sort order for citations | `unicode-collation` 0.1.3.7 | Code BSD-2-Clause; data files under other terms (https://hackage.haskell.org/package/unicode-collation-0.1.3.7/src/LICENSE) | Code yes; part of the data no | `data/tailorings/*.txt` and `data/cjk/*.txt` come from the Perl module `Unicode::Collate` "under the same terms as Perl" (Artistic-1.0-Perl OR GPL-1.0-or-later). Do not copy them. `data/allkeys.txt` (UCA 13.0.0) and `UnicodeData.txt` (13.0.0) are Unicode data: take them from unicode.org. |
| Djot reader and writer | `djot` 0.1.4.3 (`djoths`) | MIT (https://hackage.haskell.org/package/djot-0.1.4.3/src/LICENSE) | Yes | |
| Typst reader | `typst` 0.12 (`typst-hs`) | BSD-3-Clause (https://hackage.haskell.org/package/typst-0.12/src/LICENSE) | Yes | |
| Grid tables | `gridtables` 0.1.1.1 | MIT (https://hackage.haskell.org/package/gridtables-0.1.1.1/src/LICENSE) | Yes | |
| Jira reader and writer | `jira-wiki-markup` 1.5.1 | MIT (https://hackage.haskell.org/package/jira-wiki-markup-1.5.1/src/LICENSE) | Yes | |
| Jupyter notebooks | `ipynb` 0.2 | BSD-3-Clause, with a second BSD-3-Clause notice of the Jupyter Development Team (https://hackage.haskell.org/package/ipynb-0.2/src/LICENSE) | Yes | |
| Containers of `docx`, `odt`, `epub` | `zip-archive` 0.5 | BSD-3-Clause (https://hackage.haskell.org/package/zip-archive-0.5/src/LICENSE) | Yes | For the byte layout of the archives that the oracle writes. The `.cabal` file says `BSD3`; the third clause of the text is "The names of its contributors may not be used to endorse". |
| Lua interface | `hslua` 2.5.0, `hslua-core` 2.3.2.1, `hslua-marshalling` 2.3.2, `hslua-objectorientation` 2.5.0, `hslua-packaging` 2.4.1, `hslua-typing` 0.2.0, `hslua-module-doclayout` 1.2.1.1, `-path` 1.2.0, `-system` 1.3.0, `-text` 1.2.0, `-version` 1.2.0.1, `-zip` 1.2.1, `lua` 2.3.4 | MIT (https://hackage.haskell.org/package/hslua-2.5.0/src/LICENSE and the same path for each package) | Yes | The Lua API of Pandoc itself is in GPL code: implement that part from the manual and from oracle behaviour. `lua` 2.3.4 bundles Lua 5.4.8 (`cbits/lua-5.4.8`, MIT, notice in `lua.h`), the same patch version as the oracle (ADR 11). |
| Lua `pandoc.path`, inline merge and table normalization | `filepath` 1.5.4.0 (POSIX rules of `System.FilePath`), `pandoc-types` 1.23.1 (`Text.Pandoc.Builder`) | BSD-3-Clause (https://hackage.haskell.org/package/filepath-1.5.4.0/src/LICENSE, https://hackage.haskell.org/package/pandoc-types-1.23.1/src/LICENSE) | Yes | Ported in `randoc-lua` (`path.rs`, `utils.rs`, `normalize.rs`); ADR 16. |

Rust crates for infrastructure (pin the exact version). "Dependencies"
names each direct dependency of the release whose licence is outside the
list; the source is the `license` field of the dependency release that the
requirement selects:

| Need | Candidate and checked release | Licence (licence file) | Compatible | Note |
| --- | --- | --- | --- | --- |
| ZIP containers | `zip` 9.5.0 (released 2026-11-09; in use in the harness) | MIT (https://docs.rs/crate/zip/9.5.0/source/LICENSE) | Yes | Chosen in ADR 9. Dependencies: all compatible. |
| ZIP containers (rejected) | `rc-zip` 5.4.1; `async_zip` 0.0.19 | MIT OR Apache-2.0 (https://github.com/bearcove/rc-zip/blob/rc-zip-v5.4.1/LICENSE-MIT; the crate has no licence file); MIT (https://docs.rs/crate/async_zip/0.0.19/source/LICENSE) | Yes | Rejected in ADR 9. |
| Compression | `flate2` 1.1.11, `miniz_oxide` 0.9.1, `zlib-rs` 0.6.8 (the back end of `flate2` that ADR 9 selects) | MIT OR Apache-2.0 (https://docs.rs/crate/flate2/1.1.11/source/LICENSE-MIT); MIT OR Zlib OR Apache-2.0 (https://docs.rs/crate/miniz_oxide/0.9.1/source/LICENSE); Zlib (https://docs.rs/crate/zlib-rs/0.6.8/source/LICENSE) | Yes | Compressed bytes can differ from the oracle; binary formats are compared after unpacking. |
| XML reading (`docx`, `odt`, `epub`, `jats`, `docbook`, `opml`, `fb2`, `endnotexml`) | `quick-xml` 0.42.0 | MIT (https://docs.rs/crate/quick-xml/0.42.0/source/LICENSE-MIT.md) | Yes | Chosen in ADR 10. Dependencies: all compatible (`encoding_rs` is optional and has `(Apache-2.0 OR MIT) AND BSD-3-Clause`). |
| XML reading (rejected) | `roxmltree` 0.21.2; `xmlparser` 0.13.6 (released 2023-09-30); `xml-rs` 1.0.0, which only forwards to `xml` 1.0.0 (released 2025-08-25) | MIT OR Apache-2.0 (https://docs.rs/crate/roxmltree/0.21.2/source/LICENSE-MIT; https://docs.rs/crate/xmlparser/0.13.6/source/LICENSE-MIT); MIT (https://github.com/kornelski/xml-rs/blob/1.0.0/LICENSE, the crate `xml-rs` has no licence file; https://docs.rs/crate/xml/1.0.0/source/LICENSE) | Yes | Rejected in ADR 10. `xml` 1.0.0 has its own entity and size limits and stays a candidate. |
| Lua 5.4 for filters and custom readers and writers | `mlua` 0.12.2 without `vendored`, with `mlua-sys` 0.13.0 (feature `external`) and Lua 5.4.8 (`lua-5.4.8.tar.gz` of lua.org) | MIT (`LICENSE` in https://static.crates.io/crates/mlua/mlua-0.12.2.crate; `mlua-sys` 0.13.0 has no licence file and gives `license = "MIT"` in https://static.crates.io/crates/mlua-sys/mlua-sys-0.13.0.crate; Lua: the notice at the end of `src/lua.h` in https://www.lua.org/ftp/lua-5.4.8.tar.gz) | Yes | Chosen in ADR 11. A Randoc build script compiles Lua 5.4.8 without `LUA_COMPAT_5_3`. The feature `vendored` is not used, because `lua-src` always defines `LUA_COMPAT_5_3`. The copy of the Lua sources is an approved exception to rule 5 (ADR 11). |
| Lua (rejected) | `piccolo` 0.3.3 (released 2024-06-16); `rlua` 0.20.1 | MIT OR CC0-1.0 (https://docs.rs/crate/piccolo/0.3.3/source/LICENSE-MIT); MIT (https://docs.rs/crate/rlua/0.20.1/source/LICENSE) | Yes | Rejected in ADR 11. `rlua` 0.20.1 is a wrapper of `mlua` 0.9. |
| HTTP for `--embed-resources` and remote input | `ureq` 3.5.1 | MIT OR Apache-2.0 (https://docs.rs/crate/ureq/3.5.1/source/LICENSE-MIT) | Yes, with a condition | Only through the `Context` of `randoc-core`, so that `--sandbox` holds. The dependencies `webpki-roots` 1.0.11 and `webpki-root-certs` 1.0.11 have `CDLA-Permissive-2.0`, which is not in the list. The default features are `rustls` and `gzip`, and `rustls` turns on `webpki-roots`. So turn off the default features. Two feature lists have no bundled roots: `rustls-no-provider` with `platform-verifier`, or `native-tls-no-default`. The `ureq` README calls both unstable, and the agent must set the roots and the crypto provider itself. On `wasm32`, `rustls-platform-verifier` 0.7.1 depends on `webpki-root-certs`, so HTTP stays off for WebAssembly. Check that `cargo tree -i webpki-roots` and `cargo tree -i webpki-root-certs` find nothing. The other way is a decision for CDLA-Permissive-2.0 in `docs/DATA_FILES_LICENCES.md`. |
| Hashes and encodings | `sha1` 0.11.0, `sha2` 0.11.1, `base64` 0.23.1, `percent-encoding` 2.3.2 | MIT OR Apache-2.0 (https://docs.rs/crate/sha1/0.11.0/source/LICENSE-MIT, https://docs.rs/crate/sha2/0.11.1/source/LICENSE-MIT, https://docs.rs/crate/base64/0.23.1/source/LICENSE-MIT, https://docs.rs/crate/percent-encoding/2.3.2/source/LICENSE-MIT) | Yes | |
| CSV reader | `csv` 1.4.0 | MIT OR Unlicense (https://docs.rs/crate/csv/1.4.0/source/COPYING) | Yes | Compare the quoting rules with the oracle. |
| Image size (`docx`, `odt`, HTML dimensions) | `imagesize` 0.15.0 | MIT (https://docs.rs/crate/imagesize/0.15.0/source/LICENSE) | Yes | Pandoc has its own rules; compare with the oracle. |
| BibTeX and BibLaTeX input | `biblatex` 0.12.0 | MIT OR Apache-2.0 (https://docs.rs/crate/biblatex/0.12.0/source/LICENSE-MIT) | Yes | |
| CSL processing | `hayagriva` 0.10.1, `citationberg` 0.7.0 | MIT OR Apache-2.0 (https://docs.rs/crate/hayagriva/0.10.1/source/LICENSE-MIT, https://docs.rs/crate/citationberg/0.7.0/source/LICENSE-MIT) | Code yes; bundled styles no | Output must be equal to the output of `citeproc`; study before a port of `citeproc`. The `NOTICE` file of `hayagriva` says that the CSL styles and locales in `archive/` are CC-BY-SA-3.0, and the default feature `archive` builds them in. Turn off the default features. |
| Typst syntax | `typst-syntax` 0.15.1 | Apache-2.0 (https://github.com/typst/typst/blob/v0.15.1/LICENSE; the crate has no licence file) | Yes | |
| Djot parser | `jotdown` 0.10.0 | MIT (https://docs.rs/crate/jotdown/0.10.0/source/COPYING) | Yes | |
| Unicode normalization | `unicode-normalization` 0.1.26 | (MIT OR Apache-2.0) AND Unicode-3.0 (https://docs.rs/crate/unicode-normalization/0.1.26/source/LICENSE-MIT, https://docs.rs/crate/unicode-normalization/0.1.26/source/LICENSE-UNICODE) | Yes | The oracle normalizes with Unicode 18.0 data. Release 0.1.26 has `UNICODE_VERSION = (18, 0, 0)`, so ADR 7 permits a test that compares it with the shared tables, and then the replacement of those tables. |

TeX math (the workstream `math`; `texmath` is not permitted):

| Need | Candidate and checked release | Licence (licence file) | Compatible | Note |
| --- | --- | --- | --- | --- |
| Parser structure and symbol tables for TeX math | KaTeX 0.19.1, Temml 0.14.0 | MIT (https://github.com/KaTeX/KaTeX/blob/v0.19.1/LICENSE; for Temml the tag `v0.14.0` holds only `dist/` and `contrib/`, so the file is `package/LICENSE` of https://registry.npmjs.org/temml/-/temml-0.14.0.tgz) | Yes | JavaScript; a source for command lists and Unicode mappings. |
| TeX math to MathML in Rust | `pulldown-latex` 0.8.0, `latex2mathml` 0.2.3 (released 2020-04-27) | MIT (https://docs.rs/crate/pulldown-latex/0.8.0/source/LICENSE, https://docs.rs/crate/latex2mathml/0.2.3/source/LICENSE) | Yes | The MathML text must be equal to the oracle text, so use these as a base for the parser and check each rule with the oracle. |
| Symbol names | `unicode-math` 0.8r, file `unicode-math-table.tex` | LPPL-1.3c or later (https://github.com/latex3/unicode-math/blob/v0.8r/LICENSE and `COPYRIGHT`) | No, by project policy | Policy: the repository holds no table made from this file under `MIT OR Apache-2.0`. A different decision needs the owner. Reasons: LPPL covers each Derived Work, which it defines as a work that is derived from the Work under applicable law. Clause 10a permits a different licence only if that licence honours clause 6. The section "Derived Works That Are Not Replacements" says that clauses 6b and 6d do not apply to a work that does not replace the Work. Whether a generated table is a Derived Work depends on the extraction, and nobody assessed that. Prefer the Temml or KaTeX tables (MIT), or Unicode data (Unicode-3.0). |

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
