# Randoc

Randoc is a native Rust document converter aiming to be a complete drop-in
replacement for [Pandoc](https://pandoc.org) 3.13: the same input and output
formats, extensions, options, output bytes, warnings, and exit codes.

**Status: active development.** Measured compatibility lives in
[`spec/capabilities.json`](spec/capabilities.json) and
[`PROGRESS.md`](PROGRESS.md). Every number comes from differential test runs
against the pinned Pandoc 3.13 oracle.

## Quick start

```sh
# Optional: if using the external drive environment on macOS
. "/Volumes/Extreme Pro/Randoc/env/env.sh"

# Build all 21 crates, run tests, and check the conformance baseline
./init.sh

# Run Randoc
cargo run -p randoc-cli -- -f markdown -t html input.md
```

When invoked under the program name `pandoc`, `--version` prints the Pandoc
3.13 header on the first line and the Randoc version on the second line.

## Verification & measurement

Pandoc 3.13 is the **test oracle only**—Randoc never calls Pandoc.

```sh
# Fast mechanical gate (rustfmt, clippy, rustdoc, cargo test, conformance baseline)
scripts/gate.sh

# Extended gate (adds licence scan, oracle SHA-256 & clean-room checks, Python tool tests)
python3 tools/measure/gate.py --quick

# Run differential conformance harness directly
cargo run -q -p randoc-harness -- run --baseline spec/baseline.json
cargo run -q -p randoc-harness -- show 'json/roundtrip/numbers#write:json'

# Run modular measurement suites (harness, pandoc, corpus, readers, writers,
# defaults, errors, files, ast_roundtrip, lua, custom, server, extension_caps)
python3 tools/measure/measure.py --suite harness
python3 tools/measure/measure.py --suite readers --filter html
```

## Repository navigation

### Root files

| Path | Content |
| --- | --- |
| [`AGENTS.md`](AGENTS.md) | Agent instructions, environment rules, communication & model routing pointers |
| [`PROGRESS.md`](PROGRESS.md) | Current compatibility metrics, crate status, and next priorities |
| [`Cargo.toml`](Cargo.toml) | Cargo workspace definition (21 crates + fuzz excludes) |
| [`init.sh`](init.sh) | Installs commit-msg hook, builds workspace, runs tests & baseline |
| [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md) | Attribution table for all reused open-source code and data |
| [`LICENSE-MIT`](LICENSE-MIT), [`LICENSE-APACHE`](LICENSE-APACHE) | Dual licence (`MIT OR Apache-2.0`) |

### Crates (`crates/`)

#### Core & infrastructure
| Crate | Purpose |
| --- | --- |
| `crates/randoc-ast` | Pandoc 1.23.1.2 document AST, JSON codec (with `aeson`-compatible errors), tree traversal |
| `crates/randoc-core` | Shared contracts: `Reader`/`Writer`, options, extensions, `Context`/`Host`, errors, limits, shared helpers |
| `crates/randoc-layout` | Text layout engine with breakable spaces, indentation, and wrapping (port of `doclayout`) |
| `crates/randoc-templates` | Pandoc template parser, renderer, pipes, and partials (port of `doctemplates`) |
| `crates/randoc-data` | Permissively licensed embedded data files (62 BSD-3-Clause default templates + `dzslides`) |
| `crates/randoc` | Library facade and format registry (`src/formats.rs`) |
| `crates/randoc-cli` | CLI binary (`randoc` / `pandoc`): options, defaults files, filters, templates, I/O |
| `crates/randoc-harness` | Standalone differential conformance harness (stdout/stderr/exit/file/ZIP comparison) |

#### Format readers & writers
| Crate | Purpose |
| --- | --- |
| `crates/randoc-native` | `native` reader and writer |
| `crates/randoc-commonmark` | `commonmark`, `gfm`, and `commonmark_x` reader |
| `crates/randoc-markdown` | Pandoc `markdown` and variant readers |
| `crates/randoc-markdown-writer` | `markdown`, `gfm`, `commonmark`, `commonmark_x`, `plain`, and variant writers |
| `crates/randoc-html-reader` | `html` reader |
| `crates/randoc-html` | `html` and `html4` writers (with standalone templates and math) |
| `crates/randoc-latex-reader` | `latex` reader |
| `crates/randoc-latex` | `latex` and `beamer` writers |

#### Subsystems
| Crate | Purpose |
| --- | --- |
| `crates/randoc-yaml` | YAML metadata parser |
| `crates/randoc-math` | TeX math to MathML and Pandoc inline conversion |
| `crates/randoc-citeproc` | CSL style/locale parser, `csljson` reader/writer, citation evaluator (port of `citeproc`) |
| `crates/randoc-lua` | Embedded Lua 5.4.8 + LPeg 1.1.0 runtime, Lua filters, custom reader/writer hooks |
| `crates/randoc-highlight` | Syntax highlighter (port of `skylighting-core` 0.15 + 63 permissive KDE syntax definitions) |

### Top-level directories

| Directory | Content |
| --- | --- |
| [`docs/`](docs/README.md) | Architecture, ADRs, known issues, model routing, contribution guide, licence audits |
| [`conformance/`](conformance/README.md) | Differential test cases (`cases/`), oracle snapshots (`expected/`), and fixtures (`fixtures/`) |
| [`spec/`](spec/) | Pandoc 3.13 feature inventory (`inventory/`), `baseline.json`, `capabilities.json`, `pivot.sha256`, `normalizations.toml`, `unstable.json` |
| [`scripts/`](scripts/) | `gate.sh`, `fuzz.sh`, `gen_inventory.py`, and git `hooks/commit-msg` |
| [`benchmarks/`](benchmarks/) | Benchmark manifests, inputs, and recorded results |
| [`third_party/`](third_party/licences/) | Full upstream licence texts for all third-party material |
| [`tools/`](tools/) | Differential measurement, verification, and generation tools (see table below) |
| [`to-be-deleted/`](to-be-deleted/README.md) | Historical session logs and obsolete docs staged for removal |

### Tools (`tools/`)

| Tool | Purpose |
| --- | --- |
| `tools/measure/` | Unified measurement runner (`measure.py`), full verification gate (`gate.py`), copyleft shingle scanner (`licence_scan.py`), `bin/limit-run`, `metrics.toml`, `metrics.d/`, `suites/`, and tests |
| `tools/casegen/` | Seeded deterministic differential case generator (`ext-matrix`, `json-roundtrip`, `options-cli`) |
| `tools/pivot/` | 400-tree writer pivot set generator and writer compatibility runner |
| `tools/readers/` | Reader compatibility suite across all 52 Pandoc input formats |
| `tools/errors/` | Exit-code and conversion-log verification suite |
| `tools/ast/` | JSON and `native` AST round-trip verification suite |
| `tools/custom-writers/` | Lua custom writer probe suite (10 probes × 400 pivot trees) |
| `tools/server/` | `pandoc-server` HTTP and CGI compatibility suite |
| `tools/extension-caps/` | Extension capability probes across all 1,719 `(direction, format, extension)` pairs |
| `tools/pandoc-testsuite/` | Runner for Pandoc 3.13's external test suite (`$RANDOC_PANDOC_SRC/test`) |
| `tools/corpus/` | Open-licence real-document corpus runner (`$RANDOC_CORPUS`) |
| `tools/commonmark-spec/` | External CommonMark 0.31.2 specification runner |
| `tools/bench/` | Performance benchmark runner comparing Randoc vs Pandoc |
| `tools/harness-verdicts/` | Determinism checker comparing parallel harness runs |
| `tools/commonmark-cases/` | Helper scripts generating CommonMark Unicode/scheme tables |

## Licence

Randoc is dual-licensed under the [MIT licence](LICENSE-MIT) or the
[Apache License, Version 2.0](LICENSE-APACHE), at your option.

Randoc is a clean-room implementation and contains no Pandoc source code.
Permissively licensed third-party material is documented in
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
