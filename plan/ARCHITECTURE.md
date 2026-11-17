# Architecture of the remaining work

This file gives the design decisions for the work that remains. The code that
exists is described in `docs/ARCHITECTURE.md` and `docs/ADR.md`. Where this
file and `docs/ARCHITECTURE.md` disagree about work that remains, this file
applies. The order of the work is in `plan/ROADMAP.md`. The exact definition
of each metric is in `plan/SPEC.md`.

Each part has the same form: **Decision**, **Why**, **Fit** (how the decision
connects to the code that exists), **Order** (the migration order), and
**Do not**.

## 0. Terms

- **Oracle**: the pinned Pandoc 3.13 program. Its behavior is the definition
  of "correct". The oracle is a test tool. Randoc never runs the oracle.
- **Reader**: code that converts input text or bytes to a document tree.
  **Writer**: code that converts a document tree to output text or bytes.
- **Document tree**: the value `randoc_ast::Pandoc` (metadata and a list of
  blocks). A **block** is a paragraph-level element. An **inline** is a
  text-level element.
- **Format**: one name that the oracle prints for `--list-input-formats` (52
  names) or `--list-output-formats` (77 names), for example `docx`.
- **Family**: formats that share most of their code, for example `docbook`,
  `docbook4` and `docbook5`.
- **Text format**: a format whose output is text. **Binary format**: a format
  whose output or input is bytes, for example a ZIP archive.
- **Standalone output**: a complete document with header and footer, made
  with a template (option `-s`). **Fragment**: output without a template.
- **Template**: a text file with variables that gives the frame of standalone
  output. **Partial**: a template that another template includes.
- **Pipeline**: the fixed sequence of stages of one conversion (section 6).
- **Host**: the trait `randoc_core::Host`. All access to files, the network,
  processes, the clock and the environment goes through it.
- **Context**: the value `randoc_core::Context`. It holds the Host, the
  limits, the log and the media bag.
- **Media bag**: the table of binary resources of one conversion (images,
  fonts), `randoc_core::MediaBag`.
- **Data file**: a file that Pandoc ships and reads at run time (templates,
  reference documents, `epub.css`, translations, CSL files, Lua files, syntax
  definitions).
- **Data pack**: a folder outside the repository that holds the data files
  that the licence does not permit in the repository (section 4.7).
- **Scope gap**: a feature that Randoc does not have yet. The program then
  stops with exit code 4 and the text "Randoc does not support ... yet."
  (`crates/randoc-cli/src/subsystem.rs`).
- **Slice**: a small change that goes through all layers and moves one
  metric. Each work item is a slice.
- **Pivot set**: the fixed set of document trees that the measurement gives
  to each writer (`plan/SPEC.md`).
- **Depth**: the number of nested block and inline containers above an
  element.
- **Step budget**: a counter that each parser loop decreases. It stops loops
  that make no progress and it carries the time limit of the server.

## 1. Crates and the dependency rule

**Decision.** Keep the crate graph of `docs/ARCHITECTURE.md`. Add the crates
of the table below. One crate holds one family. One format direction is one
large module file (the drive uses 2 MB for each file, so use few files).

| New crate | Contents | May depend on |
| --- | --- | --- |
| `randoc-xml` | XML tree, XML parser with limits, XML output helpers (4.3) | `randoc-layout`, `quick-xml` |
| `randoc-container` | ZIP archives with limits (4.4) | `randoc-core`, `zip` |
| `randoc-media` | Image size, MIME types, data URIs, hashes, media bag fill and extraction (4.5) | `randoc-core`, `randoc-xml` |
| `randoc-highlight` | Syntax highlighting tokens and styles (section 9) | `randoc-xml` |
| `randoc-typst` | `typst` reader and writer | core set |
| `randoc-docx` | `docx` reader and writer, Word math | core set, xml, container, media, highlight, math |
| `randoc-odt` | `odt` reader and writer, `opendocument`, `fodt` | same |
| `randoc-epub` | `epub`, `epub2`, `epub3` reader and writers, `chunkedhtml` | same, `randoc-html`, `randoc-html-reader` |
| `randoc-pptx` | `pptx` reader and writer, `xlsx` reader | same as docx |
| `randoc-xmlformats` | `docbook*`, `jats*`, `bits`, `tei`, `opml`, `fb2`, `icml`, `xml` | core set, xml, media |
| `randoc-rst`, `randoc-org` | reader and writer of the format | core set |
| `randoc-wiki` | `mediawiki`, `dokuwiki`, `creole`, `tikiwiki`, `twiki`, `vimwiki`, `jira`, `xwiki`, `zimwiki` | core set |
| `randoc-roff` | `man` reader and writer, `mdoc` reader, `ms` writer | core set |
| `randoc-markup` | `asciidoc*`, `djot`, `textile`, `muse`, `t2t`, `haddock`, `pod`, `vimdoc`, `ansi`, `bbcode*`, `rtf`, `context`, `texinfo` | core set, highlight |
| `randoc-dataformats` | `csv`, `tsv`, `ipynb` | core set |

"Core set" means `randoc-ast`, `randoc-core`, `randoc-layout`,
`randoc-templates`, `randoc-math`. The bibliography formats (`bibtex`,
`biblatex`, `ris`, `endnotexml`) go into `randoc-citeproc`, next to `csljson`.
The slide formats (`revealjs`, `s5`, `slidy`, `slideous`, `dzslides`) go into
`randoc-html`.

**Why.** A worker owns one item in one worktree. Families in separate crates
keep the merges of parallel workers free of conflicts and keep build times
short.

**Fit.** The rule "no format crate depends on `randoc` or `randoc-cli`"
stays. A format crate that needs another format at run time (for example
the `epub` reader needs the HTML reader) may depend on that crate when no
cycle results. When a cycle would result, use a function pointer that the
pipeline supplies, as `randoc-lua` does (`crates/randoc-lua/src/filter.rs:61-64`).

**Do not.** Do not make one crate for each format. Do not move code between
crates when no item needs the move.

## 2. The format descriptor

**Decision.** One table holds the static facts of each format. It has two
parts.

1. `randoc_core::formats::FormatInfo`, a `const` table of all 89 format
   names in the new file `crates/randoc-core/src/formats_data.rs`. Fields:
   `name`; `input` (`None`, `Text`, `Binary`); `output` (`None`, `Text`,
   `Binary`); `file_extensions` (for the format guess from a file name);
   `template` (the name of its default template, or none); `html_family`;
   `slide_kind`; `keeps_tabs`; `always_standalone`; `pdf_route` (which PDF
   engines accept the format, section 13). The script
   `scripts/gen_inventory.py` writes the table from `spec/inventory/` and
   from a hand-written fact file `spec/format-facts.toml`. The script
   already writes `extensions_data.rs`, which stays the source of the
   extension sets.
2. `crates/randoc/src/formats.rs` holds the constructors: one line for each
   format, `(name, reader constructor or none, writer constructor or none)`.
   A test checks that each constructor name is in the `FormatInfo` table and
   that the `input` and `output` kinds agree.

Each consumer reads the descriptor. The consumers are
`crates/randoc-cli/src/info.rs` (the two format lists),
`crates/randoc-cli/src/convert.rs` (`EXTENSION_FORMATS`, `is_binary_format`,
`is_html_format`, `keeps_tabs`), `crates/randoc-lua/src/convert.rs:40` (the
names of `pandoc.readers` and `pandoc.writers`) and the template lookup. A
format that is in `FormatInfo` but has no constructor gives the scope-gap
error. A name that is not in `FormatInfo` gives exit 21 or 22.

**Why.** The facts of a format are now in 6 or more files, and no check
compares them. Each of the 100 format directions that remain would touch all
of them.

**Fit.** `FormatInfo` belongs in `randoc-core`, because `randoc-lua` and the
writers need the facts and must not depend on `randoc`.

**Order.** The first item that adds a new format makes the table and moves
the consumers in the same item. No item exists only for this change.

**Do not.** Do not add a field that only one function uses. Do not put a
constructor in `randoc-core`. Do not keep a second list of format names
anywhere, including tests; a test reads the table.

## 3. Changes to the shared contracts

The shared contracts are the public types of `randoc-ast` and `randoc-core`.
Only the lead model changes them, on `gpt/build`. A worker that needs a
change writes the request in its final message; the lead makes the change
once.

### 3.1 Document tree

**Decision.** Keep the types and the derived `Clone`, `PartialEq` and `Drop`.
Add to `randoc-ast`:

- `max_depth(&Pandoc) -> usize`, written with an explicit work list (no
  recursion).
- The module `builder` (4.1).

Safety comes from one invariant: **no tree deeper than
`Limits::max_nesting_depth` exists in memory.** Section 16 gives the rule
and its checks.

**Why.** A manual `Drop` for `Block` or `Inline` forbids moves out of a
`match` and would break most of the 140,000 lines. The invariant gives the
same safety at a small cost.

**Do not.** Do not implement `Drop` for `Block`, `Inline` or `MetaValue`. Do
not add a tree type with shared subtrees.

### 3.2 Reader and writer traits

**Decision.** Keep `TextReader`, `BinaryReader`, `TextWriter`,
`BinaryWriter`, `Reader`, `Writer` and `TemplateWriter`
(`crates/randoc-core/src/format.rs`). Add the depth check of section 16 to
`Writer::write` (the method of the enum, `format.rs:152`). A binary reader
gets all input files only if an oracle probe shows that the oracle reads
more than one; today it gets the first (`convert.rs:1542`).

**Why.** The traits already cover text and binary formats. A custom Lua
reader or writer does not fit them, because a Lua state is not `Send`. It
gets a separate path in the pipeline (section 7.4), not a change of the
traits.

### 3.3 Options, errors, log

**Decision.**

- `ReaderOptions` and `WriterOptions` get a new field only when an option of
  the oracle needs it. Each new field has the default of the oracle.
- `WriterOptions` gets `highlight: Option<randoc_core::HighlightSetup>`. The
  setup holds function pointers, so `randoc-core` does not depend on
  `randoc-highlight` (section 9).
- A subsystem that becomes available deletes its variant of `Subsystem` and
  each check that returns it, in the same item.
- New warnings are new variants of `LogMessage` with the text of the oracle.

### 3.4 Host

**Decision.** Implement the three missing methods of `NativeHost`:
`fetch_url` (crate `ureq`, behind the feature `net`), `random_bytes` and
`processor_time`. Move the direct `std` calls of the program
(`default_user_data_dir`, `write_output`, `read_inputs`) to Host calls when
the pipeline moves (section 6).

**Do not.** Do not call `std::fs`, `std::env`, `std::process`, `std::net` or
`std::time` in a library crate outside `NativeHost`.

## 4. Shared layers that do not exist yet

Each layer is built inside the first item that needs it and is measured
through that item. A layer starts with the functions that the item needs,
not with a full interface.

### 4.1 Inline builder

**Decision.** `randoc_ast::builder` gives the types `Inlines` and `Blocks`.
They join pieces with the normalization that the oracle readers apply:
adjacent `Str` values merge, adjacent `Space` and `SoftBreak` values
collapse, leading and trailing spaces are removed on request. The module
also gives `text(&str) -> Inlines`, which splits text into `Str` and `Space`
items.

**Why.** Four copies exist (`randoc-html-reader/src/text.rs`,
`randoc-latex-reader/src/inlines.rs`, `randoc-commonmark/src/builder.rs`,
`randoc-lua/src/normalize.rs`). About 39 readers remain.

**Order.** Take the copy of `randoc-commonmark` as the start. Each new
reader uses the module. A reader that exists changes only when an item
corrects a defect in its copy.

**Do not.** Do not write a fifth copy. Do not rewrite the four copies in an
item of their own.

### 4.2 Scan cursor and step budget

**Decision.** `randoc_core::shared::scan` gives `Cursor` (a position in the
input with line and column, and no copy of the text) and `StepBudget`. Each
parser loop calls `budget.step()?`. The call fails with an error when the
loop count is above a bound that is linear in the input size, or when the
deadline of the Context has passed (section 14).

**Why.** Loops with no progress and scans inside loops were the most
frequent high findings (latex reader, Markdown reader, HTML reader). One
shared guard replaces a guard for each crate.

**Do not.** Do not use a clock in a parser. The deadline check is inside
`StepBudget`.

### 4.3 XML layer

**Decision.** `randoc-xml` is the only XML code for new work.

- **Tree.** `Document { nodes: Vec<Node> }` with `NodeId(u32)` indexes. A
  node is an element (qualified name, namespace, attributes in source
  order, child ids), text, or a comment. Nodes do not own each other, so
  `Drop` and `Clone` do not recurse.
- **Parser.** `parse(bytes, &XmlLimits) -> Result<Document>` on `quick-xml`
  events (ADR 10). Limits: depth, node count, input size, total size of
  expanded entities. It expands the five predefined entities, character
  references and the entities of the internal subset. It never reads an
  external entity or an external DTD. A caller can supply a table of named
  entities.
- **Output A, layout.** Functions that build `randoc_layout::Doc` values for
  the text XML writers (element with attributes, self-closing element,
  escape of text and attributes).
- **Output B, compact.** A serializer of a `Document` for the parts of ZIP
  formats. Its bytes must equal the oracle's bytes for the same tree.

**Fit.** `crates/randoc-citeproc/src/xml.rs` is private and gives the error
texts that CSL files need. It stays until an item must change it. The syntax
definitions of the highlighter are read with `randoc-xml`.

**Do not.** Do not write a third XML parser. Do not build a tree of boxed
nodes. Do not resolve entities from the network or the file system.

### 4.4 ZIP containers

**Decision.** `randoc-container` wraps the crate `zip` (ADR 9).

- `Archive::read(bytes, &Limits) -> Result<Archive>`. It refuses an archive
  above `max_archive_entries` or `max_archive_size` (the sum of the
  decompressed sizes, counted while it reads, not taken from the header). It
  normalizes each entry path and refuses an absolute path and a path with
  `..`. Duplicate names follow ADR 9.
- `Archive` is a list of entries (name, bytes, time) in a fixed order.
- `Archive::write(&self) -> Result<Vec<u8>>` writes the entries in list
  order. The time of each entry comes from the Context clock. When the
  environment has `SOURCE_DATE_EPOCH`, that time is used, as the oracle
  does (confirm with a probe).

**Why.** Five binary families need the same limits and the same order rules.

**Do not.** Do not extract an archive to the disk. Do not trust a size field
of the archive.

### 4.5 Media layer

**Decision.** `randoc-media` holds:

- `image_size(bytes) -> Option<ImageSize>` (pixels and resolution) for PNG,
  JPEG, GIF, SVG, EPS, PDF, WebP, TIFF, EMF and AVIF, by a parse of the file
  header. The functions that convert between pixels, points, inches and
  percent move here from `crates/randoc-html/src/shared.rs` and
  `crates/randoc-latex/src/shared.rs` when an item needs them in a third
  place.
- `mime_type(path)` and `extension(mime)`, as tables.
- `fetch(source, &mut Context) -> Result<(bytes, mime)>`: a `data:` URI, a
  URL through `Host::fetch_url`, or a file through the resource path.
- `fill_media_bag(&mut Pandoc, &mut Context)` and `extract_media`.
- Base64 and SHA-1 (crates `base64` and `sha1`, already checked in
  `docs/PRIOR_ART.md`).

**Why.** `docx`, `odt`, `epub`, `pptx`, `--extract-media`,
`--embed-resources`, the PDF engines and the Lua modules `pandoc.mediabag`
and `pandoc.image` need the same functions.

**Do not.** Do not decode image pixels. Do not add an image crate.

### 4.6 Writer helpers

**Decision.** `randoc_core::shared` gets one copy of each helper that more
than one writer needs: `make_sections` (now in
`crates/randoc-lua/src/utils.rs:297`), the table of contents as a bullet
list (now in `crates/randoc-html/src/standalone.rs:194` and two other
places), the split of a document into chunks (for `epub`, `chunkedhtml`
and `pandoc.structure`), the conversion of a table to the simple grid form,
note collection, and the translation of terms (`Figure`, `Table`, and so
on) from the translation files of the data pack.

**Order.** The first new writer that needs a helper moves the best copy to
`randoc_core::shared` and uses it. The old copies change only when an item
touches them.

### 4.7 Data files and the data pack

**Decision.** One function finds a data file:
`randoc_core::data::find(name, &mut Context)`. Search order: the user data
folder (`--data-dir`), then the data pack folder, then the files that are
embedded in `randoc-data`. The data pack folder is the value of the
environment variable `RANDOC_DATA_DIR`, else `../share/randoc/data` next to
the program. When no source has the file, the result is the oracle's error
for a missing data file (exit 97).

The tool `tools/autobuild/datapack` makes the data pack for measurements
under `$RANDOC_GPT/datapack`. It asks the oracle for each data file name
(`--print-default-data-file`). The measurement tools pass `RANDOC_DATA_DIR`
to each Randoc run. The data pack is never committed.

**Why.** The reference documents, `epub.css`, the translations,
`default.csl`, the CSL locales and 132 syntax definitions have licences that
do not permit them in this repository (`docs/DATA_FILES_LICENCES.md`). Byte
equality needs them. The owner decides later how users get the data pack.
That decision is an entry in `plan/BLOCKED_ON_OWNER.md`; it does not block
the build.

**Do not.** Do not commit a data file without a permissive licence. Do not
commit a stored result that embeds such a file. A case whose oracle output
embeds one keeps its expected result in the oracle cache outside the
repository (`plan/SPEC.md`, "external expected results").

## 5. How a reader or writer plugs in

The steps of one new format direction, in order:

1. Add the facts to `spec/format-facts.toml` if a fact is missing, and run
   `scripts/gen_inventory.py`.
2. Add the module to the crate of the family (section 1). Export
   `pub fn reader(name: &str) -> Option<randoc_core::Reader>` or
   `pub fn writer(name: &str) -> Option<randoc_core::Writer>`.
3. Add one line to `crates/randoc/src/formats.rs`. For a new crate, add the
   lines to the root `Cargo.toml` and to `crates/randoc/Cargo.toml`.
4. Make the Lua conversion matrix again (`tools/convert-matrix.py`). The test
   `the_convert_matrix_has_each_registered_format` fails until this is done.
   Change each known-difference list in the same commit.
5. Add own-written cases to `conformance/cases/FAMILY/` in few large files.
   Never copy an input from the Pandoc tests or the manual.
6. Add the safety cases of section 16 for the format.

Rules for the code of a reader:

- Build inlines with `randoc_ast::builder`. Scan with `Cursor` and
  `StepBudget`.
- Count depth while reading. Stop with the oracle's behavior if the oracle
  has a limit, else with the depth error of `randoc-core`.
- Read files and URLs only through the Context.
- Use the reader of another format (for example raw TeX inside Markdown)
  through its crate or through a function pointer; do not copy a recognizer.

Rules for the code of a writer:

- Build output as a `randoc_layout::Doc` for a text format. Build a
  `randoc_xml` tree or `Doc` for XML.
- Standalone output: wrap the writer in `TemplateWriter`, build the variables
  with `randoc_core::shared::meta_to_context`, and render
  `WriterOptions::template`. Section 10 gives the variable rules.
- Take a `&Pandoc`. Do not clone the document or a subtree to change it;
  compute the changed view while writing.
- Math: use `randoc-math`. Highlighting: use `WriterOptions::highlight`.

Sources that a builder may use: the Pandoc manual and other public
documents, the output of the oracle for inputs that the builder writes,
and libraries with a licence on the list of `docs/PRIOR_ART.md`. A builder
must not open the Pandoc source tree. The port of a permissive library
follows the procedure of `docs/PRIOR_ART.md` (ADR, notice, licence text).

## 6. The pipeline

**Decision.** The conversion pipeline becomes library code:
`randoc::pipeline::run(&Request, &mut Context) -> Result<Output>`.
`Request` is the record of all options of one conversion; today it is
`Settings` in `crates/randoc-cli/src/settings.rs`. The command line, the
defaults files and the server all fill a `Request`. The program becomes:
parse arguments, fill the `Request`, call the pipeline, write the output.

The stages, in the order of `crates/randoc-cli/src/convert.rs` today: choose
formats; load the template; read abbreviations and metadata files; read the
inputs (files, standard input, URLs); reader; program transforms; filters in
the given order (JSON filter, Lua filter, citeproc); heading shift; media
stages; writer; PDF engine; resource embedding; line endings; log file;
output.

The order of the stages is behavior that a user can see. Place each new
stage by a probe: a Lua filter that prints what it sees, run through the
oracle. Record the probe in the item.

**Why.** The server mode, the library and the WebAssembly build need the
pipeline without `std` I/O. `convert.rs` has 2,576 lines and each new
subsystem adds to it.

**Order.**

1. Each new stage (citeproc, media, embed, PDF) is written from the start
   as a function in `crates/randoc/src/pipeline/` that takes the document,
   its options and the Context. `convert.rs` calls it.
2. The item that builds the server mode moves the remaining orchestration
   and renames `Settings` to `Request`. The harness units of the program
   are the proof that nothing changed.

**Do not.** Do not move `convert.rs` in an item of its own. Do not make a
second options record for the server or for Lua.

## 7. Lua

Lua is the script language of filters, custom readers, custom writers and
the `lua` mode. The crate is `randoc-lua` (Lua 5.4.8 from the unchanged
sources, `mlua`, LPeg; ADR 11, ADR 19).

### 7.1 Filters

**Decision.** The pipeline gives the registry to each filter run: set
`FilterEnvironment::readers` and `writers` to `randoc::formats::reader` and
`writer` in `crates/randoc-cli/src/lua.rs`. This is the first Lua item; it
makes `pandoc.read` and `pandoc.write` work in a filter.

The life of a Lua state (one state for each filter file, or one for the
run) follows the oracle. Decide it with a probe: one script sets a global
variable, the next script reads it.

### 7.2 Object model

**Decision.** Keep the model of `crates/randoc-lua/src/object.rs`: a
userdata value owns a Rust value (the "base") and converts a field on its
first read. Change the walk (`crates/randoc-lua/src/walk.rs`) so that it
**moves** an element into the userdata value and takes it back after the
call. A copy is made only when Lua code keeps a second reference to the
element.

**Why.** Today each call copies the subtree, so the time grows with the
depth (row I-LUA-11 of `docs/ISSUES.md`). Large filter sets, such as the
filters of Quarto, depend on this cost.

**Order.** Only after `lua.filters` has a full measurement, so that a change
of behavior is seen.

### 7.3 The `pandoc` module

**Decision.** Each missing module binds the Rust layer that already holds
the behavior. No behavior is written a second time in Lua glue.

| Lua module | Binds |
| --- | --- |
| `pandoc.layout` | `randoc-layout` (make the `Doc` constructors public) |
| `pandoc.template` | `randoc-templates` and `randoc_core::data::find` |
| `pandoc.mediabag`, `pandoc.image` | `Context::media`, `randoc-media` |
| `pandoc.zip` | `randoc-container` |
| `pandoc.structure` | the helpers of 4.6 |
| `pandoc.utils.citeproc`, `references` | `randoc-citeproc` through a function pointer |
| `pandoc.cli`, `pandoc.log` | the option parser of the program through function pointers; the Context log |
| `pandoc.readers`, `pandoc.writers` | the names of the descriptor (section 2) |
| `pandoc.scaffolding`, `write_classic`, `SimpleTable` | Rust code in `randoc-lua` |

The interface table `crates/randoc-lua/MODULES.md` and the metric `lua.api`
give the list of names that remain.

### 7.4 Custom readers and writers

**Decision.** `-f FILE.lua` and `-t FILE.lua` do not go through the registry.
The pipeline has `FormatChoice::Builtin(name)` and
`FormatChoice::Script(path)`. For a script, the pipeline calls
`randoc_lua::custom_reader` or `custom_writer` with one Lua state that lives
for the whole conversion. The load-time code of the script then runs one
time, as in the oracle. The classic writer form (one global function for
each element) and the globals `Extensions` and `Template` are part of this
path (row I-LUA-29).

**Do not.** Do not remove `Send + Sync` from `Reader` and `Writer`. Do not
start a new Lua state for each call.

### 7.5 The `lua` mode

**Decision.** `crates/randoc-cli/src/lib.rs`, `run_checked`, selects the mode
before the option parse: the program name `pandoc-lua` or the first argument
`lua` gives the Lua interpreter (`randoc_lua::run_script`); the program name
`pandoc-server` or the first argument `server` gives the server (section
14). The options and the prompt of the interpreter follow the oracle.

### 7.6 Errors and the sandbox

**Decision.** An error that ends the oracle process must end Randoc with the
same exit code, not become a Lua error (row I-LUA-15). Lua filters run with
`NativeHost` also under `--sandbox`, as the oracle does (ADR 11). The server
uses `Mode::Restricted` and `RuntimeLimits`.

## 8. Citeproc

Citeproc is the processor that converts citations and a bibliography to
formatted text with a CSL style. The crate is `randoc-citeproc` (a port of
the BSD library `citeproc`, ADR 17).

**Decision.**

- One entry: `randoc_citeproc::process_citations(&mut Pandoc, &CiteprocInputs,
  &mut Context) -> Result<()>`. The pipeline calls it for `--citeproc` and
  for the item `citeproc` in a filter list, at that position of the list.
- `CiteprocInputs` holds the style, the locale, the references and the
  abbreviations. The pipeline loads them through `randoc_core::data::find`
  and the resource path. `default.csl` and the locales come from the data
  pack.
- References come from the metadata field `references` and from the
  bibliography files. The readers `bibtex`, `biblatex`, `ris`, `endnotexml`
  and `csljson` are in `randoc-citeproc`. A field that holds Markdown or
  LaTeX text is parsed by a function pointer that the pipeline supplies
  (`fn(&str, &mut Context) -> Result<Inlines>`), so `randoc-citeproc` does
  not depend on a reader crate.
- Sorting uses the Unicode collation tables that are already ported
  (`THIRD_PARTY_NOTICES.md`). The order of the remaining units: sorting,
  disambiguation, collapse and positions, bibliography, the pipeline stage,
  the bibliography formats.

**Why.** All 107 citation tests of Pandoc's suite and the 129 tests of the
`biblatex` reader fail today. Quarto and R Markdown documents use citations
often.

**Do not.** Do not write a second evaluator. Do not embed CSL styles or
locales (CC BY-SA).

## 9. Highlighting

Highlighting splits the text of a code block into tokens with a kind
(keyword, string, comment) and writes each token with markup.

**Decision.**

- Start from the crate `randoc-highlight` of the branch `ws/highlight`
  (commit `a9c1784`) in the owner's repository
  `/Volumes/Extreme Pro/Randoc/randoc`. Import it as a SNAPSHOT, never with
  its history: the early history of the `ws/*` branches holds files that
  must never be published, and the push guard refuses commits that are
  older than this branch. Read the crate with
  `git -C "/Volumes/Extreme Pro/Randoc/randoc" archive a9c1784 crates/randoc-highlight`
  (or `git show`), find the last commit that compiles, copy the files into
  the clone, run the licence scan, and commit them as one new commit in the
  first highlighting item. Never merge, cherry-pick or push a `ws/*`
  commit. It is a port of the BSD
  library `skylighting-core`. It has its own regular expression engine,
  because the syntax definitions need back references and look-ahead.
- API: `highlight(language, code, &SyntaxSet) -> Result<Vec<Line>>`, where a
  line is a list of `(TokenKind, Text)`; `Style` (colors of each kind);
  renderers for HTML and LaTeX in the crate. Other writers (`docx`, `odt`,
  `typst`, `ansi`, `ms`, `context`) render the tokens in their own crate.
- `randoc_core::HighlightSetup` carries the style and a function pointer to
  `highlight`. The pipeline fills it from `--highlight-style`,
  `--syntax-highlighting`, `--syntax-definition` and `--no-highlight`.
- Syntax definitions: the 63 definitions with a permissive licence are
  embedded. The others load from the folder `syntax/` of the data pack. A
  language with no definition gives the oracle's warning and plain output.
- A definition is parsed on its first use, not at program start.

**Why.** Each writer of the most-used formats writes different bytes for a
code block with a known language. HTML and LaTeX output of real documents
cannot pass without it.

**Do not.** Do not use a regular expression crate that lacks back
references. Do not port code from the GPL package `skylighting` (only
`skylighting-core` is BSD). Do not embed a definition without a licence
grant.

## 10. Templates and standalone output

**Decision.** The template engine `randoc-templates` is complete; keep it.
Each writer with a default template renders it through `TemplateWriter`.
First: `latex` and `beamer` (`crates/randoc-latex/src/lib.rs:31-32`), which
give exit 4 with `-s` today.

The variables of a writer come from three sources, in this order of
precedence: variables of the command line (`-V`), metadata (through
`meta_to_context`), and values that the writer computes (`body`, `toc`,
format-specific values). The set and the form of the computed values are
facts of the oracle: find them with templates that print each variable
name. Correct the open defect of `p_partial` in
`crates/randoc-templates/src/parser.rs` (a partial that is used two times
loses log records).

**Do not.** Do not build standalone output by string concatenation in a
writer. Do not copy a default template into a test result when
`randoc-data` does not hold that template.

## 11. Defaults files

A defaults file is a YAML or JSON file that sets options
(`crates/randoc-cli/src/defaults.rs`).

**Decision.** Keep the code. A defaults file and the command line write the
same `Request` record through the same setter functions; the server options
use the same record (section 14). A new option adds one field, one
command-line action and one defaults key in the same item. The metric
`defaults.units` counts each key.

**Do not.** Do not parse a defaults key in a second place.

## 12. `--embed-resources`

The option makes HTML output that holds each image, script, style sheet and
font inside the file as a `data:` URI.

**Decision.** A pipeline stage after the HTML writer:
`randoc::pipeline::embed(html: Text, &mut Context) -> Result<Text>`. It
reads the output again with the tag tokenizer of `randoc-html-reader`,
fetches each resource with `randoc_media::fetch`, and writes the tags with
changed attributes. Style sheets are scanned for `url()` and `@import`
values. The set of tags and attributes that change, and the form of the
output tags, are facts of the oracle; find them with probes.

**Why.** The oracle works on its own HTML output. The same approach gives
the same bytes for hand-written templates.

**Do not.** Do not embed while the HTML writer runs. Do not fetch in the
restricted mode.

## 13. PDF engines

A PDF engine is an external program that makes a PDF file, for example
`xelatex`, `typst` or `weasyprint`.

**Decision.** PDF is a route of the pipeline, not a writer of the registry.
`randoc::pipeline::pdf(doc, &Request, &mut Context) -> Result<Vec<u8>>`:

1. Choose the engine from `--pdf-engine`, else the default of the
   intermediate format. The field `pdf_route` of the descriptor says which
   engines accept which format (`latex`, `beamer`, `context`, `html`,
   `typst`, `ms`).
2. Run the writer of the intermediate format with standalone output.
3. Make a temporary folder (`Host::create_temporary_directory`), write the
   input and the media bag, and run the engine with `Host::run_process`.
   Arguments, the environment, the number of runs (LaTeX engines run again
   for a table of contents) and the read of the result follow the oracle.
4. Map the failures to the errors `PdfProgramNotFound` (exit 47) and `Pdf`
   (exit 43) and to the log messages of the oracle.

The measurement uses a recording stand-in for each engine (`pdf.units`), so
the arguments and the input files are compared, not the PDF bytes.

**Do not.** Do not link a PDF library. Do not run an engine through a
shell.

## 14. The `server` mode

The server mode answers HTTP requests that ask for a conversion.

**Decision.**

- Crate `tiny_http` (MIT OR Apache-2.0) for HTTP, behind the feature
  `server` of `randoc-cli`. Check the release with the procedure of
  `docs/PRIOR_ART.md` first.
- The paths, the JSON fields of a request, the forms of the answer (plain
  text, JSON, base64 for binary output) and the options `--port` and
  `--timeout` follow the oracle. Find them from the manual of the server
  and from requests to the oracle server.
- One request: decode the JSON to a `Request`; make a `Context` with a
  `PureHost` that holds the files of the request, `Limits::restricted()`,
  no network, no processes, and Lua in `Mode::Restricted`; set the
  deadline of the Context from `--timeout`; call the pipeline on a worker
  thread whose stack size follows section 16.
- The deadline is checked by `StepBudget` (4.2) and by the Lua instruction
  limit. A conversion that passes the deadline stops with the oracle's
  answer for a timeout.
- The program name `pandoc-server.cgi` selects CGI behavior if the oracle
  has it.

**Why.** The server gets untrusted input from the network. The restricted
limits and a pure Host are the protection.

**Do not.** Do not use an asynchronous runtime. Do not let a request read a
file of the machine or the environment.

## 15. Extensions

An extension is a named switch of a reader or writer (`+smart`,
`-raw_html`). `randoc_core::Extensions` is a bit set, and
`extensions_data.rs` holds the default and supported sets of each format.

**Decision.** Keep this design. A reader or writer checks
`options.extensions.contains(...)` at the place of the behavior. An
extension that is valid for a format but changes nothing in the oracle
needs no code. The metric `caps.extensions` counts each pair of format and
extension.

## 16. Untrusted input and stack safety

All input is untrusted: the document, each archive, each fetched resource,
each template, each filter result.

**Decision 1, limits.** `randoc_core::Limits` is the one place for limits:
nesting depth, input size, archive entries and size, resource size, table
cells. Add: XML nodes and entity size (4.3), the fetch limits (response
size, redirect count, schemes `http`, `https` and `data` only), and the
deadline. `Limits::restricted()` is the profile of the server. The program
keeps the oracle's behavior where the oracle has no limit, up to the limits
of `docs/ISSUES.md` row D-002.

**Decision 2, bounded depth.** No tree deeper than
`Limits::max_nesting_depth` exists in memory. Then each recursive function,
and the derived `Drop` and `Clone`, use a bounded stack.

- Each producer of a tree checks the depth while it builds the tree: each
  reader, the JSON reader (it already checks, `randoc-ast` `from_json`),
  the Lua conversion from Lua values (`crates/randoc-lua/src/peek.rs`), and
  each transform that adds levels.
- `Writer::write` calls `randoc_ast::max_depth` first and returns the depth
  error for a deeper tree. This protects a library user who builds a tree
  by hand.
- **Check first.** A function must check the depth before it clones a
  subtree or calls itself. The three open stack overflows of the Markdown
  writer are copies before the check.
- A list that is long but flat (for example 5,000 headings) must be handled
  with a loop, not with recursion along the list.

**Decision 3, stack size.** The stack budget is 64 KiB for each level. The
program runs the conversion on a thread of 1 GiB with the limit 16,384
(`crates/randoc-cli/src/lib.rs:29-42`). The library gives
`randoc::run_with_stack(limits, closure)`, which starts a thread with
`max_nesting_depth x 64 KiB`. With the default limit 128, a conversion must
pass on a 2 MiB stack without that helper.

**Decision 4, loops.** Each parser loop consumes input or stops, under
`StepBudget`. No scan of the rest of the input inside a loop over the input.
A cache needs a test-only plain scan that gives the same answers.

**Decision 5, proof.** The suite `safety.units` holds, for each reader and
writer, generated inputs at depth 10,000 and 100,000, long flat lists, long
lines and repeated openers. Each runs under `limit-run` with a memory and
time limit. A unit passes when the program ends inside the limits with no
crash and gives the oracle's result, or the documented depth error where
the oracle needs more than the limit. A new format adds its safety cases in
the same item.

**Do not.** Do not use `unsafe`, a stack-growing crate or a larger thread to
hide a recursion. Do not use `unwrap()`, `expect()`, `panic!()`, `todo!()`
or an index that input can make invalid in code that input reaches.

## 17. Performance

**Decision.** Compatibility first: no change of output for speed. The
measurements `perf.startup`, `perf.throughput` and `perf.memory` compare
Randoc with the oracle for each benchmark case. Work on performance is an
item only when one of these metrics or `safety.units` shows the cost.

Rules that keep the base fast:

- Each algorithm is linear in the input size, or as the oracle's output
  size requires. A test proves this by a count of steps at two sizes, not
  by wall time.
- No work at program start: tables are `static` data, templates and syntax
  definitions are parsed on first use, the Lua state is made only when a
  script runs.
- A writer takes `&Pandoc` and does not copy the tree. A transform changes
  the tree in place.
- Known costs to correct when a metric shows them: `Prefix::new` in
  `crates/randoc-layout/src/render.rs` (cost grows with the square of the
  depth), the missing fast path for ASCII text width in `randoc-layout`,
  the Lua walk (7.2).
- Release builds are only for benchmark runs (disk space). Keep unwinding
  in the release profile, because a thread panic must give exit 62.

**Do not.** Do not add a cache without the plain-scan test. Do not change
`Text` from `String` unless `perf.memory` shows the need and an ADR records
the decision.

## 18. Library and WebAssembly

**Decision.**

- The crate `randoc` is the one public library. It exports
  `pipeline::run`, `Request`, `formats`, `run_with_stack`, the document
  tree, the Context types, and `randoc_templates`.
- Features of `randoc`, all on by default: `lua`, `net`, `process` (JSON
  filters and PDF engines), `citeproc`, `highlight`, and one feature for
  each large family (`office`, `xmlformats`, `markup`, `wiki`). A build
  with a feature off gives the oracle's error for a missing capability
  (for example exit 89 for Lua), never a build error in a dependent crate.
- A library user supplies the Host. `Context::pure()` works with no system
  access.
- WebAssembly target: `wasm32-wasip1`. The first WebAssembly item is a
  probe: build the library with the features `lua`, `net` and `process`
  off, and run the pivot set through a small command built on
  `pipeline::run`. Lua on WebAssembly is a second probe (the C sources need
  a WASI C compiler); if it fails in one attempt, record the result and
  keep Lua off for that target.
- Measurements: `lib.units` (the library gives the same bytes as the
  program for the pivot set, with a `PureHost`) and `wasm.units` (the same
  set through the WebAssembly build).

**Fit.** `randoc-lua` is already empty on `wasm32`
(`crates/randoc-lua/src/lib.rs:29`). The library crates have no direct
system access outside `NativeHost` (section 3.4).

**Do not.** Do not make a second API for WebAssembly. Do not use threads in
library code; only `run_with_stack` and the program start a thread.

## 19. Migration order (summary)

1. Measurement (milestone M1). No architecture change.
2. Depth check in `Writer::write`, `max_depth`, the safety suite; standalone
   `latex`; highlighting import; registry to Lua filters.
3. Format descriptor with the first new format (`typst`). Media layer,
   `fetch_url`, embed stage, PDF route, slide formats.
4. `randoc-xml`, `randoc-container`, data pack use; `docx`, `odt`, `epub`.
5. Lua modules, the `lua` mode, custom readers and writers, the walk by
   move; citeproc stage and bibliography formats.
6. The remaining markup families, then the remaining XML and data families.
   Each uses the builder, the cursor, the helpers and the descriptor.
7. Pipeline move to the library, server mode, library features,
   WebAssembly, performance.

## 20. Global "do not" list

- Do not open the Pandoc source tree. Do not copy Pandoc test files,
  manual text or copyleft data into the repository, into a test or into a
  stored result.
- Do not write a special case for one test input. Do not edit an expected
  result.
- Do not start a refactor that moves no metric. Do not set a code-size
  target.
- Do not add a dependency whose licence is not on the list of
  `docs/PRIOR_ART.md`, and not without an ADR and a notice.
- Do not let Randoc run the oracle.
- Do not accept a difference in output for speed.
