KB consulted: `best-of advise routing launchable model`, `backup artifact gdrive lifecycle`, and `verify timeout remote aid rbox`; relevant lessons concern model-family holds, artifact custody, and distinguishing a wrapper deadline from a completed remote job.

# Route and settlement validation — 2026-10-03

Reviewed and tested source: `113584e2ad0b2adf7a2d0dd1dbce8cf00fdea6f6`.
Base: `291aa2521028adce4b10860e923d57568ff1d4ed`.
The source includes launchable best-of advice routes (`wi-843a`), read-only report
preservation before backup (`wi-562b`), the remaining remote verification deadline
(`wi-8b23`), and documentation of the existing Web `--port` argument.
No release or binary installation is part of this validation.

## Completed remote checks

All Rust compilation and execution used rbox. Both full suites ran from the same
clean candidate; each job printed its source SHA, tracked-clean result, toolchain
and exact check argv before execution. Strict lint ran sequentially after the default suite. The
configured remote toolchain reported `rustc 1.99.0 (b940084d7 2026-09-28)` and
`cargo 1.99.0 (5f94df478 2026-08-27)`. All three final job headers returned the exact candidate SHA.

| Check | Command through configured remote box | Completed job | Result |
| --- | --- | --- | --- |
| Default workspace | `cargo test --workspace --locked` | `59c4849f37544817adaa76bd0813c59a` | exit 0; 34 binaries; 3,066 passed, 0 failed, 14 ignored |
| Web workspace | `cargo test --workspace --locked --features web` | `a156f4c955854728944922005bb08dcc` | exit 0; 34 binaries; 3,098 passed, 0 failed, 14 ignored |
| Strict production lint | `cargo clippy --locked -- -D warnings` then `cargo clippy --locked --features web -- -D warnings` | `554783c2d45243939d19042785225aa8` | both commands passed; exit 0 |

The suite counts use the 34 top-level Cargo summaries with zero filtered tests.
Each full log also includes one nested isolated test summary; it is excluded from
the workspace total. No new ignored test was introduced. Eight existing ignored
subprocess reporting helpers, the live price probe, GitButler detection, the TUI
performance probe and three provisional-terminal-state scenarios remain ignored.

The guide validator returned `Skill is valid!`; `git diff --check` passed.

## Reproducers and initial failures

- The original backup-only suite passed 22 tests at the base, job
  `0de4373ecf774d0abda3460645319887`, exit 0. That suite did not cover the report loss.
- On unchanged production source with only the new report regression fixtures,
  job `8141f79d62ca4d09b0ed6f06f2df3385` exited 101: one passed and two failed while
  reading the missing task artifact. The fix's original full default job
  `7ba127558b9a4f5cb16ef539d4b2a5b7` passed 3,040 top-level tests across 33 binaries,
  zero failed and 14 ignored. Its nested one-test summary is excluded.
- The outer verifier for that delivery recorded `timed_out` at 120 seconds while
  the remote job continued and eventually exited 0. Both outcomes are retained;
  a recovered completed log does not rewrite the historical wrapper result.
- Initial combined source `40ef15fa` passed default and strict lint. Its Web job
  `233c0009160046f1b25c2f1901e570c2` failed generated guide coverage: the existing
  `--port` argument was undocumented (2,921 unit tests passed, one failed,
  11 ignored; the remaining binaries did not run). Documenting the flag and
  default 8080 corrected the gap without an allowlist change or skipped test.
  The b380c181 follow-up passed both full suites and lint, but independent review
  found the further preparation/lock deadline handoff below.

## Launch-boundary reproducer and repair

The [first independent audit](audit-route-settlement-2026-10-03-round1.md)
passed best-of and report preservation, but returned **FIX** for remote verify:
a duration computed before preparation and `VERIFY_LOCK` can permit a late
launch or restart the allowance after lock acquisition.

With production behavior unchanged at `b380c181`, only the synchronized test
fixtures, fixture-helper visibility and a test-module include were added. Remote
job `5c39797d599842229f4ff645d96ddc01` exited 101: **one passed, two failed**.
Both remote cases returned success after the saved deadline; the local relative
500 ms duration control passed. The fourth new test uses the new budget API and
was deliberately excluded from that older-source compilation.

Commit `fd248424` carries one absolute deadline through the existing environment
runner using mutually exclusive duration/deadline budgets. It checks after real
lock acquisition before spawning, recomputes the wait allowance, kills/reaps on
exhaustion and treats an observed post-deadline exit as inconclusive. Local
relative durations and explicit skip/no-project behavior retain their contracts.
All four new cases passed in both final complete suites above.

## Independent review

The [independent re-audit](audit-route-settlement-2026-10-03.md) reviewed
`113584e2` and the completed logs, verified the synchronized before/after evidence
and returned **PASS** for all three questions and **SHIP** for the scoped source.
The reviewer ran no tests of its own. The first-round **FIX** remains archived.
Subsequent changes reconcile documentation only; source, tests and the embedded
authoritative guide remain identical to this reviewed and tested candidate.

## Limits and next slices

Tests use controlled agent executables, fake gws and captured archives; live
provider behavior and authenticated Google Drive compatibility are not established.
Issue #118 retains three source-only gaps: in-place project backup discovery
(`wi-56ec`), pre-agent worker failures (`wi-9942`), and malformed backup configuration
warnings (`wi-b7d8`). See [backup reconciliation](investigation-backup118-2026-10-02.md).
The existing provisional terminal-state race is not repaired by the deadline fix.
Killing a remote client does not guarantee cancellation of its already-running job.
Live API probes and Swift targets were not run because these patches change neither
client nor API contracts; the broader M0 gates remain open.

The next structural slices are advise-driven automatic cascade (`wi-7aad`) and
price-function budget scoring (`wi-739e`). Best-of production code decreases by
35 physical lines; the two separate correctness repairs add only their bounded
lifecycle/deadline logic. No dependency, CLI flag or routing layer was added.
