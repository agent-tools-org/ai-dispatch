# Target-project budget validation — 2026-09-23

## Scope and behavior

Second correctness slice of `wi-29bd`, against `ffd07e8e` (v10.47.0) plus the
uncommitted custody and budget changes. This is not a release or board closure.

Dispatch now supplies the resolved target project identity to budget enforcement,
using the same resolver as task persistence. Previously the gate rediscovered the
caller's cwd. SQL aggregation now matches persisted `project_id`; its old checkout
basename heuristic missed custom project IDs. CLI and legacy TUI budget reporting
share the same identity/window filter.

Relative directories, linked worktrees, batch dispatch and retry use the target
identity. Rejections occur before task creation. No-config Git targets use their
existing generated identity; non-Git targets have no project cap. Agent budgets
continue to apply. Legacy rows with NULL identity remain unattributed rather than
being guessed from paths; they still count toward applicable agent budgets.

External usage and time windows remain supported. This does not reserve future
concurrent spend or change project init/sync precedence and cap-removal behavior.
See the [configuration guide](../default-skills/aid-guide/references/configuration.md).

## Verification

All Rust compilation/tests ran through rbox as the non-root builder on Linux
x86_64, rustc/cargo 1.98.1. `rbox exec --untracked` synced the new integration test.
No Rust compilation ran on the operator's Mac.

| Gate | Result |
| --- | --- |
| Original implementation with first six new regressions | All six failed, reproducing wrong-cwd gating, target bypass, custom-ID usage loss, relative/linked target, no-config target and retry failures |
| Fixed `dispatch_budget_e2e` | All 10 passed, including batch, near-limit warnings, reporting/window/external usage and non-Git agent caps |
| Default workspace suite | 2,688 unit + 121 integration passed; 10 existing ignored |
| Web workspace suite, first attempt | 2,720 unit passed; integration run stopped at background required-result test failure |
| Web workspace suite, unchanged candidate rerun | 2,720 unit + 121 integration passed; 10 existing ignored |
| Strict clippy | Failed with the same 15 diagnostics in unchanged files listed in [custody evidence](validation-custody-2026-09-23.md) |
| Guide quick validator | Passed; PyYAML installed only in a temporary validation directory |
| Changelog check, diff whitespace and changed-document local links | Passed |
| Live API / Swift | Not run |

Unit totals exclude separately printed child-process results already included in
the parent test total. The initial Web failure is not erased by a successful rerun.
`missing_explicit_result_file_fails_in_background_on_the_artifact_contract` saw
`aid wait` exit 0 instead of 1. Inspection suggests a settlement race: completion
can publish Done before the post-run required-file guard marks failure. The
relevant wait/watcher/result-file code is unchanged by this slice. This remains
an unresolved hypothesis requiring a deterministic reproducer and separate fix;
the baseline is not release-ready.

## Jobs and logs

- Before fix: `21b64d66f6054c89854e59b51c971381`, exit 101; `budget-before.log`
  in remote checkout `~/.rbox/work/ai-dispatch/codex-budget`.
- Focused fixed tests: `7e969ccb741247e999b2afeba1a0c025`, exit 0.
- Full gates: `833a9904222f4ba9aaced3cdc90cb8d1`, aggregate exit 1;
  default=0, Web=101, clippy=101.
- Unchanged full Web rerun: `8ac0bc445133465dafa5ec2a7492724c`, exit 0.

Fixed checkout: `~/.rbox/work/ai-dispatch/codex-budget-fixed`; logs:
`budget-after.log`, `budget-default-tests.log`, `budget-web-tests.log`,
`budget-clippy.log`, `budget-web-retry.log`. Rbox job logs retain completion status.

The commands inside the remote checkout, with
`CARGO_TARGET_DIR="$HOME/.rbox/target/ai-dispatch"`, were:

```bash
cargo test --locked --test dispatch_budget_e2e
cargo test --locked --workspace --quiet
cargo test --locked --workspace --features web --quiet
cargo clippy --locked -- -D warnings
```

Next budget slice: specify and test the project configuration/sync contract,
including precedence and removal of previously synchronized caps.
