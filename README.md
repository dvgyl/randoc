# Randoc

Randoc is a document converter written in Rust. The goal is a complete drop-in
replacement for [Pandoc](https://pandoc.org) 3.13: the same formats, options,
output, warnings and exit codes.

**Status: early development.** Randoc does not replace Pandoc yet. The measured
compatibility is in [`spec/capabilities.json`](spec/capabilities.json) and
[`PROGRESS.md`](PROGRESS.md). Each number there comes from a test run.

## Build and test

```sh
./init.sh
```

`init.sh` builds the workspace, runs the tests and runs the conformance harness.
It also installs a Git hook for commit messages.

## Use

```sh
cargo run -p randoc-cli -- -f json -t json input.json
```

When the program name is `pandoc`, `--version` prints the Pandoc version that
Randoc matches on the first line and the Randoc version on the second line.

## How compatibility is measured

Pandoc 3.13 is the oracle. The harness in `crates/randoc-harness` runs the same
input and options through Pandoc and through Randoc. It compares the exit code,
the standard error output, the document tree and the writer output.
See [`conformance/README.md`](conformance/README.md).

Randoc never calls Pandoc. Pandoc is a test tool only.

## Repository layout

| Path | Content |
| --- | --- |
| `crates/randoc-ast` | The Pandoc document model and its JSON form |
| `crates/randoc-core` | Shared contracts: options, extensions, errors, the conversion context |
| `crates/randoc-layout` | Text layout with line wrapping, for writers |
| `crates/randoc-templates` | The Pandoc template language |
| `crates/randoc` | The library: the registry of readers and writers |
| `crates/randoc-cli` | The `randoc` program |
| `crates/randoc-harness` | The differential conformance harness |
| `conformance/` | Test cases and the expected results from the oracle |
| `spec/` | The inventory of Pandoc features and the capability matrix |
| `docs/` | Architecture, contribution rules, known issues |

## Licence

Randoc is licensed under either of the [MIT licence](LICENSE-MIT) or the
[Apache License, Version 2.0](LICENSE-APACHE), at your option.

Randoc is an independent implementation. It contains no Pandoc source code.
Files from other projects are listed with their licences in
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
