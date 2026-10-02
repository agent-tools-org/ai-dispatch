KB consulted: `cascade fallback advise launchable`, `advise model fallback metering`, and the failover granularity and model-tier notes. A selected route must retain its model and metered allowance through the launch handoff.

# Automatic cascade validation — 2026-10-03

## Scope and source

This validation covers the advise-driven automatic cascade slice (`wi-7aad`). The existing category-matrix picker is removed. Automatic fallback carries the ranked candidate's model and declared profile through prelaunch resolution, quota continuation and batch retry. Explicit cascade order, custom agents, healthy family pinning and exhausted batch lists retain their contracts.

The source baseline is `724395fd15274a1c283ad6e3d869c4cfbd70c284`.
Reviewed production source: `f4e6014a00c12d971ce4488f103e7c2b87ad1f94`.
All final checks recorded this SHA and a clean tracked source tree.

## Before/after regression

On the baseline, only the new public CLI fixture files were staged; production
source was unchanged. The job log records the baseline SHA, clean production
source, the two staged fixture paths and their SHA-256 hashes. The remote job `7ee348d65657447bbc1d73f1f2ddcfe0` executed
`cargo test --workspace --locked --test advised_cascade_e2e prelaunch_held_substitution_pins_known_cli_default_and_protects_unknown_under_budget_pressure -- --exact`.
It exited 101 with 0 passed, 1 failed and 9 filtered. Advice selected `gpt-6-sol`,
but held-route substitution under near-budget pressure launched
`gpt-5.6-luna`; the assertion compared the actual requested model with advice.

An earlier unstaged fixture attempt exited 101 because no matching test target
was synchronized. It did not execute a regression test.

The first salvaged draft (`c1f36f5b3164a314eb2fba7bf161db918343b019`)
failed remote compilation in job `2941a983084c448689ca1ee5db753de2` with missing
imports. No test ran. Its interface and runtime handoff were corrected before
final acceptance. Neither failed attempt is a passing check.

## Final completed checks

Rust and Cargo 1.99.0 executed the following commands remotely. Counts include
35 top-level test binaries per suite and exclude nested filtered subprocess summaries.

| Command | Job | Exit | Passed / failed / ignored |
| --- | --- | --- | --- |
| `cargo test --workspace --locked` | `4f5ba5eecdc24afe84a1c229656e3fd8` | 0 | 3,098 / 0 / 14 |
| `cargo test --workspace --locked --features web` | `1787d55ed12a4c6b8e2787e606c13c92` | 0 | 3,130 / 0 / 14 |
| `cargo clippy --locked -- -D warnings` and `cargo clippy --locked --features web -- -D warnings` | `f823220f571e4d66932f292e61ac0508` | 0 | Both pass |

The official guide validator passed. Production Rust changes total **-10 physical
lines**, including headers, imports, blanks and both sides of moves, excluding
dedicated test files, test-only imports and trailing test modules. Changed source files are at most
300 lines; inherited orchestration and validation functions over 50 lines remain.

An earlier full default run failed a probe-timeout assertion without exposing its
actual error. The assertion now includes that error; no production workaround was
added. Later full suites passed, but this does not establish the earlier cause.
Fixture corrections isolate advice inputs, implement CLI help identities, create
an actual workgroup, and assert the new serialized `Advised` provenance. No new
test skips or ignores were introduced.

The [first independent audit](audit-advised-cascade-2026-10-03-round1.md)
passes route fidelity and explicit contracts, but requires narrowing the guide's
exhausted-list promise to failed-task batch auto-fallback. The final candidate
contains that documentation correction. The [final independent audit](audit-advised-cascade-2026-10-03.md) returns
PASS for all three questions and SHIP for source `f4e6014a`. Evidence documents
were added afterward without changing the reviewed Rust or guide bytes.

## Limits

Controlled CLI executables establish dispatch, stored arguments, environment
redaction and parent linkage. They do not establish live provider behavior.
A dedicated quota-continuation test for `LifecycleMode::Background` was not
added; foreground/background use the same independently traced continuation
branch, and CLI regressions cover real worker handoffs. The existing ignored
terminal-settlement scenarios remain outside this slice.
Live API/Swift and Drive checks are not part of this verification. No release
or publication is asserted by local source acceptance.
