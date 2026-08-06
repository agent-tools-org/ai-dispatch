# Declared task profile + agent advise API

Status: design, approved for implementation (2026-08-05)

## The project assumption this encodes

The dispatching caller is the highest-quality model in the system. It already knows how hard a
task is, how much it is worth spending, how fast the answer is needed, and how correct it has to
be. `aid` does not know any of that and cannot infer it reliably — so `aid` must stop guessing,
require the caller to declare it, and spend its own effort on giving the caller a complete,
current picture of what the fleet can do.

Concretely this reverses today's flow. `aid run auto` guesses the task profile from prompt text
and then picks an agent silently. It should instead accept a declared profile, expose its
inventory and its recommendation, and let the caller decide.

## Problem

Two failures follow from guessing.

1. **Misrouted large tasks.** `classifier::classify` derives complexity from prompt length and
   keywords (`is_simple_for_routing`: ≤200 chars, ≤35 words). A large refactor described in one
   tight sentence classifies as `simple_edit`, where `opencode` scores 8 and `codex` scores 4.
   The scoring matrix is not at fault; the label fed into it is.
2. **Invisible inventory.** The full routing computation — category, complexity, per-category
   success rates, average cost, rate-limit penalties, team preferences — is reachable only by
   dispatching. Only one human-readable `reason` string escapes. Measured over 30 days in this
   repo: `codex` took 78% of 1374 tasks at 9m49s average, while `agy` ran 84% success / 4m31s /
   free on 5%. The caller routes by hand because it has nothing to route with.

A third, narrower gap: the only quota signal is a marker file written *after* an agent returns a
429 (`src/rate_limit.rs:16`), and `aid agent quota` just echoes it.

## Goals

1. Make the task profile **declared, not inferred**, and persist it.
2. Make the agent inventory + live state machine-readable.
3. Make the routing recommendation queryable **without dispatching**.
4. Delete `auto` — silent routing has no place once the caller can ask.

## Non-goals

- Changing the capability matrix or the scoring arithmetic.
- Network quota probing (separate aidbar-integration track).
- Blocking dispatch on a missing profile by default.

## Surface 0 — the declared task profile

Four dimensions, on `aid run`, `aid advise`, and batch TOML (`[defaults]` and `[[task]]`).
`kind` stays inferred: keyword matching for research/refactor/frontend is reliable, unlike the
length-based complexity guess. The caller may override it with `--kind`.

| Flag | Values | Default | What aid does with it |
|---|---|---|---|
| `--difficulty` | `trivial` `simple` `moderate` `complex` | `moderate` | Floor on required capability score |
| `--budget` | `free` `cheap` `standard` `premium` | `standard` | Eligible agents and model tier; `free` admits only free agents/models |
| `--urgency` | `background` `normal` `urgent` | `normal` | Rate-limit policy: `background` waits for reset, `urgent` switches agent immediately |
| `--rigor` | `draft` `standard` `critical` | `standard` | `critical` requires `--verify` and a cross-audit; does not whitelist agents |
| `--egress` | `any` `local` | `any` | `local` admits only a provider whose established endpoint is loopback |

The `--urgency` row is the answer to "codex quota is tight, what now?" — that call belongs to the
caller, not to aid.

Enforcement:

- `aid advise` **requires** all four. It is a new command; there is no legacy call site.
- `aid run` warns once per invocation when any is missing and records `declared: null` for it.
- `.aid/project.toml` may set `require_task_profile = true` to make it a hard error. The
  `production` profile sets it.

Persistence: four nullable columns on `tasks` (migration in `src/store/migrations.rs`), written
at dispatch. This is the point of the whole change — history keyed on a *declared* difficulty is
trustworthy, where history keyed on a guessed category is not. `aid stats` gains a
declared-vs-outcome view so systematic under-declaration is visible (declared `simple`, ran 40
minutes, failed verify twice) rather than silently degrading routing.

## Surface 1 — `aid agent list --json` / `aid agent show <name> --json`

No new top-level command; extend `AgentCommands::{List, Show}` (`src/cli/sub_enums.rs:7`) with
`--json`. Text output is unchanged.

```json
{
  "generated_at": "2026-08-05T14:02:11+08:00",
  "agents": [
    {
      "name": "codex",
      "kind": "builtin",
      "installed": true,
      "disabled": false,
      "trust_tier": "third-party",
      "description": "Complex implementation, multi-file refactors",
      "supports_session_resume": true,
      "quota": {
        "state": "limited",
        "recovery_at": "2026-08-05T18:27:00+08:00",
        "message": "You have hit your usage limit...",
        "source": "marker"
      },
      "capabilities": { "complex_impl": 9, "refactoring": 8, "simple_edit": 4, "research": 1 },
      "models": {
        "default": null,
        "budget": "gpt-5.4-mini",
        "available": [
          { "model": "gpt-5.5", "tier": "paid", "input_per_m": 1.25, "output_per_m": 10.0 }
        ]
      },
      "history": {
        "window_days": 30,
        "tasks": 1374,
        "success_rate": 0.78,
        "avg_duration_secs": 589,
        "avg_cost_usd": 20.16,
        "by_category": {
          "simple_edit": { "tasks": 210, "success_rate": 0.83, "avg_duration_secs": 402 }
        }
      },
      "load": { "running": 2 }
    }
  ]
}
```

Rules:

- `quota.state` is `ok` | `limited`, from `rate_limit::is_rate_limited`. `source` is `marker`
  today; the field exists so a future probe source is distinguishable.
- `capabilities` keys are `TaskCategory::label()` values — no new naming scheme.
- History below the existing `HAVING total >= 5` floor is omitted. Absent data is `null`, never
  a fabricated zero.
- `load.running` counts this agent's running tasks, from the store.
- Custom agents appear as `"kind": "custom"` with whatever subset applies.
- `aid agent show <name> --json` emits one agent object, same schema.

## Surface 2 — `aid advise`

Read-only. Runs the real selector against the **declared** profile and prints what it would pick.
Dispatches nothing, writes nothing.

```
aid advise "<prompt>" --difficulty <d> --budget <b> --urgency <u> --rigor <r>
           [--kind <k>] [--team <id>] [--top <N>] [--json] [--dir <path>]
```

Human output:

```
Declared: complex / premium / urgent / critical   (kind: refactoring, inferred)
Recommended: codex/gpt-5.5   score 13.2   ~$18.40  ~10m
  1. codex     13.2  base 8.0  +model 2.0  +history 1.2  +complexity 2.0
  2. droid     11.4  base 8.0  +model 1.4  +history 0.0  +complexity 2.0
  3. agy        4.4  base 3.0  +model 1.4  +history 0.0   [budget: free tier]
Notes: urgent + codex limited until 18:27 → switch to droid
```

JSON output:

```json
{
  "declared": { "difficulty": "complex", "budget": "premium",
                "urgency": "urgent", "rigor": "critical" },
  "inferred": { "kind": "refactoring", "file_mentions": 3, "chars": 412 },
  "recommended": {
    "agent": "codex", "model": "gpt-5.5", "score": 13.2,
    "est_cost_usd": 18.40, "est_duration_secs": 600,
    "reason": "complex/refactoring → codex/gpt-5.5 (score: 13.2)"
  },
  "candidates": [
    {
      "agent": "codex", "installed": true, "eligible": true, "score": 13.2,
      "breakdown": { "base": 8.0, "model_capability": 2.0, "budget_penalty": 0.0,
                     "rate_limit_penalty": 0.0, "history_bonus": 1.2,
                     "complexity_bonus": 2.0, "team_bonus": 0.0, "total": 13.2 }
    }
  ],
  "notes": ["codex rate-limited until 2026-08-05T18:27:00+08:00"]
}
```

Rules:

- `candidates` covers every enabled built-in plus eligible custom agents, sorted by the existing
  `compare_candidates` ordering, truncated to `--top` (default 5, `0` = all). Not-installed and
  budget-excluded agents are listed with `"installed"` / `"eligible"` false rather than dropped —
  the caller must be able to see what it is missing.
- `est_cost_usd` / `est_duration_secs` come from history; `null` when there is no sample.
- Exit 0 whenever advice was produced, including when every agent is rate-limited.

### Behavior-preserving refactor (the one correctness risk)

`selection_scoring::score_for` returns `f64` today. Split it:

```rust
pub(super) struct ScoreBreakdown { base, model_capability, budget_penalty,
                                   rate_limit_penalty, history_bonus,
                                   complexity_bonus, team_bonus, total }
pub(super) fn score_breakdown(ctx: &CandidateContext<'_>, kind: AgentKind) -> ScoreBreakdown
pub(super) fn score_for(ctx, kind) -> f64 { score_breakdown(ctx, kind).total }
```

`total` must be the same arithmetic in the same order as today. Floating-point addition is not
associative; a reordered sum can flip a tie and silently change routing. Required test: for a
fixed prompt/context set, `score_for` returns bit-identical values before and after.

## Surface 3 — MCP tools and session-start snapshot

- Register `aid_agents` and `aid_advise` in `src/cmd/mcp_tools.rs:28` beside the existing seven,
  returning the payloads above. `aid_advise` requires the four declared dimensions.
- `aid hook session-start` gains one compact line when any agent is not OK, e.g.
  `agents: codex LIMITED (resets 18:27) · agy ok · opencode ok`. Silent when all OK.

## Surface 4 — delete `auto`

`auto` exists only to guess what the caller can now declare. Remove it outright — no deprecation
shim, no alias.

- `aid run auto` and batch `agent = "auto"` / empty `agent` become hard errors naming `aid advise`
  as the replacement.
- Keep the scoring engine: `aid advise` needs it, and `--best-of N` selects its fleet through
  `budget_ranked_agents` (`src/cmd/run_bestof.rs:189`).
- `coding_fallback_for` ranks installed peers by the capability matrix for the
  task category and skips rate-limited / disabled / known-unhealthy agents
  (gemini when agy is present). It is not a second scoring engine.
- Update `CLAUDE.md`, team docs, and `default-skills/aid-guide/` in the same commit.

## Invariants

- `aid advise` and `aid agent list --json` never dispatch, never write to the store, never mutate
  rate-limit markers.
- JSON keys are stable; fields may be added, never renamed or retyped.
- JSON goes to stdout alone — no `[aid]` lines interleaved.

## Tests

- Serde round-trip snapshot for both payloads (not string equality).
- `score_for` bit-identical regression test.
- `aid advise` with every agent rate-limited still exits 0 and shows the penalty.
- `aid run auto` errors with a message naming `aid advise`.
- Declared dimensions round-trip through the store and appear in `aid show --json`.
- MCP registration test asserting both tools appear in `tool_definitions()`.
- `aid_guide_e2e` / `init_e2e` stay green; `references/command-index.md` lists the new surfaces.

## Out of scope (tracked separately)

- Network quota probes feeding `quota.source = "probe"` (aidbar integration).
- The `aid run` pre-dispatch guard beyond rate-limit auto-cascade.
- Learning loop that tunes the capability matrix from declared-vs-outcome drift.

---

## Revisions from the 2026-08-05 implementation round

Four assumptions in the sections above were falsified by building the thing. They are corrected
here rather than edited in place, so the reasoning that produced them stays visible.

### 1. "The classifier is reliable for `kind`" — false

Surface 0 kept `kind` inferred on the argument that keyword matching for research/refactor/frontend
is dependable, unlike the length-based complexity guess. The first real invocation inferred
`kind: research` for `add a null check to the parser`. Inference is unreliable on both axes.

`--kind` is therefore a first-class declared flag, and an inferred kind must never be the dominant
term in a recommendation. Report it as a hint with its source (`inferred` vs `declared`), which the
shipped output already does.

### 2. "Verification is the compensation cheap agents need" — false

Both tiers shipped non-compiling code on their first delivery. The expensive tier did it *after*
its own `aid build check` exited 101, and reported DONE anyway. Tier does not predict whether an
agent verifies its work.

Verification is the dispatcher's, unconditionally. `--rigor` does not decide *whether* to verify;
it decides *what proof is owed*:

| rigor | proof the dispatcher requires |
|---|---|
| `draft` | compiles |
| `standard` | compiles + the changed path executed, real output captured |
| `critical` | end-to-end dispatch + independent cross-audit |

aid should inject the corresponding proof requirement into the brief, so the requirement scales
with the declared rigor instead of depending on the dispatcher remembering it. Every round this
session that tightened the requirement caught the agent taking the cheapest compliant path one
level down: "verify" → skipped the build; "paste the build line" → built but never ran; "paste real
output" → pasted a unit test. The requirement must name the artifact, not the activity.

### 3. "`--rigor critical` should restrict trust tier" — false

The shipped rule is `eligible = base >= difficulty.capability_floor() && budget_allows(...) &&
trust_allows_builtin(kind, rigor)`, with `complex` requiring base ≥ 8 and `critical` admitting only
`local`-tier agents. On the refactoring category that leaves exactly one eligible agent: codex. On
the day codex ran out of quota, `aid advise` for a complex/critical task offered no alternative —
failing precisely in the scenario that motivated the feature.

It is also empirically backwards. codex (local, top tier) shipped non-compiling code twice; cursor
(api tier, ineligible for `critical` under this rule) produced the best-evidenced delivery of the
session and the cross-audit that blocked a bad merge. Trust tier did not predict delivery quality;
proof level did.

Required changes:
- Eligibility becomes a penalty, not a gate. A hard binary must not be derived from hand-authored
  capability integers where 7 vs 8 is not an evidenced difference.
- Whenever the eligible set is empty or a single agent, the output must still surface the best
  remaining options with the reason each was excluded (`cursor: base 6 < floor 8 for complex`).
  The caller decides; that is the premise of the whole design.
- `--rigor` drives the proof table in §2, not agent whitelisting.

### 4. "Completion status lives in each adapter's `parse_completion`" — false

`watch_streaming` (`src/watcher.rs`) and `finalize_streaming` (`src/pty_watch.rs`) set final status
from the exit code alone and never call `Agent::parse_completion`; only the buffered paths call it.
For every `streaming() -> true` adapter — which is all of them — that function is dead code. A CLI
that exits 0 while reporting an API error records as **Done**. qwen does this on a 403.

Established by cross-audit `t-799e30a3` (verdict BLOCK) against a branch that "fixed" the status
logic in eight adapters' `parse_completion`: 287 lines that would never execute, plus detectors
keyed to invented output shapes (real cursor/claude success ends `{"type":"result","is_error":false}`,
while the detectors matched only `type == "error"`; gemini's rule failed any line carrying a
top-level `message`, which ordinary skill/hook events do).

The fix belongs in the streaming finalize path, keyed to each CLI's real result envelope, covered by
an integration test in both directions: exit 0 + error envelope → Failed, exit 0 + real success log
→ Done.

This matters beyond one bug: success-rate history is fleet-wide corrupted by exit-0 failures
recorded as successes, and that history is exactly what `aid advise` weights its recommendations
with. The advice surface is only as honest as the outcome data feeding it.

---

## The dimension we got wrong: CLI is a provider, the model does the work

Established 2026-08-05, after a day of fixes that were all patches on the same misconception.

### The mistake

aid treats an "agent" as the unit of capability, cost, quota and history. It is none of those.
An agent is a **CLI** — a gateway. The **model** is what performs the task.

Captured the same day:

| CLI | Models it actually serves |
|---|---|
| `agy` | 8 gemini-\*, 2 claude-\*, 1 gpt-oss-\* — three vendors |
| `qwen` | 17, incl. qwen3.8-max, deepseek-v4-pro, kimi-k2.7-code, glm-5.2, MiniMax-M2.5 |
| `opencode` | 177 |
| `kilo` | 432 |
| `cursor` | ~195 |

Everything keyed on the CLI is therefore an average over things that are not alike:

- **Capability.** `agy` scores `research 9 / complex_impl 3`, yet it can run
  `claude-opus-4-6-thinking` or `gemini-3.6-flash-low`. One score cannot describe both.
- **History.** `agy`'s "84% success" blends Claude Opus outcomes with Gemini Flash outcomes. The
  number describes nothing that exists.
- **Cost.** Priced per model; recorded per agent.
- **Quota.** Metered per model family. agy's gemini allowance was exhausted while its claude
  allowance still served — and marking the CLI took the working one out with it.

The concrete loss, observed: a task dispatched to `agy --model gemini-3.6-flash-low` hit the gemini
quota, aid marked the whole CLI unavailable, cascaded to a different CLI with different models, and
failed there — while `claude-opus-4-6-thinking` sat available behind the CLI it had just abandoned.

### The correction

Two tables and a reachability relation:

- **Model** — capability by category, price, context window, quota group. Decides *what the task
  needs*.
- **CLI** — tool use, sandboxing, session resume, streaming fidelity, observed reliability. Decides
  *how the work gets done*.
- **Reachability** — which CLI can reach which model, and the current quota state of that pair.

Routing becomes two steps: the declared profile selects a **model class**, then reachability selects
the CLI that can serve it right now. `aid advise` should recommend an (model, CLI) pair and say
which dimension drove the choice.

This makes a capability we cannot currently express: **the same model is often reachable more than
one way.** When codex's quota ran out, the useful question was not "which other CLI" but "what else
reaches a model of this class" — and `agy`'s claude family was a valid answer that nothing in the
data model could surface.

### Prerequisite

**Carry the model through derived dispatches.** An earlier draft of this section claimed aid never
persists the dispatched model. That was wrong, and the error is worth keeping visible: the task I
inspected was a *cascade child*, and I attributed its empty model to its parent. Direct dispatches do
record it — `t-bd455a68` stored `gemini-3.6-flash-low`, `t-efd78b6f` stored `MiniMax-M2.5`.

The real gap is narrower: **tasks derived from another task lose the model.** The cascade child
`t-c9a80dbf` stored `model: None`. Retries, cascades and best-of children all construct fresh args,
and the model is not among the fields they inherit. Anything model-dimensioned — per-family quota
marking, cost attribution, model-level history — silently degrades to "unknown" exactly on the paths
aid takes when something has already gone wrong, which is when the record matters most.

Measured rather than assumed (6643 task rows, 2026-08-05):

| Slice | Rows | Model missing |
|---|---|---|
| derived (cascade / retry / best-of) | 582 | 92.6% |
| direct | 6061 | 84.6% |

Per agent, today only: cursor 0/18 missing, claude 0/1, qwen 4/14, agy 14/17, codex 61/65,
opencode 13/13. The spread is not explained by time or by derivation — it is per adapter. The model
is stored when the CLI echoes it in its output, or when the caller passed `--model` explicitly.
**A model aid resolved itself — a budget model, a smart-routing choice, an agent default — is not
recorded at all**, and adapters whose CLI stays silent about the model lose it entirely.

So this is the `attribution` column of the CLI audit matrix, not a store bug: the same gap the
gemini-family audit flagged for agy under plain-text output. Two fixes are needed and they are
independent — record what aid passed at dispatch, and teach each adapter to read the model its CLI
reports.

### Status

`src/agent/model_group.rs` (2026-08-05) implements per-family quota for agy. It is a special case of
this design, added before the general shape was clear, and should fold into the model table rather
than grow more per-CLI special cases.

---

## Three dimensions, not two: CLI × provider × model

The section above corrected "agent" into CLI + model. That was still one short. An execution route
is identified by three independent things, and aid currently collapses all of them into a single
opaque agent id:

    opencode / byok / deepseek-v4-flash
    └ CLI      └ provider   └ model

| Dimension | Owns |
|---|---|
| **CLI** | How to invoke: flags, output format, event shapes, session resume, sandboxing, read-only mode |
| **Provider** | Who meters and bills: the quota pool and its reset semantics, credentials, base URL, cost basis |
| **Model** | What the work is done by: capability per category, context window, per-token price |

### Why the provider dimension is not optional

Quota — the thing that broke routing repeatedly on 2026-08-05 — belongs to the provider, and the
three shapes observed cannot be expressed by a per-CLI marker:

| Provider | Metering |
|---|---|
| qwen / ModelStudio token plan | one 5-hour pool shared by all 17 served models |
| agy / individual tier | separate pools per model family: gemini exhausted at 59m while claude still served |
| opencode / BYOK | no pool at all — billed per token against the user's own key |
| cursor / subscription | not metered per task, but model tiers differ in what they cost the plan |

`rate-limit-<agent>` cannot represent any of that faithfully. The per-family markers added today
(`rate-limit-agy--gemini`) are a special case of the provider dimension, reached by patching rather
than by modelling.

### aid already has the data

`examples/byok/mimo.toml` carries all three — `base_url` and `key_env` (provider),
`default_model` (model), and a note that it "configures opencode and generates an aid agent" (CLI).
The manifest then collapses them into `id = "mimo"`. Custom agents do the same: `glm5` is really
`bash-wrapper / NVIDIA NIM / z-ai/glm5`, and the name says none of it. Nothing in the identity
survives to tell a scheduler what would still work when one dimension fails.

### What changes

- **Identity**: a route is `<cli>/<provider>/<model>`. Existing names stay as aliases so `aid run
  glm5` keeps working, but they resolve to a triple.
- **Quota**: keyed on the provider pool, with an optional family within it. One exhausted pool must
  not remove routes that draw on a different one.
- **Cost**: model price × provider basis. A subscription route and a BYOK route to the same model
  cost differently and must not be averaged.
- **Capability and history**: keyed on the model. Today `agy` carries one capability score across
  Gemini Flash and Claude Opus, and `cursor` carries one across 193 models including Opus 5 and
  GPT-5.6.
- **Routing**: the declared profile picks a model class; reachability then picks a (CLI, provider)
  pair that can serve it right now. When codex's quota ran out, `claude-opus-5-thinking-high` was
  reachable through cursor the whole time and nothing in the data model could say so.
