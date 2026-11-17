# Advice

One section for each question to the adviser model. `tools/autobuild/ops ask`
writes each section. Append only.

Section heading: `## UTC event=ITEM/EXPERIMENTS reason=KIND session=ID`.
A stuck event is one item at one number of experiments. It gets one question.
The section has the question, a summary of the answer, the path of the full
answer, and the decision of the lead (`ops ask --decision "TEXT"`).
