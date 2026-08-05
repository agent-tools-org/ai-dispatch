# aid build context reduction

## Command surface

`aid build` now accepts a typed verification surface:

```text
aid build [check|test|clippy]
  -p, --package <PKG>
      --test <FILTER>
      --warnings
  [-- <extra cargo args>]
```

When the command is omitted, it is selected from `.aid/project.toml` `verify` when that value starts
with `cargo check`, `cargo test`, or `cargo clippy`; otherwise it falls back to `cargo check`.

## Digest contract

The command runs Cargo with `--message-format=json`, captures Cargo stdout/stderr, and writes the
agent-facing digest only after Cargo exits. The digest contains:

- One status line with outcome, error count, warning count, command, and elapsed time.
- Deduplicated compiler diagnostics with `file:line`.
- A `(xN)` suffix on a diagnostic line when the same unique diagnostic occurred more than once.
- Warning detail only when `--warnings` is set.
- An explicit `... N more diagnostics suppressed` marker when the hard digest line cap is reached.

Cargo progress lines such as `Compiling ...` are not emitted in the digest. Long-running progress is
sent to the task event stream when `AID_TASK_ID` is set; otherwise it is rate-limited on stderr.

`aid build` does not override an inherited `CARGO_TARGET_DIR` for the first attempt.
If `CARGO_TARGET_DIR` is already set, the child Cargo process inherits it. If it is
not set, the command uses the existing agent cargo target helpers to select a shared
or branch-specific target directory.

When cargo fails because that chosen target directory is not writable (sandbox
`Operation not permitted` / read-only `Permission denied` at a path under the
target), `aid build` retries once under the system temp directory at
`aid-build-target/<project-key>/` and adds a digest note naming both paths. This
is keyed off cargo's real OS error, not a preflight write probe. Inherited
`CARGO_TARGET_DIR` still wins selection; fallback only runs after the permission
failure. Temp is used because some agent sandboxes block cargo writes under the
worktree even when plain file creation there succeeds.

## Verification

Measured by the maintainer outside the agent sandbox (the dispatched agent could not complete these:
its inherited `CARGO_TARGET_DIR` lock was not writable under the sandbox, and it reported that
honestly rather than estimating).

Binary under test: release build of this branch. A real type error was injected into
`src/cli/doctor_tests.rs`, measured, then reverted.

### Context reduction, cold target directory

| Command | Lines | Bytes |
|---|---:|---:|
| `cargo check --all-targets` | 132 | 4340 |
| of which `Compiling`/`Checking` progress lines | 122 (92%) | - |
| `aid build check -- --all-targets` | **2** | **133** |

**66x fewer lines, 33x fewer bytes.** ai-dispatch is a small crate; a project with a large dependency
graph produces far more progress noise, so this is a conservative floor.

The digest still carries everything needed to act:

```text
failed: 1 errors, 0 warnings; command: cargo check --all-targets; elapsed: 39.1s
error: src/cli/doctor_tests.rs:17: mismatched types
```

Exit code on failure: `101`.

### Progress events and rate limiting

Progress event details now include the number of completed Cargo compilation units:

```text
21:16:43  [build] cargo check --all-targets started
21:16:56  [build] cargo check --all-targets still running after 13s, 187 units compiled
21:17:29  [build] cargo check --all-targets finished: 0 errors, 0 warnings, 312 units compiled
```

The threshold, interval, and 3-message limit remain unchanged. In the earlier 45.7s cold-target run,
agent-facing stdout was **one line** and progress stopped after 3 messages.

### Test suite

`cargo test --bin aid` with default parallelism: 1624 passed, 0 failed, 6 ignored.

## 2026-07-25 follow-up verification

Change under test: compiled-unit progress events and diagnostic occurrence suffixes.

| Command | Result |
|---|---|
| `cargo check --all-targets` | Passed in 3.88s |
| `cargo test --bin aid build_` | Blocked before compilation: inherited `CARGO_TARGET_DIR` build lock returned `Operation not permitted` |
| `cargo test --bin aid` | Blocked before compilation: inherited `CARGO_TARGET_DIR` build lock returned `Operation not permitted` |
| Cold-target one-error digest remeasurement | Not run; this task was explicitly constrained not to override inherited `CARGO_TARGET_DIR`, and rebuilding `aid` for measurement hit the same lock error |
