# Third-party material

Randoc is an independent implementation of the behaviour of Pandoc. Pandoc is
licensed under GPL-2.0-or-later. Randoc contains no Pandoc source code and no
Pandoc test files.

This file lists each file or design that comes from a different project.

| Material | Source | Licence | Where |
| --- | --- | --- | --- |
| Document model (type names and JSON form) | `pandoc-types` 1.23.1.2 | BSD-3-Clause | `crates/randoc-ast` |
| Layout engine, width rules and character width table | `doclayout` 0.6 (https://github.com/jgm/doclayout) | BSD-3-Clause, [licence](third_party/licences/doclayout.txt) | `crates/randoc-layout/src` |
| First code points of the base emoji (from the Unicode emoji data) | `emojis` 0.1.5, `baseEmojis` (https://github.com/jgm/emojis) | BSD-3-Clause, [licence](third_party/licences/emojis.txt) | `crates/randoc-layout/src/tables.rs` |

Add a row when a workstream adds material from a different project. Check the
licence of each file first. Record a file that the Randoc licence does not
permit as a gap in `spec/not-applicable.json` or `docs/ISSUES.md`.
