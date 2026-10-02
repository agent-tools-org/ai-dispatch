KB consulted: budget scoring price subscription override returned broad budget-override and price-times-volume lessons; no direct match covered the old scoring bypass.

# Resolved-price scoring validation — 2026-10-03

Baseline: `d45681b71f65566dc68db9d85de4b4089625b726`.
Reviewed source: `e469c500c9a715ef9caab5014b4ff99ccb7cd9e0`.

Budget scoring now uses the existing `cost::resolve_pricing` instead of its own
static-catalog lookup. A known positive input or output price yields the existing
`-3.0` penalty; included subscriptions, zero prices and unknown prices do not.
Exact overrides precede subscription/catalog/feed prices and work in both
directions. Model eligibility, non-budget scores and all other score terms are
unchanged. No dependency, flag, shim, ignored test or release version changes.

## Observed regression

The unchanged baseline production source received only the new sibling test file
and its trailing three-line test-only module registration. Remote job
`f4a553fb280f411eb98fe50446f42c00` records the baseline SHA, clean production,
a bytewise registration-prefix check and fixture SHA256 matching the candidate:
`2826f5fe5b43816b2b2842b2ce1d0556013163042d6f3ad14d8acb3497bec7da`.

Command: `cargo test --workspace --locked --bin aid agent::selection::selection_price_tests::resolved_price_catalog_subscription_unknown_and_free_at_both_boundaries -- --exact`.
Actual result: exit 101, 0 passed, 1 failed, 2,930 filtered. Cursor's included
`composer-2.5` route receives `-3.0` where the expected penalty is zero. The
fixture compares direct scoring to actual advice before asserting the penalty.

The same four parameterized fixtures pass in both final full suites. They cover
subscriptions, unknown/catalog/free prices, zero/positive exact overrides,
case-insensitive names, input/output-only prices, vendor-only exact feeds,
unaffected neighboring routes, and complete non-budget candidate equality.
Synthetic prices and thread-local home/quota/cache isolation avoid live pricing
or provider dependencies.

## Completed checks

Rust/Cargo 1.99.0; exact reviewed SHA and tracked-clean markers recorded in each
final log. Counts include 35 top-level binaries, excluding nested filtered runs.

| Command | Job | Exit | Passed / failed / ignored |
| --- | --- | --- | --- |
| `cargo test --workspace --locked` | `7f1de753b7a944eba228949bae6989d0` | 0 | 3,102 / 0 / 14 |
| `cargo test --workspace --locked --features web` | `43faf0bae74b46d48f8818a40acf2e98` | 0 | 3,134 / 0 / 14 |
| `cargo clippy --locked -- -D warnings` and `cargo clippy --locked --features web -- -D warnings` | `bd1ec8975878472bbb38402e5f6f2c96` | 0 | Both pass |

Strict default/Web production lint and official guide validation passed. Physical production Rust decreases by 2 lines, counting headers/imports/
blanks and excluding dedicated tests, test-only imports and module registrations.
[Independent read-only audit](audit-resolved-price-scoring-2026-10-03.md)
returns PASS for all three questions and SHIP. Evidence documents were added
afterward without modifying the reviewed Rust or guide bytes.

## Limits

No live vendor pricing, client/API probe, Swift build/test, Drive behavior or
publication/deployment was exercised. Existing 14 ignored scenarios remain.
The implementation agent's sandbox transport refusal assigned no remote job and
ran no tests; the completed principal-run jobs above supply the actual evidence.
This is local source validation, without a version release or binary install.
