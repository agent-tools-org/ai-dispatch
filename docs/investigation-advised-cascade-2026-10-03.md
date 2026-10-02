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

Controlled CLI regressions and resolver/store tests are supplied with this change. They are draft evidence until executed. One remote verification attempt failed before any job or test was started because transport status returned `tailscale status returned no JSON (Expecting value at byte 0)`. No Rust compilation, tests or independent cross-review have run for this draft. No local Cargo fallback was used.

## Correction checks

The shared route application is also used by best-of. Definitive live served
mismatches reject `Advised` pins; absent or empty refreshed evidence permits the
exact pin, including after a stale cache. Legacy explicit substitution pins keep
`AidResolved` validation behavior. The removed marker has no compatibility path.

Lifecycle steps and batch finalization/path helpers retain their existing order
and behavior in smaller source files. All new or changed Rust files are at most
300 lines. The inherited lifecycle orchestration and dispatch/model-validation
functions still exceed 50 lines; the function-size requirement is not fully met.

The net production Rust change against `724395fd` is -4 lines, counting moved
code, headers, imports and blanks and excluding dedicated tests and trailing test
modules. Rustfmt parsed the reviewed files without syntax errors. This is not
compiler, clippy or test evidence.

Required remote verification remains pending: `aid build check -p ai-dispatch`,
`aid test -p ai-dispatch`, the full web-feature suite through the remote test
wrapper, and strict production clippy for default and web configurations. The
focused CLI target is `aid test --test advised_cascade_e2e`. None ran: transport
admission failed before a job was created. Independent read-only audit is also
pending; this checkout is an unaccepted draft.
