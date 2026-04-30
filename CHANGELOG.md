## v8.99.2 (2026-04-30)
- feat(byok): add `aid byok` subcommand (`apply`, `remove`, `probe`, `example`, `doc`) — wraps the embedded BYOK shell scripts so cargo-installed users get the full opencode custom-provider flow without cloning the repo. The raw `scripts/aid-byok-*.sh` entry points remain as a lower-level fallback; env overrides (`OPENCODE_CONFIG_DIR` / `OPENCODE_AUTH_DIR` / `AID_HOME`) and exit codes are identical.


## Unreleased
- feat(byok): add bash+jq BYOK provider scaffolding for opencode custom providers, including apply/probe/remove scripts, a MiMo example manifest, sandboxed script coverage, and user docs for routing OpenAI-compatible providers through generated aid custom agents.


## v8.99.1 (2026-04-28)
- fix(commit): skip markdown bullets in rescue commit subject (#122, #123) — `extract_task_summary` now skips lines starting with `- `, `* `, `+ `, or `<digits>. ` in both the `[Task]`-section parser and the fallback loop. When neither pass yields a usable line, falls back to a generic `agent commit (task <task-id-short>)`. Previously, when a brief lacked an explicit `[Task]` header, the rescue commit subject would be the first injected `[Team Knowledge]` markdown bullet, truncated to 60 chars.


## v8.99.0 (2026-04-28)
- fix(watcher): kill process group + bound stderr drain in kill paths (#116, #117) — `watch_streaming` kill paths (idle timeout, cost ceiling, stuck-loop detection) now `force_kill_process_group` before draining stderr, and every stderr-capture handle await is wrapped in a 2s timeout via the new `drain_stderr_capture` helper. Previously, descendant processes kept the stderr pipe open after a kill, blocking `watch_streaming` from returning, leaving the task status stuck on `Running` and `aid watch --quiet` hung indefinitely. Extracted `force_kill_process_group` / `cleanup_process_group` into a shared `crate::process_group` module.
- fix(codex): include worktree git metadata in sandbox writable roots (#115, #119) — when codex is dispatched into a git worktree, `build_command` now resolves `<dir>/.git`, parses the `gitdir:` line, and appends `-c sandbox_workspace_write.writable_roots=[<canonical-metadata-path>]` so `git add` / `git commit` inside the codex sandbox can write to the parent repo's `.git/worktrees/<name>/index.lock`. Regular repos and missing `.git` no-op cleanly. Removes the rescue-commit churn that polluted git history with garbled messages.
- fix(worktree): protect active worktrees from prune + expose --json/--active (#114, #120) — `aid worktree prune` now reads `.aid-lock` and skips any worktree whose pid is alive, regardless of age. `aid worktree list --json` emits structured per-worktree records (`path`, `branch`, `active`, `lock_pid`, `lock_task_id`, `modified_age_secs`) for external tooling. `aid worktree list --active` filters human output to live-locked worktrees. README documents the `.aid-lock` contract for external cleanup tools.


## v8.98.0 (2026-04-28)
- feat(worktree): relocate aid-managed worktrees from `/tmp/aid-wt-{branch}` to `~/.aid/worktrees/{project-hash}/{branch}` so macOS `/tmp` cleanups no longer destroy in-progress work. Project ID is `{repo-basename}-{8-hex-hash-of-canonical-path}` to prevent same-basename repos from colliding. Old `/tmp/aid-wt-*` paths are still recognized by `aid worktree prune` and `aid clean --worktrees` for cleanup of pre-upgrade worktrees.
- fix(worktree): harden sandbox checks across `clean`, `merge_git`, `run_verify`, and `worktree_gc` — `is_aid_managed_worktree_path` now normalizes paths before prefix matching, rejecting traversal-shaped paths like `~/.aid/worktrees/../../etc`. Added a sandbox guard to `worktree_gc::remove_worktree_path` that previously ran `git worktree remove` on any DB-stored path without verification.
- fix(worktree): `aid run` invoked from inside a linked worktree now derives `{project}` from the main repo (via `git rev-parse --git-common-dir`) instead of the linked-worktree basename, so the resulting worktree lands under the correct project directory.
- chore(agents): hide `claude` from the default agent registry to keep `aid run auto` selection focused on agents with reliable headless execution.
- fix(test): update `retry_uses_fallback_when_rate_limited` to use Copilot instead of Claude in its pinned detected-agent set, since Claude was removed from the fallback chain in the same change above.


## v8.97.0 (2026-04-27)
- fix(tui): the FAIL "Reason" line now surfaces the FIRST Error event (the trigger), not the LAST. On cascade failures (loop kill → process failed → rescue → verify failed) users were seeing "Reason: Failed during verification ..." even though the real cause was the loop kill — making it look like verify failure was the trigger when it was just a downstream symptom.


## v8.96.0 (2026-04-27)
- fix(droid): stop emitting duplicate ToolCall events for `tool_result` and `tool_use` — these are already paired with `tool_call` and were doubling the LoopDetector input, causing false-positive loop kills (~5 legit reads → 10 events with detail "Read" → kill)
- fix(tui): render tool calls concisely in the Output tab — known primary keys (`file_path`, `path`, `directory_path`, `url`, `command`, `pattern`, `query`, `prompt`) are surfaced as `[Tool] <value> (k=v, ...)` instead of dumping the raw single-line JSON; unknown shapes still fall back to JSON, capped at 160 chars with an ellipsis


## v8.95.0 (2026-04-27)
- fix(droid): use `--append-system-prompt-file` for context files instead of `-f` (which means "read prompt from file" in droid and silently broke multi-context dispatches)
- fix(droid): `--read-only` now uses `--use-spec` (true read-only / spec mode) instead of `--auto low` (which still allowed file modifications)
- feat(droid): wire `RunOpts.session_id` to droid's `-s` flag for session continuity
- chore(droid): map `opus` shorthand to `claude-opus-4-7` (droid's own default), was stale at 4-6
- fix(worktree): re-anchor reused worktrees to the requested branch when an agent ran `git checkout` and steered HEAD elsewhere — was silently letting commits land on the wrong branch (#113)
- feat(stop): add `aid stop --retry-tree <id>` to cancel a whole retry tree in one call — resolves the argument to the chain root, walks every transitive descendant, stops every non-terminal member; composes with `--force` (#112)


## v8.94.0 (2026-04-20)
- feat(reply): new `aid reply <task-id> <message>` command — persists messages in a new `task_messages` SQLite table, PTY monitor delivers them to the agent's stdin and records ack when the agent produces output after delivery. `aid steer` now routes through the same persisted path.
- feat(unstick): new `aid unstick <task-id>` command — manual recovery for hung tasks. New `TaskStatus::Stalled` variant plus an `IdleDetector` policy module; the PTY monitor auto-nudges at warn threshold and escalates to `Stalled` past the escalation threshold.
- feat(batch): `aid batch` auto-prunes aid-owned worktrees when tasks complete successfully. Failed and shared worktrees are preserved. Opt-out via `.aid/project.toml`'s new `keep_worktrees_after_done = true`.
- feat(batch): on GitButler-active repos, `aid batch` completion and `aid watch --quiet --group` now print the `aid merge --lanes --group <wg-id>` merge-back hint alongside the existing `aid merge --group` suggestion.
- feat(batch): first `aid batch` invocation in a GitButler repo without `.aid/project.toml` integration prompts once to enable `gitbutler = "auto"`. Non-interactive / `--yes` / `--no-prompt` contexts skip the prompt; a `suppress_gitbutler_prompt = true` marker prevents re-prompting after a decline.
- feat(group): `aid group delete --cascade` deletes the group's member tasks transactionally rather than orphaning them. Without `--cascade`, the count of still-tagged historical tasks is printed with a pointer to `--cascade`.
- feat(merge): `aid merge --force` unblocks FAIL-status tasks that verify failed but have a clean working tree. Previously required hand-running `git merge`.
- fix(batch): `dir = "."` in a batch TOML now resolves relative to the TOML file's parent directory instead of the runtime's inherited cwd. First-wave tasks no longer fail with `Not a git repository: /tmp/.`.
- fix(background): missing agent binary now fails fast on the background dispatch path with the same clear preflight error the foreground path gives (GH#89). Shared `ensure_agent_binary_available` helper lives in `src/agent/mod.rs` and is called from both paths.
- fix(tests): workspace_dir test isolation — `/tmp/aid-wg-{id}` is now test-isolated via `AidHomeGuard` so parallel tests sharing workgroup IDs don't race on the same filesystem path. Production behavior unchanged.
- fix(tests): agent fallback tests now deterministic on CI hosts without agent binaries on PATH — new `DetectAgentsGuard` pins `detect_agents()` return value per-thread under `cfg(test)`.
- fix(clippy): clear 28 pre-existing `cargo clippy -- -D warnings` lints (rust-1.93 and rust-1.95 strictness). CI's build job is now green for the first time in several releases.
- docs: add `docs/gitbutler.md` covering integration modes, the batch → review → `aid merge --lanes` pipeline, the `AID_GITBUTLER=0` escape hatch, troubleshooting, and the `keep_worktrees_after_done` knob.


## Unreleased
- fix(gitbutler): completed worktree tasks now auto-prune their aid-owned worktrees by default when the branch has committed changes, while preserving failed tasks, shared worktrees, and projects with `keep_worktrees_after_done = true`
- fix(batch): `aid batch` now offers a one-time GitButler enable prompt for detected GitButler repos without `.aid/project.toml` integration, with `suppress_gitbutler_prompt = true` and `--yes` / `--no-prompt` escape hatches for non-interactive runs
- fix(gitbutler): batch completion and `aid watch --quiet --group` now surface the GitButler lane merge-back path via `aid merge --lanes --group <wg-id>`
- docs: add `docs/gitbutler.md` covering modes, batch review flow, `AID_GITBUTLER=0`, troubleshooting, and `keep_worktrees_after_done`

## v8.93.0 (2026-04-18)
- feat(release): `scripts/release.sh` now pre-flights orphan branch and orphan worktree detection. Branches merged into `main` and worktrees pointing at merged or missing branches block the release unless `--skip-hygiene` is passed. Dry-run mode only warns.
- feat(hygiene): new `scripts/session-preflight.sh` surveys repo health at session start — fetch, ahead/behind vs `origin/main`, dirty count, aid zombie tasks, aid worktrees for current repo, /tmp disk usage. Wired as a Claude Code SessionStart hook when `.claude/settings.json` enables it locally.
- docs: `docs/ux-debt.md` records 14 UX debt items grouped by severity plus five non-negotiable principles (resource lifecycle, path-relative-to-file, cross-repo safety, error translation at config layer, board truthfulness) for the v9.0 overhaul.
- docs: `docs/roadmap.md` and `docs/design/reply-unstick-spec.md` track the unreleased port work (reply/unstick/GH#89 background preflight) and the v9.0 plan. The reply/unstick feature spec is preserved for the follow-up port — see `ai-board` item `wi-273e`.


## v8.92.0 (2026-04-17)
- fix(verify): detect when a task prompt declares new files (`Create a NEW file: <path>`) but the resulting commit does not add them — verify now fails with the missing paths instead of silently passing (closes #103)
- feat(doctor): new `aid doctor` sub-command lists prunable worktrees and deletable merged/rebased branches under aid-managed prefixes; `--apply` runs `git worktree prune` + `git branch -d` (never `-D`)
- feat(auto-gc): opt-in auto cleanup of fully-merged task worktrees + branches on task/group completion via `--auto-gc` flag or `aid_gc = "auto"` in `.aid/project.toml` (closes #104)


## v8.91.1 (2026-04-17)
- fix(rescue): never amend tagged release commits — creates a new commit on top instead when HEAD has any tag (closes #102)
- fix(rescue): honor pre-task dirty-file baseline so the user's pre-existing uncommitted work is never staged/committed by rescue
- fix(rescue): exclude aid-internal artifacts (`.aid/`, `result-t-*.md`) from rescue staging
- fix(rescue): baseline handles rename/delete/kind-transition status lines correctly (path-only match)


## v8.91.0 (2026-04-16)
- refactor: split delivery assessment from verify status and persist it as first-class task metadata, including migration of legacy hollow-output and empty-diff states
- refactor: add a shared worktree snapshot boundary and reuse it across dirty checks, post-run settlement, commit, and rescue flows
- refactor: extract lifecycle phase decisions for teardown, escape checks, worktree settlement, verify/scope handling, checklist handling, and task post-processing
- fix: isolate agent rate-limit marker tests and ignore local `.aic/` audit artifacts so release status checks stay clean
- chore: unblock release gates by sharing Gemini-family support code through one module path and making the current clippy warning policy explicit


## v8.90.0 (2026-04-16)
- fix: `aid board` anti-poll enforcement strengthened — blocked states no longer output board data, repeat limit lowered to 1, hard blocks exit with code 1, hints include running task IDs


## v8.89.0 (2026-04-14)
- fix(#102): `should_rescue_path` no longer excludes `result-*.md` files — audit/cross-audit tasks that write result files are now properly rescued instead of triggering a guaranteed dirty-worktree FAIL
- fix(#102): `persist_result_file` now runs before Failed-task worktree cleanup, so result files are saved to `~/.aid/tasks/<id>/` while the worktree still exists


## v8.88.0 (2026-04-14)
- fix(#99): `prompt_scan.rs` no longer panics on multi-byte UTF-8 characters (em-dashes, arrows, etc.) in context files during `--dry-run`. Replaced byte-based `truncate()` with char-based truncation in `truncate_snippet`.
- fix(#97): batch cost total no longer double-counts — was exactly 2x the real sum because `waiting_ids` and dispatched `task_ids` overlapped. Now deduplicates before summing.
- fix(#96): `read_only = true` + `worktree` combination in batch TOML is now caught at parse/dry-run time with a clear error, instead of silently failing at dispatch after 30+ minutes.
- fix(#100): batch `--parallel` no longer serializes same-agent tasks. The auto-concurrency cap was limited to unique agent count (1 for all-codex batches); now uses CPU-based `recommended_max_concurrent` (4-24) capped at task count.
- fix(#101): `aid group finding add` no longer fails when called by codex agents in background tasks. Stopped auto-reading stdin (which is `/dev/null` in background) when content arg is missing; now requires explicit `--stdin` flag.


## v8.87.0 (2026-04-12)
- fix(#95): stop silent data loss when agents forget to `git add` new files. aid already ran `rescue_untracked_files` post-agent, but the defense had four gaps: it only handled `??` untracked files (modified-but-unstaged tracked files fell through), it amended the last commit and silently failed when the agent made zero commits, `git status --porcelain` collapsed fully-untracked directories to `src/` hiding individual files, and there was no final assertion before marking the task DONE. Now `rescue_dirty_worktree` (new, in `src/commit/rescue.rs`) covers both untracked and modified tracked files, uses `--untracked-files=all`, creates a fresh commit when HEAD is empty, and emits loud milestone events. A shared `post_agent_dirty_worktree_cleanup` helper runs rescue → retry → final assertion on BOTH the foreground (`aid run`) and background (`aid run --bg`) paths; if the worktree is still dirty after rescue and retry, the task transitions to Failed with a listing of remaining paths instead of silently losing them on worktree cleanup. Read-only audit tasks bypass the assertion by design. The injected `[Git Staging Rule]` prompt wording is now explicit: agents are told to run `git status --porcelain` before every commit and that any task leaving unstaged files will FAIL. Closes #95.
- feat(#98): opt-in `--audit` flag on `aid run` that dispatches `aic audit <task-id>` as a foreground subprocess when a task reaches DONE. Captures verdict (`pass` / `fail` / `error` / `skipped`) and report path as task metadata (`audit_verdict`, `audit_report_path`) and surfaces `Audit: <verdict> (report: <path>)` in `aid show` output when populated. Graceful degradation when `aic` is not on PATH — warning logged, verdict set to `skipped`, task status unaffected (audit is strictly informational; parent task status never changes based on auditor verdict). Configurable via `[audit] auto = true` in `.aid/project.toml` for per-project auto-audit, with a `--no-audit` CLI escape hatch to opt individual tasks out. Batch TOML supports `audit` at `[defaults]` and per-`[[task]]` levels with task-level overrides. Timeout default 5 minutes, configurable via `AID_AUDIT_TIMEOUT_SECS` up to 30 minutes. Closes #98.
- chore: split oversized touched files into submodules while fixing #98 — `src/types.rs` 795 → 67 lines (Task struct moved to `src/types/task.rs`), `src/project.rs` 581 → 296 lines (audit/team config extracted), `src/batch.rs` 575 → 170 lines (TOML schema and validate helpers extracted). Shared test env lock `crate::aic::test_env_lock` eliminates a race between `src/aic.rs` tests and `src/cmd/run_audit_tests.rs` tests that was producing flaky failures under parallel execution.


## v8.86.0 (2026-04-12)
- feat(qwen): add Qwen Code CLI (`qwen`) as a first-class aid agent. Qwen Code 0.14.x is a Gemini-CLI fork with identical stream-json output schema, so the adapter mirrors the Gemini one (stream events, tool call classification, token accounting). Default model is `coder-model`; free-tier pricing via OAuth or Alibaba Cloud Coding Plan. `aid run qwen "..."`, `aid batch` with `agent = "qwen"`, stats, board, and smart routing all work. Wired through `AgentKind`, adapter registry, selection scoring, cost table, rate limit tracking, container/sandbox matrix, and config models.
- fix(#94): strengthen worktree validation and stop running `but setup` inside task worktrees. `is_valid_git_worktree` previously accepted any git repo at the expected path — a standalone repo squatting `/tmp/aid-wt-*` would be silently reused forever, breaking merge-back. It now also requires the candidate's `git rev-parse --git-common-dir` to match the main repo's common dir AND the canonicalized path to appear in `git worktree list --porcelain` (with `/tmp` ↔ `/private/tmp` symlink aliasing handled). Separately, `run_dispatch_prepare` no longer calls `but setup` inside per-task worktrees — `but setup` requires the main worktree and the call was the most plausible trigger for the initial mutation. GitButler hooks now only wire for tasks when the main repo already has an active GitButler project; otherwise aid emits a one-shot hint telling you to run `but setup` from the main repo. Closes #94.
- chore: gitignore `.aid-verify-deps-state` and `result-t-*.md` so transient verify state and audit result files don't leak into commits.


## v8.85.0 (2026-04-11)
- fix(#91): detect nested git repos at dispatch time and warn loudly when the inner-vs-outer repo choice is ambiguous. New `--repo-root <path>` flag on `aid run` and `aid batch` (also `[defaults] repo_root = "..."` in batch TOML) to override auto-detection. Non-submodule nesting triggers a warning that names both repos, their remotes, and the exact override commands.
- fix(#92): `aid batch` / `aid run --worktree` now reconciles reused worktrees with the current branch HEAD before dispatch. When the reused worktree is behind and has no local edits, it is fast-forwarded automatically; otherwise dispatch fails with an actionable error (`aid worktree remove <branch>` hint). Verify-failure errors that were actually caused by a missing task directory inside a stale worktree now surface the real cause instead of a generic "verify failed".
- fix(#93): fresh worktrees no longer fail verify because `node_modules` / `target` / `.venv` are missing. New `setup` hook field in `.aid/project.toml`, batch `[defaults]`, and `[[task]]` — runs once per worktree (cached via `.aid-setup-done` marker) and streams output as `setup` events. When `setup` is not defined, aid falls back to symlinking well-known dependency dirs (`node_modules`, `target`, `.venv`, `venv`, `vendor`) from the main repo into the worktree, gated by a matching project file. Disable with `--no-link-deps` on `aid run` or `[defaults] worktree_link_deps = false`. Verify failures in fresh worktrees now append a hint pointing at the `setup` field.


## v8.84.0 (2026-04-10)
- fix(batch-retry): `aid batch retry <wg>` now serializes retried tasks that share a worktree. Tasks are bucketed by `(worktree_path, worktree_branch)`; buckets with more than one task dispatch sequentially and wait for each task to reach a terminal status before starting the next. Distinct worktrees still retry in parallel. Previously, shared-worktree tasks all dispatched concurrently and trampled each other.
- fix(commit): post-task `auto_commit` no longer scoops `.aid-lock`, `result-*.md`, or `aid-batch-*.toml` into stray commits. `git add -u` uses pathspec exclusion, untracked-file detection filters `result-*.md`, and the commit is skipped entirely via `git diff --cached --quiet` when nothing substantive is staged. Eliminates the "sandwich auto-commit" noise that every feature branch used to accumulate.


## v8.83.0 (2026-04-10)
- feat(gitbutler): opt-in GitButler integration. New `[project] gitbutler = "off" | "auto" | "always"` field, auto-detected by `aid project init` when the `but` CLI is present.
- feat(gitbutler): per-dispatch worktree integration — `but setup` runs in the worktree, Claude Code agents get `.claude/settings.local.json` with `but claude pre-tool|post-tool|stop` hooks, and non-Claude agents get an on-done `but -C <wt> commit -i` chained into `args.on_done`. Gated on `AID_GITBUTLER=0` escape hatch.
- feat(gitbutler): `aid merge --group <wg-id> --lanes` applies each task branch as a GitButler virtual branch lane instead of sequentially `git merge`-ing them, so a whole batch becomes a single reviewable workspace via `but status` / `but apply` / `but unapply`. Worktrees are preserved in `--lanes` mode.
- fix(background): `build_on_done_command` now routes commands containing shell metacharacters (`&&`, `||`, `|`, `;`, `>`, `<`, backticks, `$(`) through `sh -c` instead of naive `split_whitespace` + `Command::new`. Makes chained on_done commands actually work for any aid user, not just GitButler.
- fix(merge): `--lanes --check` and `--lanes --target` now return clear errors instead of silently ignoring the flag; `--lanes` without `--group` still errors cleanly. All three combinations have unit tests.
- fix(merge): `aid merge --group --lanes` now honors `AID_GITBUTLER=0` and the project `gitbutler` mode — previously the env var only gated dispatch hooks, letting `--lanes` still run.
- docs: new "GitButler Integration (optional)" section in CLAUDE.md covering modes, per-task behavior, escape hatch, and `--lanes` post-batch assembly.


## v8.82.0 (2026-04-09)
- fix: resolve relative `dir` and `context` paths in batch TOML against the batch file's location, not CWD


## v8.81.0 (2026-04-09)
- feat: Insights dashboard — `aid stats --insights` shows activity by day/hour, top sessions, overview totals with ASCII bar charts (H7)
- feat: Credential pool — `~/.aid/credentials.toml` for multi-key rotation per provider (round_robin/fill_first/least_used), `aid credential list` CLI (H2)
- fix: Rate-limit false positives — removed 503/payment from rate-limit classification, reduced TTL to 5min, auto-clear on success (#90)


## v8.80.0 (2026-04-09)
- feat: `aid export --sharegpt <task-id>` — export task conversations in ShareGPT JSONL format for fine-tuning (H4)
- fix: Rate-limit false positives — removed 503/payment from rate-limit classification, reduced TTL from 1h to 5min, auto-clear on task success (#90)


## v8.79.2 (2026-04-09)
- fix: `best_of` in batch no longer conflicts with running sibling copies — each copy gets unique task ID (#79)
- fix: Result file isolation — shared-dir batch tasks write to `result-{task_id}.md` instead of overwriting each other's `result.md` (#85)
- feat: `max_wait_mins` in batch TOML — WAIT tasks auto-fail after specified timeout, prevents indefinite hangs (#78)


## v8.79.1 (2026-04-09)
- fix: Smart routing 503 loop — detect "no plan" 503 errors as rate-limit, skip smart routing for rate-limited agents (#88)
- fix: `aid batch --quiet` hang — reconcile zombie tasks before polling completion, ensures exit when all tasks are terminal (#86)
- fix: Droid model shorthand mapping — map `haiku`/`sonnet`/`opus` to full model IDs required by factory-cli (#87)
- fix: Agent binary pre-flight check — fail fast with clear message when agent binary not found, instead of leaving task stuck in RUN (#89)


## v8.79.0 (2026-04-09)
- feat: Prompt injection scanning — context files and skills scanned for adversarial patterns (role hijacking, system prompt injection, invisible Unicode, XML tag injection) with warnings
- feat: Smart model routing — automatically uses cheaper models for simple prompts without --budget flag, configurable via `selection.smart_routing` (default: enabled), conservative heuristic (length, word count, code blocks, keywords)
- feat: Shared `embed_context_in_prompt` helper — context files now embedded in prompts for codex, cursor, oz, and codebuff agents (previously silently dropped)
- fix: Cursor read-only mode now passes `--trust` flag — fixes workspace trust prompt blocking plan-mode tasks
- fix: Oz read-only mode — added prompt-level enforcement (was completely missing)
- fix: Rate limit detection added for cursor, claude, opencode, kilo, and oz agents — enables cascade/fallback on quota errors


## v8.78.0 (2026-04-08)
- Fix `aid board` always showing data even when anti-poll triggers — warnings go to stderr, exit code 0 (#81)
- Fix `best-of-N` output file collision — each candidate gets isolated output paths, winner's files promoted (#82)
- Fix `aid batch --quiet` zero progress visibility — new `aid_progress!` macro emits flushed lifecycle events (#83)
- Fix batch concurrency limiter: better I/O-bound defaults (cpu_count clamped 4-24), `max_concurrent` in TOML defaults, agent diversity includes fallback targets (#84)


## v8.77.0 (2026-04-08)
- Strengthen anti-polling: remove `--force` bypass hints from board messages, add 30s force cooldown, escalating resistance (hard block after 3+ force calls in 2min)


## v8.76.0 (2026-04-08)
- Add time-based anti-polling cooldown (10s) for `aid board` — blocks rapid repeated calls with actionable hints
- Add `--force` flag to `aid board` to bypass anti-polling cooldown
- Tighten fingerprint-based repeat detection threshold from 3 to 2 identical checks
- Surface ETA estimation in regular `aid board` output for running tasks (was only in `--stream` mode)
- Add progress percentage display for running tasks based on historical median duration (capped at 99%)


## v8.75.1 (2026-04-08)
- Fix batch `best_of` dispatches reusing active task IDs and harden best-result selection
- Clarify the batch TOML rename from `timeout` to `max_duration_mins` in parser errors and docs
- Stop tracking local `.aid/state.toml` so personal state no longer blocks status checks or releases


## v8.75.0 (2026-04-08)
- Add GitHub Copilot CLI as a built-in agent with setup, selection, pricing, sandbox, and usage integration
- Improve Copilot log formatting in `aid show` and summary extraction across streaming and tool boundaries
- Refresh project documentation for supported agents and scripted release flow


## v8.74.1 (2026-04-08)
- Improve streamed CLI output formatting across `aid show`, TUI, and web views
- Fix Gemini response extraction for content arrays, tool boundaries, and revision-style text events


## v8.74.0 (2026-04-08)
- Allow read-only agents to write configured `result_file` outputs
- Fix read-only mode blocking persisted result files

## v8.73.0 (2026-04-08)
- Fix batch waiting-task cleanup for active workgroups
- Persist waiting-task retry configuration correctly
- Add JSONL event streaming for `aid watch` and retry support for waiting batch tasks

## v8.72.0 (2026-04-07)
- Cherry-pick mempalace memory upgrades: tiered memory injection and compact prompt format
- Add knowledge graph CLI and store support

## v8.71.0 (2026-04-07)
- Make `watch --group` track newly added pending and waiting tasks
- Keep active workgroup tasks visible in wait and watch flows

## v8.70.0 (2026-04-06)
- Retry agents on dirty worktrees instead of auto-committing
- Clear stale worktree locks during prune
- Auto-scope conflicting `result_file` paths in batch dispatch

## v8.69.0 (2026-04-04)
- Add Claude Code as a first-class agent with auto-selection support
- Update Cursor, Gemini, OpenCode, Kilo, and Droid adapters for newer CLI versions
- Improve agent selection scoring

## v8.68.0 (2026-04-04)
- Add `aid run --iterate N --eval CMD` generator-evaluator loop
- Integrate iterate mode with batch and background flows
- Add hung-task auto-recovery and split oversized run command modules

## v8.67.0 (2026-04-04)
- Add `--prompt-file` support for long prompts in run and batch tasks
- Support batch metadata fields
- Improve failure diagnostics and stale diff/worktree recovery

## v8.66.3 (2026-04-02)
- Fix OpenCode workspace access for workgroup directories
- Fix OpenCode output parsing and rendering in `aid show` and TUI

## v8.66.2 (2026-04-01)
- Add default audit report mode: review and cross-audit tasks now auto-write `result.md`
- Prefer persisted `result.md` in `show`, TUI, and web output views
- Fix TUI/web Codex output rendering to extract final assistant messages instead of raw JSONL logs

## v8.66.1 (2026-04-01)
- Fix Codex CLI v0.118.0 non-PTY runs hanging when stdin stays open
- Preserve `stopped` task status so timeout/completion writes do not overwrite manual stop

## v8.63.0 (2026-03-26)
- Detect output file conflicts in batch analyze (bail on guaranteed data loss)
- Auto-suffix conflicting output paths in parallel batch dispatch
- Expand file path detection to 16 extensions (md, json, toml, yaml, etc.)

## v8.62.0 (2026-03-26)
- v8.62.0: Support gemini-cli 0.35+ stream-json format
- Support gemini-cli 0.35+ stream-json format

## v8.61.0 (2026-03-26)
- v8.61.0: Fix changelog for crates.io installs + prominent upgrade banner
- Fix embedded changelog for crates.io installs

## v8.60.0 (2026-03-26)
- v8.60.0: Batch TOML parity with aid run flags
- Add missing `aid run` flags to batch TOML support. Currently
- Add missing batch TOML run flag support
- Custom ID conflict handling: block running, auto-suffix terminal

## v8.59.0 (2026-03-26)
- v8.59.0: Allow human-readable custom task IDs
- chore: auto-commit changes to .aid-lock
- Allow custom task IDs in dispatch flows

## v8.58.0 (2026-03-26)
- v8.58.0: Improve batch init template and changelog embedding reliability

## v8.57.0 (2026-03-26)
- v8.57.0: Fix TUI/web output display for custom agents
- Fix TUI/web "No output available" for custom agents with plain-text logs

## v8.56.0 (2026-03-26)
- v8.56.0: Show error reasons for failed tasks on board

## v8.55.0 (2026-03-26)
- v8.55.0: Code Health Round 4 — split 4 oversized files
- Split run_prompt into run_process and run_prompt_helpers modules
- Split src/tui/ui.rs (453 lines) into focused modules. Target
- Split show command into helpers, JSON, and test modules
- Split TUI ui into ui_detail and ui_tree modules
- Split agent module into env helpers and tests submodules

## v8.54.0 (2026-03-26)
- v8.54.0: Checklist Wave 2 — output scanning, auto-retry, show display
- feat: checklist Wave 2 — output scanning, auto-retry, show display
- Implement checklist output scanning in src/cmd/checklist_sca

## v8.53.0 (2026-03-26)
- v8.53.0: Sprint contracts — --checklist prompt injection (Wave 1)
- feat(run): add checklist prompt injection
- v8.52.0: Full output by default, read_only background fix, --json output field
- Preserve background read-only runs and AID_HOME
- Make show and output default to full content
- v8.51.0: Untracked file rescue, git staging guard, batch [[task]] alias, board anti-polling
- feat: rescue untracked files before verify, reorder background lifecycle
- fix: accept [[task]] alias in batch TOML, exit on repeated board polling
- Add git staging guard to writable prompts
- Add untracked file rescue helpers
- v8.50.0: Finding API, pending reason, read_only fix, idle timeout
- chore: remove stale aid-lock
- chore: auto-commit changes to .aid-lock
- chore: auto-commit changes to .aid-lock
- chore: auto-commit changes to .aid-lock
- Implement GitHub issue #68: expose pending-timeout reason in
- Add finding get/update commands and review fields
- fix codex read-only findings writes
- feat: increase default idle timeout to 300s and add per-agent config
- v8.49.0: Worktree safety and CLAUDE.md overhaul
- docs: update CLAUDE.md with full CLI coverage
- fix: prevent worktree contention from concurrent agent access
- v8.48.0: Reliability, dispatch intelligence, and UX polish
- Remove unused PTY idle-timeout constant
- Add configurable idle timeouts for runs and batches
- Skip rate-limited agents before batch dispatch
- Track new workgroup tasks during wait
- Fix GH#58: `aid board` anti-polling is too aggressive — reje
- Update Cargo.lock for v8.47.0
- v8.47.0: Codex CLI v0.116+ compatibility and TUI polish
- v8.46.0: UX fixes from dogfooding
- Add --limit flag to `aid board` to control how many tasks ar
- Reject unknown top-level batch keys
- Suppress dir warning for non-writing runs
- Reject unknown batch TOML fields
- v8.45.0: Project runtime state file (.aid/state.toml)
- chore: auto-commit changes to src/store/queries/state_queries.rs
- Refresh project state after task completion
- Inject recent project state into run prompts
- Add project state CLI command
- Create src/store/queries/state_queries.rs (~100 lines) with
- Add project state management module
- docs: add Show section to CLAUDE.md for research task output
- v8.44.0: Research task output improvements for aid show
- Relax research output truncation
- Auto-save research task output after completion
- Show research findings for no-change tasks
- v8.43.0: Fix read_only batch false positive merge conflict warning (GH#60)
- v8.42.0: Context pollution reduction — summary tools + smart skill injection
- feat: skip skill methodology/gotchas for short prompts (<200 chars)
- feat: summary-only tool injection — name + description, no command/args
- v8.41.0: Smart tool injection + per-category agent routing
- Track task categories for category-aware agent history
- Filter toolbox injection by task category
- v8.40.0: Fix cascade fallback for quota exhaustion (GH#57)
- fix: cascade fallback for gemini quota exhaustion (GH#57)
- chore: add GitHub issue templates (bug report + feature request)
- v8.39.0: Fix stats panic (GH#52), zombie tasks (GH#53), pending dispatch (GH#54)
- fix batch slot refill latency for pending tasks
- Auto-fail stale running tasks after 24h
- Fix stats panic on zero-duration tasks
- docs: update CLAUDE.md with v8.38.0 features (worktree prune, context sync)
- v8.38.0: Worktree context sync (GH#51), worktree prune, batch/background splits
- refactor: split batch serde and interpolation helpers
- Refactor background process and spec helpers
- Sync missing context files into worktrees
- Add stale worktree prune command
- v8.37.0: Code health + UX — run.rs split, ETA, quota-aware scheduling, auto-commit
- refactor(run): extract post-run lifecycle flow
- Prevent dispatching rate-limited agents
- Add ETA estimates for running board tasks
- improve merge auto-commit messages and staging
- docs: update CLAUDE.md with v8.36.0 features
- v8.36.0: Stats dashboard, merge target, tool team, Cargo.lock sync, 4 bug fixes
- Support comma-separated batch fallback agents
- Treat 402 payment errors as fallback eligible
- Accept string values for batch list fields
- fix(run): restore full output fallback from logs
- add aid stats command
- Add target branch support to aid merge
- fix merge Cargo.lock drift before worktree merge
- Add team lookup to tool show and test
- v8.35.0: Composer-2 default, output fix, batch fallback, agent config
- Add per-agent default model configuration
- Set Cursor composer-2 as default model
- Fix batch auto fallback agent selection
- fix(show-output): merge cursor assistant deltas
- fix: VFAIL keeps Done status — stop downgrading to Failed
- v8.34.0: Auto-sequence shared-worktree batch tasks + prompt size warning
- Auto-sequence shared batch worktrees
- Add team toolbox: configurable tools injected into agent prompts
- chore: auto-commit agent changes before merge
- Remove duplicate ToolAction enum, use cli_actions::ToolAction in tool.rs
- task A
- release: v8.32.0 — Python verify auto-detection
- Add Python verify auto-detection
- release: v8.31.1 — default TUI, foreground task visibility
- fix: sort task IDs in TUI group filter test for deterministic ordering
- fix: keep foreground tui tasks visible under group filter
- Default bare aid to board
- chore: remove accidentally committed build artifacts
- task A
- release: v8.31.0 — verify enforcement, quota rescue, rate limit quality, spawn logging, pending timeout
- fix: update verify retry test for enforce_verify_status behavior
- chore: auto-commit agent changes before merge
- fix: timeout stale pending tasks
- Fix: tasks that passed verify but failed due to quota exhaus
- fix: fail tasks when verify fails without retry
- Fix: when agent process fails to spawn, write an error event
- fix: clean saved rate limit markers
- release: v8.30.1 — batch [defaults] group support, close #42 & #33
- fix: support group field in batch [defaults] for workgroup assignment (#42)
- fix: add oz & droid to setup agent detection and rate_limit list
- release: v8.30.0 — Web UI v2
- Add web task action and diff endpoints
- Add task detail actions and diff tab
- release: v8.29.3 — code health round 3
- refactor: code health round 3 — extract run/prompt tests and helpers
- Extract the `#[cfg(test)]` test block from `src/cmd/run_prom
- Extract run command tests into run_tests module
- task B
- task A
- release: v8.29.1 — batch workgroup override (GH#40)
- Add batch workgroup override flag
- release: v8.29.0 — merge safety & batch analysis
- Add merge check mode and post-merge group verify
- add batch file overlap analysis
- release: v8.28.2 — fix output file enforcement (GH#37, GH#39)
- Fix output post-processing fallbacks
- release: v8.28.1 — dev environment container mode
- Add reusable dev container execution mode
- release: v8.28.0 — shared batch directory + changelog fix
- Add shared batch directory support
- docs: update README — version badge, new agents, sandbox section
- fix: aid changelog no longer shows other repo's tags
- fix: panic on multi-byte chars in prompt preview truncation
- release: v8.27.2 — code health round 2: split cli, config, watcher
- split cli command definitions into modules
- split watcher helpers into focused modules
- refactor(config): split config command modules
- release: v8.27.1 — code health: split 3 oversized files
- Split src/main.rs (739 lines) by extracting the command disp
- Split CLI command dispatch out of main
- Split src/cmd/show_output.rs (836 lines) into focused format
- Split aid show output formatters into focused modules
- Split src/store/queries.rs (807 lines) into focused query mo
- split store query modules
- fix: include updated Cargo.lock for v8.27.0
- release: v8.27.0 — container sandbox for agent isolation
- - [Review Checklist](knowledge/review-checklist.md) — Pre-ac
- feat: add container sandbox run option
- release: v8.26.1 — single source of truth for agent lists
- refactor: derive charts AGENTS from AgentKind::ALL_BUILTIN
- - [Coding Conventions](coding-conventions.md) — File structu
- Unify built-in agent metadata on AgentKind
- release: v8.26.0 — skill scripts with structured metadata
- Add script metadata parsing and structured injection to the
- Add skill script metadata injection
- release: v8.25.1 — fix 4 GitHub issues (#30-#35)
- Fix GH#34: opencode crashes when sibling task context is inj
- Fix GH#31: `read_only = true` tasks should NOT auto-commit a
- Fix GH#32: Warn when multiple batch tasks target the same `d
- task A
- task A
- task A
- chore: auto-commit agent changes before merge
- - [Coding Conventions](coding-conventions.md) — File structu
- fix: correct test indentation in stop.rs
- Fix `aid stop` and zombie detection to properly kill agent p
- Fix the `aid changelog` command in src/cmd/changelog.rs and
- Fix process leaking in the PTY bridge. When a PTY-spawned ag
- release: v8.24.1 — changelog anywhere + cursor log cleanup
- fix: add build.rs for embedded changelog
- release: v8.24.0 — batch & UX polish
- Add two new flags to `aid show`:
- Add summary and file filters to aid show
- Fix auto-commit failing on empty git repos (repos with no HE
- Handle auto-commit in repos without HEAD
- Make `aid watch --quiet` less verbose by suppressing milesto
- Suppress quiet wait milestone progress output
- Change droid's default auto approval level from "medium" to
- agent: raise droid auto approval to high
- release: v8.23.0 — skill system v2 with folders, gotchas, and scripts
- fix: add missing test files from skill folder worktree
- Upgrade the skill system to support folder-based skills with
- release: v8.22.1 — add aid changelog command
- Add changelog subcommand for release history
- release: v8.22.0 — batch power-ups & cost visibility
- Add `.env` forwarding to agent subprocesses.
- Add synthetic progress events for droid agent (and any agent
- Add aid cost reporting command
- Add batch template variable interpolation
- Preserve partial work on retry by default
- release: v8.21.14 — custom agent docs clarification
- fix: clarify custom agents are non-interactive CLIs, not Claude Code
- feat: add --full flag to show --output and aid output
- chore: update Cargo.lock for v8.21.12
- fix: auto-commit message uses [Task] section instead of shared context
- release: v8.21.12 — performance + test subprocess leak fix
- Auto-created for batch dispatch
- Auto-created for batch dispatch
- Auto-created for batch dispatch
- release: v8.21.11 — fix GH#22 gemini tool_call name parsing
- fix: GH#22 gemini tool calls logged as 'unknown'
- release: v8.21.10 — security hardening from core audit
- fix: security hardening from core audit — 4 HIGH findings resolved
- release: v8.21.9 — zombie detection false positive fix
- fix: zombie detection false positives — waitpid ECHILD for non-child workers
- Auto-created for batch dispatch
- - [Agent System](agent-system.md) — Selection pipeline, prom
- chore: update Cargo.lock for v8.21.8
- refactor: ProcessGuard RAII subprocess abstraction + verify.rs migration
- fix: GH#27 droid/codebuff rejected in batch — replace hardcoded VALID_AGENTS with AgentKind::parse_str
- release: v8.21.6 — subprocess management perf fixes
- Auto-created for batch dispatch
- Auto-created for batch dispatch
- Auto-created for batch dispatch
- Auto-created for batch dispatch
- release: v8.21.5 — eprintln to aid output macros bulk conversion
- fix: GH#25 remove cursor from auto-skills, GH#26 batch auto-cascade for rate-limited agents
- fix: agent subprocess leak — process group isolation for all spawn paths
- Auto-created for batch dispatch
- Auto-created for batch dispatch
- fix: GH#22 gemini tool names, GH#23 auto-create group, GH#24 0B output, GH#28 judge bool
- release: v8.21.1 — fix verify process leak (GH#27)
- fix: verify process leak — process group isolation + timeout (GH#27)
- release: v8.21.0 — attention space audit + quiet mode + droid parity
- release: v8.20.9 — show-output extraction, verify isolation, auto-commit cleanup
- fix: auto-commit uses git add -u instead of -A, skips context headers in message
- fix: show-output extraction, verify isolation, batch audit safety, retry reset
- release: v8.20.8 — code health cleanup
- chore: extract inline tests to separate files — merge, selection, watcher
- chore: remove last production unwrap() in usage.rs
- chore: dead code cleanup — remove 4 dead items, 10 unnecessary annotations
- release: v8.20.7 — context_from implicit dependencies + unwrap safety
- fix: context_from creates implicit dependency in batch dispatch
- release: v8.20.6 — zero production unwrap()
- fix: remove all unwrap() from production code paths
- release: v8.20.5 — data integrity fixes
- fix: data integrity — auto-commit error events + workgroup creation rollback
- release: v8.20.4 — zero clippy warnings
- fix: eliminate all clippy warnings (11 → 0)
- release: v8.20.3 — propagate workgroup env to agent subprocesses
- fix: propagate AID_GROUP and AID_TASK_ID to agent subprocesses (#15)
- release: v8.20.2 — --dir agent isolation via GIT_CEILING_DIRECTORIES
- fix: set GIT_CEILING_DIRECTORIES to prevent --dir agent escape (#16)
- release: v8.20.1 — subprocess concurrency limits
- feat: subprocess concurrency limits for tests and runtime
- feat: [Shared Context: batch] Auto-created for batch dispatch
- feat: [Shared Context: batch] Auto-created for batch dispatch
- feat: [Shared Context: batch] Auto-created for batch dispatch
- release: v8.20.0 — Droid (Factory.ai) agent integration
- chore: auto-commit agent changes before merge
- release: v8.19.0 — agent quota + structured findings
- chore: auto-commit agent changes before merge
- feat: [Team Knowledge — ai-dispatch] - [Coding Conventions](coding
- fix: pass --cascade through BackgroundRunSpec (closes #17)
- chore: auto-commit agent changes before merge
- release: v8.18.0 — process safety, idle timeout & double-dispatch fix
- feat: v8.18.0 — process safety, idle timeout, double-dispatch fix
- release: v8.17.2 — commit message sanitization + zero warnings
- fix: strip aid tags from auto-commit messages + eliminate compiler warnings
- release: v8.17.1 — process management audit fix
- fix: reap on_done callback children to prevent process leak
- release: v8.17.0 — batch resilience + process safety
- feat: batch resilience, performance tuning, process group safety (v8.17.0)
- release: v8.16.0 — comprehensive security hardening
- feat: <aid-project-rules> - File size limit: 300 lines per file -
- feat: <aid-project-rules> - File size limit: 300 lines per file -
- feat: <aid-team-rules> - Do NOT run cargo fmt, rustfmt, or any aut
- Harden worktree cleanup and branch reset safety
- feat: <aid-team-rules> - Do NOT run cargo fmt, rustfmt, or any aut
- feat: add sanitize module — input validation + path safety layer
- release: v8.15.2 — defense-in-depth sandbox guards + docs update
- feat: <aid-project-rules> - File size limit: 300 lines per file -
- release: v8.15.1 — critical worktree sandbox guard
- fix: sandbox guard for worktree cleanup — prevent data loss
- release: v8.15.0 — local web UI dashboard
- feat: local web UI dashboard + batch init + show anti-polling (v8.15.0)
- release: v8.14.1 — code quality audit cleanup
- refactor: code quality audit — simplify error handling, fix fragile matching
- release: v8.14.0 — project init guidance, failure reasons, cursor-agent detection
- feat: CLAUDE.md emphasizes aid as primary dev method, session-start hints project init
- feat: <aid-project-rules> - File size limit: 300 lines per file -
- fix: show failure reason in CLI output, detect cursor-agent binary, remove TUI hint
- feat: <aid-project-rules> - File size limit: 300 lines per file -
- feat: <aid-project-rules> - File size limit: 300 lines per file -
- release: v8.13.0 — cursor agent overhaul, TUI failure reasons
- fix: cursor agent overhaul — standalone binary, event parsing, TUI failure reasons
- fix: correct install URL to aid.agent-tools.org
- ci: fix release workflow — use macos-15 runner (macos-13 deprecated)
- release: v8.12.0 — GitHub issues sprint, CI, repo cleanup
- feat: <aid-project-rules> - File size limit: 300 lines per file -
- fix: remove unused imports in upgrade.rs for Linux clippy
- feat: [Team Knowledge — dev] - [Review Checklist](knowledge/review
- feat: [Team Knowledge — dev] - [Review Checklist](knowledge/review
- feat: [Team Knowledge — dev] - [Review Checklist](knowledge/review
- fix: add #[cfg(target_os = "macos")] to home_cargo_bin to fix clippy on Linux CI
- ci: build release binary instead of just cargo check
- ci: add CI workflow for push/PR — cargo check, test, clippy
- Revert "feat: <aid-project-rules>"
- feat: <aid-project-rules> - File size limit: 300 lines per file -
- Reuse batch default workgroups when present
- Add stdin and file input for findings
- feat: [Team Knowledge — dev] - [Review Checklist](knowledge/review
- chore: move batch TOMLs to .aid/batches/, gitignore that directory
- chore: repo cleanup — remove stale batch TOMLs, nanobanana-output, website dirs
- ci: add GitHub Release workflow with cross-compiled binaries
- release: v8.11.0 — prompt hardening, UX improvements, commit pollution fix
- feat: UX improvements + fix commit message pollution
- chore: remove batch dispatch file
- fix: harden prompt injection pipeline against cross-task pollution
- release: v8.10.0 — configurable pricing + command consolidation
- feat: configurable pricing + command consolidation
- fix: install script now shows aid setup + aid init next steps
- feat: add /api/pricing endpoint and fix model prices
- website: replace agent matrix with positioning cards
- docs: remove ob1 references from README
- docs: update README and website to v8.9.1
- release: v8.9.1 — caller-controlled hiboss notifications
- fix: remove auto hiboss notifications, caller-controlled only
- release: v8.9.0 — interactive approval + batch organization
- feat: hiboss Layer 1 rich notifications (v8.8.0)
- release: v8.7.1 — auto-dir + background quota cascade
- fix: improve batch help — show [defaults] fields including dir
- release: v8.7.0 — reliability & cost control
- docs: update CLAUDE.md with v8.6 project features
- release: v8.6.0 — project & budget UX overhaul
- Add project sync command
- feat: <aid-system-context> [Shared Workspace] Path: /tmp/aid-wg-wg
- feat: <aid-team-rules> - Do NOT run cargo fmt, rustfmt, or any aut
- feat: <aid-system-context> [Shared Workspace] Path: /tmp/aid-wg-wg
- feat: <aid-system-context> [Shared Workspace] Path: /tmp/aid-wg-wg
- release: v8.5.3 — code quality + UX fixes
- fix: warn when merging VFAIL tasks
- fix: --context and --scope accept space-separated values
- chore: zero clippy warnings (15 fixed across 10 files)
- release: v8.5.2 — knowledge injection quality improvements
- fix: improve knowledge injection quality — filter threshold, stop words, dedup, truncation
- chore: auto-commit agent changes before merge
- release: v8.5.1 — auto-stash merge + milestone prompt fix
- fix: auto-stash local changes before merge + clarify milestone prompt
- chore: auto-commit agent changes before merge
- chore: populate project knowledge base with 5 entries
- chore: update aid-website to v8.5.0 — add project profiles, project command
- docs: add project profiles to README, CLAUDE.md, claude-prompt.md
- release: v8.5.0 — project profiles (.aid/project.toml)
- chore: suppress dead_code warnings for ProjectConfig/ProjectAgents schema fields
- feat: <aid-system-context> [Shared Workspace] Path: /tmp/aid-wg-wg
- feat: <aid-system-context> [Shared Workspace] Path: /tmp/aid-wg-wg
- feat: <aid-system-context> [Shared Workspace] Path: /tmp/aid-wg-wg
- chore: add mod project to main.rs
- chore: auto-commit agent changes before merge
- release: v8.4.0 — agent UX guardrails + team rules
- feat: team rules — always-injected behavioral constraints
- fix: UX improvements — parse hint, workspace tag, reuse test canonicalize
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-f624 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-f624 Use this direct
- fix: replace global Mutex with thread_local for test isolation
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-78d1 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-78d1 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-78d1 Use this direct
- feat: hiboss notification channel + fix --id FK constraint
- docs: update website for v8.3.0 — stop, kill, steer commands
- v8.3.0: Live Task Control — stop, kill, steer
- v8.2.0: Custom IDs, Cursor CLI Upgrade, Work Scope Verification
- v8.1.0: Model-Level Scoring, Task Pre-creation, Rate Limit Auto-clear
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-aae8 Use this direct
- v8.0.0: Programmable Orchestration — validation, structured diff, loop detection
- v7.9.1: binary size 67% reduction + SQLite index optimization
- perf: add SQLite indexes on hot query paths + fix compiler warnings
- perf: add release profile — strip + LTO + codegen-units=1
- refactor: replace ureq with curl subprocess, drop rustls dependency
- v7.9.0: Code Health — file splits + milestone strip
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-bc39 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-bc39 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-bc39 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-bc39 Use this direct
- feat: improved TUI tree view — workgroup grouping, navigation, live status
- perf: TUI performance optimization — batch queries + throttled metrics
- release: v7.8.0 — Autonomous Experiment Loop + TUI Tree View
- feat: add experiment loop core + CLI wiring
- feat: add rolling context compression for workgroup prompts
- feat: add tree view mode to TUI (toggle with 't' key)
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-54ea Use this direct
- fix: get_completion_summary NULL handling + experiment status/persist wiring
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-ca3d Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-ca3d Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-ca3d Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-ca3d Use this direct
- release: v7.7.0 — Collective Intelligence
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-a6ea Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-a6ea Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-a6ea Use this direct
- release: v7.6.0 — Shared Context Threads
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-c886 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-c886 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-c886 Use this direct
- chore: remove dispatch batch TOMLs from repo, update gitignore
- release: v7.5.2 — stabilization (zero clippy warnings, SQL fix, 295 tests)
- feat: Fix ALL clippy warnings in the codebase. Run `cargo clippy -
- fix: include merged status in similar-tasks query, align batch test fields
- fix: robust judge parsing, diff truncation, committed-diff support
- release: v7.5.1 — memory quality + dispatch intelligence
- feat: surprise-filter, cross-session hints, best-of-n dispatch (v7.5 P1)
- release: v7.5.0 — routing intelligence (budget-aware routing + auto-judge)
- feat: auto-judge review + budget-aware cost-efficiency routing (v7.5)
- feat: budget-aware cost-efficiency routing for agent auto-selection
- release: v7.4.0 — episodic memory, success routing, code health
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-6c91 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-6c91 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-6c91 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-6085 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-6085 Use this direct
- fix: add --events flag to aid show (no-op, documents default behavior)
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-6085 Use this direct
- fix: run merge verify command through shell for redirect support
- release: v7.3.0 — code health, file splits, batch UX
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-2f03 Use this direct
- fix: accept both [[task]] and [[tasks]] in batch TOML files
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-2f03 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-2f03 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-2f03 Use this direct
- release: v7.2.2 — retry --dir override, fast-fail diagnostic hint
- release: v7.2.1 — fix streaming -o, remove OB1 agent
- fix: write output file for streaming agents (-o flag)
- release: v7.2.0 — model cascade, conditional batch chains
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-f652 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-f652 Use this direct
- release: v7.1.0 — empty diff guard, foreground timeout, zero warnings
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-78b3 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-78b3 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-78b3 Use this direct
- release: v7.0.1 — retry worktree reuse, exit_code in JSON output
- fix: retry reuses existing worktree, exit_code in --json output
- fix: rename task_hook_json to avoid duplicate definition after merge
- feat: v7.0 foundation — JSON output, result forwarding, workspace, trust tiers
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-bd59 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-bd59 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-bd59 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-bd59 Use this direct
- release: v6.1.0 — teams as knowledge context, not agent restrictions
- feat: teams as knowledge context — soft preferences, not agent restrictions
- docs: update website for v6.0.0 — add Teams section, team command, version bump
- release: v6.0.1 — improved UX for in-place tasks
- fix: improve UX for in-place (no worktree) tasks
- feat: aid team — native team concept for role-based agent selection
- release: v5.9.2 — merge-group test + real-world merge validation
- chore: auto-commit agent changes before merge
- release: v5.9.1 — fix merge data-loss, comprehensive merge tests
- test: comprehensive merge tests — 17 new tests covering all data-loss scenarios
- fix: prevent data loss in aid merge — validate commits, auto-commit, proper cleanup
- chore: v5.9.0 — store v2 versioning, skill packages, graceful upgrade
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: [Shared Context: v59-features] Auto-created for batch dispat
- chore: bump version to 5.8.2
- fix: improve show --diff and merge UX for non-worktree tasks
- chore: v5.8.1 — update README, website docs for fast query & setup
- fix: setup differentiates first-time vs returning users
- fix: setup shows current config status when already configured
- fix: setup wizard UI polish — sections, key masking, verify spinner
- feat: setup detects all built-in agents + custom agents
- fix: setup wizard shows "Press Enter to skip" hint
- feat: aid setup — interactive configuration wizard
- fix: default free tier to openrouter/free
- feat: v5.8.0 — aid query (fast LLM via OpenRouter)
- feat: auto-publish to crates.io on tag push + install.sh
- fix: strip com.apple.provenance xattr in install command
- chore: v5.7.0 — broadcast bridge, false-positive fix, workspace setup
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: [Shared Context: v57-broadcast] Auto-created for batch dispa
- feat: [Shared Context: v57-broadcast] Auto-created for batch dispa
- docs: update README and website for v5.4-5.6 features
- docs: add project CLAUDE.md with install instructions
- chore: v5.6.1 — CLI arg ergonomics (group create optional context, summary positional group, run -g)
- fix: improve CLI arg ergonomics
- chore: v5.6.0 — shared findings for workgroup collaboration
- feat: [Shared Context: v56-findings] Auto-created for batch dispat
- feat: [Shared Context: v56-findings] Auto-created for batch dispat
- feat: [Shared Context: v56-findings] Auto-created for batch dispat
- feat: [Shared Context: v56-findings] Auto-created for batch dispat
- feat: [Shared Context: v56-findings] Auto-created for batch dispat
- feat: v5.5.0 — task tree visualization, workgroup summary
- feat: [Shared Context: v55-tree-summary] Auto-created for batch di
- feat: [Shared Context: v55-tree-summary] Auto-created for batch di
- feat: [Shared Context: v55-tree-summary] Auto-created for batch di
- chore: v5.4.2 — orchestrator-only memory, explicit --project flag
- feat: memory update command + age in prompt injection
- fix: memory list/search project-scoped by default, add --all flag
- fix: memory list/search auto-scopes to current project
- chore: v5.4.1 — bug fixes, task export, dogfood improvements
- fix: update auto-retry test for verify_status behavior change
- fix: revert unnecessary load_metrics expansion to completed tasks
- fix: TUI Progress column shows milestones for completed tasks
- feat: [Shared Context: v54-fixes-and-export] Auto-created for batc
- fix: verify failure should not override task status to Failed
- feat: [Shared Context: v54-fixes-and-export] Auto-created for batc
- feat: [Shared Context: v54-fixes-and-export] Auto-created for batc
- chore: v5.4.0 — agent memory system, verify status
- feat: add VerifyStatus to distinguish execution failure from verify failure
- fix: align memory CLI with canonical Memory struct
- feat: add aid memory CLI commands
- feat: add memory injection to prompt pipeline
- feat: [Shared Context: v54-memory] Auto-created for batch dispatch
- feat: [Shared Context: v54-memory] Auto-created for batch dispatch
- feat: [Shared Context: v54-memory] Auto-created for batch dispatch
- feat: [Shared Context: v54-memory] Auto-created for batch dispatch
- feat: add agent store website at store.agent-tools.org
- chore: v5.3.1 — migrate agent store to agent-tools-org, add script support
- chore: migrate repo to agent-tools-org organization
- docs: update README and website for v5.2-5.3 features
- chore: v5.3.0 — hooks, prompt compaction, UTF-8 safety
- fix: UTF-8 safe truncation + hooks test constructors
- fix: align indentation in main.rs hooks wiring
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: [Shared Context: v53-hooks-compaction] Auto-created for batc
- chore: v5.2.0 — agent analytics, agent fork, test deadlock fix
- feat: [Shared Context: v52-features] Auto-created for batch dispat
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: [Shared Context: v52-features] Auto-created for batch dispat
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: [Shared Context: v51-release] Auto-created for batch dispatc
- chore: bump version to 5.1.0
- feat: [Shared Context: v51-store-wave2] Auto-created for batch dis
- fix: custom agent display name + background worker + retry resolution
- feat: add aid store subcommand (browse, install, show)
- fix: use correct custom agent TOML fields (id + display_name)
- fix: escape AID_TASK_ID in custom agent example to fix tsc
- feat: IMPORTANT: When editing text/config files, make targeted ed
- chore: bump version to 5.0.1
- fix: v5.0.1 — custom agent dogfood fixes + contention prevention
- feat: [Shared Context: v50-contention] Auto-created for batch disp
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: [Shared Context: v50-dogfood] Auto-created for batch dispatc
- feat: [Shared Context: v50-dogfood] Auto-created for batch dispatc
- feat: [Shared Context: v50-dogfood] Auto-created for batch dispatc
- feat: v5.0 — custom agent definitions, agent CLI, worktree base branch fix
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: [Shared Context: v50-wave1] Auto-created for batch dispatch
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: add agent-optimized website at aid.agent-tools.org
- feat: v4.8 — stabilization: codebuff cost, worktree escape, TUI dim
- feat: [Shared Context: v48-bugs] Auto-created for batch dispatch
- feat: IMPORTANT: When editing text/config files, make targeted ed
- docs: update README for v4.7 — codebuff setup guide, cost warning, pricing update
- feat: v4.7 — self-evaluation fixes, pricing update, codebuff cost tracking
- chore: bump version to v4.7.0
- feat: v4.6 — cost tracking overhaul, agent-aware cost labels
- feat: [Context] [Context Files - read these before starting] - src
- fix: upgrade codebuff SDK to v0.10 — local agent execution, no WebSocket
- feat: v4.5 — codebuff plugin, TUI stats view, retry worktree fix
- feat: [Context] [Context Files - read these before starting] - src
- feat: v4.4 — intelligent task routing with classifier + capability matrix
- fix: word-boundary matching for classifier, poison-safe AidHomeGuard
- feat: [Context] [Context Files - read these before starting] - src
- chore: bump version to v4.3.0
- feat: v4.3 — ob1 coding support, cursor budget model, startup zombie cleanup
- feat: [Shared Context: v43-fixes] Auto-created for batch dispatch
- docs: update README for v4.2 — ob1 agent, worktree CLI, workspace isolation
- chore: add ob1 to available agents list in error message
- feat: add ob1 agent adapter — multi-model coding CLI
- fix: worktree list handles macOS /private/tmp symlink
- feat: add `aid worktree create/list/remove` CLI commands
- feat: worktree escape detection — warn if agent modified main repo
- fix: watch --group scope leak, auto cherry-pick on merge
- chore: bump version to v4.1.0
- refactor: split TUI modules — app.rs and ui.rs under 300-line limit
- feat: workspace isolation — AID_GROUP env var, auto-cleanup, merge precheck
- feat: upgrade agent capabilities — cursor/gemini coding support, fallback chain
- chore: bump version to v4.0.1
- feat: progress reporting in quiet watch + board poll detection
- fix: TUI color palette — fix invisible selected text, improve contrast
- docs: update README for v4.0 — clean, merge --group, CLI hints
- chore: bump version to v4.0.0
- feat: aid merge --group for bulk merging workgroup tasks
- feat: watch hints after background dispatch and batch
- feat: contextual CLI hints and after_help examples
- feat: IMPORTANT: When editing text/config files, make targeted ed
- chore: bump version to v3.9.0
- fix: auto-retry after verify failures
- feat: TUI detail view tab system — events/prompt/output
- feat: [Shared Context: v39-wave2] Auto-created for batch dispatch
- feat: [Shared Context: v39-wave2] Auto-created for batch dispatch
- feat(batch): support defaults section
- feat: [Shared Context: v39-wave1] Auto-created for batch dispatch
- docs: update README for v3.8 — stream board, batch fields, kilo agent
- chore: bump version to v3.8.0
- feat: v3.8 — modular architecture, stream board, TUI polish
- feat: batch read_only/budget fields, auto-budget detection, TUI duration fix
- chore: bump version to v3.7.0
- feat: v3.7 — rate-limit auto-expiry, batch pre-check, worktree lock fix
- feat: [Shared Context: v37-tasks] Auto-created for batch dispatch
- feat: [Shared Context: v37-tasks] Auto-created for batch dispatch
- feat: [Shared Context: v37-tasks] Auto-created for batch dispatch
- chore: bump version to v3.6.0
- feat: clear-limit CLI, codex model passthrough, gpt-5.4 registry
- chore: bump version to v3.5.1
- feat: TUI multipane v2 — scrolling, rich headers, all tasks, Enter/Esc navigation
- chore: bump version to v3.5.0
- feat: enrich TUI multipane with duration, tokens, cost, model, milestone, metrics
- feat: batch verify=true support, rate-limit precheck, diff exclude locks, CLI help
- chore: bump version to v3.4.0
- feat: model-level history stats, budget model auto-selection, improved CLI help
- feat: [Shared Context: v34-wave1] Auto-created for batch dispatch
- feat: [Shared Context: v34-wave1] Auto-created for batch dispatch
- feat: [Shared Context: v34-wave1] Auto-created for batch dispatch
- chore: bump version to v3.3.0
- feat: multi-task watch support and indent fix
- enhance rate-limit tracking to store recovery time and display in config
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: IMPORTANT: When editing text/config files, make targeted ed
- chore: bump version to v3.2.0
- fix: align multipane bridge with structured PaneData events
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: IMPORTANT: When editing text/config files, make targeted ed
- chore: bump version to v3.1.0
- feat: add --exit-on-await flag for manager notification
- fix: add Kilo to agent usage stats iteration
- feat: add history-based agent scoring for auto-selection
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: IMPORTANT: When editing text/config files, make targeted ed
- chore: bump version to v3.0.0
- docs: add kilo to agent help text
- feat: IMPORTANT: When editing text/config files, make targeted ed
- fix: disable prompt detection for streaming agents
- chore: remove batch dispatch files
- chore: bump version to v2.9.0
- feat: add OpenCode --session retry for session continuity
- fix: add missing agent_session_id to test Task structs
- feat: pass context files to OpenCode via -f flag
- feat: IMPORTANT: When editing text/config files, make targeted ed
- chore: rename crate to ai-dispatch for crates.io publish
- chore: bump version to v2.8.0
- feat(retry): add --agent flag to override agent for retries
- feat: [Shared Context: v28-resilience] Auto-created for batch disp
- feat: add text-edit prompt guard for non-code files
- feat: sync Cargo.lock toworktrees to avoid redundant dependency resolution
- feat: validate fallback agent in batch file parser
- chore: bump version to v2.7.0
- feat: [Shared Context: v27-native-flags] Auto-created for batch di
- feat(gemini): upgrade to streaming mode with native CLI flags
- feat: [Shared Context: v27-native-flags] Auto-created for batch di
- feat: use native CLI flags for read-only and full-auto modes
- feat: [Shared Context: v27-native-flags] Auto-created for batch di
- chore: bump version to v2.6.0
- feat: [Shared Context: v26-efficiency-opencode] Auto-created for b
- feat: add auto rate-limit detection for codex
- chore: bump version to v2.5.0
- feat: [Shared Context: v25-polish] Auto-created for batch dispatch
- feat: [Shared Context: v25-polish] Auto-created for batch dispatch
- fix: parse OpenCode JSON token events
- fix(cursor): parse stream-json token usage
- feat: [Shared Context: v25-polish] Auto-created for batch dispatch
- feat: add --fallback agent and fix codex worktree trust
- feat: add `aid init` command with default skills
- chore: prepare for open source release
- chore: bump version to v2.2.0
- feat: [Shared Context: v22-budget] Auto-created for batch dispatch
- feat: add budget-aware agent selection
- feat: [Shared Context: v22-budget] Auto-created for batch dispatch
- fix(show): fall back to default log output
- chore: bump version to v2.1.0
- fix(board): show awaiting prompt instead of output context
- feat: add completion notification feed
- feat(respond): accept stdin and file input
- feat: [Shared Context: v21-robustness] Auto-created for batch disp
- fix(batch): persist skipped dependency tasks
- feat: [Shared Context: v21-robustness] Auto-created for batch disp
- feat: [Shared Context: v21-robustness] Auto-created for batch disp
- feat: add show context prompt inspection
- feat(batch): limit concurrent batch dispatches
- feat: [Shared Context: v21-robustness] Auto-created for batch disp
- docs: update README for v2.0.0 and add Claude Code prompt file
- feat(cli): add merge command for completed tasks
- feat: [Shared Context: v21-robustness] Auto-created for batch disp
- feat: [Shared Context: v21-robustness] Auto-created for batch disp
- feat: [Shared Context: v21-robustness] Auto-created for batch disp
- feat: [Shared Context: v21-robustness] Auto-created for batch disp
- chore: bump version to v2.0.0
- feat: add multi-repo task dispatch
- feat(templates): add prompt template support
- feat: [Shared Context: v20-capabilities] Auto-created for batch di
- feat: [Shared Context: v20-capabilities] Auto-created for batch di
- feat: add task completion webhooks
- feat: add benchmark command for multi-agent comparisons
- docs: update README for v1.7.0 features
- chore: bump version to v1.7.0
- fix: inherit retry worktree base
- feat: show retry chain history
- feat: make task max duration configurable
- feat(usage): add per-agent execution stats
- feat: [Shared Context: v17-ux] Auto-created for batch dispatch
- feat: [Shared Context: v17-ux] Auto-created for batch dispatch
- feat: [Shared Context: v17-ux] Auto-created for batch dispatch
- chore: bump version to v1.6.0
- refactor(show): extract explain module
- refactor(cmd): extract retry logic from run
- feat: [Shared Context: v16-quality] Auto-created for batch dispatc
- feat: [Shared Context: v16-quality] Auto-created for batch dispatc
- feat: [Shared Context: v16-quality] Auto-created for batch dispatc
- docs: update README for v1.5.0 features
- chore: bump version to v1.5.0
- feat: [Shared Context: v15-fixes] Auto-created for batch dispatch
- feat: [Shared Context: v15-fixes] Auto-created for batch dispatch
- feat: [Shared Context: v15-fixes] Auto-created for batch dispatch
- feat: dependency-based DAG scheduling and v1.4.0 release
- feat: add agent capability profiles and pricing table
- feat: [Shared Context: v14-features] Auto-created for batch dispat
- feat: [Shared Context: v14-features] Auto-created for batch dispat
- feat: [Shared Context: v14-features] Auto-created for batch dispat
- feat: [Shared Context: v14-features] Auto-created for batch dispat
- feat: [Shared Context: v14-features] Auto-created for batch dispat
- chore: release aid v1.3.0
- fix: detect zombie/defunct processes in zombie task cleanup
- feat: add skills parameter to aid_run MCP tool
- feat: enforce post-task worktree commits
- feat(tui): add dashboard view
- feat(run): auto-apply default skills
- chore: release aid v1.2.0
- fix: revert unintended README changes from interactive-io task
- feat: add PTY input forwarding for background tasks
- docs: rewrite ai-dispatch readme
- feat: share workgroup milestone findings
- chore: release aid v1.1.1 — milestone reporting
- feat: surface task milestones in dashboards
- chore: release aid v1.1.0
- feat: add skill injection for methodology-guided agent dispatch
- feat: add MCP server mode for native Claude Code tool calls
- feat: add smart agent auto-selection
- feat: add process metrics to tui dashboard
- feat: consolidate CLI from 17 to 11 commands for v1.0
- feat: fix 4 reliability bugs for v1.0
- chore: fix clippy warnings and bump to v0.9.0
- feat: add task dependency DAG to batch dispatch
- feat: add `aid explain` — AI-assisted task log explanation
- feat: scope tui watch by task and workgroup
- chore: release aid v0.8.0
- feat: add workgroup lifecycle commands
- chore: release aid v0.7.0
- feat: extend workgroup task views
- feat: add workgroup shared context
- chore: release aid v0.6.0
- feat: improve streaming usage tracking
- feat: add wait commands for task orchestration
- feat: release aid v0.5.0
- feat: v0.5 Phase 0 — command stubs, store migration, audit/review extraction
- chore: v0.5 foundation — add deps, Serialize derives, parent_task_id
- feat: v0.4 verify + context + review (agent collaboration)
- feat: v0.3 worktree isolation, batch dispatch, cursor adapter
- feat: v0.2 observability — cost tracking, OpenCode adapter, stderr capture, richer events
- feat: implement aid MVP v0.1 — multi-AI CLI team orchestrator
- Initial commit: add DESIGN.md
