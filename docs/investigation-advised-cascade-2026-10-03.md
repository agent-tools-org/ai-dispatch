KB consulted: the lookup for advised cascade, model provenance, fallback and environment matched failover-granularity-must-match-metering, env-driven-config-discipline, and a-remote-verification-timeout-describes-the-wrapper-not-the-test-job. The relevant boundary is the selected model/provider allowance and runtime environment, rather than the CLI name alone.

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
| `run::lifecycle::phases` quota continuation | Advice after the actual refused model is marked; retain parent linkage and target |
| `run::dispatch_resolve_held::skip_held_to_fallback` | Used before model resolution and after selected-model hold; both retain the advice candidate |

There are six old production readers. The final reader participates in two resolver phases; textual symbol occurrences are not additional callers.

## Resulting contract

Command-layer helpers build the existing advice report with `top=0` and caller detection. Automatic selection takes its first launchable builtin candidate except the exhausted agent and Claude, including when a team prefers Claude. Advice's existing gates exclude superseded Gemini, unavailable/disabled/auth-failed routes, below-floor candidates, weaker same-pool models and selected-model holds even under background urgency. An advisory recommendation cannot rescue an empty launchable set.

The shared application helper switches agents before copying the candidate model, typed `ModelSource::Advised` provenance and forced unknown-default semantics, materializing all four default or declared profile dimensions. Cross-agent switches clear the old session and route state. The provenance remains `Advised` even when the selected model is absent, distinguishing this exact route from ordinary and explicit family substitution without a separate marker. Advice routes reject a known served-list miss or a new hold before launch; they cannot be silently defaulted or switched to another family. Explicit held-route family pin behavior, including `keep_aid_resolved_pin`, remains separate.

`Task` has category and requested-model fields, while nullable declared profiles live in separate store columns. `RunArgs::for_retry` uses category/profile columns only when saved dispatch arguments are absent. Saved arguments, including undeclared values, remain authoritative. Saved advice pins/defaults survive retry even if the row requested-model field differs. Cross-agent switches reset provenance; explicit retry model overrides are user-supplied.

## Evidence and verification limits

Code trace before: selection returned an agent name; batch conversion and continuation cleared the source model on cross-agent switch. Code trace after: advice candidate → shared apply → saved arguments → resolver → CLI model option. Known CLI defaults become exact pins; unknown defaults suppress later budget routing.

Failed batch fallback overlays the current specification's runtime environment and forwarded names using the same merged environment/shared directory mechanism as initial dispatch. Saved dispatch JSON omits all environment values. Remaining saved explicit cascade entries are authoritative; a supplied explicit fallback with an empty saved cascade is exhausted and never rescued automatically. Builtin identity parsing excludes uppercase names and Antigravity aliases from selecting themselves.

Controlled CLI regressions and resolver/store tests pass on final source `f4e6014a`. The baseline public CLI regression launches `gpt-5.6-luna` where advice selected `gpt-6-sol`; the final suite preserves the exact advised model. An early sandbox transport attempt failed before any job or test started. Final verification ran remotely; no local Cargo fallback was used.

## Correction checks

The shared route application is also used by best-of. Definitive live served
mismatches reject `Advised` pins; absent or empty refreshed evidence permits the
exact pin, including after a stale cache. Legacy explicit substitution pins keep
`AidResolved` validation behavior. The removed marker has no compatibility path.

Lifecycle steps and batch finalization/path helpers retain their existing order
and behavior in smaller source files. All new or changed Rust files are at most
300 lines. The inherited lifecycle orchestration and dispatch/model-validation
functions still exceed 50 lines; the function-size requirement is not fully met.

The net production Rust change against `724395fd` is -10 physical lines, counting
moved code, headers, imports and blanks and excluding dedicated tests, test-only imports and trailing
test modules. Full remote default and Web workspace suites pass on the same clean
source: 3,098 and 3,130 passed respectively, zero failed and 14 existing ignored
per suite. Strict default/Web production clippy and the guide validator pass.
[Validation evidence](validation-advised-cascade-2026-10-03.md) records source,
commands, completed job identifiers, failed draft attempts and verification limits.
[Independent read-only review](audit-advised-cascade-2026-10-03.md) returns
PASS on all three questions and SHIP. Dedicated background-mode quota continuation
E2E output remains unverified; both modes use the traced shared branch.
