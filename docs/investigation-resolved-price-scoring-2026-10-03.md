KB consulted: `budget scoring price subscription override`. Broad matches returned the budget-override and price-times-volume lessons; there was no direct scoring-resolver match. The structural roadmap and `src/cost/pricing_resolution.rs` define this slice's contract.

# Resolved-price budget scoring — 2026-10-03

## Source and scope

Structural roadmap slice 3 (`wi-739e`) starts from
`d45681b71f65566dc68db9d85de4b4089625b726` on `fix/resolved-price-scoring`.
The branch's initial HEAD equals current main. That baseline records acceptance
of the prerequisite cascade slice: full default/Web suites, strict production
lint and independent review in
[cascade validation](validation-advised-cascade-2026-10-03.md) and
[cascade audit](audit-advised-cascade-2026-10-03.md).

The previous `selection_scoring::model_is_paid` reads `AGENT_MODELS` directly.
It treats Cursor's positive `composer-2.5` catalog API rates as paid despite
subscription inclusion, ignores user overrides in both directions, and misses
vendor feed prices. This is a separate price definition from cost estimation.

The implementation replaces only that lookup with the existing
`cost::resolve_pricing(Some(model), agent)`. A resolved price is paid when
either input or output is positive; unknown is not paid. The existing `-3.0`
budget penalty, eligibility and other score terms are unchanged. Resolution
remains exact override, subscription inclusion, catalog figure, vendor-only
exact feed, then unknown. There is no new pricing abstraction, dependency,
flag, compatibility path or live price request.

## Regression fixtures

`src/agent/selection_price_tests.rs` contains four parametrized tests. Every
case exercises both `score_breakdown` and actual `advise`, pins an agent's
sticky model, verifies `Sticky` provenance and eligibility, and compares every
non-budget score term across free, cheap, standard and premium profiles.
Standard and premium always have zero paid-model penalty. Override/feed
transitions also compare the complete non-budget advice candidate before and
after, including its eligibility metadata.

| Test suffix (all names start with `resolved_price_`) | Contract |
| --- | --- |
| `catalog_subscription_unknown_and_free_at_both_boundaries` | Positive subscription catalog rates are included; unknown and free are unpenalized; a positive catalog route is `-3.0`. |
| `zero_override_removes_catalog_paid_penalty` | A paid catalog route changes `-3.0` to zero; subscription and unknown remain zero with zero overrides. |
| `positive_exact_override_prices_free_subscription_and_unknown` | Uppercase exact overrides change zero to `-3.0` on free, subscription and unknown routes, separately for input-only and output-only positivity; neighboring names and other agents are unaffected. |
| `vendor_exact_feed_is_paid_only_on_its_vendor` | A synthetic exact feed prices its vendor, separately for input-only and output-only rates; reseller and neighboring model names remain unknown. |

Fixtures redirect `AID_HOME` and quota cache on the test thread. They use the
existing `cost::clear_feed_for_tests` seam to clear both thread-local feed and
override caches before resolution and after fixture updates. Prices come only
from synthetic files. Existing score tests and ignored-test annotations are
unchanged.

## Baseline-compatible before/after procedure

Only the new test file and its three-line `#[cfg(test)]` module registration in
`src/agent/selection.rs` are needed on an isolated checkout of the baseline.
Both use interfaces already present there. Do not copy the production scoring
change to that checkout. Stage the new fixture so remote synchronization
includes it. Record the baseline SHA, production-clean diff and fixture SHA-256
alongside the remote output, then use the identical fixture bytes on the candidate.

The baseline reproducer is
`agent::selection::selection_price_tests::resolved_price_catalog_subscription_unknown_and_free_at_both_boundaries`.
The expected old-path mismatch is Cursor `composer-2.5`: actual `-3.0`, expected
zero in budget mode. The zero-override and vendor-feed tests independently
exercise other old-path mismatches. These are predicted failures from source
inspection, not observed test results.

Remote diagnostic payloads (run through the authorized `rbox` path, never on
the host):

```bash
aid build check -p ai-dispatch --locked
aid test --isolated --bin aid agent::selection::selection_price_tests
aid test --isolated --bin aid agent::selection::selection_score_tests
aid test --isolated --bin aid agent::selection::selection_price_tests::resolved_price_catalog_subscription_unknown_and_free_at_both_boundaries -- --exact
```

The principal acceptance checks also require full default/Web remote tests,
strict default/Web production lint, and independent read-only audit. The
repository's full-suite entry points are `scripts/remote-test.sh -- --locked`
and `scripts/remote-test.sh -- --locked --features web`. Use one remote job per
box and preserve the warm shared target. No baseline worktree or main source
was modified during this draft.

## Actual validation and limits

The attempted remote prerequisite was `rbox ensure` for the configured box.
It exited 1 with:

```text
rbox: tailscale status returned no JSON (Expecting value at byte 0)
```

The preceding `rbox status` invocation was rejected by the CLI because it
requires `--json`; it supplied no load evidence. No remote job ID was assigned.
No compilation, test, baseline reproduction or strict lint ran. No local Cargo
build/test or raw transport fallback was used. The fixture is a draft until
the above checks and independent audit complete.

Local source checks cover Rust syntax formatting of the new fixture,
changed-file size, new-function length, headers, diff whitespace, production
lookup scope and unchanged existing score tests. These checks do not establish
that the Rust files type-check or that the regressions pass.
The formatting and whitespace checks pass. The changed Rust files have 41,
193 and 186 lines; the longest new function is 36 lines. There are four new
test functions and zero new ignores. Source comparison confirms that the
production diff consists exactly of the resolver call and unused import removal.

## Production line accounting

Count physical Rust lines relative to the baseline, including comments,
imports and blanks; exclude dedicated test files and explicitly test-only
module registrations. `selection_scoring.rs` changes from 195 to 193 lines
(net **-2**). `selection.rs` adds three test-only registration lines (zero
production lines), and the new sibling fixture contributes zero production
lines. Net production Rust is **-2**. No other production Rust file changes.
All changed source files are at most 300 lines and all new functions are at
most 50 lines. The simpler choice is to reuse the existing resolver directly.
