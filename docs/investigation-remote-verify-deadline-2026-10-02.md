KB consulted: `kb verify timeout remote aid rbox` returned remote-bundle,
dispatch-time-cap, failed-sync and ensured-box lessons; no direct match for the
fixed 120-second remote verification cap.

# Remote verification deadline mismatch (wi-8b23)

## Observed evidence

Task `t-087492b6` recorded verification `timed_out` after 120 seconds while rbox
job `7ba127558b9a4f5cb16ef539d4b2a5b7` continued. The recovered completed remote
log showed the full workspace suite exited 0. This evidence was supplied with
the report; the completed log was not fetched again here. The wrapper timed out;
that result does not establish a Rust test failure or retrospectively become a
verification pass.

## Source proof and repair

At base `291aa252`, `src/remote_build.rs::verify_on` unconditionally passed
`crate::verify::VERIFY_TIMEOUT` to `run_verify_with_env`. `src/verify.rs` defines
that duration as 120 seconds. `src/cmd/run/dispatch_prepare.rs` resolves and saves
`RunArgs.timeout_policy`; `src/store/mutations.rs` persists `started_at` on entry
to Running. The remote wrapper ignored those saved deadline facts.

The repair resolves one absolute deadline from the saved policy's
`min(max_duration, hard_cap)` and the task's `started_at`, falling back to
`created_at`. A future start is clamped to the initial verification time. Each
`verify_on` call computes the remaining wall time, including after disk refusal
and re-pick. Expiry returns `success = false`, `timed_out = true`, no exit code,
and no command launch. Replacement-box persistence and refusal errors keep
their existing path. The no-remote branch still uses the original 120 seconds.

`src/remote_build/deadline_tests.rs` adds nine regressions: resolved allowance
above 120 seconds with deterministic helper evidence and a fake verify runner;
elapsed time and creation fallback; both duration caps below 120; actual short
command timeout; exhausted deadline without command or pick; future timestamp
clamping; shared deadline with attempted replacement persistence; expiry during
re-pick; and local/legacy behavior. Timer checks use isolated child test processes
to avoid contention with unrelated tests. Existing fixtures in
`src/remote_build/tests.rs` and `src/remote_build/shim_tests.rs` now use current
creation timestamps. Guide changes describe this timeout contract.

## Validation output

Remote prerequisite:

```text
export AID_BUILD_BOX='<configured-box>'
rbox ensure "$AID_BUILD_BOX"
rbox: tailscale status returned no JSON (Expecting value at byte 0)
exit 1
```

No remote build or Rust test ran. The sandbox could not establish Tailscale
access; no local Rust compilation was used. The existing fake transport checks
run no Cargo jobs and contact no box:

```text
python3 scripts/remote-test-test.py
Ran 15 tests in 13.996s
OK
```

`rustfmt --edition 2024 --config skip_children=true --emit stdout` parsed
`src/remote_build.rs` and `src/remote_build/deadline_tests.rs` successfully without
rewriting files. `git diff --check` passed. The guide's `quick_validate.py`
could not run: `ModuleNotFoundError: No module named 'yaml'`.

`AID_BUILD_BOX='<configured-box>' scripts/remote-test.sh --dry-run -- --bin aid
remote_build::` exited 0 and produced one `rbox exec` targeting the shared
`$HOME/.rbox/target/ai-dispatch` with the requested binary and filter. This was
command construction only; it launched no remote job.

Pending commands on a reachable remote build host, using the Cargo PATH shim
and its existing shared target (one build per box):

```bash
export AID_BUILD_BOX='<configured-box>'
rbox ensure "$AID_BUILD_BOX"
rbox status --json
aid build check -p ai-dispatch -- --bin aid
aid build clippy -p ai-dispatch -- --bin aid --tests
aid test --isolated -p ai-dispatch --bin aid remote_build::
aid test --isolated -p ai-dispatch --bin aid cmd::init::official_guide
aid test --isolated -p ai-dispatch --test remote_build_e2e
aid test --isolated -p ai-dispatch --test aid_guide_e2e
aid test --isolated -p ai-dispatch --test init_e2e
```

## Limits

Compilation, Rust regressions, live remote verification and independent review
remain unverified. The deterministic allowance check avoids a 120-second sleep;
it proves the computed timeout used by the wrapper alongside a fast fake command.
The wrapper's remaining duration does not change rbox's own wait/lock settings
or guarantee that killing its client stops an already-started remote job.
Existing infrastructure and target-permission classification remain unchanged.
Task terminal-status ordering is outside this repair.
