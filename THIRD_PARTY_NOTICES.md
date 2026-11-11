# Third-party notices

Randoc is an independent implementation of the behaviour of Pandoc. Pandoc is
licensed under GPL-2.0-or-later. Randoc contains no Pandoc source code and no
Pandoc test files.

This file lists each file or design that comes from a different project,
with its licence and its attribution. The full licence texts are in
`third_party/licences/`. The rule for reuse and the list of candidates are
in [`docs/PRIOR_ART.md`](docs/PRIOR_ART.md); the reason for each choice is
in [`docs/ADR.md`](docs/ADR.md). The name of this file was `THIRD_PARTY.md`
until 2026-11-10.

| Material | Source | Licence | Where | Licence text |
| --- | --- | --- | --- | --- |
| Document model (type names and JSON form) | `pandoc-types` 1.23.1.2 | BSD-3-Clause | `crates/randoc-ast` | [`third_party/licences/pandoc-types.txt`](third_party/licences/pandoc-types.txt) |
| Layout engine, width rules and character width table | `doclayout` 0.6 (https://github.com/jgm/doclayout) | BSD-3-Clause | `crates/randoc-layout/src` | [`third_party/licences/doclayout.txt`](third_party/licences/doclayout.txt) |
| First code points of the base emoji (from the Unicode emoji data) | `emojis` 0.1.5, `baseEmojis` (https://github.com/jgm/emojis) | BSD-3-Clause | `crates/randoc-layout/src/tables.rs` | [`third_party/licences/emojis.txt`](third_party/licences/emojis.txt) |
| Layout rules of the native writer (the `valToDoc` layout and the `sep`, `hang` and ribbon rules). Reimplemented from a description and from the output of the oracle. No code is copied. | `pretty-show` 1.10, `pretty` 1.1.3 | MIT, BSD-3-Clause | `crates/randoc-native/src/pretty.rs` | Not applicable: no code and no data is copied |
| Digit generation of `show` for `Double` (`floatToDigits`, ported from `GHC.Internal.Float`), and the `show` forms of `Double` (`formatRealFloat`) and of `Char` and `String` (`showLitChar`) | GHC 9.12.2 (tag `ghc-9.12.2-release`), library `base` 4.21 with `ghc-internal` 9.1202 | BSD-3-Clause | `crates/randoc-ast/src/json/haskell.rs` | [`third_party/licences/GHC-base.txt`](third_party/licences/GHC-base.txt) |
| JSON error texts and error paths | `aeson` 2.2: the texts were observed with the oracle. No `aeson` code is ported. | BSD-3-Clause | `crates/randoc-ast/src/json/` | none: no code or text is copied |
| General categories L and N (tables generated from `UnicodeData.txt`) | Unicode Character Database 15.1.0 | Unicode License v3 | `crates/randoc-ast/src/json/categories.rs` | [`third_party/licences/Unicode.txt`](third_party/licences/Unicode.txt) |
| HTML entity table (`ENTITIES`) | `commonmark` 0.3.1 (`src/Commonmark/Entity.hs` of `commonmark-hs` commit e67a048), which comes from TagSoup | BSD-3-Clause | `crates/randoc-core/src/shared/tables.rs` | [`third_party/licences/commonmark-hs.txt`](third_party/licences/commonmark-hs.txt), [`third_party/licences/tagsoup.txt`](third_party/licences/tagsoup.txt) |
| ASCII table of CommonMark identifiers (`COMMONMARK_ASCII`) | `commonmark-extensions` 0.2.7.3 (`AutoIdentifiers.hs` `asciiMap`, `commonmark-hs` commit e67a048) | BSD-3-Clause | `crates/randoc-core/src/shared/tables.rs` | [`third_party/licences/commonmark-hs.txt`](third_party/licences/commonmark-hs.txt) |
| Emoji alias tables (`EMOJI_BY_ALIAS`, `ALIAS_BY_EMOJI`) | `emojis` 0.1.5 (`src/Text/emojis.inc`, commit 880b6a2), data from GitHub `gemoji` | BSD-3-Clause (package), MIT (gemoji data) | `crates/randoc-core/src/shared/tables.rs` | [`third_party/licences/emojis.txt`](third_party/licences/emojis.txt), [`third_party/licences/gemoji.txt`](third_party/licences/gemoji.txt) |
| Character classes and ASCII forms (`CLASSES`, `PANDOC_ASCII`) | Unicode Character Database 15.1.0 (`UnicodeData.txt`) and the canonical decompositions of Unicode 16.0.0 (Python `unicodedata`) | Unicode-3.0 | `crates/randoc-core/src/shared/tables.rs` | [`third_party/licences/Unicode.txt`](third_party/licences/Unicode.txt) |
| NFC tables (`COMBINING_CLASSES`, `DECOMPOSITIONS`, `COMPOSITIONS`, `UNSAFE`) | Unicode Character Database 18.0.0 (`UnicodeData.txt`, `CompositionExclusions.txt`) | Unicode-3.0 | `crates/randoc-core/src/shared/nfc_tables.rs` | [`third_party/licences/Unicode.txt`](third_party/licences/Unicode.txt) |
| Template language: parser, renderer, pipes and tests | `doctemplates` 0.11.1 (https://github.com/jgm/doctemplates) | BSD-3-Clause | `crates/randoc-templates` | [`third_party/licences/doctemplates.txt`](third_party/licences/doctemplates.txt) |
| Error semantics and error text of the parser combinators | `parsec` 3.1.17.0 (https://github.com/haskell/parsec) | BSD-2-Clause | `crates/randoc-templates/src/parsec.rs`, `src/parser.rs` | [`third_party/licences/parsec.txt`](third_party/licences/parsec.txt) |
| Default templates and partials (62 files from `data/templates`; `COPYRIGHT` gives the BSD-3-Clause option; copyright notice in `crates/randoc-data/data/templates/LICENSE`) | Pandoc 3.13 `data/templates` | BSD-3-Clause | `crates/randoc-data/data/templates` | [`third_party/licences/pandoc-templates.txt`](third_party/licences/pandoc-templates.txt) |
| dzslides template (JavaScript and CSS of Paul Rouget) | Pandoc 3.13 `data/dzslides/template.html`; https://github.com/paulrouget/dzslides | WTFPL | `crates/randoc-data/data/dzslides` | [`third_party/licences/dzslides.txt`](third_party/licences/dzslides.txt) |
| CSL processor: types, style parser, locale parser, primary dialect table, locale merge, reference model, CSL JSON markup | `citeproc` 0.14.1.1 (https://hackage.haskell.org/package/citeproc-0.14.1.1) | BSD-2-Clause | `crates/randoc-citeproc/src` | [`third_party/licences/citeproc.txt`](third_party/licences/citeproc.txt) |
| BCP 47 language tag parser and renderer (code only, no data file) | `unicode-collation` 0.1.3.7 `src/Text/Collate/Lang.hs` | BSD-2-Clause | `crates/randoc-citeproc/src/lang.rs` | [`third_party/licences/unicode-collation.txt`](third_party/licences/unicode-collation.txt) |
| XML reader (dependency) | `quick-xml` 0.42.0 | MIT | `crates/randoc-citeproc` | [`third_party/licences/quick-xml.txt`](third_party/licences/quick-xml.txt) |
| General categories L, N and Ll for the CSL JSON markup and name particles (generated by `crates/randoc-citeproc/tools/unicode_classes.py` from Python `unicodedata`) | Unicode Character Database 16.0.0 | Unicode-3.0 | `crates/randoc-citeproc/src/unicode_class.rs` | [`third_party/licences/Unicode.txt`](third_party/licences/Unicode.txt) |
| Candidate command names for the oracle probes (`katex-names.txt`: the names only, 1,013 names). The tables of `randoc-math` hold results of the oracle for these names. No KaTeX code and no KaTeX symbol data is copied. | KaTeX 0.16.11 (`dist/katex.js` of the npm package, SHA-256 `e0cf1ac3508a85a439800821de03c9b77d1bedbdfb47dc70738dc417ea1b5cbe`; https://github.com/KaTeX/KaTeX) | MIT | `crates/randoc-math/tools/katex-names.txt`, command names in `crates/randoc-math/src/symbols.rs` | [`third_party/licences/KaTeX.txt`](third_party/licences/KaTeX.txt) |

Add a row when a workstream adds material from a different project. Put the
full upstream licence file, with its copyright notice and disclaimer, in
`third_party/licences/` and link it in the last column. A licence name alone
is not sufficient for ported code. A crate that holds such material also
carries the notice in its own folder (`crates/NAME/NOTICE.md`), because a
source package of one crate does not include `third_party/`. Check the
licence of each file first. Record a file that the Randoc licence does not
permit as a gap in `spec/not-applicable.json` or `docs/ISSUES.md`.
