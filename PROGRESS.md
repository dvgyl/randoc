# Randoc progress

This file holds the current state of `main`. Detailed machine-readable numbers
live in [`spec/capabilities.json`](spec/capabilities.json),
[`spec/baseline.json`](spec/baseline.json), and [`spec/status.json`](spec/status.json).
Historical handoffs from sessions 1–6 are archived in
[`to-be-deleted/PROGRESS-sessions-1-to-6.md`](to-be-deleted/PROGRESS-sessions-1-to-6.md).

## Current state of `main`

- **45 Rust crates** in the workspace (Wave 8B adds `randoc-tagsoup`,
  `randoc-embed`, and `randoc-server`). Wave 1 adds Org, Rich Text Format
  (RTF), and four wiki readers. The command line and Lua share the format registry.
- **Core conformance workstreams A–C** improve HTML highlighting layout,
  Markdown parsing and writing, CommonMark source positions, citation
  processing, and short-option clusters. Round 2 improves the LaTeX reader’s
  command, macro, and environment handling. All 7,232 LaTeX reader units pass.
- **Wave 2** adds XML document formats, EPUB, and bibliography formats.
  The command line and Lua use the same registry for these formats. The citation
  filter uses the bibliography readers for BibTeX, BibLaTeX, and RIS.
- **Wave 3** adds Typst and man readers and writers, an mdoc reader,
  an ms writer, and a DOCX writer. The CLI and Lua use the shared registry.
  The math crate renders TeX as Typst, Office Math Markup Language (OMML),
  and eqn with `to_typst`, `to_omml`, and `to_eqn`. The archive crate
  now matches the oracle for the 10 MathML-to-TeX results that differed
  in the EPUB features fixture. See [known issues](docs/ISSUES.md) for the remaining
  long-division fallback and format differences.
- **Wave 4** adds CSV, TSV, Perl Plain Old Documentation (POD), and
  EndNote XML readers. It adds AsciiDoc, Jupyter notebook, and Djot readers
  and writers. The AsciiDoc writer also supports the `asciidoctor` and
  `asciidoc_legacy` variants.
- **Waves 5A and 5C slides** add TikiWiki, TWiki, and Vimwiki readers,
  XWiki and ZimWiki writers, and five HTML slide writers.
- **Wave 6 is complete.** The command line supports all 52 Pandoc 3.13 input
  formats and all 77 output formats, including PDF. The PDF driver supports
  all 15 `--pdf-engine` values. Lua uses the shared registry for all 52 readers
  and 76 writers. See [known issues](docs/ISSUES.md) for remaining differences.
- **Waves 7 and 8A** improve reader and writer parity across text formats and
  office archives. The harness adds binary fixtures and checks ZIP content.
  Template compilation limits nested partial loads. Citation sorting uses the
  Spanish order for `ñ`. The HTML reader handles iframe data and host fetches.
- **Clean-room highlighting, Batches 1–2** add a Rust syntax builder and 21
  built-in clean-room definitions (`JavaScript`, `Haskell`, `Literate Haskell`,
  `Python`, `Elixir`, `CSS`, `HTML`, `Bash`, `YAML`, `R Script`, `Java`, `TOML`,
  `Markdown`, `LaTeX`, `reStructuredText`, `XML`, `Ruby`, `INI Files`, `JSON`,
  `C`, `C++`), plus external keyword lists for Sass and Zsh and Pandoc-compatible
  malformed custom XML diagnostics (`I-HL-08`). All 200/200 `highlight/*`
  harness units and all 230/230 corpus HTML documents now pass.
- **Wave 8B & Wave 9** complete `--embed-resources` / `--self-contained` across
  the 8 HTML/slide writers and HTML-to-PDF backends, add the zero-dependency
  HTTP/1.1 server (`randoc-server`, wired as `randoc server` / `pandoc-server`
  with `+server` in default features), implement all 8 remaining `pandoc.*` Lua
  modules (`structure`, `mediabag`, `log`, `layout`, `zip`, `image`,
  `SimpleTable`, `run_filter`, `references`), add clean-room 107-locale
  `terms.tsv` + 88 Babel/Polyglossia aliases, close 151 Pandoc 3.13 upstream
  command gaps and all 32 DOCX/PPTX reference-doc harness gaps, and harden
  bounded template rendering, cooperative conversion deadlines, worker panic
  recovery, and Org/ZIP/CSL/TagSoup resource limits.
- **Consolidated verification & measurement suites** from the `gpt/build` and
  `M1-*` workstreams under `tools/measure/` and `tools/{casegen,pivot,readers,errors,ast,custom-writers,server,extension-caps}/`, with Codex-specific `ops`/`guard`/`plan` tooling removed.
- **Upgraded `randoc-harness`** with work-folder file diffing, symlink hashing,
  and ZIP archive content/first-entry verification (`capture_files`), plus new
  CLI, file-output, highlight, and 11 reader-baseline conformance suites.

### Implemented formats & subsystems

- **Readers:** `json`, `native`, `commonmark`, `gfm`, `commonmark_x`,
  `markdown` (+ variants), `html`, `csljson`, `docx` (103/105 Pandoc `readers/docx`
  tests, 10/11 corpus `.docx` docs), `odt`, `fodt` (54/54 Pandoc `odt`/`fodt`/`opendocument`
  tests, 8/8 corpus `.odt` docs), `rst` (16/16 Pandoc `rst` tests, 14/16 corpus
  read & write), `latex`, `org`, `rtf`, `mediawiki`, `dokuwiki`, `jira`, `creole`,
  `docbook`, `jats`, `bits`, `fb2`, `epub`, `bibtex`, `biblatex`, `ris`,
  `typst`, `man`, `mdoc`, `csv`, `tsv`, `pod`, `endnotexml`, `asciidoc`,
  `ipynb`, `djot`, `tikiwiki`, `twiki`, `vimwiki`, `textile`, `muse`,
  `t2t`, `haddock`, `opml`, `xml`, `xlsx`, and `pptx`.
- **Writers:** `json`, `native`, `html`, `html4`, `csljson`, `markdown`,
  `gfm`, `commonmark`, `commonmark_x`, `plain` (+ variants, with standalone
  templates), `latex`, `beamer` (fragments + standalone templates, `$highlighting-macros$`, `$pdf-trailer-id$`),
  `opendocument`, `odt`, `fodt` (fragment + standalone `default.opendocument`, `--reference-doc`, MathML),
  `rst` (fragment + standalone `default.rst`, simple/grid/list tables, `+literate_haskell`),
  `org`, `rtf`, `mediawiki`, `dokuwiki`, `jira`, `docbook`, `docbook4`,
  `docbook5`, `jats`, `jats_archiving`, `jats_publishing`, `jats_articleauthoring`,
  `fb2`, `tei`, `epub`, `epub2`, `epub3`, `bibtex`, `biblatex`, `typst`,
  `man`, `ms`, `docx`, `asciidoc`, `asciidoctor`, `asciidoc_legacy`,
  `ipynb`, `djot`, `xwiki`, `zimwiki`, `revealjs`, `slidy`, `dzslides`,
  `slideous`, `s5`, `textile`, `muse`, `t2t`, `haddock`, `opml`,
  `vimdoc`, `icml`, `texinfo`, `context`, `ansi`, `bbcode`,
  `bbcode_fluxbb`, `bbcode_hubzilla`, `bbcode_phpbb`, `bbcode_steam`,
  `bbcode_xenforo`, `xml`, `chunkedhtml`, and `pptx`.
- **CLI & infrastructure:** command-line option parsing, defaults files,
  resource search, `--extract-media`, `--citeproc` / `-C` (full CSL 1.0.2
  engine with DUCET 13.0.0 collation, sorting, disambiguation, collapsing,
  positions, bibliography generation; 63/72 Pandoc `cite` tests pass with
  runtime data dir), templates, JSON filters, Lua 5.4.8 + LPeg 1.1.0 filters,
  Lua custom readers (`-f FILE.lua`), Lua custom writers (`-t FILE.lua`),
  `pandoc lua` / `pandoc-lua` interpreter mode, `pandoc.read`/`pandoc.write`
  in filters, syntax highlighting (`randoc-highlight` wired into `randoc-html`,
  `randoc-latex`, and `randoc-cli`), PDF output through `randoc-pdf`,
  logging, errors, and exit codes.

### Latest recorded baseline metrics

- **Conformance harness (`spec/baseline.json`).** 65,848 of
  66,351 units pass. The update adds 101 passes to the previous
  baseline of 65,747, with zero regressions. The harness reports
  83 failures outside the baseline (all 83 gated: 32 ODT/FODT generator
  label, 21 GPL default templates/stylesheets, 15 `I-CLI-06` GHC backtraces,
  6 `I-JSON-01`, 4 `I-ROFF-01`, 5 `I-CITE-01`) and 420 skipped units.
  65,749 results match byte for byte. All passing results
  match in content.
- **Capability matrix (`spec/capabilities.json`).** The harness ran
  65,931 units. Every applicable capability has a
  recorded test result.

| Capability | Total | Passing | Partial | Failing | Not started | Not applicable |
|---|---:|---:|---:|---:|---:|---:|
| Input formats | 52 | 49 | 3 | 0 | 0 | 0 |
| Output formats | 77 | 73 | 1 | 2 | 0 | 1 |
| Extensions | 90 | 90 | 0 | 0 | 0 | 0 |
| Options | 109 | 94 | 14 | 0 | 0 | 1 |

- **Pandoc 3.13 command suite (`command/`).** 1,774 of 1,884 command cases pass
  (+151 new passes over Wave 8A's 1,623, 0 lost passes) — 100% of all 1,774
  non-CSL, non-gated commands. The remaining 110 failures are 106 `I-CITE-01`
  (CC-BY-SA default CSL style/locales), 1 `9275#2` (CSL locale page terms),
  and 3 `completion.md` (GPL shell completion scripts).
- **Real-document corpus (`spec/corpus.json`).** 690 of 690 units pass (100.0%)
  across all 230 real-world documents: JSON 230/230, HTML 230/230, and
  Markdown 230/230 (0 failures).
- **HTTP server (`randoc server` / `pandoc-server`).** 120 of 120 Pandoc 3.13
  server contract oracle cases pass (0 failures) with zero new third-party
  dependencies.
- **Lua interface (`crates/randoc-lua/MODULES.md`).** 451 of 468 Pandoc 3.13
  API names match (up from 279). All 8 remaining `pandoc.*` modules are
  implemented and pass 534 of 546 new oracle probes (+12 documented known
  differences) alongside 260 existing conversion replay runs.
- **Lua conversion replay** covers 52 readers and 76 writers in 130 filters
  with 2,669 probes. It runs 260 filter and document pairs in each Lua mode.
  The command-line standalone replay covers 76 writers in 608 cases.
- **Wave 3 oracle checks** cover the new formats in 81 Lua replay filters. The CLI replay adds 32 writer cases.
  The mdoc reader matches 36 of 40 harness cases. Pandoc exits with an
  internal parser error in the other four cases. The EPUB oracle tests
  pass with one documented MathML fallback difference.
- **Wave 2 oracle checks** cover the new readers and writers in the Lua
  replay. The harness adds 40 passing BITS cases and 40 passing RIS cases.
  See [known issues](docs/ISSUES.md) for remaining differences.
- **Wave 1 oracle checks:** The wiki suite matches 97 of 100 cases. Both
  MediaWiki figure fixtures match. Org and RTF checks run separately from
  the full harness. See [known issues](docs/ISSUES.md) for details.

## Open priorities

1. **Fix open review findings on existing crates:**
   - `randoc-cli` / `randoc-templates`: partial re-use losing INFO/log records
     (`p_partial`) and remaining round-10 findings.
   - `randoc-html-reader`, `randoc-markdown`, `randoc-commonmark`,
     `randoc-math`, `randoc-yaml`: remaining medium/low findings in `docs/ISSUES.md`.
   - `randoc-markdown-writer`: stack overflows and round-2 high findings fixed (`fd3d4e9`); remaining minor/cross-crate items tracked in `I-MDW-18`/`I-MDW-19`.
2. **Harden & close open gaps on existing crates:**
   - Connect `pandoc.utils.citeproc` (`I-LUA-04`).
3. **Add the `pandoc-server` subsystem.**

## Oracle & licence decisions

- **Pinned oracle:** Pandoc 3.13 (`pandoc-3.13-arm64-macOS.zip`), binary
  SHA-256 `8f17378a70e46eed7b63b5433fff6f5a0802f92659aa5782418548e0ca4a8e44`
  (recorded in `spec/inventory/oracle.json`).
- **Copyleft data files (D1–D4):** GPL/CC-BY-SA Pandoc data files (`default.csl`,
  translations, reference docs, 98 copyleft + 34 unlicensed syntax definitions)
  are excluded from this MIT/Apache-2.0 repo and loaded at runtime from a data
  directory (`--data-dir` / `RANDOC_DATA_DIR`).
- **External test suites (D5):** Pandoc tests and CommonMark spec examples are
  never committed; tools under `tools/` run them externally and record numbers.
- **Output script literals (D7):** Fixed output byte sequences required for
  byte-identical HTML (`--katex` loader, etc.) may live as documented literals
  in source; stored conformance snapshots verify them by digest rather than
  embedding script text.
