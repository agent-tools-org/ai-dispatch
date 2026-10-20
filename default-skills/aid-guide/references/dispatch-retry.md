# Retry and Verification

## Retry and fallback

```bash
aid retry <task-id> --feedback "Address the failed invariant"
aid retry <task-id> --feedback-file notes.md --model gpt-5.4 --idle-timeout 900
aid run codex "Task" --cascade opencode,cursor
```

A retry is a new attempt linked to its parent. It does not erase or rewrite the
failed attempt. Inspect the tree with `aid tree <task-id>`. Unspecified
`--model` / `--idle-timeout` keep the original task values; `--feedback` and
`--feedback-file` cannot be combined.

Automatic cascade takes the first launchable builtin candidate in `aid advise`'s
ranked list, using the task's difficulty, budget, urgency, rigor, kind, team,
history and detected caller. It excludes the exhausted agent and always excludes
Claude, even when team-preferred. Advice gates exclude disabled, missing,
auth-failed, below-floor, weaker same-pool and superseded Gemini routes, plus
holds on the candidate's selected model group even under background urgency.
An advisory `recommended` route never rescues an empty launchable set.

The candidate's model is retained through dispatch: known CLI defaults are
pinned, and unknown defaults cannot be replaced by later budget routing. A known
unservable advice model fails before launch rather than running another default.
When served-model evidence is unknown, the exact requested advice pin is allowed.
Advice provenance is persisted for both model pins and unknown defaults.
Explicit cascade/fallback lists keep their order, custom-name resolution,
remaining entries and unknown-name errors. Failed-task batch `auto_fallback`
never replaces an exhausted explicit list with advice. Explicit `aid batch retry`
can use advice when the saved list is empty and the agent is held. Explicit Gemini
and Claude remain allowed when
agy is installed. Cross-agent switches clear the source model and session before
applying the selected route.

A live hold blocks the requested route; an explicit `--cascade` can divert dispatch.
A held model never triggers automatic agent substitution. A hold may end on a stated
time, a dated aidbar window, or only when a person runs
`aid config clear-limit <agent>` (see `references/configuration.md` for the
four hold classes). Signatures match only the refusing agent. A custom id keeps
its own marker path while classification uses its configured `rate_limit_kind`
(OpenCode by default); overlay error parsing supplies its declared signature kind.
Markers retain that identity for readback without borrowing another built-in's
signatures. Reset times must
follow that agent's known reset phrase directly or come from a typed provider
field; unrelated timestamps and window lengths are ignored. Holds longer than
24 hours and holds requiring a person require stderr, a typed error envelope,
or a provider JSON field. CLI-written diagnostic logs use the stderr evidence
class. Unsplit PTY/output text records at most a transient
excerpt and cannot turn an exit-zero run into a failure. An already-failed run
can use its own agent's quota signature in that excerpt to launch a quota
continuation; the excerpt's hold stays transient. Grok reads only its
`type:error` message; generic status tokens give a transient hold. Codex
app-server uses typed error notifications and rate-limit snapshots instead of
matching raw lines when typed evidence is available. Typed refusal messages
retain their reset dates; a later shorter hold cannot replace a longer active
dated hold for the same provider marker. Unrecognised failures
record one milestone event when a provider-attributed excerpt exists, with
`unclassified_failure: true`, the agent, and a capped
`provider_excerpt`; this metadata does not change routing or task status and
does not replace the original error reason.
A marker whose stated
time has already passed does not block anything, and neither does the short
cooldown left by an unrecognised refusal — that window is shorter than the cost
of moving off the agent you asked for. Where a tiered agent has only one tier
held — cursor's premium pool, or droid's standard or Core pool — dispatch stays on the
agent and switches to a tier that still serves, reporting the swap rather than
making it silently. A droid refusal that names neither `standard usage` nor
`weekly Droid Core usage limit` holds the whole agent.

Cursor's `ActionRequiredError: You've hit your usage limit` monthly refusal
holds the dispatched model group, including Auto when Auto refused. Its
`M/D/YYYY` cycle-end date holds through that entire UTC day (release at the
following midnight UTC); a missing or invalid date uses a 30-day hold when
the refusal has provider attribution.
Quoted prose and assistant/tool envelopes do not qualify as this error line.

A hold is scoped to what actually refused. When a CLI serves several providers,
a refusal is attributed to the provider of the route aid dispatched, so one
provider running out of credit leaves its siblings dispatchable — an
`opencode/` balance failure does not hold `opencode-go/`. When the refusing
provider cannot be identified, the hold covers the whole agent rather than
guessing a provider.

Model validation distinguishes who chose the model. A model you named with
`--model` that the CLI does not serve is a hard error listing the served
models. A model aid resolved for you — from the catalog, the declared budget,
or a stored per-agent default — is dropped with a warning and the agent's own
default runs instead, because a stale catalog entry is aid's problem to absorb,
not a reason to refuse your dispatch. Where the CLI cannot be asked what it
serves, dispatch proceeds unvalidated and says so.

For explicit fallback substitution — where a held agent was replaced by a
caller-listed fallback before dispatch — an aid-resolved model survives a served-list miss
rather than being dropped. The substitution already proved the requested
family spent, and the fallback's own default can re-enter that exhausted
family (agy's default is a gemini model, the group the hold just escaped), so
the aid-resolved pin is retained through the cache lag. This covers any
aid-resolved effective model on the substituted route, not only the healthy
family pin: `switch_agent` clears `--model`, so a fallback's per-agent default
or a budget-picked model arrives as aid-resolved too, and the same trade
applies. A served-list miss is cache lag, not proof the CLI will reject the
model.

The served list is cached on disk for 24 hours, so a slow CLI is asked once
rather than on every dispatch. When the model you asked for is absent from the
cached list, aid re-probes once before rejecting it, so a model the CLI gained
since the last probe is accepted rather than refused for a day. Codex cache
entries also track the modification time and size of `$CODEX_HOME/models_cache.json`;
when Codex refreshes that file, aid refreshes its disk and in-process model
lists on the next lookup, including in a long-running `aid mcp` process.

`aid agent list --json`, `aid config pricing`, and `aid advise` merge every
model in the 24-hour served-model cache that has no catalog row into the
catalog views, for each agent with a served-model probe: codex, grok, cursor,
qwen, agy, and opencode (so providers such as `opencode-go` appear beside the
built-in `opencode/*` rows). For Codex, a changed source-file stamp refreshes
the cache from its local model list. Other agents read the cache only and never probe;
an absent or expired cache adds nothing. Each `models.available` row carries
`rated`, `capability` (the complex-impl rule, or `null`), `capability_evidence`
(the same raw/selected/harness structure as advice), and `source` (`catalog`,
`served`, or `pricing_override`). The agent-list response has the same `sources`
metadata block as advice. A served-only
row keeps `source: "served"`; `rated` reflects applicable leaderboard evidence, and
`capability` is `null` when unknown. Every
displayed price (`input_per_m`/`output_per_m` in the JSON, `aid config pricing`,
the `aid config agents` model lines) is the price cost estimation resolves below;
with none known the JSON carries `null` and the text prints `unknown`. aid never
invents ratings for models without leaderboard evidence, and routing never auto-selects one.
Cost estimation prices a model only from an exact match, in this order: an
explicit `pricing.json` override for the agent and model, subscription
inclusion, its static catalog row, or an exact price-feed id or alias.
An explicit override also replaces a listed row's tier and description while
preserving its origin; ratings resolve independently from the leaderboard feed.
The price feed carries each vendor's own per-token API rate, so it prices only
the vendor's own CLI (codex, claude, gemini, grok); on a reseller route (droid,
oz, opencode, and the others) a model without a catalog figure or override
costs `unknown`. It never uses a substring, vendor-prefix, family, or
`-free`-suffix match, and never a fixed fallback model: a served-only model
gets its price from an exact price-feed entry on its vendor's CLI, never from a
similar-name rate. A task with neither a pinned nor an observed model (for
example unpinned gemini or codex) costs `unknown`, stored as NULL. Subscription
agents (Cursor, Copilot) cost 0.0 as included.

A catalog row storing 0.0/0.0 is a price only when its tier is `free`; on any
other tier (droid, oz, grok rows) it means "no figure on record" and costs
`unknown`, never free, so a `max_task_cost` ceiling on that route warns that it
cannot be enforced. An override outranks every other source, including
subscription inclusion and the catalog row.

For providers represented by aidbar, a successful cached snapshot can release a
time-based, transient, or Windowed older marker for this dispatch decision only
when its `fetched_at` is newer than the marker file's modification time and
every relevant usage window has headroom. A Windowed hold also requires a
dated `resets_at` on at least one of those windows. Cursor premium's relevant
window is the one labelled `Plan`; On-demand is ignored. A `NeedsHuman` hold
— prepaid, a plan change, or invalid credentials — is never released by
percentages: used-percent readings say nothing about a spend or balance hold
(opencode refused at $19.37 of a $20 window; oz logged-out stderr is
`credentials are invalid`). The marker is not deleted, and aidbar errors, missing
probes, and unrecognized providers do not release it. `aid advise` continues
to score and report the marker state rather than this one-round dispatch view.

The held route is **not spawned**. Explicit cascade substitution happens before dispatch, so no
task row is recorded for the agent that was never run, and the fallback carries
none of the held route's model: a model name means something only inside one
CLI. The substitution is announced on stderr and recorded as an event on the
dispatched task, both naming `aid config clear-limit <agent>`. When no usable
aidbar snapshot can release the marker, that escape hatch is how a topped-up
account or other changed provider state releases a stale hold.

`--urgency background` keeps the agent you asked for when the hold is a clock
or Windowed quota — those can clear before a background task runs. A NeedsHuman
hold (logged-out credentials, prepaid, plan-change) still blocks and substitutes
or errors exactly as under normal urgency. The wait itself blocks only on a
clock or a dated mapped snapshot; prepaid, plan-change, and unmapped holds
return immediately and tell you to `aid config clear-limit` or pick another
agent.

A dry-run substitution milestone says `would dispatch` and records JSON
metadata naming both routes and whether the model class was preserved. It does
not mean the substitute ran.

A `--cascade` entry aid cannot resolve is an error, not a skipped entry. Custom
agents are valid cascade targets and are checked against their own hold, not a
shared one.

## Verify before review

```bash
aid wait <task-id>
aid show <task-id> --summary
aid show <task-id> --diff
aid show <task-id> --result
```

Agent success and verification are evidence for review, not acceptance.
