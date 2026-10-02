KB consulted: `backup artifact gdrive lifecycle`; no direct backup-specific lesson found. Relevant matches: `a-pending-acceptance-criterion-needs-a-denominator` and `a-write-denied-auditor-cannot-run-a-compiler-that-writes-artifacts`.

# Read-only settlement result preservation

Source base: `291aa2521028adce4b10860e923d57568ff1d4ed`.
Scope: the declared report loss path tracked by wi-562b in issue #118.
The implementation and reproducer are recorded in the same commit as this file;
its SHA can be obtained with `git log -1 --format=%H -- docs/investigation-backup-result-2026-10-02.md`.

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

## Commands and results

Remote commands to execute with `AID_BUILD_BOX=<configured-box>`:

```sh
rbox status --json
rbox ensure "$AID_BUILD_BOX"
aid build check -p ai-dispatch
aid test -p ai-dispatch --bin aid read_only_stop_
aid test -p ai-dispatch --bin aid run_lifecycle_verify_gate_tests
aid test -p ai-dispatch --bin aid backup::lifecycle_tests
aid test -p ai-dispatch --test read_only_enforcement_e2e
aid test -p ai-dispatch --test aid_guide_e2e
scripts/remote-test.sh -- --bin aid
```

Fleet discovery and ensure returned `tailscale status returned no JSON
(Expecting value at byte 0)`. No remote Rust check, compilation, or test ran.
Regression failure on the unchanged base and success with the fix have not been
demonstrated at runtime. The configured full-suite verification remains required.

Completed checks:

- Extracted shell fixtures: fake agent writes exact report and violating edit;
  missing-report variant writes only the edit. Fake gws captures identical tar
  bytes, responds to folder operations, and fails only upload when requested.
- Fake-rbox routing for `aid build check -p ai-dispatch` and
  `aid test -p ai-dispatch --bin aid read_only_stop_`: both reached `rbox exec`
  with untracked-file sync enabled. The stub stopped before transport with exit
  64; both aid commands failed, and no Rust test executed. Output included
  `FAKE_RBOX: stopped before transport; no Rust tests ran`.
- `scripts/remote-test.sh --dry-run -- --bin aid`: one rbox command, no execution.
- Rustfmt parsed the changed Rust files; the new test file passes its targeted
  formatting check with `skip_children=true,max_width=120`. No workspace formatting.
- `git diff --check`: passed. New test file is 291 lines; its functions are under
  50 lines. The existing oversized lifecycle file has only five added lines.
- Guide quick validation could not run because the available Python lacks `yaml`.

Shell smoke checks and Rust parsing do not establish Rust type correctness or
lifecycle execution. Tests use synthetic local executables and archives only.
