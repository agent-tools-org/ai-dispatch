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
- Warning detail only when `--warnings` is set.
- An explicit `... N more diagnostics suppressed` marker when the hard digest line cap is reached.

Cargo progress lines such as `Compiling ...` are not emitted in the digest. Long-running progress is
sent to the task event stream when `AID_TASK_ID` is set; otherwise it is rate-limited on stderr.

`aid build` does not override an inherited `CARGO_TARGET_DIR`. If `CARGO_TARGET_DIR` is already set,
the child Cargo process inherits it. If it is not set, the command uses the existing agent cargo target
helpers to select a shared or branch-specific target directory.

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

Run with `AID_TASK_ID` set, threshold 3000ms, interval 5000ms, limit 3, against a cold target dir:

```text
21:16:43  [build] cargo check --all-targets started
21:16:46  [build] cargo check --all-targets still running after 3s
21:16:51  [build] cargo check --all-targets still running after 8s
21:16:56  [build] cargo check --all-targets still running after 13s
21:17:29  [build] cargo check --all-targets finished: 0 errors, 0 warnings
```

Agent-facing stdout for that 45.7s build was **one line**. Progress stopped after 3 messages and did
not resume for the remaining 32 seconds, confirming the limit is enforced.

### Test suite

`cargo test --bin aid` with default parallelism: 1624 passed, 0 failed, 6 ignored.
