# Triage: Codex resume vs deleted task HOME + delivery-guard false fail

Date: 2026-08-09  
Scope: read-only call-chain trace. No behaviour change in this write-up.

Observed incidents today:

| Task | Parent | Failure |
|------|--------|---------|
| `t-9d1e0576` | `t-749e9a2b` | exit 1 in ~2.5s — `thread/resume` missing rollout under parent HOME |
| `t-626c8144` | `t-f6bc9710` | same |
| `t-346c5194` | `t-68a83a48` | exit 0, then `missing_final_delivery` (`last_message_chars: 157`) |

---

## Bug 1 — Codex session resume points at a deleted task HOME

### What happened

Child tasks resumed a parent Codex `thread_id` after the parent's isolated `$HOME` had been removed. Codex still resolved the session to an absolute rollout path under the **parent** task home:

```text
Error: thread/resume: thread/resume failed: failed to resolve rollout path
`/Users/mingsun/.aid/tasks/t-749e9a2b/home/.codex/sessions/2026/08/09/rollout-...-019fe609-....jsonl`:
file does not exist (code -32600)
```

The same rollout **still exists** under the durable real Codex home:

```text
/Users/mingsun/.codex/sessions/2026/08/09/rollout-2026-08-09T17-20-31-019fe609-d234-7410-a2b3-46faa9868bdf.jsonl
```

So this is a stale absolute path in Codex's session index, not a missing transcript.

Timing (local):

- `t-f6bc9710` completed `15:06:42`; child `t-626c8144` started `15:14:21` (~8 min later)
- `t-749e9a2b` completed `17:45:06`; child `t-9d1e0576` started `18:03:02` (~18 min later)

Both children carried the parent session id in `dispatch_args.session_id` and died with 0 parsed events.

### Call chain (file:line)

1. **Capture session id (parent run)**  
   Codex emits `thread.started` → `parse_thread_started` puts `agent_session_id` in event metadata (`src/agent/codex.rs:362-373`).  
   Watcher persists it once via `store.update_agent_session_id` (`src/watcher/stream.rs:130-134`).

2. **Propagate id onto a new task** (any of these; all clone `task.agent_session_id` into `RunArgs.session_id` when `supports_session_resume()`):
   - dirty / uncommitted follow-up: `src/cmd/run_dirty.rs:168-169`
   - verify retry: `src/cmd/run_verify.rs:216-218` (and checklist path `260-261`)
   - hang / post retry: `src/cmd/run_post.rs:111-112`
   - iterate: `src/cmd/run_iterate.rs:133-134`
   - model self-heal: `src/cmd/run_model_selfheal.rs:58-59`
   - manual `aid retry`: `src/cmd/retry.rs:112-114`
   - missing-delivery recovery: `src/cmd/run_delivery_recovery.rs:29-38`

3. **Wire into Codex CLI**  
   `build_run_opts` copies `args.session_id` (`src/cmd/run_dispatch_execute.rs:30`).  
   `CodexAgent::build_command` switches to resume argv when set (`src/agent/codex.rs:83-91`):

   ```text
   codex exec resume --json --skip-git-repo-check <session_id> <prompt>
   ```

   There is **no** rollout existence check and **no** fallback to a fresh `codex exec` here.

4. **Per-task HOME isolation**  
   Foreground dispatch sets `HOME` to `~/.aid/tasks/<id>/home` (`src/agent/env.rs:175-176`, `src/cmd/run_dispatch_execute.rs:209`).  
   Isolation symlinks nearly everything from the real home, including `.codex` (`.codex` is **not** in `DEFAULT_DENYLIST`) (`src/agent/home_isolation.rs:14-18`, `145-152`).  
   On drop, the whole isolated home directory is deleted (`src/agent/home_isolation.rs:223-228`). Symlink removal does not delete real `~/.codex` contents, but it **does** invalidate any absolute path that went through `.../tasks/<old-id>/home/.codex/...`.

5. **Capability gate**  
   Codex opts into resume via `AgentKind::supports_session_resume` (`src/types/agent.rs:111-122`).

### Is the rollout/session path recorded per task and reused across tasks?

| What aid stores | Where | Reused across tasks? |
|-----------------|-------|----------------------|
| Codex `thread_id` only (`agent_session_id`) | `tasks.agent_session_id` / `dispatch_args.session_id` | **Yes** — copied onto child `RunArgs` |
| Absolute rollout path | **Not stored by aid** | Codex's own session index records `$HOME/.codex/sessions/...` using the HOME from the **creating** task |

Aid therefore reuses a **session id** across task ids while Codex binds that id to a **HOME-scoped absolute path**. After the creating task's `IsolatedHomeGuard` drops, resume from a later process fails even though the jsonl still lives under `~/.codex/sessions/`.

Same-process follow-ups can still succeed: `_home_guard` lives through `post_run_lifecycle` (`src/cmd/run_dispatch_execute.rs:209-304`), so dirty rescue like `t-346c5194` (started ~400ms after parent completion) can still open the parent's isolated path. Cross-process / delayed retries are the broken case.

### Existence check before `thread/resume`?

**None** in aid. Resume is unconditional once `opts.session_id` is `Some`. Failure surfaces only as Codex stderr + exit 1.

### What should happen when the rollout path is gone?

**Chosen (minimal):** before invoking resume for Codex, detect that the creating task's isolated rollout path is no longer resolvable; clear `session_id`, emit a Milestone such as `Codex session resume skipped: rollout missing; starting fresh session`, and run a normal `codex exec ... --full-auto` once.

Practical detection (prefer simple):

1. Prefer durable indexing going forward: set `CODEX_HOME` to the real absolute `~/.codex` (or `aid_dir()/codex`) in `apply_run_env` while keeping isolated `HOME` for identity. New sessions then store durable absolute paths.
2. For already-stale indexes: on stderr matching `failed to resolve rollout path` / `thread/resume failed`, auto-retry **once** with `session_id = None` and the Milestone above. Do not loop.

Rejected as sole fix: “only check `find_session_file` under real `~/.codex`” — for today's incidents the file **is** findable there, yet Codex still fails on the stale indexed path. Existence-by-thread-id alone is insufficient without either durable `CODEX_HOME` or a resume-error fallback.

---

## Bug 2 — Delivery guard fails a task that actually delivered

### What happened

`t-346c5194` is aid's auto dirty follow-up (`You have uncommitted changes...`) for parent `t-68a83a48`. Codex:

- resumed successfully (same `agent_session_id` `019fe604-...`)
- committed `b9525d8f`
- exited 0
- final `agent_message` (157 chars):

  > Committed all changes:  
  > `b9525d8f docs: investigate dead heartbeat decision counters`  
  > Worktree is clean. No source files, configs, or scripts were modified.

Aid then recorded `DeliveryAssessment::MissingFinalDelivery` and marked the task **FAILED**, with metadata:

```json
{"delivery_guard":"missing_final_delivery","exit_code":0,"last_message_chars":157,"last_work_kind":"command_execution"}
```

### Call chain (file:line)

1. **Evidence accumulation** while streaming Codex JSONL: `DeliveryEvidence::observe_codex_jsonl` (`src/delivery_guard.rs:139-163`). Work items (`command_execution`, etc.) update `last_work_*`; completed `agent_message` updates `last_message_chars` (Unicode scalar count of trimmed text).

2. **Validate** (`src/delivery_guard.rs:166-178`): delivery requires
   - last message sequence **after** last work sequence, **and**
   - `last_message_chars >= MIN_FINAL_MESSAGE_CHARS`

3. **Threshold:** `MIN_FINAL_MESSAGE_CHARS = 200` (`src/delivery_guard.rs:7`). Documented in `docs/design/codex-final-delivery-guard.md` (floor to reject acknowledgements / fragments; originally aimed at hollow **read-only** investigations).

4. **Fail closed on success exit** (`src/watcher.rs:155-178`): if Codex exited 0 but validate returns `MissingFinalDelivery`, force `TaskStatus::Failed`, persist assessment, insert the exact error string seen in the incident.

5. **Dirty follow-up origin** (`src/cmd/run_dirty.rs:69-70`, `167-185`): parent finished Done; aid spawned the commit-only child. Correct completion for that child is inherently a short confirmation after `git commit` / `git status` work events.

### Why 157 characters trips it

Ordering was fine: final message **after** `command_execution` (`last_work_kind: command_execution`). Only the length floor failed: `157 < 200`.

Tests encode this deliberately (`src/delivery_guard_tests.rs:44-52` rejects 138 chars). The guard does **not** look at git state, commit events, or “task purpose”; `output.md` length (641) is irrelevant — only the last JSONL `agent_message` text counts.

The design note that write-task diffs “do not manufacture a missing user-facing final response” (`docs/design/codex-final-delivery-guard.md`) is exactly why a successful commit follow-up still dies: the floor built for long investigation reports is applied unchanged to short operational confirmations.

### Proposed minimal fix

**Chosen:** keep the 200-char floor for **read-only** Codex runs (original hollow-report incident). For **non-read-only** Codex runs, treat a trailing non-empty `agent_message` after the last work event as `Delivered` (length floor waived).

That is a one-branch change in `DeliveryEvidence::validate` (needs a `read_only` flag plumbed into the watcher check, or a separate validate path). Dirty-rescue / commit follow-ups stop false-failing; read-only investigations keep the substantive-report requirement.

Rejected as sole fix: globally lowering 200 — re-opens the hollow read-only hole. Exempting only prompts that start with `You have uncommitted changes` — too narrow; any short write-task closing message can be valid.

---

## How the bugs relate

Same resume plumbing feeds dirty rescue. When resume still works (parent HOME alive in-process), Bug 2 can still fail a correct short delivery. When resume runs later (parent HOME gone), Bug 1 fails first with exit 1 and never reaches delivery assessment.

---

## What did I miss?

- Whether Codex exposes a way to re-bind a `thread_id` to a canonical path under `CODEX_HOME` without starting fresh (would preserve context better than skip-resume).
- Whether background workers drop `IsolatedHomeGuard` on a different schedule than foreground (may widen Bug 1 beyond delayed `aid retry`).
- Whether waiving the length floor for all write tasks is too loose for “exited after tools with only `ok`” — if so, a slightly higher non-read-only floor (e.g. 40) or requiring sentence punctuation may be safer.
- Interaction with missing-delivery auto-recovery (`src/cmd/run_delivery_recovery.rs`): it also resumes by session id and is read-only-only today; Bug 1 can break that path the same way.
- Confirm whether setting durable `CODEX_HOME` has auth/config side effects with the current `.codex` symlink layout.
