# Shared-checkout custody validation — 2026-09-23

## Scope

Implementation of the non-worktree settlement slice of `wi-dc6a`, against local
`ffd07e8e` (v10.47.0) plus the current uncommitted working-tree changes. This is
verification evidence, not a release or an external work-item closure.

`cmd/run/dirty.rs` now checks the persisted task worktree before selecting rescue.
Tasks without one preserve eligible checkout changes through a private Git index,
a recovery commit and a new `refs/aid/recovery/<task-id>/<attempt>` ref. They do not
stage the real index or commit/amend the principal branch, and they still run
configured verification. Recorded worktrees retain their existing rescue path;
a mismatched settlement directory is rejected.

Recovery refs snapshot shared working files, including unrelated user edits. They
are inspection/recovery evidence, not task-owned merge artifacts or acceptance.
Ignored files, excluded generated files, and dirty submodule contents are outside
this checkpoint's coverage. Read-only/audit-report tasks bypass it. Non-Git tasks
are untouched. These boundaries are documented in the
[operating guide](../default-skills/aid-guide/references/task-lifecycle.md).

## Environment

- Four Tailscale build-host entries configured locally outside the repository;
  two online and two offline at setup. One online host was initialized and verified
  with `rbox ensure`; all Rust builds/tests ran as the non-root `builder` account.
- Remote toolchain: rustc 1.98.1, cargo 1.98.1, Linux x86_64.
- `rbox exec --untracked` included the three new Rust files along with tracked edits.
  Target cache: `$HOME/.rbox/target/ai-dispatch` on the build host.
- Fake-rbox wrapper checks ran locally with the Python 3.12.14 runtime installed
  alongside rbox; they invoke neither a real box nor Cargo.

## Results

| Gate | Result |
| --- | --- |
| New Git checkpoint + shared lifecycle tests | 12 distinct regression tests passed; two overlap between command filters |
| `cargo test --locked --workspace --quiet` | 2,688 unit tests + 111 integration tests passed; 10 existing ignored tests; no failures |
| `cargo test --locked --workspace --features web --quiet` | 2,720 unit tests + 111 integration tests passed; 10 existing ignored tests; no failures |
| `python3.12 scripts/remote-test-test.py` | 12 passed |
| `bash .github/scripts/check-changelog.sh` | Passed for v10.47.0 |
| `git diff --check` and changed-guide/roadmap local links | Passed |
| `cargo clippy --locked -- -D warnings` | Failed: 15 diagnostics in unchanged source files; see below |
| Live Web API probe / Swift targets | Not run in this slice |

Unit totals exclude the separately printed child-process test result, which is
already accounted for by its parent test. The 10 ignored tests were not enabled
or newly skipped by this work.

Remote job `77e9e8ec02504da0a074c6d2d98a2c46` ran the focused `checkpoint` and
`shared_checkout_tests` filters (exit 0). Job `8e2b802bf6344b6eb76907e01a0715d4`
ran default tests, Web tests and strict clippy independently. Its aggregate exit
was 1 because clippy exited 101; both test commands exited 0. Logs were written
as `default-tests.log`, `web-tests.log`, and `clippy.log` in the remote validation
checkout; the rbox job log retains summaries and gate exit codes.

New regressions cover byte-for-byte real-index preservation with different staged
and unstaged content, unborn HEAD, tagged HEAD, independent repeated recovery refs,
renames/deletions from a subdirectory, clean/bookkeeping-only trees, failed ref
publication, non-Git directories, done/failed task status preservation, read-only
bypass, failed configured verification, and worktree path mismatch.

## Existing lint gate debt

All diagnosed files are unchanged relative to HEAD in this slice. No new lint
allowances were added. The strict gate is still red and must not be represented
as a release-ready baseline.

| Files | Diagnostics |
| --- | --- |
| `src/agent/home_isolation.rs`, `src/cmd/merge_lanes.rs` | 2 unused imports |
| `src/agent/model_group.rs` | 3 needless explicit lifetimes |
| `src/cmd/agent_json.rs`, `src/cmd/build_process.rs`, `src/tui/app_tasks.rs`, `src/tui/ui_detail.rs` | 8 needless borrows |
| `src/cmd/clean_size.rs` | 1 obfuscated if/else |
| `src/cmd/clean_cargo_target.rs` | 1 match-like-matches expression |

Track this mechanical cleanup separately from the custody behavior change. The
next correctness slice remains target-project budget identity (`wi-29bd`).
