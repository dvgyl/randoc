# Advice

One section for each question to the adviser model. `tools/autobuild/ops ask`
writes each section. Append only.

Section heading: `## UTC event=ITEM/EXPERIMENTS reason=KIND session=ID`.
A stuck event is one item at one number of experiments. It gets one question.
The section has the question, a summary of the answer, the path of the full
answer, and the decision of the lead (`ops ask --decision "TEXT"`).

## 2026-11-18T18:30:14Z event=M1-04/acceptance-failed reason=acceptance-failed session=20261118T183014Z-advice-4c2a8a9c

Question:

> M1-04 moved caps.writers from the built-in harness to the writers plugin but declared the active key in the base registry. The new plugin rows otherwise live in metrics.d/writers.toml. I intend to move the active caps.writers entry into metrics.d/writers.toml and keep a to-build base entry if convention requires it. This keeps the exact merged metric command, pass rule, total, and correction. What flaw in this repair should I check before committing and rerunning the experiment and gate?

Evidence:

> M1-04 gate 20261118T181259Z-d5d028524887 failed at tool-tests after experiment 1. python3 -B -m unittest discover -s tools/autobuild/tests -p test_measure_plugins.py reproduces two errors. ToolError: caps.writers is active, but measure.py has no runner for suite writers. PluginTest replaces SUITES_DIR and METRICS_DIR and unregisters every production plugin. It still reads the production base registry. The real scored writer suite passed with 5510/30400, caps.writers 2/77, and 61449 harness verdicts equal to the diagnostic.

Session: 01a1efec-cf5f-78a0-92b4-f9c83ebe785e

Answer (summary): Keep the base entry inactive and the plugin entry active. Check all 78 production writer rows before rerunning the gate.

Answer (full): /Volumes/Extreme Pro/Randoc/broker-out/20261118T183014Z-advice-4c2a8a9c.json

Decision: Keep the base entry at to-build and put the active entry in the writer overlay. The 14 focused tests passed. The merged registry matches all 122 previous definitions in order, and the 78 active writer keys match experiment 1. Keep the existing caps.writers correction. AGENTS.md sections 5 and 7 require a clean full run at the ten-commit limit. Run it on the fix commit, check its full gate, then rerun the item experiment and compare all 78 scores.

## 2026-11-19T16:46:00Z event=M1-08/no-next-step reason=no-next-step session=20261119T164600Z-advice-c13f7c06

Question:

> What compliant Git and ops sequence can put the committed lead exclusion on the active M1-08 worker branch before its last scored attempt? AGENTS.md sections 4 and 9 constrain branch changes and assign shared files to the lead. The worker prompt says not to merge or rebase. The ops worker merge conflict recovery suggests merging gpt/build into an active worker. Evaluate an inbound merge of the lead commit under these rules, or name another supported path. Keep the existing item and commits, the three-attempt limit, the guard and the required merged-tree gate. Use no manually written scoreboard rows.

Evidence:

> M1-08 used two scored attempts. The second completed all 25,393 inputs at 563d8c4. It found 25,392 passes and one oracle memory limit, id fe5a14ba29f4998754b5fd1b11ed3ce1e29ddcd0f7c77a64524cd185a523e059. The AST parser refuses a row until spec/unstable.json lists that id. The report is $RANDOC_RUNS/measure/20261119T111812Z-563d8c438932/ast-report.json. ops worker merge requires a committed worker gain row. The lead owns the shared exclusion file and can commit it on gpt/build after the clean reader runs. The worker has one scored attempt left.

Session: 01a1f4b3-7416-73c0-aaa9-cec207704d4f

Answer (summary): The lead can merge the committed exclusion into the idle worker branch, then use attempt three for the worker gain. The required merged-tree check records one further attempt after that gain.

Answer (full): /Volumes/Extreme Pro/Randoc/broker-out/20261119T164600Z-advice-c13f7c06.json

Decision: Use the lead merge described by ops worker merge recovery. Both worktrees use the configured Git guard. After the clean M1-05 runs and scoreboard commit, record the verified AST oracle memory limit on gpt/build. Pin the lead commit and merge it into the idle active worker as lead. Check that the exclusion is identical to the lead copy and that all other worker changes are within the worker scope. Run experiment three on the clean worker tree. A worker gain permits the separate merged-tree measurement and gate required by AGENTS.md section 9 and plan/SPEC.md section 4.1.
