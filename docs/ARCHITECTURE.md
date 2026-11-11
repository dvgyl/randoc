# Architecture

Randoc converts a document in three steps, as Pandoc does:

1. A **reader** parses the input into a document tree (`randoc_ast::Pandoc`).
2. Filters and transformations change the tree.
3. A **writer** renders the tree in the output format.

## Crates

| Crate | Purpose | Depends on |
| --- | --- | --- |
| `randoc-ast` | The document tree, its Pandoc-compatible JSON form, and tree traversal | none |
| `randoc-layout` | The `Doc` type: text with breakable spaces, indentation and wrapping | none |
| `randoc-templates` | The Pandoc template language: parser, renderer, pipes and partials | `randoc-ast`, `randoc-layout` |
| `randoc-data` | The data files that the Randoc licence permits: the default templates and their partials | none |
| `randoc-core` | The shared contracts (see below) and the shared utilities (`randoc_core::shared`) | `randoc-ast`, `randoc-layout`, `randoc-templates` |
| `randoc-FORMAT` | One crate for each format family: the reader and the writer (`randoc-native` is the first one) | `randoc-ast`, `randoc-core` |
| `randoc` | The library facade and the registry of formats | all format crates |
| `randoc-cli` | The `randoc` program: options, defaults files, input and output | `randoc` |
| `randoc-harness` | The conformance harness. It runs programs and does not link Randoc. | no Randoc crate |

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
- **No `unsafe`.** Each crate forbids `unsafe` code through the workspace lints.
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
