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
| I-C01 | Corpus | Randoc fails each unit of the document corpus (`spec/corpus.json`). The corpus has no `json` input, and Randoc has only the JSON reader and writer. | Open. The result changes when the format workstreams merge. See `docs/CORPUS.md`. |
| I-C02 | Corpus (oracle) | Pandoc needs about 100 s of wall time (5 s of CPU time) to read `jupyter-running-code` (`jupyter/notebook`, `Running Code.ipynb`). The other notebooks need less than 1 s. | Kept as found. `run.py` has a default time limit of 180 s for this reason. |

## Review findings with low severity

None yet.
