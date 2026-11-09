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
| I-003 | Layout | `randoc-layout` has no port of `realLengthWideContext` (ambiguous characters as width 2). No Pandoc writer that Randoc ports needs it yet. | Open. Port it when a writer needs it. |
| I-004 | Layout | The Lua module `pandoc.layout` has no `link` function, so the OSC 8 output of `link` has no oracle test. Its tests follow the `doclayout` 0.6 source. | Open. Compare with `pandoc -t ansi` when the ansi writer exists. |

## Review findings with low severity

None yet.
