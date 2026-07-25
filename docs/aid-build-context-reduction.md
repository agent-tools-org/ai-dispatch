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

Environment:

```text
CARGO_TARGET_DIR=/Users/mingsun/.cargo-target/ai-dispatch/_base
```

Completed:

```text
$ cargo check -p ai-dispatch --tests
Finished `dev` profile [unoptimized + debuginfo] target(s) in 11.88s

$ cargo check --all-targets
Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.62s
```

Blocked in this sandbox:

```text
$ cargo test --bin aid
error: failed to open: /Users/mingsun/.cargo-target/ai-dispatch/_base/debug/.cargo-build-lock

Caused by:
  Operation not permitted (os error 1)
```

The same inherited target lock prevented building or running a fresh `aid` binary, so the required
runtime measurements for `aid build` could not be completed without changing or clearing the inherited
`CARGO_TARGET_DIR`, which this task explicitly disallowed.

Temporary compile-error measurement attempt:

```text
$ cargo check --all-targets 2>&1 | wc -l -c
       4     145
```

The output was the target-lock failure above, so no valid Cargo-vs-`aid build` reduction factor could
be measured in this environment. The temporary compile-error scaffold was removed after the attempt,
and `src/cli/doctor_tests.rs` now contains only the real parser coverage.
