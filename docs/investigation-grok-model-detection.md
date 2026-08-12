# Investigation: grok model detection at v10.27.0

Date: 2026-08-13
Status: root-caused, fix not yet dispatched

## Problem

Question asked: does aid auto-detect grok's newest model?

Answer: no. Worse — the gap between aid's static catalog and grok's live model
list has made **every default `aid run grok` dispatch fail before the agent
starts**.

## Evidence

`grok models` (grok CLI 1.0.3, cache fetched 2026-08-12T23:26Z):

```
Default model: grok-4.6

Available models:
  * grok-4.6 (default)
  - grok-4.5
```

aid's catalog carries exactly one grok row — `src/model_catalog_data.rs:163`:

```rust
AgentModel { agent: AgentKind::Grok, model: "grok-4.5", ..., tier: "unknown" }
```

Reproduced with the installed binary (aid 10.27.0, cdc46cbb), all four budgets:

```
$ aid run grok "say hi" --dry-run --difficulty simple --budget <any> --urgency normal --rigor draft
Error: Agent 'grok' does not serve model 'grok-4.5'. Served models: grok-4.6, -
```

`--model grok-4.6` dispatches normally. The profile gate makes `--budget`
mandatory, and any budget value resolves a catalog model, so there is no
code path that reaches grok without the rejected `grok-4.5`.

## Root cause — two defects that only bite together

### 1. Catalog staleness (the answer to the original question)

There is no auto-detection feeding model *selection*. `served_models()` exists
for grok (`src/agent/grok.rs:98`, shells out to `grok models`) but its only
consumer is `validate_model_for_agent` (`src/cmd/run_dispatch_resolve.rs:276`)
— a veto, never a source. The default and budget models come from the static
`AGENT_MODELS` table, which still says grok-4.5.

### 2. `parse_grok_models_output` drops every non-default model

`src/agent/grok.rs:109`. The parser strips a leading `*` but not the `-` that
marks non-default rows:

```rust
let clean = trimmed.trim_start_matches('*').trim();
let model_name = clean.split_whitespace().next().unwrap_or(clean);
```

`"- grok-4.5"` → `clean = "- grok-4.5"` → `model_name = "-"`.

Verified against real output: parsed list is `["grok-4.6", "-"]`. grok-4.5 *is*
still served; aid just cannot see it, and it emits a literal `-` as a model
name in the user-facing error.

So defect 2 turns defect 1 from "we use a slightly older model" into a hard
dispatch failure.

## Why earlier runs today still worked

Recent grok tasks (`aid board --json`) all show `requested_model = None` with
`observed_model` of `grok-4.6-build` / `grok-4.5-build`. `aid show t-2457f13d
--json` explains it: `budget = False`, every profile field `None` — those
dispatches declared no budget, so no catalog model was resolved, so the
validator was never reached and grok's CLI picked its own default. They ran
under v10.26.0 (v10.27.0 committed 03:10, last good run 02:48).

Two consequences:

- Detection is **observe-only**. `observed_model` correctly recorded
  `grok-4.6-build` — aid can see 4.6 after the fact, it just never selects it.
- Those runs are evidence that deferring to the CLI's own default works fine,
  which is the fallback fix 3 proposes.

## Verified end-to-end

`aid run grok "…" --model grok-4.6` → t-b41ef3e2, done in 13s, agent replied
`grok-4.6`, `observed_model = grok-4.6-build`. Only an explicit `--model` gets
there; nothing in aid chooses 4.6 on its own.

Side finding from the same task: `requested_model` was stored as `None` despite
`--model grok-4.6` on the command line. Worth a separate look — it defeats the
requested-vs-observed split that `project_model_attribution_split` shipped.

## Fix options

1. `src/agent/grok.rs:109` — strip the `-` bullet too (`trim_start_matches(['*', '-'])`)
   or match on the bullet explicitly. Restores grok-4.5 to the served list and
   unblocks dispatch on its own. Smallest change.
2. `src/model_catalog_data.rs:163` — add the `grok-4.6` row and make it the
   default/premium pick, keeping grok-4.5 as the cheap tier. Both stay unpriced
   (`MeteringShape::Unknown`; do not invent rates).
3. Structural: the validator must not treat a user-supplied `--model` and an
   aid-resolved catalog model the same way.
   - User passed `--model X`, X not served → hard error is correct.
   - aid resolved X from its own stale catalog, X not served → erroring is
     self-inflicted. Drop the model, warn, let the CLI pick its default — which
     the pre-v10.27.0 runs above prove works.

   This turns the next staleness event (grok 4.7) from an outage into a
   degradation. The same staleness applies to other agents (see
   `project_model_table_staleness`).

1 + 2 are needed to ship; 3 is small enough to ship with them.

Test requirement for fix 1: the fixture must be verbatim current `grok models`
output (login header, `* grok-4.6 (default)`, `- grok-4.5`) asserting
`["grok-4.6", "grok-4.5"]`. While there, constrain the parser to bullet lines —
today it swallows any footer text after `Available models:` as a model name.
