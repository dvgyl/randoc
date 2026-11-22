# Randoc progress

This file holds the current state of `main`. Detailed machine-readable numbers
live in [`spec/capabilities.json`](spec/capabilities.json),
[`spec/baseline.json`](spec/baseline.json), and [`spec/status.json`](spec/status.json).
Historical handoffs from sessions 1–6 are archived in
[`to-be-deleted/PROGRESS-sessions-1-to-6.md`](to-be-deleted/PROGRESS-sessions-1-to-6.md).

## Current state of `main`

- **25 Rust crates** in the workspace, including `randoc-highlight` (extracted
  from `ws/highlight` after full per-definition licence and shingle audit),
  `randoc-archive` (shared bounded ZIP/OPC/media/XML container layer),
  `randoc-docx` (Pandoc 3.13 DOCX reader), `randoc-odt` (Pandoc 3.13
  `odt`/`fodt`/`opendocument` readers & writers), and `randoc-rst` (Pandoc 3.13
  reStructuredText reader & writer).
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
  read & write), `latex` (partial).
- **Writers:** `json`, `native`, `html`, `html4`, `csljson`, `markdown`,
  `gfm`, `commonmark`, `commonmark_x`, `plain` (+ variants, with standalone
  templates), `latex`, `beamer` (fragments + standalone templates, `$highlighting-macros$`, `$pdf-trailer-id$`),
  `opendocument`, `odt`, `fodt` (fragment + standalone `default.opendocument`, `--reference-doc`, MathML),
  `rst` (fragment + standalone `default.rst`, simple/grid/list tables, `+literate_haskell`).
- **CLI & infrastructure:** command-line option parsing, defaults files,
  resource search, `--extract-media`, `--citeproc` / `-C` (full CSL 1.0.2
  engine with DUCET 13.0.0 collation, sorting, disambiguation, collapsing,
  positions, bibliography generation; 63/72 Pandoc `cite` tests pass with
  runtime data dir), templates, JSON filters, Lua 5.4.8 + LPeg 1.1.0 filters,
  Lua custom readers (`-f FILE.lua`), Lua custom writers (`-t FILE.lua`),
  `pandoc lua` / `pandoc-lua` interpreter mode, `pandoc.read`/`pandoc.write`
  in filters, syntax highlighting (`randoc-highlight` wired into `randoc-html`,
  `randoc-latex`, and `randoc-cli`), logging, errors, and exit codes.

### Latest recorded baseline metrics

- **Conformance harness (`spec/baseline.json`):** 61,653 passing baseline units
  out of 64,170 (+2,085 from `randoc-highlight` integration, +10 from
  `randoc-markdown-writer` round-2 fixes, +14 from `randoc-latex` standalone &
  `randoc-lua` CLI wiring, +2 from `--extract-media`, +4 from `randoc-odt`,
  +25 from `randoc-rst`; 0 regressions).
- **Capability matrix (`spec/capabilities.json`):**
  - Input formats: 4 passing, 9 partial, 11 failing (new reader baselines) of 52
  - Output formats: 7 passing, 11 partial of 77
  - Extensions: 61 passing, 17 partial, 1 failing of 90
  - Options: 93 passing, 15 partial, 1 not started of 109
- **Pandoc 3.13 test suite (`tools/pandoc-testsuite`):** 646 of 2,231 tests pass.
- **Real-document corpus (`tools/corpus`):** 162 of 690 units pass.

## Open priorities

1. **Fix open review findings on existing crates:**
   - `randoc-cli` / `randoc-templates`: partial re-use losing INFO/log records
     (`p_partial`) and remaining round-10 findings.
   - `randoc-html-reader`, `randoc-markdown`, `randoc-commonmark`,
     `randoc-math`, `randoc-yaml`: remaining medium/low findings in `docs/ISSUES.md`.
   - `randoc-markdown-writer`: stack overflows and round-2 high findings fixed (`fd3d4e9`); remaining minor/cross-crate items tracked in `I-MDW-18`/`I-MDW-19`.
2. **Harden & close open gaps on existing crates:**
   - Land verified GPT 6.2 Sol review fixes for `randoc-archive`, `randoc-docx`, `--extract-media`, and Slices 1–3 follow-ups.
   - Add BibTeX / BibLaTeX / RIS bibliography readers for `--citeproc` (`I-CITE-18`) and connect `pandoc.utils.citeproc` (`I-LUA-04`).
3. **Add next format families:**
   - `epub` (`epub`/`epub2`/`epub3`), `typst`, `org`, `docx` writer, `pptx`/`xlsx`, `djot`, `man`/`mdoc`/`ms`, `docbook`/`jats`/XML formats, wiki formats, PDF engine driver, and `pandoc-server`.

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
