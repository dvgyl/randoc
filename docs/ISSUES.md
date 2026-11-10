# Known issues and divergences

Each entry gives the difference from Pandoc 3.13, its reason and its state.

## Deliberate divergences

| ID | Difference | Reason |
| --- | --- | --- |
| D-001 | `--version` prints a second line that names Randoc and its version. | Required by the project goal. The first line is equal to the Pandoc line, so tools that check the version keep working. |
| D-002 | Randoc stops at a nesting limit (`Limits::max_nesting_depth`). Pandoc has no limit. | Input is not trusted. The limit of the program is 16384 levels. |

## Open divergences

| ID | Area | Difference | State |
| --- | --- | --- | --- |
| I-001 | JSON reader | The text of a JSON error is different. Pandoc prints the path of the error (`Error in $.blocks[0]: ...`) and its own type names. The exit code is equal. | Open. 8 units fail: `json/roundtrip/{old-version,missing-version,not-json,unknown-block}`. |
| I-002 | Program | Only `-f`, `-t`, `-o`, `--version` and input files work. | Open. The command-line workstream replaces the first version. |
| I-LAYOUT-01 | Layout | `randoc-layout` has no port of `realLengthWideContext` (ambiguous characters as width 2). No Pandoc writer that Randoc ports needs it yet. | Open. Port it when a writer needs it. |
| I-LAYOUT-03 | Layout | `randoc-layout` does not export the `Doc` constructors or the `HasChars` class of `doclayout`. A Rust `Doc` holds `String` text only, and the constructors are internal so that the flat form of concatenation stays valid. `realLengthWideContextNoShortcut` is missing for the reason of I-LAYOUT-01. | Open. Add a function when a workstream needs one. |
| I-LAYOUT-02 | Layout | The Lua module `pandoc.layout` has no `link` function, so the OSC 8 output of `link` has no oracle test. Its tests follow the `doclayout` 0.6 source. | Open. Compare with `pandoc -t ansi` when the ansi writer exists. |
| I-LAYOUT-04 | Layout | `Doc::update_column` returns `usize`, so a column that is negative after integer overflow becomes 0. Input: `lblock(isize::MAX as usize, literal("")) + lblock(isize::MAX as usize, literal("")) + literal("c")`. Pandoc gives `update_column(d, 0)` = -1, and Randoc gives 0. `offset`, `min_offset` and `height` are equal to Pandoc (0, 0 and 1). Only widths near the largest integer can give a negative column. | Accepted. A `usize` column is simpler for writers. |
| I-LAYOUT-05 | Layout | A block of a very large width allocates the padding of its lines. Input: `rblock(usize::MAX, literal("a"))`. Pandoc returns at once, because Haskell builds the padding only when it is needed; Randoc stops with an allocation failure. The documentation of `lblock`, `rblock` and `cblock` says that the width must be a real column width. | Accepted. Writers give column widths. |

## Review findings with low severity

None yet.
