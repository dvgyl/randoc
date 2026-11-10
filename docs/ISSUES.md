# Known issues and divergences

Each entry gives the difference from Pandoc 3.13, its reason and its state.

## Deliberate divergences

| ID | Difference | Reason |
| --- | --- | --- |
| D-001 | `--version` prints a second line that names Randoc and its version. | Required by the project goal. The first line is equal to the Pandoc line, so tools that check the version keep working. |
| D-002 | Randoc stops at a nesting limit (`Limits::max_nesting_depth`). Pandoc has no limit. | Input is not trusted. The limit of the program is 16384 levels. The JSON reader counts JSON levels: it accepts 32768 nested arrays and objects, so the document depth that it accepts depends on the node type (for example about 16000 nested block quotes or 10900 nested divs). |
| D-003 | The types of the document model use the derived `Clone`, `PartialEq` and `Drop`, which recurse. In a debug build on a 2 MB stack, `clone` of nested table cells overflows the stack between 200 and 400 levels, and `clone` of nested block quotes between 800 and 1600 levels. | A manual `Drop` for a node type forbids moving a field out of a node, which readers and writers do. The crate documentation of `randoc-ast` states the precondition: 200 levels are safe on a 2 MB stack (test `a_tree_at_the_documented_depth_is_safe_on_a_small_stack`). Readers stop at `Limits::max_nesting_depth` (128 by default), and the program uses a 1 GiB stack for its limit of 16384. Found by both reviews of json-codec, round 2 (Opus R2-01, Codex JC2-01). |

## Open divergences

| ID | Area | Difference | State |
| --- | --- | --- | --- |
| I-001 | JSON reader | The text of a JSON error is different. Pandoc prints the path of the error (`Error in $.blocks[0]: ...`) and its own type names. The exit code is equal. | Open. 8 units fail: `json/roundtrip/{old-version,missing-version,not-json,unknown-block}`. |
| I-002 | Program | Only `-f`, `-t`, `-o`, `--version` and input files work. The formats on `main` are `json` and `native`. | Open. The command-line workstream replaces the first version. |
| I-CORPUS-01 | Corpus | Randoc fails each unit of the document corpus (`spec/corpus.json`). The corpus has no `json` input, and Randoc has only the JSON reader and writer. | Open. The result changes when the format workstreams merge. See `docs/CORPUS.md`. |
| I-CORPUS-02 | Corpus (oracle) | Pandoc needs about 100 s of wall time (5 s of CPU time) to read `jupyter-running-code` (`jupyter/notebook`, `Running Code.ipynb`). The other notebooks need less than 1 s. | Kept as found. Its manifest entry sets `timeout = 300`. The default time limit stays 60 s. |
| I-LAYOUT-01 | Layout | `randoc-layout` has no port of `realLengthWideContext` (ambiguous characters as width 2). No Pandoc writer that Randoc ports needs it yet. | Open. Port it when a writer needs it. |
| I-LAYOUT-03 | Layout | `randoc-layout` does not export the `Doc` constructors or the `HasChars` class of `doclayout`. A Rust `Doc` holds `String` text only, and the constructors are internal so that the flat form of concatenation stays valid. `realLengthWideContextNoShortcut` is missing for the reason of I-LAYOUT-01. | Open. Add a function when a workstream needs one. |
| I-LAYOUT-02 | Layout | The Lua module `pandoc.layout` has no `link` function, so the OSC 8 output of `link` has no oracle test. Its tests follow the `doclayout` 0.6 source. | Open. Compare with `pandoc -t ansi` when the ansi writer exists. |
| I-LAYOUT-04 | Layout | `Doc::update_column` returns `usize`, so a column that is negative after integer overflow becomes 0. Input: `lblock(isize::MAX as usize, literal("")) + lblock(isize::MAX as usize, literal("")) + literal("c")`. Pandoc gives `update_column(d, 0)` = -1, and Randoc gives 0. `offset`, `min_offset` and `height` are equal to Pandoc (0, 0 and 1). Only widths near the largest integer can give a negative column. | Accepted. A `usize` column is simpler for writers. |
| I-LAYOUT-05 | Layout | A block of a very large width allocates the padding of its lines. Input: `rblock(usize::MAX, literal("a"))`. Pandoc returns at once, because Haskell builds the padding only when it is needed; Randoc stops with an allocation failure. The documentation of `lblock`, `rblock` and `cblock` says that the width must be a real column width. | Accepted. Writers give column widths. |
| I-NATIVE-01 | JSON form (`randoc-ast`) | An infinite `Double` is written as `null`, and `-0.0` stays negative when it is read. Pandoc writes `"+inf"` and `"-inf"`, and the JSON reader does not accept those strings. The native reader and writer are correct. | Open. `native/read/double-forms#read`, `native/read/double-forms#write:native`, `native/read/double-special#read`, `native/read/double-special#write:native` and `native/read/double-negative-zero#write:native` fail until the `json-codec` workstream merges. The last unit fails because the Pandoc JSON reader reads `-0.0` as `0.0`. |
| I-NATIVE-02 | Native writer | The writer stops at `max_nesting_depth` and returns an application error (exit code 4). Pandoc has no limit. | Deliberate, as D-002. The limit of the program is 16384 levels. |
| I-NATIVE-03 | Native reader | Input deeper than 6 value levels for each permitted tree level gives `couldn't read native`. Pandoc has no limit. | Deliberate, as D-002. |
| I-NATIVE-04 | Native reader | Pandoc expands each literal tab in the input before the native reader runs (`Str "a<TAB>b"` reads as `Str "a  b"`, unless `--preserve-tabs`). The command-line workstream owns that step in input preparation, so the native reader does not handle tabs. | Open until the command-line workstream adds tab expansion. An escaped `\t` stays a tab in both programs. The units `native/read/literal-tab#read` and `native/read/literal-tab#e2e:native` fail until then. |
| I-NATIVE-05 | Native writer | The output grows with the square of the nesting depth, because each line has its indentation. For example, 3000 nested block quotes give 36,093,037 bytes. Pandoc gives the same bytes (checked at 1000 and 3000 levels), so the writer has no output limit. 10,000 nested block quotes give 400,310,037 bytes, the same bytes as Pandoc: the release build of Randoc needs 8.2 s and Pandoc needs 12.1 s on the same loaded machine. The debug build needs more than 60 s. | Not a divergence. Recorded for capacity planning. |

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
| L-003 | `docs/research-data/commands.md` | The merge command reads `probe-cf.tsv`, but the recorded probe run writes `probe.tsv`. The recorded commands cannot make `results.tsv` again as written. | Opus review of research, round 2 (N-1) |
| L-004 | `docs/DATA_FILES_LICENCES.md` | The notice rule for `julia.xml` omits the author that the file header names (Timothy E. Holy). Keep that credit when Randoc ships the file. | Codex review of research, round 2 (R2-001) |
| L-005 | `tools/corpus/test_corpus.py` | Three tests of `SlowServerTests` compare the download files in the shared temporary folder before and after one download. They fail when a second download process uses the same folder at the same time. The tool itself is correct. Correction: give each test a private temporary folder. | Opus review of corpus, round 6 (R6-01) |
| L-006 | `crates/randoc-ast/src/json.rs` | `show_double` writes one digit fewer than Pandoc when the shortest digits are exactly halfway between two `Double` values: `4.079326013337682e16` for `4.0793260133376816e16` (97 of 3000 large integers in the reviewer's sample). The JSON codec branch ports the digit generation of GHC and corrects this. | Opus and Codex reviews of native, rounds 2 and 3 (R2-2) |
