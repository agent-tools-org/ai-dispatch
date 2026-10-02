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

## Executed before/after evidence

At `b380c181`, with production behavior unchanged and only the first three new
test fixtures included, job `5c39797d599842229f4ff645d96ddc01` exited 101:
one passed and two failed. Both remote results incorrectly reported success;
the local duration control passed. This executes the source counterexample.

Final candidate `113584e2` passed all four cases in complete remote default and
Web suites: 3,066 and 3,098 top-level tests passed, zero failed and 14 existing
ignored in each. Both strict production lint configurations passed. Actual job
headers record the exact source SHA, tracked-clean state, toolchain and command.
The guide validator passed. Independent re-audit returned PASS on all questions
and SHIP. [Completed validation](validation-route-settlement-2026-10-03.md)
records commands, job IDs, earlier failures and limits.

No authenticated provider, live Drive, API/Swift or release proof is established.
Mutex responsiveness and cancellation of an already-running remote job remain
outside this deadline-handling guarantee.
