KB consulted: the supplied search for absolute deadline, lock, spawn and verify returned generic latency, liveness and test-integrity lessons; no exact match.

# Verification launch deadline handoff

The independent audit's P2 finding identifies a lost absolute deadline: `remote_build::verify_on` calculated a remaining duration before shim configuration, then `verify::execute_verify` acquired `VERIFY_LOCK`, spawned and started a fresh relative timer. Preparation and lock delay could exhaust the saved task deadline while still allowing a successful command to launch afterward. This counterexample was confirmed by source inspection; it was not executed in this draft.

The repair passes a mutually exclusive internal duration/deadline budget into the existing environment runner. Remote attempts retain the same saved deadline through configuration, command preparation and lock acquisition. The runner checks expiry immediately before `ProcessGuard::spawn`, returning a truthful timeout without an exit code or launch when exhausted. At wait it recomputes the remainder, kills and reaps a command if that remainder is exhausted, and refuses to report success for an exit observed after the absolute deadline. Duration callers retain their existing relative timer. Disk re-pick and replacement persistence still use the original deadline.

## Regression evidence and limits

Three new non-ignored timing tests in `src/remote_build/launch_deadline_tests.rs` execute in child test processes to isolate the production mutex:

- A readiness channel proves the actual `VERIFY_LOCK` is held before saved-policy remote verification begins. The holder observes shim creation, then keeps the lock past a one-second deadline. Assertions require no launch marker, `timed_out = true`, `success = false`, no infrastructure failure and no exit code.
- With a three-second saved allowance, the holder observes preparation and consumes two seconds. A two-second command must launch and time out with the remainder; a reset allowance would let it succeed. Assertions also require retained command output and no exit code.
- A local custom 500-millisecond duration follows an 800-millisecond lock delay, then runs a 200-millisecond command. It must retain its full relative allowance and succeed.

A fourth non-ignored test calls the environment runner with an expired deadline and verifies that explicit `skip` and no-project auto-detection retain their existing success, timeout and no-exit semantics without taking the mutex.

Existing saved-policy, initial-expiry, local/legacy and disk re-pick regressions remain in place. The guide explicitly includes preparation and verifier-lock time in the remote deadline.

Rust compilation, tests and clippy did not run in this sandbox. The single remote prerequisite attempt failed because Tailscale returned no JSON; live remote attempts stopped at that point. No local Rust build or test was substituted. Local Rust parsing, diff hygiene, source limits and production `unwrap()` checks passed. Guide validation could not start because its Python environment lacks PyYAML. A staged-byte security-guard scan passed before committing. These checks do not establish compilation or runtime correctness. Independent review, pre-fix reproduction, complete default/Web tests and lint remain integration gates.
