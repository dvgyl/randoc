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
