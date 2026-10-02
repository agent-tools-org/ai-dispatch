KB consulted: `backup artifact gdrive lifecycle`; no direct backup-specific lesson found. Relevant matches: `a-pending-acceptance-criterion-needs-a-denominator` and `a-write-denied-auditor-cannot-run-a-compiler-that-writes-artifacts`.

# Read-only settlement result preservation

Source base: `291aa2521028adce4b10860e923d57568ff1d4ed`.
Scope: the declared report loss path tracked by wi-562b in issue #118.
Implementation and reproducer: `e26d90bd237e97d40a4e82c3a2daf8c414c447fe`.

## Verified source path

1. `src/cmd/run/dirty.rs` routes read-only settlement to
   `src/cmd/run/read_only.rs::settle`, using the dispatch snapshot. An explicit
   result path is allowed; a changed tracked file outside that path is a violation.
2. `settle` records the read-only error, marks the task Failed, leaves checkout
   contents intact, and returns `DirtyWorktreeAction::Failed`.
3. `src/cmd/run/lifecycle.rs::run_lifecycle_phases` converts that action to Stop.
   Configured verification that cannot run is recorded as `VerifyStatus::Failed`.
   The branch returns before `run_task_postprocess_phase` and its result persistence.
4. `post_run_lifecycle` still calls `backup::on_settled`. Backup claims one attempt
   before bundling. `src/cmd/export.rs::read_output` reads the output path or the
   task artifact `result.md`; it does not read the declared source report.

Consequently, at the source base a report in the checkout can be absent from the
only uploaded export. This source trace confirms the early-return premise.

The change reuses the existing lifecycle result persistence helper inside the
read-only Stop branch, after the existing verification-not-run record and before
returning to backup. Ordinary result persistence stays in postprocessing.
Backup claiming, warning-only errors, configuration resolution, and the preserved
checkout are unchanged. No new task state or dependency is introduced.

## Reproducer

`src/cmd/run/lifecycle_read_only_backup_tests.rs`, included by the existing
verify-gate test module, creates a real Git worktree and captures its actual
read-only baseline. An isolated shell agent then writes `reports/explicit.md`
and edits tracked `base.txt`. The test calls `post_run_lifecycle` without
manually persisting the result. An explicitly configured fake gws copies the
uploaded archive before returning a synthetic response; no provider is contacted.

The three tests assert:

- `read_only_stop_preserves_declared_report_in_single_backup`: Failed outcome,
  unchanged agent exit code, read-only error excluding the allowed report,
  failed configured verification without executing its command, exact original
  report bytes in both artifact and archived Output, failed status and the
  verification-not-run event in the archive, unchanged HEAD/index and violating
  file, and one upload/event after repeated real settlement.
- `read_only_stop_upload_failure_is_warning_only_and_not_retried`: the same
  report archive reaches the fake upload, upload failure adds a milestone,
  preserves the existing latest error/status/verification, stores no URL, and
  repeated settlement produces no second upload or backup event.
- `read_only_stop_missing_optional_report_without_backup_stays_failed`: no
  artifact is invented, no fake gws call occurs, verification stays Skipped,
  delivery assessment stays absent, and violation evidence remains reviewable.

The Markdown archive records verification through its existing event timeline;
its header has a task status field but no separate verify-status field.

## Executed verification

The unchanged production source at `291aa252`, with only the regression fixtures
added, reproduced the loss using:

```sh
AID_BUILD_BOX=<configured-box> scripts/remote-test.sh -- --locked --bin aid read_only_stop_
```

Remote job `8141f79d62ca4d09b0ed6f06f2df3385` exited 101: **1 passed,
2 failed, 0 ignored**. Both report-preservation cases failed while reading the
missing task artifact. The optional-report/no-backup control passed.

On the fix commit, `scripts/remote-test.sh -- --locked` completed as remote job
`7ba127558b9a4f5cb16ef539d4b2a5b7`, exit 0: **33 top-level test binaries, 3,040 passed,
0 failed, 14 existing ignored tests**. This includes all three new regressions.
Guide validation with `quick_validate.py default-skills/aid-guide` passed.

The outer AID verifier recorded a timeout after its fixed 120-second allowance,
while the remote job continued to completion. The completed rbox log establishes
the test result; the wrapper timeout remains a distinct result, not a test
failure or a pass. That deadline mismatch is tracked separately as `wi-8b23`.

Tests use synthetic executables and captured archives. No live Google Drive
compatibility is established. The other configuration and prelaunch gaps in
[investigation-backup118-2026-10-02.md](investigation-backup118-2026-10-02.md)
remain outside this fix. Independent review and combined-candidate verification
are recorded separately.
