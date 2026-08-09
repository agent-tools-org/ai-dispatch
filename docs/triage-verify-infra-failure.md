# Triage Report: Verify Infrastructure Failure

## 1. Where does aid run verify, and how does it classify the result?
Verification is executed in `src/verify.rs` within `run_verify_with_timeout` (line 47). The command is built and spawned, and its exit status is checked at line 104 (`success: status.is_some_and(|status| status.success())`).

Any non-zero exit code—whether due to a test failure or an infrastructural failure like a missing toolchain or an `sccache` crash—results in `success == false`.

This result is then processed in `src/cmd/run_verify.rs` in `maybe_verify_impl` (lines 70-88). A false success unconditionally maps to a `VerifyStatus::Failed` record (via `record_verify_failed` at line 87 or 91). The system **does not distinguish** between an infrastructural failure (`sccache` failing to spawn) and the code under test failing.

## 2. Environment differences between agent and verify
The agent and the `verify` step run in significantly different environments:
* **Agent Environment:** Configured in `src/agent/env.rs` via `apply_run_env` (line 158). It creates an `IsolatedHomeGuard` (line 175) and explicitly overrides `HOME` to this temporary isolated directory (line 176). The agent's `cargo` builds and downloads crates into this isolated `HOME`.
* **Verify Environment:** Configured in `src/verify.rs` via `build_verify_command` (line 220) and `split_command` (line 246), which uses `Command::new(program)`. By default, this inherits the host's entire environment (including the host's `HOME`, `PATH`, and `RUSTC_WRAPPER`). 

**The Bug Trigger:** Because `CARGO_TARGET_DIR` is shared, the agent compiles artifacts pointing to source/registry paths inside its isolated `HOME`. By the time `verify` runs, the agent has finished and `IsolatedHomeGuard` has deleted that temporary home. When `verify` runs with the host's `HOME` (and potentially `RUSTC_WRAPPER=sccache` inherited from the user's shell), `sccache` or `cargo` encounters broken absolute paths in the cached target directory or fails to map them to the host's registry, resulting in the fatal `sccache` spawn error.

## 3. What happens to the delivered work?
When the `verify` step fails, `src/cmd/run_verify.rs` calls `enforce_verify_status` (line 114, defined in `src/verify.rs:170`), which marks the overall task as `Failed`.
This triggers a failure transition in `src/task_lifecycle.rs` (`fail_completed_verify_gate` at line 55), calling `salvage_failed_task`.
In `src/failure_salvage.rs` (`try_salvage_failed_task` at line 28):
1. A summary of the work is written to `partial-work.md` in the task directory (line 47).
2. All unstaged and uncommitted changes in the worktree are committed using `git commit --no-verify -m "wip: partial work salvage (task {task_id} failed)"` (line 115).

**Outcome:** The branch and changes are preserved as a WIP commit, but the task itself is marked as `Failed` with the generic verification failure message. There is no automated indication to the user that the failure was infrastructural.

## Proposed Minimal Fix
In `src/verify.rs`, clear the `RUSTC_WRAPPER` environment variable for verify commands to prevent host `sccache` from interfering with the artifacts built under an isolated home.

Minimal change (in `src/verify.rs` `run_verify_with_timeout`):
```rust
// Prevent host sccache from tripping over isolated-home paths in CARGO_TARGET_DIR
cmd.env_remove("RUSTC_WRAPPER");
```
