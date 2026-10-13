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
