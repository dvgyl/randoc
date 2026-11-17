# Roadmap

This file is the master plan of the build. It gives the milestones in order,
the exit gate of each milestone, and the first work items of each milestone.
It has no dates and no durations. The machine-readable gates are in
`plan/milestones.toml`. The design is in `plan/ARCHITECTURE.md`. The exact
definition of each metric is in `plan/SPEC.md`.

## The final goal

Randoc is a complete drop-in replacement for Pandoc 3.13
(`docs/ORIGINAL_SPEC.md`, "Definition of done"). The goal is reached when
all of these hold:

- Each metric of the scoreboard is at TOTAL/TOTAL.
- The held-out measurement is at TOTAL/TOTAL.
- Pandoc's own test suite passes against Randoc.
- Quarto and R Markdown give the same results with Randoc in place of
  Pandoc.

This goal is the exit gate of the last milestone. No milestone, item or
decision can lower it. Until it holds, there is always a next item.

## Terms

- **Oracle**: the pinned Pandoc 3.13 program. Its behavior is "correct".
- **Metric**: one number of the scoreboard, written as PASS/TOTAL.
  **Metric key**: its name, for example `suite.pandoc`.
- **Unit**: one comparison of Randoc with the oracle. A unit passes or
  fails.
- **Scoreboard**: the files `plan/scoreboard/*.tsv`. One row is one metric
  of one measurement run.
- **Milestone**: a group of work with one goal and one exit gate.
- **Exit gate**: a list of metric thresholds. `ops milestone check ID`
  checks it and exits 0 only when each threshold holds.
- **Entry condition**: what must hold before work on the milestone starts.
- **Item**: one small piece of work with an id (`M<n>-<nn>`), one metric
  key and one expected gain. The list of items is `plan/ITEMS.tsv`.
- **Experiment**: one measurement run for an item. **Gain**: the increase
  of PASS of the item's metric above its baseline.
- **Cluster**: a group of failing units with one probable cause, at most
  about 40 units. `ops next` prints clusters from the newest measurement.
- **Pivot set**: the fixed set of document trees that the measurement
  gives to each writer.
- **Data pack**: the folder outside the repository with the data files
  that the licence does not permit in the repository
  (`plan/ARCHITECTURE.md`, section 4.7).
- **Finding id** (for example `I-MDW-06`, `cli-R10-03`): the name of a
  known defect. Rows that start with `I-` are in `docs/ISSUES.md`. The
  other ids come from earlier reviews; the title of the item says what the
  defect is.

## Rules of this roadmap

1. **The active milestone** is the milestone with the lowest number whose
   exit gate does not pass. The lead model and the workers take items of
   the active milestone first.
2. **Work ahead.** A free worker may take an item of a later milestone when
   that milestone's entry condition holds and the active milestone has no
   independent item left. Each milestone states its entry condition for
   this purpose.
3. **Each item is a slice.** An item changes all layers that it needs and
   moves its one metric. No item exists only to build a layer, to move
   code or to remove lines. A shared layer is built inside the first item
   that needs it (`plan/ARCHITECTURE.md`, section 4).
4. **Size.** An item must show a gain within 3 experiments. If its first
   experiment shows that it is too large, the lead splits it: the part with
   a gain is closed as done, the rest becomes a new item.
5. **First items and next items.** The tables below give the first items of
   each milestone. They do not give all items. After each full measurement,
   `ops next` prints the largest clusters; the lead adds them as items of
   the active milestone.
6. **Expected gain.** The number in the column "Gain" is the expected
   increase of PASS. It is an estimate from the counts that are known now.
   The lead writes the real baseline when the item starts.
7. **An item that makes a metric.** In milestone M1, the gain of an item is
   the first row of its metric with a TOTAL above 0. Its baseline is 0/0.
8. **Thresholds only rise.** After a milestone passes, the lead may raise a
   threshold of a later milestone in `plan/milestones.toml`. A threshold
   never falls. A TOTAL never shrinks, except through a recorded correction
   of a tool (`plan/REVIEW_LOG.md`).
9. **Safety is never deferred.** From M2 on, each exit gate needs
   `safety.units` and `gate.steps` at TOTAL/TOTAL. An item that adds a
   reader or a writer adds its safety cases.
10. **Licence.** No Pandoc source, test file, manual text or copyleft data
    file enters the repository, a test or a stored result. A case whose
    oracle output embeds a data pack file keeps its expected result outside
    the repository.
11. **Owner decisions.** A blocker that only the owner can remove goes to
    `plan/BLOCKED_ON_OWNER.md`. The work continues with other items. The
    known entries are in the last section of this file.

## Metric keys that this roadmap adds

The gates need keys that the first list of keys does not have. Milestone M1
builds their measurements. `plan/SPEC.md` gets the exact definitions.

| Key | PASS / TOTAL |
| --- | --- |
| `gate.steps` | Steps of the gate and self-checks of the measurement tools that pass inside the sandbox / all steps. The self-checks are: each oracle run has the heap limit; a seeded regression makes each baseline check fail; two runs of one commit under load give the same verdicts. |
| `safety.units` | Stress inputs (deep nesting, long flat lists, long lines, repeated openers, large archives) that end inside the memory and time limit with no crash and with the oracle's result or the documented limit error / all stress inputs. |
| `fmt.write.F` | Trees of the pivot set that the writer F converts with the same result as the oracle / trees of the pivot set. One key for each of the 76 writers other than `pdf`. |
| `fmt.read.F` | Inputs of format F that the reader converts to the same tree as the oracle / inputs. The inputs are: the oracle's output of the pivot set in format F (when F is also a writer), the own-written case inputs of F, and the visible corpus documents of F. One key for each of the 52 readers. |
| `fmt.write`, `fmt.read` | The sums of the rows above. |
| `suite.pandoc.inline` | Tests of Pandoc's suite that exist only as Haskell code: the tool reads each input string at run time and compares Randoc with the oracle / extracted inputs. |
| `suite.pandoc.lua` | Lua test files of Pandoc's Lua engine that give the same result with Randoc and the oracle / test cases. |
| `server.units` | Requests to the server mode with the same answer as the oracle server / requests. |
| `lib.units` | Pivot conversions through the library API with a pure Host that give the same bytes as the program / conversions. |
| `wasm.units` | The same conversions through the WebAssembly build / conversions. |

Two rules make the capability metrics strict. A reader counts in
`caps.readers` only when `fmt.read.F` is at TOTAL/TOTAL and each harness
unit of F passes; the same rule applies to `caps.writers` with
`fmt.write.F`. `pdf` counts in `caps.writers` when `pdf.units` is at
TOTAL/TOTAL.

A reader that is not also a writer has no oracle-written inputs. These
readers are `xlsx`, `bits`, `endnotexml`, `ris`, `creole`, `tikiwiki`,
`twiki`, `vimwiki`, `pod`, `mdoc`, `csv` and `tsv`. The first item of such a
reader adds 40 or more own-written inputs (for `xlsx`: 10 or more documents
with an open licence). Their gates have a minimum PASS for this reason.

---

## M1. Measurement complete and trustworthy

**Goal.** Each metric key has a working measurement. The gate runs inside
the sandbox. A baseline of all metrics exists. After M1, each claim about
compatibility comes from a scoreboard row.

**Scope.** Measurement tools, the harness, the gate, test data that the
team writes, the data pack tool. No feature work on readers, writers or
subsystems, except the smallest change that a measurement needs to run
(for example the `ps` call in a test).

**Entry condition.** The repository builds on `gpt/build`, and
`ops status` runs.

**Exit gate.** Each metric key has a row; `gate.steps` is at TOTAL/TOTAL;
`harness.units` >= 59,513; `suite.pandoc` >= 646; `corpus.units` >= 162
(the numbers of the start state).

**Items.**

| Id | Metric | Gain | Title |
| --- | --- | --- | --- |
| M1-01 | `gate.steps` | first row, all steps pass | Gate in the sandbox: `crates/randoc-citeproc/tests/time.rs` runs `ps` (refused); `init.sh` stops at the hook copy; the 20 s wall limit of a harness unit fails correct code under load (use CPU time of the child); no tool passes `GHCRTS=-M3g` to the oracle (harness `exec.rs`, the suite, corpus and CommonMark tools) |
| M1-02 | `gate.steps` | +2 | Baseline checks: the suite tool writes its baseline before it reads it, so the check cannot fail; the corpus tool has no baseline check |
| M1-03 | `files.units` | first row, TOTAL >= 60 | Harness captures each file that a run writes (`-o`, `--log`, `--extract-media`); 0 units compare a written file today |
| M1-04 | `fmt.write` | first row, 76 keys | Pivot set (each block, inline, attribute and table form) and one row for each writer |
| M1-05 | `fmt.read` | first row, 52 keys | Reader inputs from the oracle's pivot output, the own cases and the corpus; one row for each reader |
| M1-06 | `caps.extensions` | TOTAL = 1,719 | One probe set for each (direction, format, extension) pair, kept only when the oracle output changes with the switch; 11 extension names have 0 units today |
| M1-07 | `caps.options` | TOTAL = 109 | Value sets for each option on 5 base conversions; short options map to long names; 20 options rest on 1 to 3 units, and `embed-resources` and `pdf-engine` are "passing" on parse-only cases |
| M1-08 | `ast.roundtrip` | first row | JSON and `native` round trip of each distinct oracle tree; cases for each wrong `pandoc-api-version` form |
| M1-09 | `templates.units` | first row | 15 variable sets and 2 documents for each format with a default template; user partials |
| M1-10 | `defaults.units` | first row | One number for each defaults key that the oracle accepts |
| M1-11 | `errors.units` | first row | One confirmed trigger for each exit code; `--verbose --log` files of the corpus and pivot conversions |
| M1-12 | `lua.api` | TOTAL = 468 | Call probes for each name of the `pandoc` module through one printer, run as a filter |
| M1-13 | `lua.filters` | first row | Filter matrix: element type, return kind, traversal, filter form, on the pivot set |
| M1-14 | `custom.readers` | first row | 10 own-written custom readers and their inputs |
| M1-15 | `custom.writers` | first row | 10 own-written custom writers (new form, classic form, `Template`, `Extensions`) |
| M1-16 | `suite.pandoc.lua` | first row | Run the 20 Lua test files of Pandoc's Lua engine from the Pandoc tree at run time |
| M1-17 | `suite.pandoc.inline` | first row, TOTAL >= 1,000 | Read the input strings of the Haskell-only test groups at run time (about 1,939 tests have no number today) |
| M1-18 | `citeproc.units` | first row | Styles, bibliography formats, citation matrix and 4 output formats; styles are fetched, never committed |
| M1-19 | `zip.units` | first row | Data pack tool; oracle self-check that lists the archive entries that differ between two oracle runs; expected results outside the repository |
| M1-20 | `pdf.units` | first row | Recording stand-in for each PDF engine; the "engine missing" case for each engine |
| M1-21 | `perf.throughput` | first row | Speed, startup and peak memory against the oracle for each corpus document (`perf.startup`, `perf.memory` in the same run); the stored benchmark record is out of date |
| M1-22 | `safety.units` | first row, TOTAL >= 100 | Generated stress inputs for each reader and writer that exists, under `limit-run` |
| M1-23 | `server.units` | first row | Requests to the oracle server on a local port; Randoc gives 0 now |
| M1-24 | `lib.units` | first row | A small command on the library API with a pure Host (`wasm.units` in the same item: 0 until the build exists) |
| M1-25 | `downstream.quarto` | first row | Sample projects with open licences; record each real call of the oracle; replay it with both programs (`downstream.rmarkdown` in the same form). If a tool cannot be installed, write the row 0/TOTAL and a BLOCKED ON OWNER entry |
| M1-26 | `casegen.visible` | row for seeds 1 to 1000 | Run the visible generated cases and each family key; request the first held-out result |

Order: M1-01 and M1-02 first, then M1-03, M1-04 and M1-05, because later
items use the file capture and the pivot set. The other items are
independent and suit parallel workers. The last step of M1 is one full
measurement on a clean commit and a `milestone` entry in
`plan/REVIEW_LOG.md` with the baseline of each metric.

---

## M2. Core text formats and safety

**Goal.** The formats that exist reach at least 95%: the Markdown variants,
CommonMark and GFM, HTML, LaTeX and Beamer, JSON, `native`, `plain`. The
known safety defects are closed. Standalone LaTeX works. Code blocks are
highlighted.

**Scope.** `randoc-markdown`, `randoc-commonmark`, `randoc-html-reader`,
`randoc-latex-reader`, `randoc-html`, `randoc-latex`,
`randoc-markdown-writer`, `randoc-math`, `randoc-templates`,
`randoc-layout`; the import of `randoc-highlight`; the depth check in
`Writer::write`; the registry for Lua filters.

**Entry condition.** M1 passes.

**Exit gate.** `safety.units` and `gate.steps` at TOTAL/TOTAL;
`ast.roundtrip` at TOTAL/TOTAL; `fmt.read.F` >= 0.95 for the 12 core
readers and >= 0.90 for `latex`; `fmt.write.F` >= 0.95 for the 18 writers
that exist; `harness.units` >= 60,600; `suite.pandoc` >= 850;
`corpus.units` >= 195; `defaults.units` >= 0.95; `errors.units` >= 0.80;
`caps.readers` >= 10; `caps.writers` >= 14.

**First items.**

| Id | Metric | Gain | Title |
| --- | --- | --- | --- |
| M2-01 | `safety.units` | +3 | Markdown writer: 3 stack overflows through the library (a tree copy before the depth check at depth 835 for figure alternative text and 838 for the TOC citation fallback; TOC cleanup with 4,678 headings); add `max_depth` and the check in `Writer::write` |
| M2-02 | `safety.units` | +2 | `meta_to_context` in `randoc-core` copies template variables with no depth check (html-writer-R7b-04; corrected only inside `randoc-html`) |
| M2-03 | `harness.units` | +100 | `-s -t latex` and `-s -t beamer` give exit 4: render the template through `TemplateWriter`; 175 units of suite `templates` fail |
| M2-04 | `templates.units` | +5 | A partial that is used two times loses INFO and log records (`p_partial`, cli-R10-03) |
| M2-05 | `fmt.write.html` | +20 | Import `randoc-highlight`; HTML code blocks with a known language get the `sourceCode` token markup (I-HTML-02) |
| M2-06 | `fmt.write.latex` | +20 | LaTeX code blocks: one `\NormalTok` for each line in place of real tokens (I-LATEX-01); `--syntax-definition` accepts an invalid file (I-CLI-07) |
| M2-07 | `harness.read` | +40 | LaTeX reader: the largest cluster of the 917 failing units of suite `latex-reader`; start with `\textup` that loses its text (I-LTXR-01) |
| M2-08 | `corpus.units` | +6 | LaTeX reader: the oracle stops with exit 64 on 7 corpus documents and for an unclosed argument of an unknown command, Randoc gives a document (I-LTXR-11, I-LTXR-50); `ltnews` and `encguide` differ (I-LTXR-53) |
| M2-09 | `harness.write` | +40 | Markdown writer: the largest cluster of the 206 failing units; start with `--reference-links` and `--reference-location=block` (I-MDW-06) |
| M2-10 | `fmt.write.markdown` | +15 | Markdown writer loses text: caption text, a metadata field, changed attribute values, a figure joined to the next block in `plain` (review round 2, findings R2-03 to R2-16) |
| M2-11 | `harness.read` | +28 | Markdown reader: literal backticks change block boundaries (28 cases, R4-01); a span inside link text stays literal (I-MD-31) |
| M2-12 | `safety.units` | +3 | Markdown reader: a long reference key gives quadratic bracket work (R3-12); nested HTML containers with unclosed fence lines repeat the fence search (I-MD-76) |
| M2-13 | `safety.units` | +2 | `randoc-layout`: `Prefix::new` cost grows with the square of the depth; nested tables in the Markdown writer take more than linear time (R1-02) |
| M2-14 | `fmt.write.html` | +5 | Math: multiline text loses its line feed; HTML-only presentation selectors enter the inline list (math-R4-01, math-R4-02) |
| M2-15 | `lua.filters` | +10 | `pandoc.read` and `pandoc.write` find no format in a filter: give the registry to `FilterEnvironment` in `crates/randoc-cli/src/lua.rs`; make the conversion matrix again |
| M2-16 | `suite.pandoc` | +40 | Tests of Pandoc's suite with `-s` (42 of 231 pass): the largest cluster after M2-03 |
| M2-17 | `corpus.units` | +10 | Markdown documents of the corpus (58 of 84 units pass): the largest cluster |
| M2-18 | `caps.extensions` | +8 | Pairs of `raw_html` (61 failing units), `raw_tex` (131), `implicit_figures` (54), `hard_line_breaks` (20), `tex_math_dollars` (22) |
| M2-19 | `errors.units` | +9 | JSON input errors: 9 units of the JSON reader error path (I-001, I-CLI-12); `serde_json` texts in place of the oracle texts (I-CLI-11, I-CLI-47) |
| M2-20 | `fmt.read.html` | +10 | HTML reader: repeated scans (R4-06, R4-08, R4-09) and the `raw_tex` rows; MathML input |

---

## M3. Typst, media, embedded resources, slide shows, PDF

**Goal.** Typst input and output work. Images are fetched, measured,
extracted and embedded. The HTML slide formats work. `-t pdf` runs each
engine whose intermediate format exists.

**Scope.** The format descriptor (with the first Typst item);
`randoc-typst`; `randoc-media`; `fetch_url`; the pipeline stages for media,
embedding and PDF; the slide writers in `randoc-html`.

**Entry condition.** M1 passes. For the PDF items: M2-03 is done.

**Exit gate.** `safety.units`, `gate.steps` at TOTAL/TOTAL;
`fmt.write.typst` >= 0.95; `fmt.read.typst` >= 0.90; `fmt.write.F` >= 0.95
for `revealjs`, `s5`, `slidy`, `slideous`, `dzslides`; `files.units` >=
0.90; `pdf.units` >= 0.75; `suite.pandoc` >= 930; `corpus.units` >= 205;
`caps.readers` >= 11; `caps.writers` >= 19.

**First items.**

| Id | Metric | Gain | Title |
| --- | --- | --- | --- |
| M3-01 | `fmt.write.typst` | +80 | Format descriptor and Typst writer: paragraphs, inlines, headings, lists, code, standalone template (0 of 80 suite tests pass) |
| M3-02 | `fmt.write.typst` | +60 | Typst writer: tables, figures, notes, citations, TeX math to Typst math |
| M3-03 | `fmt.read.typst` | +60 | Typst reader: markup, functions of the standard library that the oracle reads (0 of 20 suite tests, 0 of 12 corpus units) |
| M3-04 | `files.units` | +20 | Image size and resolution from file headers; media bag fill; `--extract-media` gives exit 4 when the media bag is not empty |
| M3-05 | `files.units` | +10 | `NativeHost::fetch_url`; input from a URL; `--request-header`; fetch limits and warnings |
| M3-06 | `suite.pandoc.command` | +8 | `--embed-resources` for HTML gives exit 4 (0 of 8 suite tests): images, scripts, style sheets with `url()` |
| M3-07 | `fmt.write.revealjs` | +150 | `revealjs` writer: slide split by heading level, slide attributes, incremental lists (0 of 15 suite tests) |
| M3-08 | `fmt.write.slidy` | +150 | `slidy`, `s5`, `slideous`, `dzslides` writers on the slide split of M3-07 |
| M3-09 | `pdf.units` | +30 | PDF route for the LaTeX engines: arguments, repeat runs, temporary folder, missing engine (exit 47), engine error (exit 43) |
| M3-10 | `pdf.units` | +20 | PDF route for the HTML engines and `typst`; `--pdf-engine-opt` |

---

## M4. Office and e-book formats

**Goal.** DOCX, ODT and EPUB work in both directions, with the same archive
contents as the oracle.

**Scope.** `randoc-xml`, `randoc-container`, the data file search with the
data pack, `randoc-docx`, `randoc-odt`, `randoc-epub`, Word math in both
directions, `chunkedhtml`.

**Entry condition.** M1 passes; M3-04 (media) is done.

**Exit gate.** `safety.units`, `gate.steps` at TOTAL/TOTAL; `fmt.read.F` >=
0.90 for `docx`, `odt`, `epub`, `fodt`; `fmt.write.F` >= 0.95 for `docx`,
`odt`, `opendocument`, `fodt`, `epub`, `epub2`, `epub3`, `chunkedhtml`;
`zip.units` >= 0.90; `files.units` >= 0.95; `suite.pandoc` >= 1,100;
`corpus.units` >= 265; `caps.readers` >= 13; `caps.writers` >= 24.

**First items.**

| Id | Metric | Gain | Title |
| --- | --- | --- | --- |
| M4-01 | `fmt.read.docx` | +40 | XML layer, ZIP container and DOCX reader: paragraphs, runs, headings, styles (0 of 116 suite tests, 0 of 33 corpus units) |
| M4-02 | `fmt.read.docx` | +40 | DOCX reader: lists and numbering, tables, links, notes |
| M4-03 | `fmt.read.docx` | +30 | DOCX reader: images to the media bag, track changes (3 suite tests), comments, the extension `styles` |
| M4-04 | `fmt.read.docx` | +20 | Word math to TeX math |
| M4-05 | `zip.units` | +40 | DOCX writer: package parts from the reference document of the data pack; paragraphs, runs, headings, metadata |
| M4-06 | `fmt.write.docx` | +60 | DOCX writer: lists, tables, images, notes, table of contents, highlighted code |
| M4-07 | `fmt.write.docx` | +20 | TeX math to Word math |
| M4-08 | `fmt.write.opendocument` | +150 | `opendocument` writer (0 of 34 suite tests) |
| M4-09 | `fmt.write.odt` | +150 | ODT container from the reference document; `fodt` writer |
| M4-10 | `fmt.read.odt` | +40 | ODT reader (0 of 41 suite tests, 0 of 24 corpus units); `fodt` reader (4 suite tests) |
| M4-11 | `fmt.write.epub3` | +150 | EPUB 3 writer: chunks, navigation, package file, style sheet from the data pack, cover image, fonts |
| M4-12 | `fmt.write.epub2` | +150 | EPUB 2 writer; `chunkedhtml` writer |
| M4-13 | `fmt.read.epub` | +15 | EPUB reader (0 of 3 suite tests, 0 of 21 corpus units) |

---

## M5. Lua and citations

**Goal.** Lua filters, the whole `pandoc` module, custom readers and
writers and the `lua` mode behave as the oracle. `--citeproc` works with
each bibliography format.

**Scope.** `randoc-lua`, the mode selection in `randoc-cli`,
`randoc-citeproc`, the citeproc stage of the pipeline, the readers
`bibtex`, `biblatex`, `ris`, `endnotexml`, the writers `bibtex`,
`biblatex`.

**Entry condition.** M1 passes. For `pandoc.zip`, `pandoc.mediabag` and
`pandoc.image`: M3-04 and M4-01 are done. The citation items need no other
milestone.

**Exit gate.** `safety.units`, `gate.steps` at TOTAL/TOTAL; `lua.api` >=
0.98; `lua.filters` >= 0.98; `custom.readers` >= 0.95; `custom.writers` >=
0.95; `suite.pandoc.lua` >= 0.95; `citeproc.units` >= 0.95; `fmt.read.F`
>= 0.95 for `bibtex`, `biblatex`, `ris`, `endnotexml`, `latex`;
`fmt.write.F` >= 0.95 for `bibtex`, `biblatex`; `caps.options` >= 0.85;
`suite.pandoc` >= 1,320; `corpus.units` >= 275; `caps.readers` >= 18;
`caps.writers` >= 28.

**First items.**

| Id | Metric | Gain | Title |
| --- | --- | --- | --- |
| M5-01 | `citeproc.units` | +40 | Sorting with Unicode collation and year suffixes; disambiguation of names |
| M5-02 | `citeproc.units` | +40 | Collapse, positions across citations (`ibid`, first, subsequent), note styles |
| M5-03 | `suite.pandoc` | +60 | `--citeproc` gives exit 4 (0 of 107 citation tests): the bibliography, the pipeline stage, `citeproc` in a filter list, `nocite`, `link-citations` |
| M5-04 | `fmt.read.biblatex` | +60 | `biblatex` and `bibtex` readers (0 of 129 and 0 of 5 suite tests) |
| M5-05 | `fmt.write.bibtex` | +100 | `bibtex` and `biblatex` writers; `ris` and `endnotexml` readers with 40 own-written inputs each |
| M5-06 | `lua.api` | +39 | `pandoc.layout`: 39 names absent |
| M5-07 | `lua.api` | +16 | `pandoc.mediabag` (10), `pandoc.image` (2), `pandoc.zip` (4) |
| M5-08 | `lua.api` | +30 | `pandoc.template` (5), `pandoc.cli` (3), `pandoc.log` (3), `pandoc.structure` (2), `pandoc.utils` (7), `pandoc.scaffolding`, `pandoc.types.Sources`, 13 top-level names |
| M5-09 | `custom.readers` | +20 | `-f FILE.lua` gives exit 21: script path in the pipeline with one Lua state for the conversion |
| M5-10 | `custom.writers` | +20 | `-t FILE.lua`: new form and classic form; globals `Extensions` and `Template` (I-LUA-29) |
| M5-11 | `suite.pandoc.lua` | +10 | The `lua` mode: `pandoc lua` and the program name `pandoc-lua` |
| M5-12 | `errors.units` | +5 | Errors that end the oracle process are Lua errors in Randoc (I-LUA-15) |
| M5-13 | `perf.throughput` | +3 | Lua walk copies each subtree, time grows with depth (I-LUA-11): move the element into the userdata value |
| M5-14 | `lua.filters` | +59 | 59 known differences of Markdown reader and writer options seen through `pandoc.read` and `pandoc.write` (I-LUA-31) |

---

## M6. Remaining markup and wiki formats

**Goal.** Each light markup, wiki, roff and typesetting format works.

**Scope.** `randoc-rst`, `randoc-org`, `randoc-wiki`, `randoc-roff`,
`randoc-markup`; the PDF routes for `context` and `ms`.

**Entry condition.** M1 passes; the format descriptor exists (M3-01).

**Exit gate.** `safety.units`, `gate.steps` at TOTAL/TOTAL; `fmt.read.F` >=
0.95 for `rst`, `org`, `asciidoc`, `djot`, `textile`, `muse`, `t2t`,
`haddock`, `man`, `rtf`, `mediawiki`, `dokuwiki`, `jira`, `pod`, `mdoc`,
`creole`, `tikiwiki`, `twiki`, `vimwiki` (the last six also with PASS >=
36); `fmt.write.F` >= 0.95 for the 28 writers of the scope; `pdf.units` at
TOTAL/TOTAL; `casegen.visible` >= 0.90; `holdout.generated` >= 0.85;
`holdout.corpus` >= 0.60; `suite.pandoc` >= 1,750; `suite.pandoc.inline` >=
0.60; `corpus.units` >= 500; `caps.readers` >= 34; `caps.writers` >= 52.

**First items.** One reader or one writer is one or more items. The order
follows the number of tests and corpus units.

| Id | Metric | Gain | Title |
| --- | --- | --- | --- |
| M6-01 | `fmt.read.rst` | +60 | RST reader: paragraphs, sections, lists, inline markup (0 of 88 suite tests, 0 of 48 corpus units) |
| M6-02 | `fmt.read.rst` | +40 | RST reader: directives, roles, tables, references |
| M6-03 | `fmt.write.rst` | +150 | RST writer (0 of 48 suite tests) |
| M6-04 | `fmt.read.org` | +60 | Org reader: headings, lists, blocks, inline markup (0 of 59 suite tests, 0 of 27 corpus units, 308 Haskell-only tests) |
| M6-05 | `fmt.write.org` | +150 | Org writer (0 of 29 suite tests) |
| M6-06 | `fmt.read.mediawiki` | +60 | MediaWiki reader (0 of 27 suite tests, 0 of 48 corpus units) |
| M6-07 | `fmt.write.mediawiki` | +150 | MediaWiki writer (0 of 28 suite tests) |
| M6-08 | `fmt.write.asciidoc` | +150 | AsciiDoc writers: `asciidoc`, `asciidoctor`, `asciidoc_legacy` (0 of 35 suite tests) |
| M6-09 | `fmt.read.asciidoc` | +40 | AsciiDoc reader (0 of 24 corpus units) |
| M6-10 | `fmt.read.man` | +40 | `man` reader (0 of 17 suite tests); then `mdoc` (4 suite tests, 103 Haskell-only tests, 18 corpus units) |
| M6-11 | `fmt.write.man` | +150 | `man` writer (0 of 8 suite tests); `ms` writer (5) and the `pdfroff` route |
| M6-12 | `fmt.read.rtf` | +40 | RTF reader (0 of 24 suite tests, 0 of 24 corpus units); then the RTF writer |
| M6-13 | `fmt.write.context` | +150 | ConTeXt writer (0 of 8 suite tests) and the `context` PDF route; Texinfo writer (10) |
| M6-14 | `fmt.read.djot` | +60 | Djot reader and writer (4 and 6 suite tests, 15 corpus units) |
| M6-15 | `fmt.read.textile` | +60 | Textile reader and writer (10 and 4 suite tests, 12 corpus units) |
| M6-16 | `fmt.read.dokuwiki` | +60 | DokuWiki reader and writer (11 and 8 suite tests) |
| M6-17 | `fmt.read.muse` | +60 | Muse reader and writer (4 and 4 suite tests, 240 Haskell-only tests) |
| M6-18 | `fmt.read.jira` | +60 | Jira reader and writer; then `creole`, `tikiwiki`, `twiki`, `vimwiki` readers and `xwiki`, `zimwiki` writers, one item each |
| M6-19 | `fmt.write.bbcode` | +150 | BBCode writers (6 names); then `t2t`, `haddock`, `pod`, `vimdoc`, `ansi`, one item for each direction |

---

## M7. Remaining XML, presentation and data formats

**Goal.** Each format of the inventory has a reader or writer. The
capability matrix has no entry "not started".

**Scope.** `randoc-xmlformats`, `randoc-pptx`, `randoc-dataformats`.

**Entry condition.** M1 passes; `randoc-xml` and `randoc-container` exist
(M4-01).

**Exit gate.** `safety.units`, `gate.steps` at TOTAL/TOTAL; `fmt.read.F` >=
0.95 for `docbook`, `jats`, `opml`, `fb2`, `xml`, `ipynb`, `bits`, `csv`,
`tsv` (the last three also with PASS >= 36); `fmt.read.pptx` >= 0.90;
`fmt.read.xlsx` >= 0.90 with PASS >= 9; `fmt.write.F` >= 0.95 for the 14
writers of the scope; `templates.units` >= 0.95; `caps.extensions` >= 0.90;
`zip.units` >= 0.95; `holdout.corpus` >= 0.80; `suite.pandoc` >= 1,950;
`suite.pandoc.inline` >= 0.85; `corpus.units` >= 590; `caps.readers` >= 44;
`caps.writers` >= 66.

**First items.**

| Id | Metric | Gain | Title |
| --- | --- | --- | --- |
| M7-01 | `fmt.read.docbook` | +60 | DocBook reader (0 of 29 suite tests, 0 of 24 corpus units); named entities from a table that the caller supplies |
| M7-02 | `fmt.write.docbook5` | +150 | DocBook writers: `docbook`, `docbook4`, `docbook5` (0 of 8 suite tests) |
| M7-03 | `fmt.read.jats` | +60 | JATS reader (0 of 25 suite tests, 0 of 12 corpus units); `bits` reader |
| M7-04 | `fmt.write.jats` | +150 | JATS writers, 4 names (0 of 30 suite tests); the extension `element_citations` |
| M7-05 | `fmt.write.pptx` | +150 | PPTX writer from the reference document of the data pack (90 Haskell-only tests); slide layouts, speaker notes |
| M7-06 | `fmt.read.pptx` | +12 | PPTX reader (0 of 15 corpus units); XLSX reader (0 of 12 corpus units) |
| M7-07 | `fmt.read.ipynb` | +20 | Jupyter notebook reader and writer (4 and 2 suite tests, 24 corpus units); `--ipynb-output` |
| M7-08 | `fmt.read.csv` | +40 | CSV and TSV readers with 40 own-written inputs each |
| M7-09 | `fmt.write.icml` | +150 | ICML writer (0 of 13 suite tests) |
| M7-10 | `fmt.write.tei` | +150 | TEI writer; OPML reader and writer (3 and 1 suite tests) |
| M7-11 | `fmt.read.fb2` | +60 | FictionBook reader and writer (6 and 9 suite tests) |
| M7-12 | `fmt.write.xml` | +150 | The `xml` reader and writer (the document tree as XML) |
| M7-13 | `caps.extensions` | +40 | The 11 extension names with no code: `amuse`, `element_citations`, `native_numbering`, `ntb`, `raw_markdown`, `smart_quotes`, `special_strings`, `styles`, `tagging`, `xrefs_name`, `xrefs_number`; `sourcepos` (0 of 16) |

---

## M8. Server mode, library, WebAssembly, performance

**Goal.** The `server` mode works. The library runs a whole conversion with
no system access. The WebAssembly build passes the same conversions. Randoc
is not slower than the oracle and does not use more memory.

**Scope.** The pipeline move to `crates/randoc`; the `server` mode; the
features of the library; the `wasm32-wasip1` build; performance items that
the `perf.*` metrics show.

**Entry condition.** M1 passes; the pipeline stages of M3 and M5 exist.

**Exit gate.** `safety.units`, `gate.steps` at TOTAL/TOTAL; `server.units`
>= 0.98; `lib.units` at TOTAL/TOTAL; `wasm.units` >= 0.95; `perf.startup`
>= 0.95; `perf.throughput` >= 0.90; `perf.memory` >= 0.90; `errors.units`
>= 0.98; `caps.options` >= 0.95; `defaults.units` at TOTAL/TOTAL;
`fmt.read` >= 0.97; `fmt.write` >= 0.97.

**First items.**

| Id | Metric | Gain | Title |
| --- | --- | --- | --- |
| M8-01 | `lib.units` | +100 | The program reads `std::env`, `std::fs` and standard input directly (`default_user_data_dir`, `write_output`, `read_inputs`): move the pipeline to `randoc::pipeline::run` on the Host |
| M8-02 | `server.units` | +30 | `server` mode: mode selection, one conversion request, text and JSON answers, `--port`, `--timeout` |
| M8-03 | `server.units` | +20 | `server` mode: batch requests, the version path, files of the request, binary output, restricted limits, the deadline |
| M8-04 | `wasm.units` | +100 | `wasm32-wasip1` build of the library with `lua`, `net` and `process` off; then the Lua probe |
| M8-05 | `perf.startup` | +10 | Startup cases where Randoc is slower than the oracle: work at program start |
| M8-06 | `perf.throughput` | +10 | The largest cluster of documents where Randoc is slower: text width without an ASCII fast path in `randoc-layout` |
| M8-07 | `perf.memory` | +10 | The largest cluster of documents where the peak memory of Randoc is above the oracle's |
| M8-08 | `caps.options` | +5 | `--version` prints `-server` and no scripting engine line (I-CLI-09, I-CLI-50); `--bash-completion` |

---

## M9. Complete drop-in replacement

**Goal.** The final goal of this file.

**Scope.** Each failing unit that remains, in each suite. Quarto and R
Markdown. The held-out sets.

**Entry condition.** M2 to M8 pass.

**Exit gate.** Each metric key at TOTAL/TOTAL, including `holdout.corpus`,
`holdout.generated`, `suite.pandoc`, `suite.pandoc.inline`,
`suite.pandoc.lua`, `downstream.quarto` and `downstream.rmarkdown`;
`caps.readers` = 52; `caps.writers` = 77; `caps.extensions` = 1,719;
`caps.options` = 109; each `fmt.read.F` and `fmt.write.F` at TOTAL/TOTAL.

**First items.** The lead takes the clusters of `ops next`, largest first,
in this order of suites: `safety.units`, `downstream.*`, `suite.pandoc*`,
`corpus.units`, `fmt.*`, `caps.*`, then the others.

| Id | Metric | Gain | Title |
| --- | --- | --- | --- |
| M9-01 | `downstream.quarto` | +20 | The largest cluster of recorded Quarto calls that differ |
| M9-02 | `downstream.rmarkdown` | +20 | The largest cluster of recorded R Markdown calls that differ |
| M9-03 | `suite.pandoc` | +40 | The largest cluster of failing tests of Pandoc's suite |
| M9-04 | `holdout.generated` | +20 | The area where the held-out fraction is lowest against its visible fraction: add visible cases of that area with new seeds, then correct the clusters |
| M9-05 | `caps.extensions` | +40 | The largest cluster of failing (format, extension) pairs |

A unit that the oracle itself does not pass in two runs is not a unit. The
tool removes it with a recorded correction (`plan/REVIEW_LOG.md`). No other
unit leaves a TOTAL.

---

## Known entries for BLOCKED ON OWNER

These decisions belong to the owner. None of them stops an item.

| Entry | What the team does in the meantime |
| --- | --- |
| How users get the data pack (reference documents, `epub.css`, translations, `default.csl`, CSL locales, Lua data files, 132 syntax definitions) | The measurement uses the data pack that the tool makes from the oracle. The repository holds none of these files |
| The abbreviation list and the DocBook entity list | Own list from oracle tests; the entity table comes from the caller |
| Old commits of `main` that hold GPL text | Nothing. The team never rewrites history |
| Install of Quarto, R and the PDF engines, if the sandbox cannot install them | The rows stay at 0/TOTAL or use the recording stand-ins |
