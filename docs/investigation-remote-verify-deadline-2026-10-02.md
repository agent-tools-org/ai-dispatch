KB consulted: `verify timeout remote aid rbox` returned remote-bundle, dispatch-time-cap, failed-sync and ensured-box lessons; no exact match for the fixed 120-second remote verification cap.

# Remote verification deadline mismatch (`wi-8b23`)

Base: `291aa2521028adce4b10860e923d57568ff1d4ed`.
Implementation: `018a57b1a88707fe911cc2a98ad5227f231fdb58`.
Final combined source: `113584e2ad0b2adf7a2d0dd1dbce8cf00fdea6f6`.

## Observed failure and source proof

Task `t-087492b6` recorded verification `timed_out` after 120 seconds while rbox
job `7ba127558b9a4f5cb16ef539d4b2a5b7` continued. The completed remote log later
showed exit 0 and 3,040 top-level workspace tests passed, zero failed and 14
ignored. The nested isolated-test summary is excluded from that total.
This is a wrapper deadline mismatch. The historical timeout remains truthful;
a completed job does not retrospectively turn it into a verification pass.

At the base, `src/remote_build.rs::verify_on` unconditionally supplied
`verify::VERIFY_TIMEOUT`, defined as 120 seconds. Dispatch already saved the
resolved `RunArgs.timeout_policy`, and Store persisted `started_at` when entering
Running. The remote verifier ignored those saved deadline facts.

## Repair

Remote verification now resolves one absolute deadline from the saved policy's
`min(max_duration, hard_cap)` and persisted `started_at`, falling back to
`created_at`. A future start is clamped to the initial verification time. Each
attempt computes remaining wall time against that same deadline, including after
a disk refusal and re-pick. Expiry returns `success=false`, `timed_out=true`, no
exit code and no command launch. Replacement-box persistence and refusal errors
retain their existing paths. The no-remote branch retains its 120-second contract.
The first audit found that converting this deadline to a duration before
configuration and mutex acquisition could still allow a late launch. Commit
`fd248424` carries the absolute deadline into the existing runner, checks after
lock acquisition before spawn, recomputes at wait and rejects a post-deadline
observed exit as inconclusive. Local relative durations remain unchanged.
No new flag, configuration key, lifecycle state or dependency was added.

## Executed coverage

`src/remote_build/deadline_tests.rs` adds nine regressions: deterministic allowance
above 120 seconds with a fast fake runner; elapsed start/creation fallback; both
sub-120 duration caps; actual short-command timeout; exhausted deadline with no
verify or pick; future timestamp clamping; shared deadline with attempted
replacement persistence; expiry during re-pick; and local/legacy behavior. Three
timer checks use isolated child test processes to avoid unrelated lock contention.
Existing fixtures now use current creation timestamps. The guide documents the
remaining-deadline contract.

All nine retained regressions plus four launch-boundary/skip regressions
executed in both complete remote workspace suites on the
combined source. Default: 3,066 passed; Web: 3,098 passed; both zero failed,
14 existing ignored, exit 0. Strict production clippy passed in both configurations.
The held-lock reproducer on unchanged production behavior at `b380c181`
failed both remote cases while its local-duration control passed (exit 101).
Independent re-audit returned SHIP. Guide validation returned `Skill is valid!`; `git diff --check` passed.
[Completed validation](validation-route-settlement-2026-10-03.md) records exact
candidate SHA, toolchain, commands, job IDs and independent review.

## Limits

The above-120 check deliberately avoids a long sleep: it verifies the computed
allowance supplied by the wrapper and a fast command. The remaining duration
does not change rbox's own lock/wait settings. Killing the remote client does not
guarantee cancellation of an already-started job. Existing infrastructure and
target-permission classification retain their paths. The separate provisional
terminal-state race is outside this repair. No binary installation or release
was performed; these results validate the source candidate.
