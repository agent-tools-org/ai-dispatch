# v10.47.1 bugfix validation — 2026-09-23

## Scope

This patch adds worker-settlement waiting to `aid wait` and `aid watch --wait`.
The worker job spec remains present until delivery, verification and error
settlement finish. Waiters observe that barrier before reading final task state;
group and implicit selection include terminal tasks with outstanding job specs.
A dead worker with an unfinished spec yields an error rather than success.
`--exit-on-await` remains available while a live task requests input.

Job spec updates use same-directory atomic replacement, and loading a concurrently
removed spec returns absence. Error handling now completes before spec removal.
The previous eight wait tests were moved unchanged into a sibling test file;
five settlement cases and three spec/error-lifecycle cases were added.

The patch also removes the 15 production clippy diagnostics recorded in the
[custody evidence](validation-custody-2026-09-23.md), without new lint allowances,
and adds default/Web build, workspace test and strict clippy coverage to CI.
It includes the shared-checkout custody and target-project budget fixes already
committed in `d7dc85ef`; their original evidence remains historical.

## Reproduction and verification history

All Rust compilation and tests run through rbox as the non-root Linux builder,
using rustc/cargo 1.98.1. No Rust compilation ran on the operator's Mac.

- Baseline strict clippy: job `27851c30ed2947b4a0c398df7122de53`, exit 101,
  reproducing all 15 documented production diagnostics.
- Before the wait fix: job `2f7a3ba177ab40f9800c39dba39480d1`, exit 101.
  All four initial deterministic regressions failed: premature success before
  failed/successful settlement, omitted group discovery, and interrupted settlement.
- Focused fix: job `ac67de272cae46c2b1684b1f35745b33`, exit 0. All 13 wait tests
  and default strict clippy passed.
- First full candidate: job `064f72810bd64e7ebe036d703ae54e64`. Both Rust suites
  exposed a newly added test fixture missing `paths::ensure_dirs()`; stderr-file
  creation failed before the hook under test. The fixture was corrected. This
  failed attempt is retained rather than described as a passing baseline.

Local checks: 12 fake-rbox wrapper tests, release orphan/hygiene regression script,
guide quick validation, workflow YAML/matrix validation, changelog validation and
`git diff --check` passed. PyYAML was installed only under a temporary validation
path for the guide/YAML checks.

## Final candidate gates

Job `ef8e45e6dc4c48099ae7dfb838cbffc1` completed with exit 0. The tested code
is committed in `b623f680` (lint) and `fb2befff` (settlement). Only CI/docs changes
follow those code commits before the scripted release.

| Gate | Result |
| --- | --- |
| `cargo test --locked --workspace --quiet` | 2,696 unit + 121 integration passed |
| `cargo test --locked --workspace --features web --quiet` | 2,728 unit + 121 integration passed |
| `cargo clippy --locked -- -D warnings` | Passed |
| `cargo clippy --locked --features web -- -D warnings` | Passed |

Each suite retains 10 existing ignored tests; no new skips or ignores were added.
Nested child-process test summaries are excluded from totals. Existing test-only
compiler warnings remain; the strict production clippy gates pass. Remote logs
are `/tmp/aid-release-{default,web,lint,lint-web}.log` in the builder environment;
the rbox job log records every gate status and test summary.

## Release boundary

Release version/changelog/commit/tag/push must go through `scripts/release.sh`,
with its dry-run first and `AID_RELEASE_TEST_CMD='scripts/remote-test.sh'`.
The published binary feature set is unchanged (default Rust features). No Swift
or Web API behavior change is included; live API probing and Swift builds remain
separate roadmap work, as do project-budget init/sync precedence and cap removal.
