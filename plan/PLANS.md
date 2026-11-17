# ExecPlans: the rules and the form

An ExecPlan is a design and progress document for one milestone. It is the
memory of the work: a new session that has only the working tree and the
ExecPlan must be able to continue the work without other help. Context
compaction and restarts remove details from a session, so the ExecPlan must
hold them.

## Rules

1. **Self-contained.** The ExecPlan holds all knowledge that the work needs:
   the current state, the file paths, the commands, the definitions. It does
   not depend on an earlier session or on a document outside the
   repository. It may point to `AGENTS.md`, `plan/SPEC.md`,
   `plan/ROADMAP.md` and `plan/ARCHITECTURE.md`, which are in the repository.
2. **Defined terms.** Each special term gets a plain definition at its first
   use, or is not used.
3. **Purpose first.** The plan starts with what a user of Randoc can do after
   the milestone, and how to see it work.
4. **Measured result.** The plan ends in behaviour that a command shows, and
   in scoreboard numbers. Code that compiles is not a result.
5. **Living document.** Update the plan at each stop point. Each revision
   stays self-contained. At the end, add a short note that says what
   changed in this revision and why.
6. **Items.** Each step that changes product code names its work item
   (`M<n>-<nn>`), its metric, and its expected gain. The item rules of
   `AGENTS.md` (3 experiments, cut and revert) apply.
7. **Prose first.** Write the plan as prose. Use lists only where the form
   below asks for them. Show commands and short outputs as indented blocks
   (four spaces), not as fenced blocks.
8. **Prototypes.** A prototype step is allowed when it lowers a risk. Name
   it as a prototype, and state how you decide to keep or discard it.
9. **No schedule.** A UTC time stamp records a past event: a done step, a
   decision, a revision. Never write a date or a duration of future work.
10. **Startup order.** A session first does the action that
    `tools/autobuild/ops status` names, then the first open step of
    `Progress`. Keep the open steps in the order of the work, so that the
    two agree.

## The form

The file `plan/exec/M<n>.md` has these sections, in this order, with these
exact headings.

    # <Short action title of the milestone>

One paragraph: this is a living document; the sections `Progress`,
`Surprises & Discoveries`, `Decision Log` and `Outcomes & Retrospective` are
kept current; the rules are in `plan/PLANS.md`.

    ## Purpose / Big Picture

What a Randoc user gains, and how to see it work.

    ## Progress

A checkbox list of small steps. Each entry has a UTC time stamp, for example
`- [x] (2026-11-20 14:05Z) Item M1-03 done: harness.units 59513/61366 -> 59530/61366.`
Split a partly done step into a done part and a remaining part. This section
always shows the real current state.

    ## Surprises & Discoveries

Entries of the form `- Observation: ...` followed by `Evidence: ...` (a
command output is the best evidence).

    ## Decision Log

Entries of the form `- Decision: ...`, `Rationale: ...`, `Date: ...` (the
UTC date when the decision was made). Each decision and each change of
direction.

    ## Outcomes & Retrospective

At each milestone gate and at the end: the results compared with the
purpose, the gaps, and the lessons.

    ## Context and Orientation

The current state for a reader who knows nothing: the key files by full
path, the commands, the definitions.

    ## Plan of Work

The sequence of changes as prose: for each step, the file, the place in it
(function, module) and the change, with its item id and metric.

    ## Concrete Steps

The exact commands, the folder to run them in, and short expected outputs.
Update them while the work goes on.

    ## Validation and Acceptance

How to see that the milestone works: the commands, the expected scoreboard
numbers, the tests that fail before and pass after, and the milestone gate
command `tools/autobuild/ops milestone check M<n>`.

    ## Idempotence and Recovery

Which steps are safe to run again; how to retry or revert a risky step.

    ## Artifacts and Notes

Short outputs, diffs or snippets that prove a result, as indented blocks.

    ## Interfaces and Dependencies

The types, traits, functions, modules and libraries that must exist at the
end of the milestone, with their signatures.
