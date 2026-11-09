# Architecture

Randoc converts a document in three steps, as Pandoc does:

1. A **reader** parses the input into a document tree (`randoc_ast::Pandoc`).
2. Filters and transformations change the tree.
3. A **writer** renders the tree in the output format.

## Crates

| Crate | Purpose | Depends on |
| --- | --- | --- |
| `randoc-ast` | The document tree, its Pandoc-compatible JSON form, and tree traversal | `serde` |
| `randoc-layout` | The `Doc` type: text with breakable spaces, indentation and wrapping | none |
| `randoc-templates` | The Pandoc template language and the template variables | `randoc-layout` |
| `randoc-core` | The shared contracts (see below) | the three crates above |
| `randoc-FORMAT` | One crate for each format family: the reader and the writer | `randoc-core` |
| `randoc` | The library facade and the registry of formats | all format crates |
| `randoc-cli` | The `randoc` program: options, defaults files, input and output | `randoc` |
| `randoc-harness` | The conformance harness. It runs programs and does not link Randoc. | none |

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

## Input preparation

The program decodes text input as UTF-8, removes a byte order mark and removes
each carriage return before it calls a text reader, as Pandoc does. A reader
gets `Sources`: one named text for each input file.

## Stack depth

The `randoc` program runs the conversion on a thread with a 1 GiB stack and
sets `max_nesting_depth` to 16384. A library user gets the default limit of 128
levels, which is safe on a thread with a 2 MB stack.

## Licence boundary

Pandoc is GPL-2.0-or-later. Randoc is MIT OR Apache-2.0. Do not read Pandoc
source code to write Randoc code, and do not copy Pandoc test files. Use the
Pandoc manual, the documented interfaces and the output of the oracle.
Libraries with permissive licences (for example `doclayout`, `doctemplates`,
`commonmark-hs`, `pandoc-types`, `citeproc`: BSD) can be studied and ported
with attribution in `THIRD_PARTY.md`. Check the licence first.
