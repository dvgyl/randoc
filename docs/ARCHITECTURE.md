# Architecture

Randoc converts a document in three steps, as Pandoc does:

1. A **reader** parses the input into a document tree (`randoc_ast::Pandoc`).
2. Filters and transformations change the tree.
3. A **writer** renders the tree in the output format.

## Workspace crates (45)

The workspace members are in [`Cargo.toml`](../Cargo.toml). Each crate link opens its manifest and dependency list.

### Core and library

| Crate | Role |
| --- | --- |
| [`randoc-ast`](../crates/randoc-ast/Cargo.toml) | Document tree, JSON codec, and tree traversal. |
| [`randoc-core`](../crates/randoc-core/Cargo.toml) | Reader and writer interfaces, conversion context, host access, and limits. |
| [`randoc-data`](../crates/randoc-data/Cargo.toml) | Embedded templates, document labels, language aliases, and citation data. |
| [`randoc-layout`](../crates/randoc-layout/Cargo.toml) | Text layout, indentation, and line wrapping. |
| [`randoc`](../crates/randoc/Cargo.toml) | Public library and registry of document formats. |
| [`randoc-templates`](../crates/randoc-templates/Cargo.toml) | Template parser and renderer, with pipes and partials. |

### Format readers and writers

| Crate | Role |
| --- | --- |
| [`randoc-asciidoc`](../crates/randoc-asciidoc/Cargo.toml) | AsciiDoc formats. |
| [`randoc-bibtex`](../crates/randoc-bibtex/Cargo.toml) | BibTeX, BibLaTeX, RIS, and EndNote XML bibliography formats. |
| [`randoc-commonmark`](../crates/randoc-commonmark/Cargo.toml) | Readers for CommonMark, GitHub Flavored Markdown (GFM), and CommonMark with extensions. |
| [`randoc-djot`](../crates/randoc-djot/Cargo.toml) | Djot documents. |
| [`randoc-docx`](../crates/randoc-docx/Cargo.toml) | Microsoft Word documents. |
| [`randoc-epub`](../crates/randoc-epub/Cargo.toml) | EPUB publications. |
| [`randoc-html`](../crates/randoc-html/Cargo.toml) | HTML and HTML slide writers. |
| [`randoc-html-reader`](../crates/randoc-html-reader/Cargo.toml) | HTML reader. |
| [`randoc-ipynb`](../crates/randoc-ipynb/Cargo.toml) | Jupyter notebooks. |
| [`randoc-latex`](../crates/randoc-latex/Cargo.toml) | LaTeX and Beamer writers. |
| [`randoc-latex-reader`](../crates/randoc-latex-reader/Cargo.toml) | LaTeX reader. |
| [`randoc-light`](../crates/randoc-light/Cargo.toml) | Lightweight text and outline formats, including Textile, Muse, and OPML. |
| [`randoc-markdown`](../crates/randoc-markdown/Cargo.toml) | Pandoc Markdown readers. |
| [`randoc-markdown-writer`](../crates/randoc-markdown-writer/Cargo.toml) | Markdown, CommonMark, GFM, plain text, and Markua writers. |
| [`randoc-native`](../crates/randoc-native/Cargo.toml) | Reader and writer for the Haskell text form of the document tree. |
| [`randoc-odt`](../crates/randoc-odt/Cargo.toml) | OpenDocument text formats. |
| [`randoc-office`](../crates/randoc-office/Cargo.toml) | Excel spreadsheet reader and PowerPoint presentation reader and writer. |
| [`randoc-org`](../crates/randoc-org/Cargo.toml) | Emacs Org documents. |
| [`randoc-pod`](../crates/randoc-pod/Cargo.toml) | Perl Plain Old Documentation (POD). |
| [`randoc-roff`](../crates/randoc-roff/Cargo.toml) | Roff formats, including man pages. |
| [`randoc-rst`](../crates/randoc-rst/Cargo.toml) | reStructuredText documents. |
| [`randoc-rtf`](../crates/randoc-rtf/Cargo.toml) | Rich Text Format documents. |
| [`randoc-tabular`](../crates/randoc-tabular/Cargo.toml) | Comma-separated and tab-separated tables. |
| [`randoc-tail-writers`](../crates/randoc-tail-writers/Cargo.toml) | Additional writers, including ConTeXt, Texinfo, and chunked HTML. |
| [`randoc-typst`](../crates/randoc-typst/Cargo.toml) | Typst documents. |
| [`randoc-wiki`](../crates/randoc-wiki/Cargo.toml) | Wiki formats, including MediaWiki, DokuWiki, and Jira. |
| [`randoc-xmlformats`](../crates/randoc-xmlformats/Cargo.toml) | XML formats, including DocBook, JATS, BITS, FictionBook, and TEI. |

### Conversion and service components

| Crate | Role |
| --- | --- |
| [`randoc-archive`](../crates/randoc-archive/Cargo.toml) | Bounded ZIP, XML, media, and office package helpers. |
| [`randoc-citeproc`](../crates/randoc-citeproc/Cargo.toml) | Citation Style Language (CSL) processing and CSL JSON reader and writer. |
| [`randoc-embed`](../crates/randoc-embed/Cargo.toml) | HTML resource embedding. |
| [`randoc-highlight`](../crates/randoc-highlight/Cargo.toml) | Syntax highlighting engine with clean-room Rust definitions and permissively licensed KDE definitions. |
| [`randoc-lua`](../crates/randoc-lua/Cargo.toml) | Lua 5.4 and LPeg, Lua filters, and custom readers and writers. |
| [`randoc-math`](../crates/randoc-math/Cargo.toml) | Math conversion, including TeX math to MathML and document inlines. |
| [`randoc-pdf`](../crates/randoc-pdf/Cargo.toml) | PDF conversion through external engines. |
| [`randoc-server`](../crates/randoc-server/Cargo.toml) | HTTP/1.1 document conversion service. |
| [`randoc-tagsoup`](../crates/randoc-tagsoup/Cargo.toml) | HTML tag tokenizer and renderer. |
| [`randoc-yaml`](../crates/randoc-yaml/Cargo.toml) | YAML parser for metadata and settings. |

### Program and test harness

| Crate | Role |
| --- | --- |
| [`randoc-cli`](../crates/randoc-cli/Cargo.toml) | The `randoc` command-line program. |
| [`randoc-harness`](../crates/randoc-harness/Cargo.toml) | Test tool that runs Randoc and Pandoc and compares their output. |

## Server mode

Use `randoc server` to start the HTTP/1.1 conversion service. `randoc-server`
uses `std::net` for TCP and has separate worker pools for connections and
conversions. `ServerLimits` sets the maximum request and response body
sizes. The server uses deadlines and a restricted conversion context.
See [ADR 26](ADR.md#adr-26-http11-transport-with-no-http-dependency) and
the [security policy](../SECURITY.md).

## Shared contracts

The coordinator owns these items. A workstream must not change them. If a
workstream needs a change, it asks the coordinator, and the coordinator makes
the change one time for all workstreams.

- `randoc-ast`: all types, the JSON form, `walk`.
- `randoc-core`: `Reader`, `Writer`, `TextReader`, `BinaryReader`, `TextWriter`,
  `BinaryWriter`, `Sources`, `ReaderOptions`, `WriterOptions`, `Extension`,
  `Extensions`, `FormatSpec`, `Error`, `LogMessage`, `Context`, `Host`, `Limits`.
- `crates/randoc/src/formats.rs`: the registry.
- The root `Cargo.toml`, `scripts/`, `spec/`, `PROGRESS.md`, `docs/MERGE_LOG.md`.
- `randoc-harness` and the case file format.

## Rules for a reader or a writer

- **Options.** Read the settings from `ReaderOptions` or `WriterOptions`. Check
  an extension with `options.extensions.contains(Extension::Smart)`. Do not
  read global state or environment variables.
- **System access.** Use the `Context` for each file read, network request,
  clock read and random value. Do not call `std::fs`, `std::env`, `std::time`
  or `std::process` in a reader or a writer. This rule makes sandbox mode and
  WebAssembly possible.
- **Warnings.** Report a warning with `context.report(LogMessage::...)`. The
  text of each message must be equal to the Pandoc text. Do not print to
  standard error.
- **Errors.** Return `randoc_core::Error`. The variant decides the exit code.
  The `Display` text must be equal to the Pandoc text.
- **No panics.** Input is not trusted. Code that input can reach has no
  `unwrap()`, `expect()`, `todo!()`, `unimplemented!()` or `panic!()`. Index a
  slice only when the index is proven to be in range. Prefer `get`.
- **Nesting.** Stop at `context.limits.max_nesting_depth` levels and treat
  deeper input as plain text or return `Error::Parse`. Recursion without a
  depth count is a defect.
- **Archives.** Obey `context.limits` for the size and the number of entries.
  Reject an entry path that is absolute or that contains `..`.
- **Size limits.** Refuse an input above `context.limits.max_input_size`
  bytes before you read or open it. Stop when the tables of one document need
  more than `context.limits.max_table_cells` cells and columns that the input
  does not write (a span attribute or a short row can ask for them).
- **Haskell `show`.** Use `randoc_core::shared::show_string` and `show_char`
  (also `randoc_ast::json`) for the text that Pandoc writes with `show`.
- **No `unsafe`.** The workspace forbids `unsafe` code. `randoc-lua` uses
  `deny` to permit the audited Lua C interface in ADRs 11 and 19.
- **Strings.** Use `randoc_ast::Text` for strings in the tree.
- **General logic.** Implement the rule of the format. Do not add a special
  case for one test.

## How a format crate connects

A format crate exports two functions:

```rust
/// Returns the reader for a format name of this crate, for example "markdown" or "gfm".
pub fn reader(format: &str) -> Option<randoc_core::Reader>;
/// Returns the writer for a format name of this crate.
pub fn writer(format: &str) -> Option<randoc_core::Writer>;
```

The coordinator adds the crate to the root `Cargo.toml` and one line to
`crates/randoc/src/formats.rs`.

## System access of the host

Each system access is a method of the trait `Host`: files, directories, the
working directory, the environment, processes, the clock and the network.
Each method has a default body that returns an error of the kind
`Unsupported`, so a host refuses a method that it does not implement.
`NativeHost` uses the real system; it keeps environment changes in a table
of the program and gives them to `run_process`. `PureHost` answers from its
file map and its environment map. The `--sandbox` host of the program reads
only the files that the command line names. ADR 18 lists the methods.

## JSON values

`randoc_ast::json::parse_value` parses one JSON value with a depth limit and
gives the error texts of `aeson`. The result is a `JsonTape`; its root is a
`JsonValue`, and `JsonValue::kind` gives a `JsonKind`. Use it for JSON that
is not a Pandoc document, for example CSL JSON.

## Input preparation

The program decodes text input as UTF-8, removes a byte order mark and removes
each carriage return before it calls a text reader, as Pandoc does. A reader
gets `Sources`: one named text for each input file.

## Output

A text writer returns the text as the Pandoc writer function gives it: with no
final newline for a fragment. The program adds one newline when the output is
not standalone and the text does not end with a newline. Thus an empty
document gives one newline. Standalone output comes from the template and is
written as it is.

## Stack depth

The `randoc` program runs the conversion on a thread with a 1 GiB stack and
sets `max_nesting_depth` to 16384. A library user gets the default limit of 128
levels, which is safe on a thread with a 2 MB stack.

## Licence boundary

Pandoc is GPL-2.0-or-later. Randoc is MIT OR Apache-2.0. Do not read Pandoc
source code to write Randoc code, and do not copy Pandoc test files. Use the
Pandoc manual, the documented interfaces and the output of the oracle.
Libraries with permissive licences (for example `doclayout`, `doctemplates`,
`commonmark-hs`, `pandoc-types`, `citeproc`: BSD) can be studied, used as
dependencies and ported. Follow the reuse rule in `docs/PRIOR_ART.md`:
check the licence first, record the choice in `docs/PRIOR_ART.md` and
`docs/ADR.md`, and keep the attribution in `THIRD_PARTY_NOTICES.md`.
