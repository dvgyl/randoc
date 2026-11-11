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

## ADR 9: ZIP containers use the crate `zip` 9.5.0 (2026-11-10)

- **Context.** Randoc must read and write the containers of `docx`, `odt`,
  `epub` and `pptx`. The input is untrusted. The harness unpacks each
  archive into a map from entry name to contents. It skips directories and
  ignores the entry order and the compression method
  (`crates/randoc-harness/src/compare.rs`). The oracle writes `mimetype`
  as the first entry of an `epub`, with the method Stored.
- **Decision.** Use `zip` 9.5.0, pinned as `=9.5.0`, with
  `default-features = false` and `features = ["deflate-flate2-zlib-rs"]`.
  The reader applies these rules:
  1. It checks the size of the archive before it calls `ZipArchive::new`.
  2. It rejects an archive with more entries than
     `Limits::max_archive_entries`.
  3. It rejects an archive for which `ZipArchive::has_duplicate_names()`
     is true.
  4. It finds a part with `ZipArchive::index_for_name_raw` and then reads
     it with `ZipArchive::by_index`.
  5. It reads each entry through `Read::take(limit + 1)`.
  6. It adds the bytes of each entry to one total, which must not exceed
     `Limits::max_archive_size`.

  The writer sets `last_modified_time` of each entry to
  `DateTime::default()` (1980-01-01 00:00:00).
- **Why.** Source of 9.5.0 and oracle probes:
  - `ZipWriter` writes the entries in the order of the calls to
    `start_file`. `SimpleFileOptions` sets the compression method and the
    time of each entry. The default time depends on the feature `time`
    (`src/datetime.rs`), and Cargo unifies features, so the writer sets
    the time itself.
  - `ZipArchive::new` reads the full central directory and has no option
    to stop earlier (`src/read/config.rs`). Its loop reads at least 46
    input bytes for each record (`read_central_header` in
    `src/read/zip_archive.rs`), so rule 1 bounds this step.
  - The crate keeps one record for each name, the last one. The oracle
    uses the first of two `word/document.xml` entries. So Randoc rejects
    duplicate names (rule 3).
  - `by_name` falls back to a search of the decoded names when the bytes
    do not match. `index_for_name_raw` compares only bytes, and `by_index`
    decompresses (`src/read/zip_archive.rs`). The oracle compares names
    literally: the entries `/word/document.xml` and
    `word/../word/document.xml` give `DocxError` (exit code 63).

  A `cargo check` for `wasm32-unknown-unknown` and `wasm32-wasip1` passes
  with this feature list. Rejected: `rc-zip` 5.4.1 (a reader only);
  `async_zip` 0.0.19 (asynchronous, a 0.0 version); a port of
  `zip-archive` 0.5 (the harness compares the contents after it unpacks
  them).
- **Licence.** MIT (`LICENSE` of the crate 9.5.0). Each crate in the
  dependency tree of the selected features has a licence or an `OR` option
  in the list of "The rule". Keep the licence text in
  `third_party/licences/`.
- **Consequences.** `Limits` has no size limit for an archive file yet;
  rule 1 needs one (a request to the coordinator). On `wasm32` the crate
  depends on `getrandom` 0.4 with `wasm_js`; only encryption and
  `staging_path` call it. The root `Cargo.toml` (line 50) has
  `zip = { version = "9", default-features = false, features = ["deflate"] }`;
  the coordinator decides the pin. Record the duplicate-name difference
  in `docs/ISSUES.md`.

  To verify in the workstream, with oracle tests:
  - Unicode Path fields. The crate replaces the header name with the name
    of a valid Info-ZIP Unicode Path field (`src/extra_fields/extra_field.rs`),
    so `index_for_name_raw` sees the replaced name. Open question: which
    name the oracle uses for each such entry, and whether Randoc must
    reject the archive or recover the header name.
  - CP437 names. Open question: how the oracle matches a name without the
    UTF-8 flag that has bytes above 127.
  - Media names of `--extract-media`. Probes gave these output paths:
    `media/rId9.png` for the entry `word/media/rId9.png` of a `docx`;
    `media/sub/pic.png` for `word/media/sub/pic.png`; `pic2.png` for the
    target `media/../pic2.png`; `media/pic3.png` for the target
    `/word/media/pic3.png`; the SHA-1 name
    `4a5eb7171b58e08a6881721e3b43d5a44419a2be.png` for the target
    `media/../../../../evil1.png`; `media/file0.png` for the entry
    `EPUB/media/file0.png` of an `epub`. Open question: the general rule,
    for each input format and each name form. Each path that Randoc writes
    must stay inside the output folder, with a check that is the same on
    each host system.
  - Tests: a ZIP bomb, 100,000 entries, many records with one name, `../`
    names, a wrong declared size, and a fuzz target.

## ADR 10: XML reading uses `quick-xml` 0.42.0 (2026-11-10)

- **Context.** Randoc reads XML for `docx`, `odt`, `epub`, `jats`,
  `docbook`, `opml`, `fb2` and `endnotexml`. The input is untrusted. The
  oracle expands the entities of an internal DTD subset in text and in
  attribute values; a replacement text with `<emphasis>em</emphasis>` gives
  `Emph [ Str "em" ]`. It knows `&eacute;` without a DTD. It does not load
  a `SYSTEM` entity (`Unresolved entity &x;`, exit code 44).
- **Decision.** Use `quick-xml` 0.42.0, pinned as `=0.42.0`, with
  `default-features = false`. The reader applies these rules:
  1. It decodes the input to UTF-8 before the XML reader.
  2. It reads events with `Reader::from_str` or `NsReader::from_str`.
  3. It keeps its own element stack with the Randoc limit
     `Limits::max_nesting_depth`.
  4. It reads the internal `ENTITY` declarations from the `DocType`
     event, with a Randoc limit for their number.
  5. It expands each `Event::GeneralRef` itself, and it reads a
     replacement text that contains markup with a nested
     `Reader::from_str` on the same element stack.
  6. It expands an attribute value with
     `Attribute::normalized_value_with(version, depth, resolver)`. The
     closure `resolver` returns the four entities `lt`, `gt`, `quot` and
     `apos`, the declared entities, and each entity of the named-entity
     table. It counts the bytes that it returns. Rule 5 uses the same
     table: in an oracle probe with no DTD, `role="&eacute;"` gives `é`.
  7. One byte budget and one nesting limit apply to each document, for
     text, nested markup and attribute values.
  8. It converts each byte offset of `quick-xml` to a character offset.

  Randoc writes XML with `randoc-layout` (ADR 3).
- **Why.** Source of 0.42.0 and oracle probes:
  - Without the feature `encoding`, the decoder is always UTF-8
    (`src/encoding.rs`). For an ISO-8859-1 file the oracle prints
    `is not UTF-8 encoded: falling back to latin1.` and ignores the
    encoding declaration.
  - Events of `Reader<&[u8]>` borrow from the input
    (`src/reader/slice_reader.rs`), so one token needs no copy. The
    `BufRead` reader copies each token into a growing buffer
    (`src/reader/buffered_reader.rs`).
  - In an attribute, `Attribute::value` holds the raw value
    (`src/events/attributes.rs`). `normalized_value_with` handles `&#N;`
    and `&amp;` itself and calls the closure for each other name
    (`src/escape.rs`). It stops at `depth` nested references and has no
    byte limit.
  - `buffer_position()` and `error_position()` give byte offsets. The
    oracle counts characters: `</x>` at byte offset 120 after three `é`
    gives `1:118 (117)`.

  A `cargo check` for `wasm32-unknown-unknown` and `wasm32-wasip1` passes.
  Rejected: `roxmltree` 0.21.2 resolves entities itself, gives the error
  `UnknownEntityReference` for `&eacute;` without a declaration, fixes the
  entity depth at 10 and has no depth option. `xmlparser` 0.13.6 has had
  no release since 2023-09-30. `xml-rs` 1.0.0 forwards to `xml` 1.0.0,
  which has its own entity and size limits; it is not compared in detail.
- **Licence.** MIT (`LICENSE-MIT.md` of the crate 0.42.0). The only
  non-optional dependency is `memchr` (Unlicense OR MIT). Keep the licence
  text in `third_party/licences/`.
- **Consequences.** Randoc maintains the entity expansion, the named-entity
  table and the mapping to oracle error texts. A file that Randoc reads
  needs a size limit in `Limits` (a request to the coordinator). Do not
  read Pandoc's `xml-light` (GPL).

  To verify in the workstream, with oracle tests:
  - The decoding of a part of an archive that is not UTF-8.
  - The oracle result for an entity whose replacement text has `<` in an
    attribute value.
  - Tests: a billion-laughs input in text and in an attribute, 100,000
    nested elements, large literal text, a large attribute, a large DTD,
    a bare `&`, a wrong end tag, a missing end tag, and a fuzz target.

## ADR 11: Lua 5.4 uses `mlua` 0.12.2 and Lua 5.4.8 built by Randoc (2026-11-10)

- **Context.** Pandoc 3.13 embeds Lua 5.4.8 for filters, custom readers
  and writers, and `pandoc lua`. The Haskell package `lua` 2.3.4 compiles
  it with `-DLUA_USE_MACOSX` on macOS, `-DLUA_USE_LINUX` and `-Wl,-E` on
  Linux, and `-DLUA_USE_POSIX` and `-Wl,-E` on FreeBSD (`lua.cabal`). It
  does not define `LUA_COMPAT_5_3`. In the oracle `math.pow` is `nil`. The
  oracle gives each filter `io`, `os`, `debug` and `package` with
  `package.loadlib` and 5 searchers, also under `--sandbox`.
- **Decision.** Use `mlua` 0.12.2 (`=0.12.2`) with
  `default-features = false` and `features = ["lua54"]`, and `mlua-sys`
  0.13.0 (`=0.13.0`) with `features = ["lua54", "external"]`. A Randoc
  build script compiles Lua 5.4.8 with `cc` and the defines of `lua`
  2.3.4, without `LUA_COMPAT_5_3`.
  The sources are a copy, an approved exception to rule 5 of
  `docs/PRIOR_ART.md`. Randoc must change the compile-time configuration
  of Lua, and no released crate offers that configuration. The change
  lives in the build script. The 59 files stay byte-identical to the
  folder `src` of `lua-5.4.8.tar.gz` of lua.org (SHA-256
  `4f18ddae154e793e46eeab727c59ef1c0c0c2b744e7b94219710d76f530629ae`),
  without `lua.c`, `luac.c`, `Makefile` and `lua.hpp`.
  Put Lua behind a Cargo feature `lua`; a WebAssembly build turns it off.
  1. Full mode, the default, also under `--sandbox`:
     `unsafe { Lua::unsafe_new_with(StdLib::ALL, LuaOptions::default()) }`.
     Randoc adds the fifth searcher of the oracle.
  2. Restricted mode, for a server: `Lua::new_with` without `StdLib::IO`,
     `StdLib::OS`, `StdLib::PACKAGE` and `StdLib::DEBUG`. Randoc sets `dofile`, `loadfile` and
     `string.dump` to `nil`, and it replaces `load` with a function that
     calls the original with the mode `"t"`.

  Both modes set a memory limit with `Lua::set_memory_limit` and a limit
  for Lua VM instructions with `Lua::set_global_hook` and
  `every_nth_instruction(1000)`. When the budget is spent, the callback
  calls `lua.current_thread().set_hook(...)` with
  `every_nth_instruction(1)` and a callback that always returns an error,
  and it returns the error itself.
- **Why.** Source and probes on `aarch64-apple-darwin`:
  - The feature `vendored` builds Lua from the crate `lua-src`. Its
    releases 551.0.1 and 551.0.2 always define `LUA_COMPAT_5_3` for
    Lua 5.4 (`src/lib.rs`). With `vendored`, `mlua-sys` 0.13.0 needs
    `lua-src` 551.0.2 or later. An earlier probe, built with `mlua` 0.12.1
    and `lua-src` 551.0.1, has `math.pow`, and `a <= b` with only `__lt`
    succeeds through the `__lt` fallback. A probe built by a Randoc build script
    gives the oracle results for each compile-time property that was
    probed, among them the C stack limit (196 nested parentheses load,
    197 fail).
  - The 59 files of lua.org and of `lua` 2.3.4 are the same.
  - With `external`, `mlua-sys` compiles and links nothing
    (`build/main_inner.rs`). Then the `mlua-sys` 0.13.0 requirement
    `lua-src >=551.0.2` (Lua 5.4.9) does not apply, because `lua-src` is
    optional. The Lua 5.4 binding files of `mlua-sys` 0.12.0 and 0.13.0 are
    the same. The changelog of `mlua` 0.12.2 lists fixes in callback error
    handling and hook restoration.
  - `Lua::new_with` refuses `StdLib::DEBUG` and replaces the C searchers
    (`src/state.rs`), so only `unsafe_new_with` gives the oracle
    libraries. The base library with `load` (mode `"bt"`), `dofile` and
    `loadfile` is always loaded.
  - `Lua::set_interrupt` exists only with `luau`. With the mechanism of
    the decision, a probe stopped a loop that does its work inside
    `pcall`, in the main thread and in a coroutine that was created before
    the budget was spent. With a count of 1000 and no switch, the `pcall`
    loop ran to its end.
  - In a probe, a C module (`require`, `luaL_checkinteger`) loaded and ran
    in a debug build on macOS with no extra linker option.
  - `set_memory_limit` stops an allocation above the limit with
    `memory error: not enough memory`.

  Rejected: `piccolo` 0.3.3 (not the reference Lua; last release
  2024-06-16); `rlua` 0.20.1 (a wrapper of `mlua` 0.9).
- **Licence.** MIT for `mlua` 0.12.2 (`LICENSE` of the crate) and
  `mlua-sys` 0.13.0 (`license = "MIT"`; the crate has no licence file).
  MIT for Lua 5.4.8 (the notice at the end of `lua.h`, which stays
  unchanged). The duties: the MIT licence text of Lua in
  `third_party/licences/`, a `NOTICE.md` in the crate that holds the
  sources, a row in `THIRD_PARTY_NOTICES.md`, and a test that compares the
  SHA-256 of each copied file with a list. The build dependencies of
  `mlua-sys` are `cc`, `cfg-if` and `pkg-config` (MIT OR Apache-2.0).
- **Consequences.** Full mode needs an exception to standing order 22 for
  one `unsafe` call (a request to the coordinator). Full mode is not a
  security boundary, as in the oracle: a filter can call `debug.sethook()`
  and remove the instruction limit. The instruction limit does not bound
  one long C function, for example a pattern match of `string.find`; the
  memory limit bounds only its allocation. In a probe, `string.find` with
  the pattern `("a?"):rep(n) .. ("a"):rep(n) .. "b"` took 1.7 s for
  `n = 20` and 29.8 s for `n = 24`, and the hook never stopped it.
  The removal of Lua libraries does not restrict a host function, which
  is Rust code that Lua calls. In an oracle probe, `pandoc.pipe` started
  a process after `io`, `os`, `package` and `debug` were set to `nil`.
  So in restricted mode, each host function of the `pandoc` module that
  reads or writes a file, starts a process or uses the network must go
  through the restricted `Context` of `randoc-core`. A host function with
  no restricted form is not registered in restricted mode. This rule is
  separate from full mode, which keeps the access of the oracle.

  To verify in the workstream, with oracle tests:
  - A test in restricted mode for each host function with file, network
    or process access (`pandoc.pipe`, the modules `pandoc.system` and
    `pandoc.mediabag`, `pandoc.read` and `pandoc.write` with files, a
    template or a data file read): the call fails or goes through the
    restricted `Context`.
  - The link of C modules in full mode. Open question: which linker
    options export the Lua symbols in a release build on macOS, on Linux
    (the oracle build uses `-Wl,-E`; the `mlua` FAQ names `-rdynamic`) and
    on Windows. On this Mac the oracle cannot load
    an unsigned module (library validation), so the oracle behaviour is
    not probed here.
  - An answer for long C functions in restricted mode (for example a
    worker process with a time limit). Pattern matching is not bounded
    now.
  - The instruction limit with more coroutine cases: a coroutine that
    yields across `pcall`, and a coroutine created by Rust code.
  - The fifth searcher of the oracle.
  - A build on Linux and Windows, and the WebAssembly targets.
  - Tests: the library set of full mode against the oracle; the removals
    of restricted mode; a binary chunk; a loop inside `pcall`.

## ADR 17: The CSL processor is a port of `citeproc` 0.14.1.1 (2026-11-11)

- **Context.** `--citeproc` must give the bytes of Pandoc 3.13, which uses
  the Haskell library `citeproc` 0.14.1.1. The CSL test suite has no
  licence grant. The 61 CSL locales and `default.csl` are CC BY-SA 3.0.
- **Decision.** The crate `randoc-citeproc` is a port of `citeproc`
  0.14.1.1 (BSD-2-Clause), module by module. The BCP 47 tag type is a port
  of `Text/Collate/Lang.hs` of `unicode-collation` 0.1.3.7 (BSD-2-Clause).
  XML comes from `quick-xml` 0.42.0 under the rules of ADR 10; the crate
  builds its own element tree with the nesting limit and maps each XML
  error to the text of `xml-conduit`, from an oracle matrix. Styles and
  locales are read at run time through `Context` from the data directory:
  `DATADIR/csl/locales/locales-TAG.xml` and `DATADIR/default.csl`. No
  locale and no style is in the repository (decisions D1 and D2).
- **Why.** The output of `citeproc` depends on many small rules (term
  order, locale merge order, defaults of each attribute). A port keeps
  them. `hayagriva` and `citationberg` (MIT OR Apache-2.0) implement CSL,
  but their output is not the output of `citeproc`.
- **Licence.** BSD-2-Clause for both packages; `third_party/licences/citeproc.txt`,
  `third_party/licences/unicode-collation.txt`, and
  `crates/randoc-citeproc/NOTICE.md`. The data files `tailorings` and
  `cjk` of `unicode-collation` are not used.
- **Consequences.** Without the data package, Randoc has no en-US locale
  and returns an error where the oracle gives output (`docs/ISSUES.md`).
  The tests of the gate use small styles and locales written for Randoc.
  Tests with the real locales read the folder of `RANDOC_CSL_DATA`.
