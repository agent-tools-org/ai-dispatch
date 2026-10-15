# Routing and Context

## Preview routing without dispatch

```bash
aid advise "Refactor the scheduler" \
  --difficulty complex --budget premium --urgency urgent --rigor critical \
  --kind refactoring --top 5 --json
```

`aid advise` requires all four declared dimensions. It reads the live inventory,
rate-limit markers, aidbar disk snapshots, team preferences, and task history,
then runs the production selector without launching an agent or writing the task store.
Fresh live used-percent ranks remaining headroom (a penalty as the window
fills; unused quota never boosts). Held routes still take today's −10 when
urgency is not `background`. Quota penalties and candidate quota use the resolved
model’s metered group; a hold on another group does not penalize that route. Use `--top 0` for all candidates, `--team` for
team preferences, and omit `--json` for a concise human-readable breakdown
(including a headroom term). JSON candidates add a `quota` object (`status`,
`wall`, `used_percent`, `resets_at`, `freshness_secs`, `stale`, `source`)
without renaming existing keys. `quota.status` is `unknown` when no successful
probe observed the quota (`source: "none"`, a failed probe, or an expired
marker); it is never reported as dispatchable-by-evidence without one.

Eligibility uses the same "can this route run" predicate as `aid agent list`
and the `aid run` preflight: a candidate whose binary is missing from `PATH` is
`installed: false, eligible: false` with reason
`not installed: binary '<bin>' missing from PATH`. The binary is the one the
adapter would spawn: Cursor counts an `agent` on `PATH` only when it identifies
as Cursor, else it needs `cursor-agent`. Every ineligible candidate
carries `exclusion_reason` (human text, `; `-joined) and `exclusion_codes`
(one stable code per reason):

| Code | Reason text |
|---|---|
| `not_installed` | `not installed: binary '<bin>' missing from PATH` |
| `below_floor` | `base 6 < floor 8 for complex` (team override or rated model capability below the floor) |
| `no_budget_model` | `no model for budget <budget>` |
| `auth_failed` | `auth failed (observed <time>)` |
| `weaker_on_caller_pool` | `weaker model on caller's pool` |

Candidates rank eligible first, then eligible-but-demoted, then ineligible;
ineligible alternatives still appear with their reasons. The recommendation is
the first eligible installed route permitted for recommendation; fallback routes
must also be installed, enabled, and outside weaker-caller-pool exclusions. When
none remain, `recommended` is `null` and the run hint stays silent. Claude stays listed and requires team preference for advice. Advice has no
declared-agent field: explicit `aid run claude` selects Claude for execution
without changing the hint’s advice profile. Each candidate also carries `auth`: `state` is
`failed` (with `observed_at` and `message`) when a run of that agent ended on a
recognised not-signed-in refusal within the last hour (grok `Not signed in`,
claude `Please run /login` / `Not logged in`, oz `credentials are invalid`),
otherwise `unknown`. A successful run clears it; aid never reports `ok` and
never marks auth failed from a run or probe that could not start.

Leaderboard evidence: `aid advise` (text and JSON) shows each candidate's
`capability_evidence`: `canonical_id`, all raw `scores`, the `selected` score,
`capability` (0–10 or `null`), and `harness` (`measured` or `harness unmeasured`).
Each score preserves `source`, `board`, `cli`, `effort`, `value`, `unit`, `rank`,
`of`, `ci_low`, `ci_high`, and `date`; missing effort/rank/CI stays `null`.
The report's `sources` block preserves each source's `ok`, `count`, `updated_at`,
`url`, `licence`, `attribution`, and optional `error` (`null` when absent).
Custom candidates also expose raw evidence; their configured scoring scale stays separate.

Scoring chooses one source, never a blend: simple-edit/complex-impl/debugging/
testing/refactoring use Terminal-Bench 4.0 accuracy for the exact canonical model,
CLI, and configured effort, falling back to Epoch ECI. Frontend uses LMArena
webdev, preferring the configured effort when available, otherwise the highest
raw webdev value; research/documentation use Epoch ECI. Price-feed exact IDs and aliases
provide the canonical mapping; there is no fuzzy model matching. Terminal-Bench
results from another CLI or effort do not rate this harness: fallback evidence
is labelled `harness unmeasured`. Codex effort comes from `model_reasoning_effort`
in its configured home; Claude uses `CLAUDE_CODE_EFFORT_LEVEL` then `effortLevel`
in its user settings. Unobserved effort matches only an effort-less benchmark row.
No applicable data (including a failed source) is unknown, never a fabricated zero.

The sole linear rescale is `10 * (value - min) / (max - min)`: Terminal-Bench
accuracy uses its 0–100 percent (or 0–1 rate) range; ECI and webdev use the
observed model-level min/max of their own source, board, and unit in the snapshot
(including effort-tagged webdev rows).
A degenerate range is unknown. A measured minimum may legitimately score zero.
Free/cheap budget selection keeps the lowest-price rule and catalog order for
price ties; standard/premium
compare model-level ECI within each preferred tier, with unknown ties keeping catalog order.
`breakdown.complexity_bonus` was removed; no CLI gets an automatic +2 for complexity.
`scores.json` and `prices.json` share the aid home directory and 24-hour TTL.
Refresh starts a detached download that survives a short command's exit and
terminal signals. A `.part` file younger than 60 seconds suppresses another
download. The next refresh check validates the completed `.ready` file and
atomically replaces the cache; malformed, empty, or server-stale downloads are
deleted and preserve the old cache, which remains usable offline. Failed downloads
also preserve the cache. Without a valid scores cache, evidence is unknown.

Caller pool: advise reads the calling session (`AID_CALLER_KIND`, Claude Code,
Codex; see `aid board --mine`) and the caller's own model from
`--caller-model <model>` (wins) or `AID_CALLER_MODEL`. Claude Code maps to the
`anthropic` pool and Codex to `openai-chatgpt-plan`. The JSON report adds
`caller` (`session`, `agent`, `provider`, `model`, `capability`). A candidate on
the same pool whose recommended model has a lower leaderboard capability than the
caller's model is excluded as `weaker model on caller's pool`. When either
capability is unknown, same-pool candidates stay eligible but carry
`demotion_reason` and rank below every eligible other-pool candidate. This is
advice only: an explicit `aid run <agent>` is never blocked by it. Custom agents are
reported separately because their configured capability values are not on the
built-in score scale. A candidate's `model`, `pinned`, and `source` come from
the same resolver `aid run` uses for that declared profile, so `model` is what
`aid run` would launch. For a declared `free` or `cheap` budget it is the
catalog budget model (`source: "budget_route"`, `pinned: true`). A codex
`model = "gpt-6-sol"` in its config makes the candidate `gpt-6-sol` with
`pinned: false` and `source: "cli_config"`. With no readable CLI default,
`model` is `null`, `source` is `agent_default`, and the human output says
`agent default (unknown)`; advise never names a catalog model it will not
launch. A built-in candidate's base is its team override, else its rated model
catalog capability, else a neutral `6.0`. Model capability is the base, not an
averaged adjustment. Without either source of evidence, the floor does not
exclude it and report `notes` includes `unrated: no measured or model capability
for <category>`. Custom agents keep their configured capabilities and floor.
The caller-pool comparison treats unknown model capability as unknown.
When a candidate's model is older
than a served-only model of the same family (for example catalog `gpt-5.6-sol`,
served `gpt-6-sol`), the candidate carries `unrated_served_models` (omitted
when empty) and the human output adds one `note:` line. Those models stay
unrated and are not selected. Inferred kind is advisory; pass `--kind` when the
caller knows the task kind. Advice exits successfully even when every agent
is rate-limited. Advise does not spawn `aidbar`.

For `free` and `cheap` profiles, `breakdown.budget_penalty` is `-3.0` only
when the candidate's resolved model price has a positive input or output rate.
Scoring uses the same price resolution as cost estimation: exact agent/model
override (case-insensitive), subscription inclusion, catalog figure, then an
exact vendor-CLI feed entry. Unknown prices and included subscription routes
have no paid-model penalty, even when the catalog lists API-looking rates.
A zero override removes the penalty from a paid route; a positive override
adds it to a free, subscription, or previously unknown route. Vendor feed
rates do not price reseller routes. `standard` and `premium` profiles have no
paid-model penalty. These price rules do not change model eligibility or the
other score terms.

`aid run` hints use this ranker with the declared profile and task kind. Missing
values default to `moderate` / `standard` / `normal` / `standard` and inferred kind;
keywords never choose budget. Hints stay silent for prompts under 20 characters,
`--no-hint`, or a recommendation matching the chosen agent.

`aid run auto` and batch `agent = "auto"` (or an empty agent) are hard errors.
There is no silent routing shim: declare a task profile, run `aid advise`, then
dispatch an explicit agent.

## Context and instructions

Use `--context <path>...` for source material, not extra positional arguments.

The task brief is always delivered whole, including retry feedback before the
original task on session resume. Shared context, file context, findings, peer
milestones, parent/sibling summaries (including file lists), prior/shared output,
methodology, memories, project/team context, toolbox instructions, and batch
sibling previews are bounded independently with in-place omission notes. When prompt
composition drops auxiliary content through byte bounds or entry caps, `aid run` prints one `Prompt context bounded:`
warning and records a setup event with the omitted sections. `aid show --context`
and ShareGPT export use the stored resolved prompt without reconstruction. For
legacy tasks without it, show reports that launch context was not stored and
ShareGPT uses the raw caller prompt. Large task briefs may
exceed an agent's context window; aid does not shorten them to fit.
Retries prepend new feedback to the failed task's own raw caller brief, preserving
all existing correction, checklist, and iteration blocks. Missing-delivery
recovery does the same, including when a missing Codex rollout starts a fresh session. Parent linkage never
replaces that brief. Background cascades reuse the raw brief and assemble fresh
launch context; workers launch the separately stored resolved prompt.
A template without `{{prompt}}` gets the caller brief appended after it.
OpenCode, Kilo and MiMoCode take plain `--context` file paths as native file
arguments, which the worker does not pass, so those files do not reach them.
On-demand memory search stops at the memory cap per query; matches beyond it
are not counted as omitted.

Use `--context-from <task>...` to inject prior task output. If that task
declared `-o` and the owned file is missing, aid reports the absence and does
not substitute the task log. Use `--scope` to state intended files. Use
`--checklist` or `--checklist-file` for explicit acceptance criteria.

Prefer a project verify command for consistent results:

```toml
[project]
id = "example"
verify = "cargo test --bin app"
```

## Automatic route holds

Ordinary quota holds may remain advisory recommendations, but held routes are
never launchable through advised cascades or automatic fallback. A needs-human
limit marker carrying a payment, authentication, or recognized human-action
refusal is excluded from recommendations as well (`needs_human`). A generic
manual hold without such refusal evidence retains ordinary advisory behavior.
Background urgency can remove the numeric limit penalty; it does not make a
needs-human refusal eligible.

## Subscription billing state

Fresh successful aidbar snapshots may include `billing.state`: `in_plan`,
`overage`, or `exhausted`, with optional `credits.balance` and `credits.unlimited`.
Balances retain provider credit units. Absent, stale, or failed billing evidence
is unknown and preserves existing routing behavior. Malformed billing fields do
not discard the quota snapshot or prevent a fresh probe from releasing a hold.
Unknown billing adds no text or score term to candidate lines; JSON keeps its
additive billing fields.

Overage routes are excluded from automatic selection for `free` and `cheap`
budgets. A `standard` budget applies a separate −4.0 `billing` score term;
`premium` or `critical` rigor removes that penalty. Critical rigor does not
permit paid overage on a free or cheap budget. Exhausted routes are excluded
regardless of profile. These rules apply to advice, advised cascades and automatic
fallback. Candidate lines show billing state and balance; JSON candidates include
`billing` (null when unknown) and `breakdown.billing`.

Billing alone never blocks an explicit `aid run <agent>`. Dispatch prints one
warning for an overage or exhausted route, including its balance when reported.
A fresh login refusal is excluded from recommendation fallback even when no
other candidate is eligible. Billing-event storage failures print a warning and
allow explicit dispatch to continue.
A task dispatched in overage records one setup event such as
`billing: overage (credits balance 48698.135)`; later cache changes do not alter it.
`aid agent quota` displays `OVERAGE` with balance or `EXHAUSTED` in place of `OK`.
Existing refusal markers still hold their routes until their recovery conditions
are met.

## API account billing

Configured `[[api_route]]` accounts run Claude Code with separate route IDs,
provider identities, spend totals, scores and refusal markers. They do not draw
on the Claude subscription pool or inherit its quota evidence. Configure them
in [configuration](configuration.md#claude-api-account-routes).

Month-to-date spend sums reported per-task USD costs for the route, using task
creation dates within the local calendar month. This measures aid tasks only;
external account activity is not included. Below the budget the state is
`in_plan`. At or above it, `over_budget = "stop"` makes the route `exhausted`
and prevents dispatch; `"bill"` makes it `overage`. The subscription billing
profile rules above also apply to API-route advice: overage excludes free/cheap
profiles and adds the applicable `billing` penalty for standard profiles.

A separate `pace` term subtracts 2.0 when spend exceeds
`monthly_budget_usd × elapsed_fraction_of_month × 1.25`. Pace never excludes a
candidate. Route history and team overrides use the route ID. Quota rows show,
for example, `claude-api-ming  IN PLAN  $37.20 / $200 this month`; advice lines
show the same monthly billing evidence and their own `pace` term.

The Claude `system/init` stream event must report
`apiKeySource = "apiKeyHelper"`. A missing field or any other value, including
an environment-key source, stops the process and fails the task with `route <id> is not billed to its API key`.
That route receives a needs-human hold. Fix the key or CLI billing configuration,
then use `aid config clear-limit <id>`. Credit-balance or billing refusals hold
only that route until midnight on the first day of the next local month.
