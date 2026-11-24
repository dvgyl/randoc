# Documentation index

Active documentation lives in `docs/` and the repository root. Historical
session logs and one-time research archives have been moved to
[`to-be-deleted/`](../to-be-deleted/README.md).

## Start here

| Document | Purpose |
| --- | --- |
| [Project README](../README.md) | Project overview and quick start |
| [Citations](CITATIONS.md) | Built-in citation data, style and locale selection, and optional upstream CSL files |
| [Changelog](../CHANGELOG.md) | Changes by release |
| [Security policy](../SECURITY.md) | Vulnerability reporting and security boundaries |
| [`../AGENTS.md`](../AGENTS.md) | Agent operating guide, communication preference, environment rules, and commands |
| [`../PROGRESS.md`](../PROGRESS.md) | Current compatibility metrics, crate status, and next priorities |
| [`MODEL_ROUTING.md`](MODEL_ROUTING.md) | Standing model routing policy and Randoc-specific routing rule |

## Architecture & design

| Document | Purpose |
| --- | --- |
| [Architecture](ARCHITECTURE.md) | Conversion pipeline, all 45 workspace crates, shared interfaces, and reader and writer rules |
| [`ADR.md`](ADR.md) | Architecture decision records through ADR 27 |
| [`ISSUES.md`](ISSUES.md) | Known divergences from Pandoc 3.13, deliberate safety limits, and open findings |

## Verification, testing & benchmarks

| Document | Purpose |
| --- | --- |
| [`CONTRIBUTING.md`](CONTRIBUTING.md) | Merge gate requirements, finding severity levels, reuse rules, and verification commands |
| [`../conformance/README.md`](../conformance/README.md) | Differential conformance case format, file/ZIP snapshotting, and harness commands |
| [`PANDOC_TESTSUITE.md`](PANDOC_TESTSUITE.md) | Running Pandoc 3.13's external test suite against Randoc |
| [`CORPUS.md`](CORPUS.md) | Real-world open-licence document corpus (`tools/corpus/`) |
| [`BENCHMARKS.md`](BENCHMARKS.md) | CPU/memory benchmarking against Pandoc (`tools/bench/`) |
| [`../tools/measure/suites/README.md`](../tools/measure/suites/README.md) | Adding and running pluggable measurement suites (`tools/measure/`) |
| [`../tools/casegen/README.md`](../tools/casegen/README.md) | Deterministic differential case generator (`tools/casegen/`) |
| [`../tools/pivot/README.md`](../tools/pivot/README.md) | 400-tree writer pivot set and writer measurement (`tools/pivot/`) |

## Licensing & provenance

| Document | Purpose |
| --- | --- |
| [`../THIRD_PARTY_NOTICES.md`](../THIRD_PARTY_NOTICES.md) | Attribution for reused code, dependencies, and data |
| [`PRIOR_ART.md`](PRIOR_ART.md) | Reuse policy, active open-source ports/dependencies, and audited candidates |
| [`DATA_FILES_LICENCES.md`](DATA_FILES_LICENCES.md) | Licence audit of every Pandoc 3.13 data file (decisions D1–D4) |

## Repository navigation

| Path | Contents |
| --- | --- |
| [`Cargo.toml`](../Cargo.toml) | Workspace manifest for the [45 crates](ARCHITECTURE.md#workspace-crates-45) |
| [`crates/`](../crates/) | Randoc library, format support, command-line program, and test harness |
| [`conformance/`](../conformance/) | Test cases, expected output, and test input files |
| [`spec/`](../spec/) | Compatibility inventory and baseline data |
| [`scripts/`](../scripts/) | Build and verification scripts |
| [`tools/`](../tools/) | Measurement, verification, and data preparation tools |
| [`benchmarks/`](../benchmarks/) | Benchmark definitions and results |
| [`third_party/licences/`](../third_party/licences/) | Licence texts for reused material |
