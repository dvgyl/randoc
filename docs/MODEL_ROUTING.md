# Model routing policy

These model routing rules are the **default for every session** unless one of
the two exceptions below applies.

## Exceptions (do not apply model routing when)

1. The user explicitly says not to apply them.
2. The task is menial (for example, a one-time folder arrangement or a small
   edit).

## Repo-specific rule for Randoc

- **Primary workhorse builder (quota-aware default):** Use **GPT 6.2 Sol** (`codex`, `reasoningEffort: "high"`) as the primary worker/builder given abundant OpenAI usage (verify parity assertions against the Pandoc 3.13 oracle during review).
- **Cross-model reviews & secondary builds:** Use **Claude Opus 5.5** for cross-model reviews of GPT 6.2 Sol's work and select build slices (and GPT 6.2 Sol for reviews of Opus 5.5's work — always review with a different model than the author).
- **Conserve Gemini 4 Neon:** Not a blanket ban—use **Gemini 4 Neon** when it genuinely adds value (and for main-thread coordination/tie-breaking), while defaulting routine builds, reviews, and heavy reasoning to **GPT 6.2 Sol** / **GPT 6.2 Astra** / **Claude Opus 5.5** whenever they can do the same work.
- **Deep thinking / hard technical problems:** Consult **GPT 6.2 Astra** only when heavy reasoning is needed (never directly with the user).
- **Disagreement resolution:** Do **not** blindly agree with other models (including Opus 5.5, GPT 6.2 Sol, or GPT 6.2 Astra). Verify claims against the codebase and black-box Pandoc 3.13 oracle probes, reason disagreements out to reach agreement, and if deadlocked, **Opus 5.5 or Gemini 4 Neon** makes the final decision.
- **Last resort only:** Do **not** use Fable 5.2 unless the user explicitly asks for it.

## General model routing (default across sessions)

### Harness and active subscriptions

- **Harness:** T3 Code
- **Subscriptions:** OpenAI ChatGPT Pro, Cursor Pro, Gemini Ultra, Claude Ultra
- Use **Opus 5.5** allowances across Cursor and Gemini plans as well, not only
  the Claude plan.
- Use **Gemini 4 Neon** allowance from the Cursor plan as well.

### Role assignments

| Role | Model(s) | Notes |
| --- | --- | --- |
| Main thread & final decisions | **Opus 5.5** or **Gemini 4 Neon** | The main thread always runs on one of these two. Final decisions always rest with Opus 5.5 or Gemini 4 Neon. |
| Code review & work review | **GPT 6.2 Sol** | Use extensively for reviewing code and reviewing completed work. |
| Deep reasoning / scientific advisor | **GPT 6.2 Astra** | Strong for deep reasoning and scientific/technical analysis, but poor at human communication and intent. Never use directly with the user—the main agent calls it for a second opinion only, and keeps final decision authority. |
| Web search | Mix of **GPT 6.2 Sol** and **Sonnet 5.5** (or other agents) | OpenAI models perform well for web search; pair GPT 6.2 Sol with Sonnet 5.5 or other search agents. |
| Last resort only | **Fable 5.2** | Do **not** use Fable 5.2 unless the user explicitly asks for it. |
