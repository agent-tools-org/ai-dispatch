# Delivery and Worktree Execution

## Result delivery

Prompts expressing `read-only … audit`, including `read-only audit` and
modifiers such as `read-only comparative audit`, `read-only cross-audit`, or
`read-only re-audit`, are dispatched as report tasks from the prompt alone.
`--read-only` still permits writing the task result file and audit report; it
forbids modifying the repository under test. AID auto-selects a task-specific
result file at `~/.aid/tasks/<id>/result.md` for host launches and omits implementation
methodology and Git staging instructions. Explicit `--result-file` paths remain delivery destinations.
For native Codex read-only host runs, the agent writes `agent-result.md` and `agent-output.txt`
in a fresh, empty staging directory outside the repository and the trusted task directory.
That staging directory is the run's only writable grant. Dispatch refuses destinations inside
the repository, its Git metadata, staging, or any other Codex-writable root, resolving existing
symlinks and parent traversal. Dispatch also refuses a trusted task directory under `AID_HOME`
that resolves inside the repository or its Git common directory. Aid opens each staged source
once without following links, checks that descriptor for a regular file with a single link,
and uses its captured content for every sink. It writes a temporary file in the destination
directory and renames it over the destination, replacing external symlinks without changing
their targets. Rejected publication marks the task Failed and skips official report copying. Report and
output paths resolving to the same destination are rejected at dispatch; choose separate paths
or request one artifact. Missing or refused staged reports/outputs and failed official-report
copies record task events and a missing-delivery assessment. CLI, TUI and web report preference
then show "not delivered by this run" instead of selecting an old destination or official report.
The requested parent directory receives no sandbox grant. Container launches retain their existing delivery path.
Container launches retain their checkout-relative automatic report path.
This prompt formatting decision is independent of dirty-worktree enforcement.
Implementation noun phrases such as `add an audit log` and write requests such as
`add tests for the read-only audit module` or
`make changes to the read-only audit logic` remain normal writable tasks.
Write verbs after the audit phrase also keep implementation scaffolding unless
they are negated, as in `do not modify` or `without modifying`.
An explicit `--result-file` controls report formatting and delivery; it does not
by itself remove implementation methodology or Git staging instructions.
A declared writable kind (`--kind simple-edit|complex-impl|frontend|testing|refactoring`
without `--read-only`) is never turned into a report task by prompt wording.
The kind carries over to `aid retry`, so a brief that discusses audits,
reviews, or findings stays an implementation task on every attempt. To get a
report, declare `--kind research|debugging|documentation` or pass `--read-only`.

Unsupported agent and flag combinations are refused before a task row is created,
with an error that names what to do instead. The same preflight resolves the agent
command (built-in or custom) and refuses when the binary is missing from `PATH`,
naming the missing binary.
Unknown models that the agent CLI does not report as invalid are passed
through.

If the agent process still fails to start after preflight, the task ends in
`failed` with an agent-spawn error — it is never left `running` with no worker.

When `--result-file` is set (audit and review prompts set it automatically) and
the agent never writes that file, AID salvages the captured agent output into
the task's `result.md` so evidence is not lost. If that output is pre-tool
narration rather than a report, AID records a `missing_final_delivery`
assessment and an error event. `aid show` then prints the missing-result banner
instead of presenting the tool log as findings. Treat that banner as "no audit
happened" and re-dispatch.

## Native Codex permissions

Fresh and resumed Codex sessions explicitly select a permissions profile. Read-only runs extend
`:read-only` and disable approval escalation; write runs extend `:workspace` and use automatic
approval review. The adapter uses `permissions` and `default_permissions`, without legacy
`sandbox_mode`, `sandbox_workspace_write`, `--sandbox`, or write approval shortcut flags.

Aid keeps the executed read-only attempt's native profile identity, canonical writable roots,
approval policy and transport in a private database record, outside the task event stream.
Non-read-only tasks write no provenance. App-server records after thread configuration succeeds;
exec records after spawn. Exec fallback and resume retain the configuration, and each new attempt
clears older proof. A later unproven launch replaces the proof with an unknown record.
Changes outside the designated result file, `-o/--output` file and aid-owned paths fail with
`Read-only violation` unless the executed attempt has proven restrictive native permissions.
For a proven launch, checkout differences produce a warning naming the
baseline/current HEAD and changed paths; input stability is unknown. Aid preserves the agent's result.
The audited inputs changed during the run; the warning does not verify conclusions against one
immutable revision. Missing or unknown provenance still fails closed, including older task rows,
Claude and custom launches. Snapshot/configuration failures still fail. The native boundary covers
local filesystem writes, not effects through external services or privileged brokers.

The exec fallback uses `--ignore-user-config` to exclude user sandbox defaults. App-server
instead uses a task-scoped `CODEX_HOME` with symlinks to every real-home top-level entry except
`config.toml` and `config.toml.*`. Authentication, session storage, instructions, hooks, rules,
and skills remain shared; the task-scoped directory is removed after execution.
The next Codex launch sweeps directories and markers left by dead workers, checking PID start
identities before preserving a live process or terminating an orphaned task server. The requested
model is passed explicitly when configured by aid. Other user config settings are not loaded. Project config layers require trust in
Codex; absent user trust records, they remain inactive. Managed requirements can reject profiles.

The read-only staging root is resolved through symlinks and parent traversal. A root containing
or contained by the repository top level, primary checkout, worktree, Git common directory,
or trusted task directory is rejected. The task directory and Cargo target receive no write grant.
Aid's official result persistence, automatic `output.md`, and transcript writers remain outside staging.
Those writers use ordinary path writes into `AID_HOME`; the guarantee assumes `AID_HOME`
and its subdirectories (database, logs, jobs, tasks) are outside every repository a
read-only task runs against and contain no links planted into one. Dispatch checks
only the resolved task directory. `aid show --result` reads the stored `result.md`
as-is and does not apply the missing-delivery assessment; `aid wait` prints the latest
error event, which can be an earlier failure rather than the delivery assessment.

Aid publishes staged artifacts after settlement and persists the staged report as the official
result. Delivery failures mark the task Failed. Ref enforcement checks this run's HEAD and branch;
updates to recovery refs, sibling branches, remote-tracking refs, or tags produce diagnostic events.

### Native test notes

The native permission-transition test is ignored by default. It requires an already installed
Codex CLI and its OS sandbox on an authorized build box; do not install Codex to run this test.
On a box meeting those requirements, invoke it explicitly:

```bash
AID_BUILD_BOX='<box-with-codex>' scripts/remote-test.sh --lock-timeout 3600 -- --bin aid \
  fix_round_one_native_codex_fresh_and_resumed_permission_transitions -- --ignored --nocapture
```

## Worktree safety

```bash
aid worktree create feat/change
aid worktree list
```

AID task worktrees are custody containers, not disposable scratch directories.
Completion, failure, stop, retry, and merge preserve them. Do not use raw
`git worktree prune` or direct `git worktree remove` on them.

If AID reports missing worktree registration or conflicting metadata, stop and
identify the owning task. Automatic pruning is intentionally forbidden because
linked-worktree Git metadata may contain unique submodule objects.

Each worktree holds a `.aid-lock` lease for the active task. Unrelated tasks
still collide and are refused. A nested child whose `parent_task_id` chain
reaches the lease holder may re-enter the same worktree so edits stay on the
parent branch.

Creating a worktree also adds AID's own runtime files — `.aid-*`, `aid-batch-*`,
`result-t-*.md` — to the repository's local `.git/info/exclude`. That file is never
committed and the repository's `.gitignore` is left alone; it exists so an agent
running `git add .` cannot commit AID's lease file. Dirty-worktree enforcement
ignores those paths as well, so AID clearing its own lease at task end is not read
as the agent leaving work uncommitted.

## Recursive delegation

Agents already receive `AID_TASK_ID` (and now `AID_TASK_DEPTH`) and can run
`aid run` from inside a task. When `AID_TASK_ID` is set:

- `aid run` fills `parent_task_id` from it so `aid tree` shows the child.
- Depth is parent depth + 1 and dispatch beyond depth `2` is refused.
- `--bg` is refused; the child must finish before the parent releases the lease.
- Child `--difficulty` / `--budget` may not exceed the parent's declared values.

```bash
# inside an agent process (AID_TASK_ID already set):
aid run opencode "Extract the parser helper" \
  --dir . \
  --worktree feat/request-validation \
  --difficulty simple --budget cheap --urgency normal --rigor draft
```

## Build and test

```bash
aid build check
aid build clippy -- --all-targets
aid test --bin aid
aid test --bin aid my_module::my_test -- --exact
aid test -- my_filter
aid test -- my_filter --exact
aid test --isolated --bin aid
```

Use `aid build` for compile checks. It emits compact, deduplicated diagnostics
and integrates progress into task events. `aid build` no longer accepts `test`
as a command — use `aid test` so agents cannot mistake a zero-match filter or
empty target set for a green suite.

`aid build` guarantees:

- A run that matched no build targets never looks like a pass (for example
  `cargo check --lib` on a binary-only crate).
- A cached no-op build (cargo exit 0, everything already fresh) is still success.
- Task events say `succeeded` or `failed`, not an ambiguous `finished` line.

`aid test` reuses the same cargo process supervision and diagnostic pipeline as
`aid build`, then parses libtest stdout. Guarantees:

- A filter that matches zero tests exits non-zero and names the filter.
  Filters may be positional (`aid test name`) or free args after `--`
  (`aid test -- name`), matching cargo muscle memory.
- Target selectors are aid flags only: `--lib`, `--bin NAME`, `--test NAME`
  (integration-test *target*, not a name filter). Do not put them after `--`.
- A run with no test targets never looks like a pass
- The digest lists which tests ran (not only a pass count)
- Failure output stays compact (panics and assertion diffs)

`--isolated` gives the cargo test process a temporary `AID_HOME` so the run
cannot read or pollute the developer's `~/.aid/`. It also clears nested
`AID_TASK_ID` / `AID_TASK_DEPTH` in that child so unit tests that call
`prepare_dispatch` are not refused as over-depth when `aid test` itself runs
inside a task. It is opt-in, not the default.

Inherited `CARGO_TARGET_DIR` wins for the first attempt. If cargo cannot write
that directory (common under agent OS sandboxes that only allow the worktree
and temp dirs), `aid build` / `aid test` retries once under the system temp
directory (`aid-build-target/<project-key>/`) and records the fallback paths in
the digest. Do not preflight with a generic write probe — the fallback is keyed
off cargo's real permission error.

## Codex app-server transport

Host Codex runs probe `codex app-server` once per dispatch and use its stdio
protocol when available; unavailable app-server launches retain `codex exec`.
Container and sandbox wrappers retain their exec transport.
App-server preserves the native permission profile, approval review, requested
model, effort overrides, and working directory. Retry resumes through
`thread/resume` wherever exec would resume. `steer` and `reply` require a
confirmed `turn/steer` response; `respond` remains refused.
App-server uses a task-scoped `CODEX_HOME` whose top-level entries symlink to
the real Codex home, excluding `config.toml` and `config.toml.*`. Auth, sessions,
instructions, rules, hooks, and skills remain shared. The directory is removed
when the app-server process ends. Rate-limit notifications retain usage
percentages, reset times, window lengths, and account/spend-control evidence;
they do not supply aidbar billing data.

### App-server recovery and completion

Startup RPCs share the piped exec first-output budget (`idle-timeout`), and `thread/resume`
sets `excludeTurns`. A startup failure before a turn is submitted records its cause and uses
exec for the same task. A submitted turn with an unconfirmed start is never replayed in exec.
Silent descendant CPU work and remote waits use the same activity probe and hung-retry evidence
as the exec watcher; the hard deadline still applies.

Automatic approval review remains server-controlled. Client approval requests are declined without
ending the turn; permission requests receive no grant. Input requests receive an empty answer and
MCP elicitation is canceled. Requests that require unavailable client capabilities fail clearly.
After completion, aid closes input and drains final notifications and session writes within a
bounded shutdown window. Rate-limit windows at 100% do not create a hold when paid or unlimited
credits remain available; explicit credit/spend-control refusals remain evidence of a hold.

## Completion judgment and exit status

`TaskStatus` answers lifecycle and integration: whether a task is running, has
delivered artifacts, or has been merged. `VerifyStatus` answers what happened
to verification. Delivery assessment records empty-diff / hollow / missing-final
observations. `TaskOutcome` is derived from those facts and is the only axis
that answers whether the task succeeded.

The terminal outcomes are:

- `Verified`: delivered and verification passed.
- `Delivered`: delivered without required verification.
- `Broken`: delivered but verification failed.
- `Unverified`: delivered, but verification was inconclusive because it timed
  out, failed as infrastructure, produced no result, or the agent exited
  unobserved because no completion event survived the agent exit (kill and
  success cannot be told apart).
- `Failed`, `Stopped`, and `Skipped`: the task did not produce a successful
  delivery; `InProgress` is non-terminal.

Only `Verified` and `Delivered` are success outcomes. Hollow or missing-final
delivery assessments demote an otherwise successful outcome to `Failed` so
empty runs do not inflate `aid stats` / `aid advise`. `empty_diff` alone does
not demote. A foreground `aid run` exits 0 only for success outcomes; all other
outcomes use a non-zero exit. Do not read `Done` or `Merged` as success without
checking the outcome.

When `aid wait` observes `verify_status = pending`, it
continues waiting for verification, bounded by the verification timeout. Once
the task settles, either command returns non-zero if the outcome is not a
success outcome.
