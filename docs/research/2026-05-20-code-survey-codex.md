## Findings

### 1. Codex write-permission intent is not passed through to the Codex CLI

Severity: High

#### Current implementation

- `aid run` exposes `--sandbox` as a boolean CLI flag in `src/cli/command_args_a.rs:75`.
- Batch specs expose `sandbox` as a task/default boolean in `src/batch/schema.rs:84` and `src/batch/schema.rs:167`; `task_to_run_args` copies it directly into `RunArgs` at `src/cmd/batch_args.rs:112`.
- Foreground execution only uses `args.sandbox` to decide whether to wrap the already-built agent command in aid's Apple Container wrapper: `src/cmd/run_dispatch_execute.rs:200`, `src/cmd/run_dispatch_execute.rs:207`, `src/cmd/run_dispatch_execute.rs:216`.
- Background execution has the same meaning: `spec.sandbox` wraps the agent command with `crate::sandbox::wrap_command` at `src/background.rs:160` and `src/background.rs:164`.
- The sandbox wrapper is aid-owned container execution, not a per-agent CLI sandbox flag. `wrap_command` constructs `container run ... aid-sandbox:latest ...` in `src/sandbox.rs:46` through `src/sandbox.rs:104`.
- `wrap_command` only adds `--read-only` to the container when `read_only` is true: `src/sandbox.rs:53`.
- The Codex adapter builds `codex exec --json --skip-git-repo-check --full-auto <prompt>` in `src/agent/codex.rs:83` and `src/agent/codex.rs:84`.
- The Codex adapter never passes `-s`, `--sandbox`, `--approval-mode`, `--dangerously-bypass-approvals-and-sandbox`, or `--add-dir`.
- The only Codex sandbox-related config currently added is `sandbox_workspace_write.writable_roots=<gitdir>` for worktree git metadata, and only when a worktree `.git` file is detected: `src/agent/codex.rs:100`, `src/agent/codex.rs:101`, `src/agent/codex.rs:102`, `src/agent/codex.rs:203`.
- Existing tests explicitly assert that read-only Codex commands still use `--full-auto` and do not pass `-s` or `read-only`: `src/agent/codex.rs:691` through `src/agent/codex.rs:713`.
- Local Codex CLI help confirms current Codex supports `-s, --sandbox <read-only|workspace-write|danger-full-access>` and `--add-dir`, but aid does not set either.

#### Behavior

`sandbox=false` in aid means "do not run the agent process inside aid's Apple Container sandbox." It does not mean "force the underlying agent CLI to use a writable workspace sandbox."

For Codex specifically, aid always relies on whatever Codex's own default/profile/config sandbox mode is. If the user's Codex profile defaults to `read-only`, aid does not override it even when `read_only=false` and `sandbox=false`.

This explains the reported failure mode: a user writes `sandbox=false` expecting Codex to be able to write `src/`, but aid only disables aid's container wrapper. The Codex command itself is still launched without an explicit `-s workspace-write`, so Codex can remain read-only.

#### Gap / Shortcomings

- aid conflates two different concepts under `sandbox`:
  - aid-managed process/container sandboxing.
  - underlying agent CLI filesystem/approval sandboxing.
- `read_only=false` is not translated into a positive Codex write policy.
- The Codex adapter has no explicit permission model despite Codex exposing a first-class sandbox flag.
- The current `sandbox_workspace_write.writable_roots` workaround only covers the worktree Git metadata path; it does not select Codex workspace-write mode.

#### Recommendations

- P0: Add an explicit Codex sandbox mapping in `src/agent/codex.rs`.
  - When `opts.read_only == true`, pass `-s read-only` or the current safest Codex equivalent.
  - When `opts.read_only == false`, pass `-s workspace-write`.
  - If aid intentionally wants fully unrestricted Codex for externally sandboxed runs, add a separate opt-in field instead of deriving it from `sandbox=false`.
- P0: Rename or split aid's `sandbox` concept at the `RunArgs` layer.
  - Suggested code direction: keep `container_sandbox: bool` for aid's Apple Container wrapper and add `agent_sandbox: Option<AgentSandboxMode>`.
  - Avoid using `sandbox=false` as an implicit write-permission signal.
- P1: Add adapter tests that assert non-read-only Codex includes `-s workspace-write`, and read-only Codex includes `-s read-only`.
- P1: In `src/cmd/run_validate.rs`, warn when `agent=codex`, `read_only=false`, and no explicit Codex sandbox mode will be sent.

### 2. Agent permission behavior is inconsistent across Cursor, OpenCode, and Gemini

Severity: Medium

#### Current implementation

- Cursor always receives `--trust`.
  - Read-only Cursor: `-p --trust <prompt> --mode plan --output-format stream-json` at `src/agent/cursor.rs:31` through `src/agent/cursor.rs:40`.
  - Non-read-only Cursor: `-p <prompt> --trust --force --output-format stream-json` at `src/agent/cursor.rs:41` through `src/agent/cursor.rs:50`.
- OpenCode is excluded from aid's container sandbox support by `can_sandbox`: `src/sandbox.rs:11` through `src/sandbox.rs:21`.
- OpenCode read-only is prompt-level only and explicitly warns that it is not enforced: `src/agent/opencode.rs:25` through `src/agent/opencode.rs:40`.
- OpenCode always sets `OPENCODE_CONFIG_CONTENT` to allow external directory access: `src/agent/opencode.rs:48` through `src/agent/opencode.rs:52`.
- Gemini is not excluded by `can_sandbox`, so `--sandbox` can wrap Gemini in aid's container.
- Gemini's adapter comments that Gemini native sandboxing exists but aid manages sandboxing outside the adapter: `src/agent/gemini.rs:39`.
- Gemini read-only maps to `--approval-mode plan`; non-read-only maps to `-y`: `src/agent/gemini.rs:40` through `src/agent/gemini.rs:44`.

#### Behavior

- Cursor trust is always set by aid, regardless of read-only. Read-only changes Cursor mode to planning, while non-read-only adds `--force`.
- OpenCode receives no enforced read-only protection from aid and no aid container sandbox. Its read-only mode is only a prompt prefix.
- Gemini's permission behavior is split: aid container sandbox if `sandbox=true`, and Gemini CLI approval mode based on `read_only`.
- Codex is the outlier for the reported issue: it gets neither an aid-side write/read-only distinction beyond optional container wrapping nor a Codex CLI sandbox flag.

#### Gap / Shortcomings

The same aid fields (`sandbox`, `read_only`) mean different things per adapter. This makes specs hard to reason about:

- Cursor: `read_only` changes CLI mode.
- Gemini: `read_only` changes approval mode.
- OpenCode: `read_only` only changes prompt text.
- Codex: `read_only` only changes prompt text, while CLI sandbox is left to Codex defaults.

#### Recommendations

- P1: Add a per-agent permission capability table, for example `AgentPermissionSupport { enforced_read_only, write_mode_flag, container_supported, trust_flag }`.
- P1: Surface the effective permission plan in dry-run and task events before launch.
  - Example event: `Permissions: aid_container=false, agent_sandbox=workspace-write, trust=true`.
- P2: For OpenCode, either wire the current OpenCode permission flags if available or make `read_only=true` fail unless `worktree` is used, because the code already knows it is not enforced.

### 3. Audit-report mode auto-detection is broad and can silently convert implementation/research tasks into result-file report tasks

Severity: Medium

#### Current implementation

- Auto-detection is implemented in `src/cmd/report_mode.rs`.
- Explicit audit terms are defined at `src/cmd/report_mode.rs:9` through `src/cmd/report_mode.rs:17`:
  - `audit`
  - `cross-audit`
  - `cross audit`
  - `adversarial audit`
  - `review`
  - `code review`
  - `peer review`
- Structured finding terms are defined at `src/cmd/report_mode.rs:18` through `src/cmd/report_mode.rs:24`:
  - `findings`
  - `pass/fail`
  - `severity`
  - `evidence`
  - `open questions`
- `is_audit_report_task` lowercases the prompt and checks substring membership: `src/cmd/report_mode.rs:31` through `src/cmd/report_mode.rs:40`.
- Explicit audit terms trigger report mode regardless of `read_only`: `src/cmd/report_mode.rs:34`.
- The structured-finding branch requires `read_only=true`, category `Research`, `Documentation`, or `Debugging`, and at least one structured term: `src/cmd/report_mode.rs:35` through `src/cmd/report_mode.rs:40`.
- Task category comes from the classifier in `src/agent/classifier.rs:139` through `src/agent/classifier.rs:164`.
- `research`, `investigate`, and related terms affect category selection but are not direct audit-report keywords:
  - Research prefixes/terms at `src/agent/classifier.rs:54` through `src/agent/classifier.rs:62`.
  - Debugging includes `investigate` at `src/agent/classifier.rs:83` through `src/agent/classifier.rs:92`.
- `apply_defaults` auto-sets `args.result_file = Some("result.md")` when report mode is detected and no output/result file exists: `src/cmd/report_mode.rs:43` through `src/cmd/report_mode.rs:50`.
- After a task ID exists, auto result files are changed to task-specific files like `result-t-123.md`: `src/cmd/run_dispatch_prepare.rs:203` through `src/cmd/run_dispatch_prepare.rs:207`, then `src/cmd/run_dispatch_prepare.rs:275` through `src/cmd/run_dispatch_prepare.rs:278`.
- Prompt assembly injects the `<aid-result-file>` instruction at `src/cmd/run_output.rs:11` and `src/cmd/run_output.rs:12`.
- Prompt assembly appends the Markdown audit report instruction at `src/cmd/run_prompt.rs:294` through `src/cmd/run_prompt.rs:298`; instruction text is in `src/cmd/report_mode.rs:65` through `src/cmd/report_mode.rs:75`.
- `--audit` / project `[audit].auto` is a separate cross-audit flow, not report-mode detection. Project defaults toggle `args.audit` in `src/cmd/run_dispatch_resolve.rs:58` through `src/cmd/run_dispatch_resolve.rs:62`; post-run AIC execution happens in `src/cmd/run_post.rs:73` through `src/cmd/run_post.rs:115`.

#### Behavior

When report mode triggers, aid:

- May auto-create a result file target.
- Tells the agent to write structured findings/results to that file.
- Appends instructions requiring a Markdown audit report starting with `## Findings`.

It does not automatically set `read_only=true`. It also does not enforce "do not edit code" unless the chosen adapter's read-only mode is separately active.

#### Gap / Shortcomings

The explicit keyword branch is too broad because it uses substring matching and does not require read-only intent. Known risky triggers:

- `review`: any prompt asking "review and fix", "review the design", or "review the implementation then update it" enters report mode.
- `audit`: prompts like "add audit log", "implement audit trail", or "redesign the audit subsystem" enter report mode. Batch warning logic excludes `audit trail` and `audit log` at `src/batch/warnings.rs:84` through `src/batch/warnings.rs:86`, but `report_mode` does not.
- `peer review`: can trigger even when the desired output is code changes after peer review.
- `research` alone does not trigger, but `read_only=true` plus "findings", "evidence", "severity", "pass/fail", or "open questions" does.
- `investigate` alone does not trigger, but `read_only=true` plus a structured term does because `investigate` classifies as debugging.
- `redesign` alone does not trigger directly; however, a prompt like "review the redesign" triggers via `review`.

The flow can make an implementation task look like a report task by auto-injecting result-file and audit-report instructions before the agent sees the prompt.

#### Recommendations

- P0: Tighten explicit audit detection in `src/cmd/report_mode.rs`.
  - Require either `read_only=true`, explicit `result_file`, or an explicit `audit_report=true` field before auto result-file mode.
  - Add exclusions for `audit log`, `audit trail`, and implementation verbs near `audit`/`review`.
- P1: Replace substring matching with token/phrase-aware matching.
  - Reuse or extend the word-boundary helper pattern from `src/agent/classifier.rs:212` through `src/agent/classifier.rs:223`.
- P1: Add tests for false positives:
  - "Add an audit log feature"
  - "Review and implement the requested fix"
  - "Redesign the audit subsystem"
  - "Investigate and fix the crash"
- P1: Make report-mode activation visible in stored task metadata, not just a launch log.
  - Suggested field: `task.delivery_mode = code | report | audit_report`.

### 4. aid partially observes "done but no code changes", but text board hides the most important signal

Severity: Medium

#### Current implementation

- Task completion status is based on process exit status.
  - Streaming watcher maps exit success to `TaskStatus::Done` and failure to `TaskStatus::Failed`: `src/watcher.rs:151` through `src/watcher.rs:180`.
  - Foreground runner stores `info.status` in the task table: `src/cmd/run_agent.rs:159` through `src/cmd/run_agent.rs:167`.
  - Store persistence writes `status`, timing, model, cost, and `exit_code`: `src/store/mutations.rs:270` through `src/store/mutations.rs:289`.
- The task domain stores both `status` and `delivery_assessment`: `src/types/task.rs:20`, `src/types/task.rs:37`, and `src/types/task.rs:47`.
- Delivery assessment currently has `empty_diff` and `hollow_output`: `src/types/delivery.rs:7` through `src/types/delivery.rs:31`.
- Empty worktree diff is flagged only when all of these are true:
  - task is not read-only,
  - task status is done,
  - verify status is skipped,
  - task has an existing worktree path,
  - worktree snapshot reports empty diff.
  Implementation: `src/cmd/run_post.rs:119` through `src/cmd/run_post.rs:138`.
- Hollow output is flagged only for done tasks with skipped verify, less than 200 characters of saved output, and no worktree changes: `src/cmd/run_lifecycle.rs:507` through `src/cmd/run_lifecycle.rs:535`.
- `aid show` displays `[no changes]` when `delivery_assessment` implies no changes: `src/cmd/show.rs:233` through `src/cmd/show.rs:237`.
- `aid show --json` includes `exit_code`, `verify_status`, and `delivery_assessment`: `src/cmd/show_json.rs:71` through `src/cmd/show_json.rs:73`.
- `aid board --json` includes `verify_status` and `delivery_assessment`: `src/cmd/board.rs:255` and `src/cmd/board.rs:256`.
- Text `aid board` renders only the task status, verify failure suffix, running milestones, and failed-task errors. It does not append delivery assessment for done tasks: `src/board.rs:87` through `src/board.rs:101`, and `src/board.rs:355` through `src/board.rs:369`.
- `aid tree` does show `[delivery:<value>]`: `src/cmd/tree.rs:80` through `src/cmd/tree.rs:87`.

#### Behavior

aid can distinguish "process exited successfully" from "task produced no code changes" in some cases:

- For worktree tasks with no verification, empty diff is persisted as `delivery_assessment=empty_diff`.
- For tasks with tiny/no output and no changes, hollow output is persisted as `delivery_assessment=hollow_output`.
- JSON views expose the field.
- `aid show` exposes it.

But the default text `aid board` still shows the task as `DONE`, without surfacing `empty_diff` or `hollow_output`.

There is also an exit-code observability wrinkle: foreground streaming tasks record the exit code in the completion event text, but `watch_streaming` does not set `info.exit_code` before returning. The event contains `exit code 0` at `src/watcher.rs:162` through `src/watcher.rs:167`, while `info.status` is set at `src/watcher.rs:180`; no `info.exit_code = exit_status.code()` exists in this path. Background PTY streaming does set it at `src/pty_watch.rs:447` through `src/pty_watch.rs:454`.

#### Gap / Shortcomings

- `aid board` text, the primary monitoring surface, hides delivery assessment.
- Empty diff detection is worktree-only and skipped when `verify_status != skipped`.
- In-place tasks with no uncommitted diff are not persisted as `empty_diff`; `aid show --diff` only says the edit may already be committed: `src/cmd/show_output_diff.rs:46` through `src/cmd/show_output_diff.rs:56`.
- Foreground streaming success exit codes are not stored in `tasks.exit_code`, even though the completion event has the exit code.
- There is no explicit "expected code changes" vs "report-only deliverable" model, so aid cannot reliably detect semantic drift from implementation to report.

#### Recommendations

- P0: Show delivery assessment in text `aid board`.
  - Suggested change: update `task_status` in `src/board.rs:355` to append `[delivery:empty_diff]` or `[delivery:hollow_output]` for terminal tasks.
- P0: Store foreground streaming exit codes.
  - Suggested change: in `src/watcher.rs`, after `let exit_status = child.wait().await?`, set `info.exit_code = exit_status.code()` before returning.
- P1: Expand empty-diff detection to in-place tasks using `start_sha`.
  - For non-worktree tasks, compare `start_sha..HEAD` plus working tree diff before concluding no changes.
- P1: Do not skip empty-diff assessment just because verification ran.
  - A task can pass verification while still making no changes if tests were already green.
- P1: Add a persisted `delivery_mode` or `expected_artifact` field.
  - Examples: `code_change`, `report_file`, `final_answer_only`, `verification_only`.
  - Use it to flag "implementation-like prompt produced only result_file and no code diff".

### 5. result_file content with empty git diff is treated as a valid done task unless worktree empty-diff detection catches it

Severity: Medium

#### Current implementation

- Prompt injection tells the agent that `<aid-result-file>...</aid-result-file>` is the official result: `src/cmd/run_output.rs:11` and `src/cmd/run_output.rs:12`.
- Postprocess copies the result file into the task artifact directory as `result.md`: `src/cmd/run_lifecycle.rs:416` through `src/cmd/run_lifecycle.rs:428`, using `src/cmd/run_output.rs:17` through `src/cmd/run_output.rs:32`.
- Background has a parallel result-file persistence path at `src/background.rs:284` through `src/background.rs:286`.
- `aid show --result` reads the persisted task `result.md`: `src/cmd/show.rs:159` through `src/cmd/show.rs:169`.
- Missing result for prompt-only audit tasks surfaces a retry banner: `src/cmd/show.rs:175` through `src/cmd/show.rs:194`.

#### Behavior

If a task writes a non-empty result file and makes no code changes:

- Task status can still be `DONE`.
- The result file is preserved and shown by `aid show --result`.
- If it was a worktree task, `delivery_assessment=empty_diff` may be set.
- If it was an in-place task, there may be no persisted `delivery_assessment`.
- The result file itself can prevent the outcome from looking hollow, because the agent produced a deliverable, but aid does not know whether that deliverable was appropriate for the original task.

#### Gap / Shortcomings

aid currently observes artifacts, not intent. It can say "there is a report" and sometimes "there is no code diff", but it cannot say "this task was supposed to modify code and only wrote a report" except by human inspection.

#### Recommendations

- P0: Track expected deliverable at dispatch time.
  - Derive an initial value from explicit flags and classifier:
    - `result_file` or report-mode: report expected.
    - implementation/refactor/testing/simple-edit categories without read_only: code changes expected.
    - read_only: no code changes expected.
- P1: On completion, compare expected deliverable to observed artifacts.
  - `expected=code_change` and `diff_empty=true` and `result_file_present=true` should become a first-class warning, for example `delivery_assessment=report_only_for_code_task`.
- P1: Store report-mode activation separately from `result_file`, so user-specified result files do not get conflated with auto audit reports.

## Open Questions

- What should `sandbox=false` mean going forward: only "do not use aid's container", or "ensure the agent can write the workspace"? The current code implements only the first meaning.
- Should Codex non-read-only mode be `-s workspace-write` or `--dangerously-bypass-approvals-and-sandbox` when aid already uses worktrees/containers? The safer default is `workspace-write`; full bypass should be explicit.
- Should audit-report mode be opt-in for write-capable tasks, or should `review` continue to auto-trigger report files even when `read_only=false`?
