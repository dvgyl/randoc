# Decision records

A decision record (ADR, architecture decision record) says in a few lines
what was decided and why. Write one before you reuse open-source material
(see [`PRIOR_ART.md`](PRIOR_ART.md)) and for each decision that changes a
shared contract.

All records are in this one file, newest last. Give each record the next
number. Use this form:

- **Context.** The need, in one or two sentences.
- **Decision.** What Randoc does, with the exact upstream name and version.
- **Why.** The reason for this choice, and the choices that were rejected.
- **Licence.** The licence of the material and where its text is kept.
- **Consequences.** What Randoc must maintain, test or watch.

Records 2 to 7 describe reuse that happened before the rule of 2026-11-10.
They were written afterwards from the merge records in `MERGE_LOG.md`.

## ADR 1: Reuse open source before writing new code (2026-11-10)

- **Context.** The first sessions wrote almost each part from zero or ported
  it by hand. The owner gave a new rule: much of what Randoc needs exists in
  open source, so study it and build on it where that is faster or more
  reliable.
- **Decision.** Each workstream starts with a search for prior art and
  records the result in `PRIOR_ART.md`. Reuse needs a row there, a record
  here, a licence check and attribution in `THIRD_PARTY_NOTICES.md`. A pinned
  dependency is better than a copy. A fork or a vendored copy is permitted
  only when Randoc must change the code; the changes stay small, documented
  and tested. Reused code passes the same merge gate as new code.
- **Why.** Less code to write and to review, and code that many users have
  already tested. The licence check is necessary because the repository is
  public.
- **Licence.** Not applicable.
- **Consequences.** The library and the program can get run-time
  dependencies. Each one needs a pinned version, a check of its behaviour on
  untrusted input, and a check that the WebAssembly build still works.
  Pandoc itself and `texmath` stay excluded (GPL).

## ADR 2: The document model follows `pandoc-types` 1.23.1.2

- **Context.** Filters and the JSON format need the same tree as Pandoc.
- **Decision.** `randoc-ast` defines the types of `pandoc-types` 1.23.1.2
  with the same names and the same JSON form. The code is new Rust code.
- **Why.** The JSON form is a documented interface, and byte-identical JSON
  needs the same type shapes. No Rust crate gives this exact model and JSON
  text.
- **Licence.** BSD-3-Clause; `third_party/licences/`.
- **Consequences.** A new Pandoc version can change the model version
  (`[1,23,1,2]`).

## ADR 3: The layout engine is a port of `doclayout` 0.6

- **Context.** Each text writer of Pandoc wraps and indents its output with
  `doclayout`. Byte-identical output needs the same line-break decisions.
- **Decision.** `randoc-layout` is a port of `doclayout` 0.6 to Rust.
- **Why.** The break rules are many and small. A port keeps each rule; a
  new design from black-box tests would need far more tests to reach the
  same result. Rust crates for pretty printing have other rules.
- **Licence.** BSD-3-Clause; `third_party/licences/`.
- **Consequences.** Randoc maintains the port. The crate has its own oracle
  tests through the template interface.

## ADR 4: The template engine is a port of `doctemplates` 0.11.1

- **Context.** Pandoc templates have their own language, with error texts
  that come from the parser library `parsec`.
- **Decision.** `randoc-templates` is a port of `doctemplates` 0.11.1. A
  small module ports the error-merge rules of `parsec` 3.1.17.0, so that the
  error texts are equal.
- **Why.** The language has no other implementation. The error texts depend
  on the internal rules of `parsec`, which black-box tests cannot find in a
  reasonable time.
- **Licence.** BSD-3-Clause (`doctemplates`), BSD-2-Clause (`parsec`);
  `third_party/licences/`.
- **Consequences.** The renderer differs from upstream on purpose: it works
  on views, because the upstream design copies data and Randoc must stay
  linear on untrusted input.

## ADR 5: The default templates of Pandoc are data files in `randoc-data`

- **Context.** Standalone output (`-s`) needs the default template of each
  format, byte for byte.
- **Decision.** `randoc-data` holds the 62 files of `data/templates` and the
  dzslides template of Pandoc 3.13, unchanged.
- **Why.** The `COPYRIGHT` file of Pandoc gives these files a BSD-3-Clause
  option. New templates could not be byte-identical.
- **Licence.** BSD-3-Clause (templates), WTFPL (dzslides);
  `crates/randoc-data/data/templates/LICENSE`, `third_party/licences/`.
- **Consequences.** The other data files of Pandoc have no permissive
  option and stay outside (decisions D1 to D4 in `PROGRESS.md`).

## ADR 6: Number formatting follows GHC 9.12.2

- **Context.** The JSON form and the native format print `Double` values
  and strings with the `show` function of Haskell.
- **Decision.** `randoc-ast` (`src/json/haskell.rs`) ports the digit
  generation and the `show` forms of GHC 9.12.2.
- **Why.** The shortest-digit rule of GHC differs from the Rust rule for
  some values. A port gives the same digits for each value.
- **Licence.** BSD-3-Clause (the GHC licence); `third_party/licences/`.
- **Consequences.** None after the merge; the functions are small and have
  replay tests.

## ADR 7: Character data comes from the upstream tables, not from crates

- **Context.** Identifiers, entities, emoji and normalization need tables.
- **Decision.** `randoc_core::shared` holds generated tables: the entity
  table of `commonmark` 0.3.1, the emoji tables of `emojis` 0.1.5, and
  tables from the Unicode Character Database 15.1.0, 16.0.0 and 18.0.0.
- **Why.** The oracle uses different Unicode versions for different rules
  (15.1 for letter classes, 18.0 for normalization). A Rust crate has one
  Unicode version, so its results differ for new characters.
- **Licence.** BSD-3-Clause, MIT (gemoji data), Unicode-3.0;
  `third_party/licences/`.
- **Consequences.** A tool script makes each table again. If a later check
  shows that a pinned crate gives the same results, replace the table by the
  dependency (ADR 1).

## ADR 8: TeX math conversion is written from oracle output (2026-11-10)

- **Context.** Writers without math support convert TeX math to an inline
  list or to MathML. Pandoc does this with `texmath`.
- **Decision.** The workstream `math` writes the crate `randoc-math` from
  black-box oracle output. It can use parsers and symbol tables with a
  permissive licence as a base (`PRIOR_ART.md`, "TeX math"), after a licence
  check.
- **Why.** `texmath` has the licence GPL-2.0-or-later, so a port is not
  permitted in a repository with the licence `MIT OR Apache-2.0`.
- **Licence.** Not applicable until a base is chosen.
- **Consequences.** More oracle probes than a port would need. The branch
  `ws/math` holds the first version.
