# Third-party material

Randoc is an independent implementation of the behaviour of Pandoc. Pandoc is
licensed under GPL-2.0-or-later. Randoc contains no Pandoc source code and no
Pandoc test files.

This file lists each file or design that comes from a different project.

| Material | Source | Licence | Where | Licence text |
| --- | --- | --- | --- | --- |
| Document model (type names and JSON form) | `pandoc-types` 1.23.1.2 | BSD-3-Clause | `crates/randoc-ast` | [`third_party/licences/pandoc-types.txt`](third_party/licences/pandoc-types.txt) |
| Layout engine, width rules and character width table | `doclayout` 0.6 (https://github.com/jgm/doclayout) | BSD-3-Clause | `crates/randoc-layout/src` | [`third_party/licences/doclayout.txt`](third_party/licences/doclayout.txt) |
| First code points of the base emoji (from the Unicode emoji data) | `emojis` 0.1.5, `baseEmojis` (https://github.com/jgm/emojis) | BSD-3-Clause | `crates/randoc-layout/src/tables.rs` | [`third_party/licences/emojis.txt`](third_party/licences/emojis.txt) |
| Layout rules of the native writer (the `valToDoc` layout and the `sep`, `hang` and ribbon rules). Reimplemented from a description and from the output of the oracle. No code is copied. | `pretty-show` 1.10, `pretty` 1.1.3 | MIT, BSD-3-Clause | `crates/randoc-native/src/pretty.rs` | Not applicable: no code and no data is copied |
| Digit generation of `show` for `Double` (`floatToDigits`, ported from `GHC.Internal.Float`), and the `show` forms of `Double` (`formatRealFloat`) and of `Char` and `String` (`showLitChar`) | GHC 9.12.2 (tag `ghc-9.12.2-release`), library `base` 4.21 with `ghc-internal` 9.1202 | BSD-3-Clause | `crates/randoc-ast/src/json/haskell.rs` | [`third_party/licences/GHC-base.txt`](third_party/licences/GHC-base.txt) |
| JSON error texts and error paths | `aeson` 2.2: the texts were observed with the oracle. No `aeson` code is ported. | BSD-3-Clause | `crates/randoc-ast/src/json/` | none: no code or text is copied |
| General categories L and N (tables generated from `UnicodeData.txt`) | Unicode Character Database 15.1.0 | Unicode License v3 | `crates/randoc-ast/src/json/categories.rs` | [`third_party/licences/Unicode.txt`](third_party/licences/Unicode.txt) |

Add a row when a workstream adds material from a different project. Put the
full upstream licence file, with its copyright notice and disclaimer, in
`third_party/licences/` and link it in the last column. A licence name alone
is not sufficient for ported code. A crate that holds such material also
carries the notice in its own folder (`crates/NAME/NOTICE.md`), because a
source package of one crate does not include `third_party/`. Check the
licence of each file first. Record a file that the Randoc licence does not
permit as a gap in `spec/not-applicable.json` or `docs/ISSUES.md`.
