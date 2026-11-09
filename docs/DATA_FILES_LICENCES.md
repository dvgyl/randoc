# Licences of the Pandoc data files

This document examines the data files that Pandoc 3.13 uses for its output.
For each file or group it gives the licence, whether Randoc can include it,
the necessary attribution and the size. For each file that Randoc cannot
include, it gives the gap and a legal way to close it.

Randoc uses the licence `MIT OR Apache-2.0`.

This is an engineering analysis, not legal advice. The owner must approve the
decisions in the section "Decisions for the owner" before a workstream adds
data files to the repository.

## Summary

1. Pandoc's own licence rule is in `COPYRIGHT`: all material is
   GPL-2.0-or-later "with the exceptions noted below". A file in `data/`
   without an exception in `COPYRIGHT` and without its own licence statement is
   GPL-2.0-or-later. `pandoc.cabal` line 5 also says `license: GPL-2.0-or-later`.
2. **Randoc can include the templates.** `COPYRIGHT` says that the files in
   `data/templates` are dual-licensed as GPL-2.0-or-later or BSD-3-Clause. Randoc
   uses the BSD-3-Clause option and keeps the notice. These 62 files (105 556
   bytes) give byte-identical standalone output for the text formats.
3. **Randoc can include the dzslides template** (WTFPL, `COPYRIGHT`) and 65 of
   the 195 syntax definitions (MIT, BSD, WTFPL or LLVM licence in the `license=`
   attribute of each file).
4. **Randoc can use, with attribution, the BSD libraries that hold data**:
   the emoji table (`emojis`, BSD-3-Clause, data from `gemoji`, MIT), the
   highlight styles (`skylighting-core`, BSD-3-Clause) and the HTML entity
   table (`commonmark`, BSD-3-Clause, or the WHATWG list).
5. **Randoc cannot include these files under its licence**: the reference
   documents (`data/docx`, `data/odt`, `data/pptx`), the translations, `epub.css`,
   `creole.lua`, `init.lua`, `abbreviations`, `docbook-entities.txt`, the
   biblatex localization strings, the bash completion script and 130 syntax
   definitions (LGPL, GPL, FDL or no licence). `default.csl` is CC BY-SA 3.0.
6. **To generate a file from the oracle does not change its licence.** The
   output of `pandoc --print-default-data-file reference.docx` contains the
   same GPL material as the source file. "Generate at build time" is legal only
   when the build runs on the machine of the user and the result is not
   distributed under the Randoc licence.
7. **The recommended way to close the gaps** is one optional data package with
   its own licence (GPL-2.0-or-later, with LGPL and CC BY-SA files kept under
   their own licences), separate from the `MIT OR Apache-2.0` crates. Randoc
   loads the package at run time from a data directory, in the same way as
   Pandoc's `--data-dir`. Without the package, Randoc uses new files that the
   project writes, and records each output difference in `docs/ISSUES.md`.
8. **TeX math has a licence gap that is not in `data/`.** Pandoc converts TeX
   math with `texmath`, which is GPL-2 (`texmath.cabal`: `License: GPL-2`).
   Randoc must write its own conversion and test it against the oracle as a
   black box.

## Method

- Sources: `$RANDOC_PANDOC_SRC/COPYRIGHT`, `$RANDOC_PANDOC_SRC/pandoc.cabal`
  (the `data-files` list, lines 53 to 210), the first lines of each data file,
  and the licence files of the libraries named below.
- Sizes: the byte count of each file (`stat -f %z`), not the disk use. The
  drive uses exFAT, so `du` shows 2 MB or more for each file.
- The oracle `pandoc 3.13` gave the observable facts (194 highlight languages,
  8 highlight styles, the bash completion script).
- The Haskell source files in `src/` were not opened. The location of the
  bash completion script comes from the module list in `pandoc.cabal`
  (`Text.Pandoc.App.Completion`, line 739).

## The licence rule of Pandoc

`$RANDOC_PANDOC_SRC/COPYRIGHT` has these statements that apply to `data/`:

| Statement | Applies to |
| --- | --- |
| "With the exceptions noted below, this code is released under the GPL, version 2 or later" | Each file without an exception |
| "Pandoc's templates (in `data/templates`) are dual-licensed as either GPL (v2 or higher, same as pandoc) or (at your option) the BSD 3-clause license. Copyright (c) 2014--2024, John MacFarlane" | `data/templates/*` |
| "The dzslides template contains JavaScript and CSS from Paul Rouget's dzslides template ... Released under the Do What the Fuck You Want To Public License." | `data/dzslides/template.html` |
| BSD-3-Clause for the `pandoc-types` modules | Not a data file. Randoc already records it in `THIRD_PARTY.md`. |
| MIT licence for Lua (via hslua) | Not a data file. Relevant to Lua embedding. |

`COPYRIGHT` gives no exception for the reference documents, the translations,
`epub.css`, the Lua files, `abbreviations`, `docbook-entities.txt`,
`default.csl` or the biblatex localization strings. `default.csl` has its own
licence statement (see below).

## Each file or group in `data/`

Size is the sum of the bytes of the files in the group.

| File or group | Files | Bytes | Licence | Proof | Randoc can include? | Attribution |
| --- | ---: | ---: | --- | --- | --- | --- |
| `templates/*` (all `default.*`, `styles.html`, `styles.citations.html`, the LaTeX parts, `affiliations.jats`, `article.jats_publishing`, `template.typst`) | 62 | 105 556 | GPL-2.0-or-later or BSD-3-Clause | `COPYRIGHT`, templates paragraph | **Yes**, under BSD-3-Clause | Keep "Copyright (c) 2014--2024, John MacFarlane" and the BSD-3-Clause text in `THIRD_PARTY.md` and in the distribution. Do not use the name of the author to promote Randoc. |
| `dzslides/template.html` | 1 | 18 800 | WTFPL (JavaScript and CSS of Paul Rouget) | `COPYRIGHT`, dzslides paragraph | **Yes** | None necessary. Give credit to Paul Rouget in `THIRD_PARTY.md`. The rest of the file is a sample page; record it as part of the same WTFPL file. |
| `docx/*` (unpacked `reference.docx`: `styles.xml`, `numbering.xml`, `theme1.xml`, `settings.xml` and others) | 16 | 55 021 | GPL-2.0-or-later | No exception in `COPYRIGHT`; no notice in the files | **No** | Not applicable |
| `odt/*` (unpacked `reference.odt`) | 6 | 69 104 | GPL-2.0-or-later | Same | **No** | Not applicable |
| `pptx/*` (unpacked `reference.pptx`) | 49 | 108 140 | GPL-2.0-or-later | Same | **No** | Not applicable |
| `translations/*.yaml` | 106 | 46 832 | GPL-2.0-or-later | No exception in `COPYRIGHT`; no comment lines in the files (`grep -l '^#'` finds none) | **No** | Not applicable |
| `epub.css` | 1 | 4 782 | GPL-2.0-or-later | No exception in `COPYRIGHT` | **No** | Not applicable |
| `creole.lua` (sample custom reader) | 1 | 5 395 | GPL-2.0-or-later | No exception in `COPYRIGHT`; no notice in the file | **No** | Not applicable |
| `init.lua` (3 comment lines) | 1 | 182 | GPL-2.0-or-later | No exception in `COPYRIGHT` | **No** (the file has no code, but the text is still Pandoc text) | Not applicable |
| `abbreviations` (80 English abbreviations) | 1 | 385 | GPL-2.0-or-later | No exception in `COPYRIGHT` | **No** (see decision D3) | Not applicable |
| `docbook-entities.txt` (2 237 lines: entity name and code point) | 1 | 28 876 | GPL-2.0-or-later | No exception in `COPYRIGHT` | **No** as a copy. The facts come from public entity sets. | Not applicable |
| `default.csl` (Chicago author-date) | 1 | 152 596 | CC BY-SA 3.0 | Line 33 of the file: `<rights license="http://creativecommons.org/licenses/by-sa/3.0/">` | **Only as a separately licensed file** (see decision D2) | Keep the `<info>` block (authors, `<rights>`, link to the Zotero style repository). |
| `citeproc/biblatex-localization/*.lbx.strings` (outside `data/`, listed in `data-files`) | 48 | 106 962 | GPL-2.0-or-later | No exception in `COPYRIGHT`; no licence file in the directory | **No** | Not applicable |

### Data that is not in `data/`

Byte-identical output also needs data that Pandoc gets from libraries or from
its source code.

| Data | Where Pandoc has it | Licence | Proof | Randoc can include? | Attribution |
| --- | --- | --- | --- | --- | --- |
| Syntax definitions (KDE XML), 194 languages in `pandoc --list-highlight-languages` | `skylighting-core` 0.15 (`pandoc.cabal` line 577), directory `xml/` | Code: BSD-3-Clause. Each XML file: its own `license=` attribute | `skylighting-core/LICENSE`, `skylighting-core.cabal` (`license: BSD3`), the `license=` attribute in each XML file | **65 of 195 files: yes. 130: no** (table below) | Keep the comment header and the `license=` attribute of each file. List the authors in `THIRD_PARTY.md`. |
| Highlight styles (`pygments`, `tango`, `espresso`, `zenburn`, `kate`, `monochrome`, `breezedark`, `haddock`) | `skylighting-core` source code | BSD-3-Clause | `skylighting-core/LICENSE` | **Yes** | Copyright John MacFarlane, BSD-3-Clause text |
| Emoji names (`emoji` extension) | Haskell package `emojis` 0.1.x (`pandoc.cabal` line 558) | BSD-3-Clause; the data comes from `gemoji` (MIT) and Unicode emoji files (Unicode licence) | `emojis/LICENSE`, `emojis.cabal` (`license: BSD3`), `emojis/README.md`; `github/gemoji/LICENSE` (MIT) | **Yes** | BSD-3-Clause notice of `emojis`, MIT notice of `gemoji`, Unicode licence notice |
| HTML named entities | `commonmark` 0.3 (`pandoc.cabal` line 548) and `tagsoup` | BSD-3-Clause (`commonmark/LICENSE`, `commonmark.cabal`: `license: BSD-3-Clause`) | The licence files named | **Yes**. The WHATWG `entities.json` list is an alternative source. | BSD-3-Clause notice |
| Bash completion script (`pandoc --completion=bash`, 12 569 bytes) | Pandoc source module `Text.Pandoc.App.Completion` (`pandoc.cabal` line 739) | GPL-2.0-or-later | `COPYRIGHT` default rule | **No** | Not applicable |
| TeX math to MathML, OMML and others | `texmath` 0.13 (`pandoc.cabal` line 582) | GPL-2 | `texmath/LICENSE` and `texmath.cabal` (`License: GPL-2`) at <https://github.com/jgm/texmath> | **No** (not a data file, but the same gap) | Not applicable |
| CSL processing | `citeproc` 0.14 (`pandoc.cabal` line 547) | BSD-2-Clause | `citeproc/LICENSE` at <https://github.com/jgm/citeproc> | **Yes** (code port) | BSD-2-Clause notice |
| Layout and templates engines | `doclayout` 0.6, `doctemplates` 0.11 | BSD-3-Clause | `LICENSE` of each repository at <https://github.com/jgm> | **Yes** (code port) | BSD-3-Clause notice |
| Typst reader | `typst` (typst-hs) | BSD-3-Clause | `typst-hs/LICENSE` | **Yes** (code port) | BSD-3-Clause notice |

### The syntax definitions in detail

The count comes from `skylighting-core/xml/*.xml` at commit
`9a414fca3ac94dee81fb76e0ef4f88c9e6037618` (version 0.15, the version range of
Pandoc 3.13). The directory has 195 XML files. It also has 5 patch files
(`haskell`, `isocpp`, `latex`, `php`, `raku`): skylighting changes these 5
definitions from the KDE versions. The oracle lists 194 languages. The
command was:

```sh
for f in xml/*.xml; do grep -o -m1 'license="[^"]*"' "$f" || echo 'license=(none)'; done | sort | uniq -c
```

| `license=` value | Files | Randoc can include? |
| --- | ---: | --- |
| `MIT` | 59 | Yes |
| `BSD`, `BSD3`, `New BSD License` | 3 | Yes |
| `WTFPL` | 1 | Yes |
| `LLVM Release License` | 1 | Yes |
| `GPL,BSD` (dual) | 1 | Yes, under BSD |
| `LGPL`, `LGPLv2`, `LGPLv2+`, `LGPLv2.1+`, KDE e.V. LGPL text | 77 | No (LGPL) |
| `GPL`, `GPLv2`, `GPLv2+`, `GPLv3+` | 18 | No |
| `FDL` | 2 | No |
| no attribute or an empty attribute | 33 | No (no licence means no permission) |
| **Total** | **195** | **65 yes, 130 no** |

The carta project made the same split for `skylighting-core` 0.14.7 (164
files: 48 permissive, 116 other). Its record is
`carta/vendor/syntax-highlighting/ATTRIBUTION.md`. Carta compiles the
permissive files into the program and loads the other files at run time or
with a feature flag. Its count of permissive files is lower because it is an
older version.

## The gaps and how to close them

The list below gives the gap for each file that Randoc cannot include. "Data
package" means the optional package of decision D1.

| File or group | Gap without the file | Legal ways to close the gap |
| --- | --- | --- |
| Reference documents `docx/*`, `odt/*`, `pptx/*` | DOCX, ODT and PPTX output cannot be content-identical, because `styles.xml`, `numbering.xml`, the theme and the slide layouts go into each output file. `--print-default-data-file reference.docx` cannot be equal. | (a) Data package. (b) At run time, read the files from a Pandoc data directory that the user names (`--data-dir`) or from `pandoc --print-default-data-file reference.docx` on the machine of the user; do not distribute the result. (c) Write new reference documents (new styles with the same style names and IDs, because the writers refer to them) and accept the content difference. The structural parts (`[Content_Types].xml`, `_rels/*.rels`) follow the OOXML and ODF specifications and are needed in any case; write them from the specifications. |
| `translations/*.yaml` | Localized terms ("Figure", "Table", "Contents", "Abstract") in LaTeX, HTML, DOCX and other output for `lang` other than the default are different. | (a) Data package. (b) Load from a Pandoc data directory at run time. (c) Write new tables from sources with compatible licences: Unicode CLDR (Unicode licence) has some of the terms; volunteers write the others. Expect differences from Pandoc for each term that is not equal. |
| `epub.css` | The default EPUB style sheet is different. EPUB output is not content-identical. | (a) Data package. (b) Load at run time. (c) Write a new style sheet. |
| `creole.lua` | `--print-default-data-file creole.lua` is different. No converter uses it. | Write a new sample reader or print an error. Low priority. |
| `init.lua` | `--print-default-data-file init.lua` is different. The Lua behaviour is the same, because the file has only comments. | Write a new comment with the same function. |
| `abbreviations` | The `smart` extension keeps a non-breaking space after these abbreviations in Markdown. A different list changes output. `--print-default-data-file abbreviations` is different. | (a) Data package. (b) Load at run time. (c) See decision D3: the list is 80 common abbreviations in alphabetical order. The owner can decide that such a list is a set of facts and write a list from the behaviour of the oracle. |
| `docbook-entities.txt` | The DocBook reader resolves named entities (for example `&aacgr;`). A different table changes the reader output. | Build the table from the public entity sets: the W3C "XML Entity Definitions for Characters" (W3C document licence, permissive with notice) and the ISO 8879 entity sets that DocBook ships. Compare each name and code point with the behaviour of the oracle in conformance cases. |
| `default.csl` | Citations without `--csl` use a different style. Output with `--citeproc` is different. | (a) Include the file as a separately licensed CC BY-SA 3.0 file (decision D2). (b) Download the same version from the Zotero style repository at build time. (c) Load at run time. |
| `biblatex-localization/*.lbx.strings` | The BibLaTeX reader gives different localized terms. | (a) Data package. (b) The strings use biblatex macro syntax (for example `ed\adddot`), so they derive from the biblatex `.lbx` files (LPPL-1.3c). A build step can extract the same strings from the biblatex distribution that the user has; distribute the result under the LPPL, not under the Randoc licence. |
| Bash completion script | `pandoc --completion=bash` output is different. | Write a new completion script. The list of options and formats in it comes from Randoc's own tables. Accept a byte difference and record it in `docs/ISSUES.md`. |
| 130 syntax definitions | Code blocks in these 130 languages have no highlighting, or different highlighting. | (a) Data package with the LGPL, GPL and FDL files under their own licences (LGPL permits use from a program with a different licence when the files stay replaceable). (b) Load at run time from a KDE syntax-highlighting installation (`/usr/share/org.kde.syntax-highlighting/syntax/`) or from a skylighting checkout; the KDE versions can differ from the skylighting copies. (c) Ask the KDE authors to relicense to MIT. KDE asks for MIT for new files (carta's record says the same). |
| TeX math (`texmath`) | MathML, OMML and other math output need a new implementation. | Write a new converter from the manual and from black-box tests against the oracle. MIT crates (`math-core`, `pulldown-latex`) can give a parser, but their MathML is not the same as texmath output. |

## Decisions for the owner

- **D1. One optional data package.** Recommendation: make a separate package
  (for example `randoc-data-pandoc`) that holds the files of the "No" rows
  under their own licences, outside the `MIT OR Apache-2.0` crates. Randoc
  finds it at run time (a data directory, as Pandoc's `--data-dir` and
  `~/.local/share/pandoc`). This keeps the crates permissive and gives
  byte-identical output to users who install the package. A binary that
  embeds the package is a GPL-2.0-or-later program as a whole; do not make such
  a binary the default distribution.
- **D2. `default.csl` under CC BY-SA 3.0.** The standing orders list the
  licences of dependencies (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode). CC BY-SA
  is not in that list. Recommendation: put the file in the data package of D1,
  not in the main repository.
- **D3. Short lists of facts.** `abbreviations` (80 entries) and the name
  and code point pairs in `docbook-entities.txt` are lists of facts. A legal
  review can decide that Randoc can write the same facts from independent
  sources. Until that review, treat them as GPL material.

## Attribution text for `THIRD_PARTY.md`

When a workstream adds one of the "Yes" files, it adds a row to
`THIRD_PARTY.md` with this content:

| Material | Source | Licence | Notice to keep |
| --- | --- | --- | --- |
| Default templates | Pandoc 3.13 `data/templates` | BSD-3-Clause (option of the dual licence in `COPYRIGHT`) | "Copyright (c) 2014--2024, John MacFarlane" and the BSD-3-Clause text |
| dzslides template | Pandoc 3.13 `data/dzslides/template.html`, from Paul Rouget's dzslides | WTFPL | Credit to Paul Rouget |
| 65 syntax definitions | `skylighting-core` 0.15 `xml/` | MIT, BSD, WTFPL, LLVM (per file) | The header comment and the `license=` attribute of each file |
| Highlight styles | `skylighting-core` 0.15 | BSD-3-Clause | "Copyright (c) 2016-2018, John MacFarlane" and the BSD-3-Clause text |
| Emoji table | `emojis` 0.1.5, `gemoji` | BSD-3-Clause, MIT, Unicode-3.0 | The three notices |
| HTML entity table | `commonmark` 0.3 or WHATWG | BSD-3-Clause | The notice |
