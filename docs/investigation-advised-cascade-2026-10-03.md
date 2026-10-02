KB consulted: failover-granularity-must-match-metering; agent-selection-and-model-tiers. The lookup also found the related admission-gate and model-family quota notes. The relevant finding is that a CLI name alone does not preserve a selected model/provider allowance.

# Advice-preserving automatic cascades

## Observed boundary

The removed selection fallback ranked installed agent names by category capability and returned `AgentKind`. It did not retain advice's model, declared task profile, history, team or detected caller gates. Batch argument conversion eagerly turned that name into an explicit cascade entry. Failed batch fallback returned two strings, so a selected model could not survive dispatch.

Actual production readers before this change:

| Reader | Behavior after the change |
| --- | --- |
| `config_display::render_rate_limit_line` | Default prompt/profile advice without a store; display the advised peer |
| `batch::args::task_to_run_args` / eager auto cascade | Removed; run held resolution selects with the complete profile and store |
| `batch::dispatch_support::auto_fallback_agent` | Replaced by prepared fallback arguments retaining the candidate model and saved dispatch inputs |
| `batch::retry::retry_task_to_run_args` | Saved arguments first; advice selection retains the route |
| `run::lifecycle` quota continuation | Advice after the actual refused model is marked; retain parent linkage and target |
| `run::dispatch_resolve_held::skip_held_to_fallback` | Used before model resolution and after selected-model hold; both retain the advice candidate |

There are six old production readers. The final reader participates in two resolver phases; textual symbol occurrences are not additional callers.

## Resulting contract

Command-layer helpers build the existing advice report with `top=0` and caller detection. Automatic selection takes its first launchable builtin candidate except the exhausted agent and Claude, including when a team prefers Claude. Advice's existing gates exclude superseded Gemini, unavailable/disabled/auth-failed routes, below-floor candidates, weaker same-pool models and selected-model holds even under background urgency. An advisory recommendation cannot rescue an empty launchable set.

The shared application helper switches agents before copying the candidate model, aid-resolved provenance and forced unknown-default semantics. Cross-agent switches clear the old session and route state. A persisted `advised_route` bit distinguishes this exact selected route from ordinary and explicit family substitution. Advice routes reject a known served-list miss or a new hold before launch; they cannot be silently defaulted or switched to another family. Explicit held-route family pin behavior, including `keep_aid_resolved_pin`, remains separate.

`Task` has category and requested-model fields, while nullable declared profiles live in separate store columns. `RunArgs::for_retry` uses category/profile columns only when saved dispatch arguments are absent. Saved arguments, including undeclared values, remain authoritative.

## Evidence and verification limits

Code trace before: selection returned an agent name; batch conversion and continuation cleared the source model on cross-agent switch. Code trace after: advice candidate → shared apply → saved arguments → resolver → CLI model option. Known CLI defaults become exact pins; unknown defaults suppress later budget routing.

Controlled CLI regressions and resolver/store tests are supplied with this change. They are draft evidence until executed. One remote verification attempt failed before any job or test was started because transport status returned no JSON. No Rust compilation, tests or independent cross-review have run for this draft. No local Cargo fallback was used.
