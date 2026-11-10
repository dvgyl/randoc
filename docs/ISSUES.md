# Known issues and divergences

Each entry gives the difference from Pandoc 3.13, its reason and its state.

## Deliberate divergences

| ID | Difference | Reason |
| --- | --- | --- |
| D-001 | `--version` prints a second line that names Randoc and its version. | Required by the project goal. The first line is equal to the Pandoc line, so tools that check the version keep working. |
| D-002 | Randoc stops at a nesting limit (`Limits::max_nesting_depth`). Pandoc has no limit. | Input is not trusted. The limit of the program is 16384 levels. The JSON reader counts JSON levels: it accepts 32768 nested arrays and objects, so the document depth that it accepts depends on the node type (for example about 16000 nested block quotes or 10900 nested divs). |

## Open divergences

| ID | Area | Difference | State |
| --- | --- | --- | --- |
| I-001 | JSON reader | The text of a JSON error is different. Pandoc prints the path of the error (`Error in $.blocks[0]: ...`) and its own type names. The exit code is equal. | Open. 8 units fail: `json/roundtrip/{old-version,missing-version,not-json,unknown-block}`. |
| I-002 | Program | Only `-f`, `-t`, `-o`, `--version` and input files work. | Open. The command-line workstream replaces the first version. |

## Harness limits

| ID | Limit | State |
| --- | --- | --- |
| H-001 | The harness has no rule for output with random values. A test with the oracle on 76 writers (not `pdf`) showed: with `SOURCE_DATE_EPOCH` set, which the harness does, only two kinds of output change from run to run: the `urn:uuid` identifier of `epub`, `epub2` and `epub3`, and the `id` of each `ipynb` cell. | Open. Add a rule that replaces these two values with a fixed text before the comparison, and make `record` run the oracle two times to find output that is not stable. Do this before the first `ipynb` or EPUB writer workstream. |
| H-002 | The harness always sets `SOURCE_DATE_EPOCH`. Pandoc output can depend on the variable (for example, standalone LaTeX output gets a PDF trailer identifier when it is set). The behaviour without the variable has no units. | Open. Add a case field that removes a variable from the environment of one case. |

## Review findings with low severity

| ID | Area | Finding | Source |
| --- | --- | --- | --- |
| L-001 | `tools/bench/run.py` | In a very small time window, the timer of the time limit can send a kill signal to the process group after the measured program ended. The run result stays correct. | Codex review of bench, round 4 (R2-3) |
| L-002 | `tools/bench/run.py` | The fallback that stops a child which left its process group has no test. | Opus review of bench, final check (R4-1) |
