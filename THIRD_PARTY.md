# Third-party material

Randoc is an independent implementation of the behaviour of Pandoc. Pandoc is
licensed under GPL-2.0-or-later. Randoc contains no Pandoc source code and no
Pandoc test files.

This file lists each file or design that comes from a different project.

| Material | Source | Licence | Where | Licence text |
| --- | --- | --- | --- | --- |
| Document model (type names and JSON form) | `pandoc-types` 1.23.1.2 | BSD-3-Clause | `crates/randoc-ast` | [`third_party/licences/pandoc-types.txt`](third_party/licences/pandoc-types.txt) |

Add a row when a workstream adds material from a different project. Put the
full upstream licence file, with its copyright notice and disclaimer, in
`third_party/licences/` and link it in the last column. A licence name alone
is not sufficient for ported code. Check the
licence of each file first. Record a file that the Randoc licence does not
permit as a gap in `spec/not-applicable.json` or `docs/ISSUES.md`.
