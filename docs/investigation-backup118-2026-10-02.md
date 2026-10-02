KB consulted: `backup artifact gdrive lifecycle` returned general artifact-custody and audit lessons, with no direct backup-specific match. Relevant lessons: `a-pending-acceptance-criterion-needs-a-denominator` and `a-write-denied-auditor-cannot-run-a-compiler-that-writes-artifacts`.

# Issue #118: backup acceptance reconciliation

Source baseline: `291aa2521028adce4b10860e923d57568ff1d4ed` (v10.50.0 plus roadmap documentation). The backup subsystem is implemented. The following source paths still need regression coverage and correction; this investigation does not establish live Google Drive compatibility or close #118.

## Result custody on early settlement exit — `wi-562b`

`post_run_lifecycle` invokes `backup::on_settled` after `run_lifecycle_phases`, including an early Stop. A read-only violation marks the task Failed and preserves the checkout, but the Stop branch precedes `run_task_postprocess_phase`, where the declared result is normally persisted. Export reads the task's persisted `result.md` or output path rather than the original declared result. A completed allowed report can consequently be absent from the one-attempt backup.

Baseline evidence: `src/cmd/run/lifecycle.rs` lines 71–78, 109–125 and 486–493; `src/cmd/run/read_only.rs` lines 47–60; `src/cmd/export.rs` lines 137–149; `src/backup/mod.rs` line 60.

Acceptance: exercise a real read-only violation with an already-written allowed report, capture the fake-gws archive, and assert the exact report content, Failed outcome, final verification state, one upload, and preserved violating edit. Reuse result persistence on the early Stop path while retaining ordinary verification and postprocessing order.

## Configuration discovery for in-place execution — `wi-56ec`

Backup configuration discovery only consults `task.repo_path` and `task.worktree_path`. Ordinary cwd or `--dir` dispatches can have neither path while retaining `effective_dir` and saved dispatch arguments. Project backup settings can therefore be omitted; a CLI target override can also miss project folder/include/on settings.

Baseline evidence: `src/cmd/run/prompt_helpers.rs` lines 216–219; `src/cmd/run/dispatch_worktree.rs` lines 204–218; `src/backup/config.rs` lines 163–173.

Acceptance: reproduce cwd, explicit `--dir`, detached checkout and explicit repo/worktree execution with captured uploads. Resolve configuration using the existing target-project contract and test conflicting main-repository and checkout-local settings.

## Worker failure before agent launch — `wi-9942`

Saved dispatch arguments precede agent process launch. The worker's error fallback can mark a task Failed and invoke the common lifecycle after command preparation fails. Backup admission checks Done/Failed and saved arguments but does not establish that agent execution began. This path can consume an attempt or upload on a prelaunch failure, contrary to the guide's pre-agent exclusion.

Baseline evidence: `src/background.rs` lines 99–119 and 148–163; `src/background_launch.rs` lines 20–40; `src/background_lifecycle.rs` line 28; `src/backup/config.rs` lines 152–155; `src/backup/mod.rs` lines 53–60.

Acceptance: a command-preparation failure with saved backup intent produces zero fake-gws calls and zero backup claims/markers, while a legitimate failed agent execution retains its configured backup behavior.

## Malformed project configuration — `wi-b7d8`

Project discovery discards parsing errors with `.ok()`. Invalid types or unknown keys under `[backup]` can therefore disappear rather than produce the warning promised by the guide. Correctly typed unsupported include/on strings and unknown targets do reach the warning path.

Baseline evidence: `src/project/discovery.rs` lines 19–25; `src/project.rs` lines 253–257; `src/backup/config.rs` lines 16–23 and 171–173; `default-skills/aid-guide/references/configuration.md` lines 125–128.

Acceptance: cover `target = 42`, an unknown key and unsupported include/on values; distinguish absent configuration from invalid configuration and preserve the successful task outcome when recording a backup warning.

## Executed baseline and limits

On the baseline SHA, `AID_BUILD_BOX=<configured-box> scripts/remote-test.sh -- --locked --bin aid backup::` completed with exit 0: **22 passed, 0 failed, 0 ignored**, with 2,859 tests filtered out. Remote job: `0de4373ecf774d0abda3460645319887`.

Those tests cover the existing bundle, adapter, warning and one-attempt behavior using fake gws. They do not reproduce the four gaps above. The source review ran no tests of its own; the remote run is separate executed evidence. No real Drive upload, authenticated gws probe or issue closure was performed. Required remaining evidence includes the new failure-path regressions and a controlled CLI round trip through completion, archive inspection, persisted URL and `aid show`.
