KB consulted: `budget scoring price subscription override`. Broad matches returned the budget-override and price-times-volume lessons; there was no direct scoring-resolver match. The structural roadmap and `src/cost/pricing_resolution.rs` define this slice's contract.

# Resolved-price budget scoring — 2026-10-03

## Source and scope

Structural roadmap slice 3 (`wi-739e`) starts from
`d45681b71f65566dc68db9d85de4b4089625b726`. That baseline contains the
prerequisite cascade slice: full default/Web suites, strict production
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

## Observed before/after evidence

The unchanged baseline production source received only the new sibling fixture
and its trailing three-line test-only registration. Remote job
`f4a553fb280f411eb98fe50446f42c00` records baseline `d45681b7`, clean production,
bytewise selector-prefix equality and the same fixture SHA256 as candidate
`e469c500`. The exact catalog/subscription boundary regression fails: 0 passed,
1 failed and 2,930 filtered, exit 101. Cursor `composer-2.5` receives `-3.0`
where subscription inclusion requires zero. Direct scoring and actual advice
agree before the assertion fails. This is observed output, not a prediction.

The same four fixtures pass in both complete candidate suites. No existing
score test or ignored-test annotation was changed. Price, eligibility and
non-budget comparisons use the actual scoring/advice boundaries.

## Completed validation and limits

Full remote default/Web suites pass on the same clean source `e469c500`: 3,102
and 3,134 passed respectively, zero failed, 14 existing ignored per suite.
Both strict production clippy configurations and standalone guide validation
pass. [Validation evidence](validation-resolved-price-scoring-2026-10-03.md)
records the exact source, toolchain, commands and completed job identifiers.
[Independent audit](audit-resolved-price-scoring-2026-10-03.md) returns
PASS for all three questions and SHIP for the exact reviewed source.

An earlier sandbox transport attempt assigned no job and ran no Rust checks;
the completed remote jobs establish the actual results. No local Cargo fallback
was used. Synthetic pricing fixtures do not establish live vendor prices.
Client/API, Swift, Drive and publication/deployment behavior remain outside
this slice. Package version and installed release are unchanged.

## Production line accounting

Count physical Rust lines relative to the baseline, including headers, imports
and blanks; exclude dedicated tests and explicitly test-only registrations.
`selection_scoring.rs` decreases from 195 to 193 lines. The selector's three
new registration lines and sibling fixture contain no production code. Net
production Rust is **-2**; no other production Rust file changes. Changed source
files are at most 300 lines and new functions at most 50. The existing price
resolver is reused directly, without a new pricing owner.
