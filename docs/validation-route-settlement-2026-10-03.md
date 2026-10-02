KB consulted: `best-of advise routing launchable model`, `backup artifact gdrive lifecycle`, and `verify timeout remote aid rbox`; relevant lessons concern model-family holds, artifact custody, and distinguishing a wrapper deadline from a completed remote job.

# Route and settlement validation — 2026-10-03

Reviewed and tested source: `b380c181fb8dab098d3910b4c6c05e8aa9d38390`.
Base: `291aa2521028adce4b10860e923d57568ff1d4ed`.
The source includes launchable best-of advice routes (`wi-843a`), read-only report
preservation before backup (`wi-562b`), the remaining remote verification deadline
(`wi-8b23`), and documentation of the existing Web `--port` argument.
No release or binary installation is part of this validation.

## Completed remote checks

All Rust compilation and execution used rbox. Both full suites ran from the same
clean candidate. Strict lint ran sequentially after the default suite. The
configured remote toolchain reported `rustc 1.99.0 (b940084d7 2026-09-28)` and
`cargo 1.99.0 (5f94df478 2026-08-27)`. Metadata job
`be1c03d7221848e78bcbf7115545e086` returned the exact candidate SHA and exited 0.

| Check | Command through configured remote box | Completed job | Result |
| --- | --- | --- | --- |
| Default workspace | `scripts/remote-test.sh -- --locked` | `a74fb30d027842d38d56d13cbf5c70b2` | exit 0; 34 binaries; 3,062 passed, 0 failed, 14 ignored |
| Web workspace | `scripts/remote-test.sh -- --locked --features web` | `9120e40e4bdf48ebaa1041d792d04f71` | exit 0; 34 binaries; 3,094 passed, 0 failed, 14 ignored |
| Strict production lint | `cargo clippy --locked -- -D warnings` then `cargo clippy --locked --features web -- -D warnings` | `328513dd2f7e494291efb86f85dabe46` | both commands passed; exit 0 |

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
  Both full suites and lint were then rerun at the reviewed candidate above.

## Independent review

The [first independent audit](audit-route-settlement-2026-10-03-round1.md)
reviewed the frozen source and supplied completed logs without running tests of
its own. Best-of and read-only result preservation passed. The combined verdict
was **FIX**: converting the remote absolute deadline to a duration before
preparation and verifier-lock acquisition can launch a command after expiry.
A bounded launch-boundary repair and isolated held-lock regressions are pending;
the passing suites above do not cover that counterexample.

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
