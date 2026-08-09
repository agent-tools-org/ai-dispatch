# Investigation — which recent task failures are aid's own bugs

Date: 2026-08-09
Method: read `~/.aid/aid.db` (`tasks`, `events`) and `~/.aid/logs/*.{jsonl,stderr}`
directly; three independent read-only code traces for the clusters that survived
triage. Status lines were never trusted on their own — every classification below
cites a captured log line or a `file:line`.

## Scope

Primary window, as requested: **2026-08-09 17:18–18:18** (15 tasks, 13 done,
2 failed). Two out-of-window clusters were investigated afterwards on request.

## Verdict

| # | Cluster | Tasks | Ours? |
|---|---------|-------|-------|
| 1 | Codex `thread/resume` points at a deleted task `$HOME` | t-9d1e0576, t-626c8144 | **aid bug** |
| 2 | Delivery guard fails a task that delivered correctly | t-346c5194 | **aid bug** |
| 3 | Verify cannot tell "build tool broke" from "change failed" | 8 tasks, 08-08/08-09 | **aid bug** |
| 4 | `mimo/mimo-auto` forced model rejected upstream | t-856cc6eb, t-275aa0a2 | **aid bug** (stale constant) |
| 5 | Dry-run milestone claims a dispatch that will not happen | t-402001d3, t-09bf6de1 | **aid bug** (cosmetic, but it misled triage) |
| 6 | Buffered agents reaped as hung (180s / 600s) | 8 tasks | known open bug, new instances |
| 7 | Provider refusals | ~12 codex + 1 opencode | not ours |
| 8 | Agent left the tree non-compiling | 4 codex retries | agent fault |
| 9 | Operator actions (SIGTERM, dry runs, probe prompts, worktree refusal) | 14 rows | not failures |

Both failures inside the requested one-hour window are #1 and #2 — i.e. **every
failure in that hour was aid's own**, and codex ran six other tasks green in the
same hour.

---

## 1. Codex session resume points at a deleted task HOME — aid bug

`t-9d1e0576` (18:03) and `t-626c8144` (15:14) both died in ~2.5s, exit 1, zero
parsed events:

```
Error: thread/resume: thread/resume failed: failed to resolve rollout path
`/Users/mingsun/.aid/tasks/t-749e9a2b/home/.codex/sessions/2026/08/09/rollout-…-019fe609-….jsonl`:
file does not exist (code -32600)
```

`t-749e9a2b` is a *different, already finished* task. Verified directly:

- `/Users/mingsun/.aid/tasks/t-749e9a2b/home` no longer exists.
- The rollout file itself **does** exist, under the durable real home:
  `~/.codex/sessions/2026/08/09/rollout-2026-08-09T17-20-31-019fe609-….jsonl`.

So the transcript is fine; the path recorded in Codex's session index is stale.

Mechanism (`docs/triage-codex-resume-and-delivery-guard.md`):

- aid stores only the Codex `thread_id` (`src/agent/codex.rs:362-373`,
  `src/watcher/stream.rs:130-134`) and copies it onto child tasks from seven
  call sites (`run_dirty.rs:168`, `run_verify.rs:216`, `run_post.rs:111`,
  `run_iterate.rs:133`, `run_model_selfheal.rs:58`, `retry.rs:112`,
  `run_delivery_recovery.rs:29`).
- Codex binds that id to an absolute path under the **creating** task's
  `$HOME` (`src/agent/env.rs:175-176`); `.codex` is symlinked into the isolated
  home rather than denylisted (`src/agent/home_isolation.rs:145-152`).
- `IsolatedHomeGuard::drop` deletes the whole isolated home
  (`src/agent/home_isolation.rs:222-228`), invalidating that absolute path.
- `build_command` issues `codex exec resume <id>` unconditionally with **no**
  existence check and **no** fresh-session fallback (`src/agent/codex.rs:83-91`).

Same-process follow-ups still work (the guard is alive), so this only bites
delayed / cross-process retries — 8 min and 18 min in the two observed cases.

Proposed fix (not implemented): give Codex a durable `CODEX_HOME` outside the
ephemeral task home, plus a one-shot fallback to a fresh session, recording a
milestone when resume resolution fails.

## 2. Delivery guard fails a correct delivery — aid bug

`t-346c5194` (17:22) exited **0** after committing `b9525d8f` with a clean
worktree. aid recorded:

```
error: Missing final delivery: Codex exited after work without a substantive final message
metadata: {"delivery_guard":"missing_final_delivery","exit_code":0,"last_message_chars":157}
```

`MIN_FINAL_MESSAGE_CHARS = 200` (`src/delivery_guard.rs:7`), enforced in
`DeliveryEvidence::validate` (`src/delivery_guard.rs:166-178`). The agent's
final message — "Committed all changes: `b9525d8f` … Worktree is clean." — is
157 chars and correct. Ordering passed; only length failed.

The task was aid's own auto-dispatched "you have uncommitted changes, please
commit them" follow-up, whose correct delivery is inherently short. A length
floor is the wrong instrument for that task shape.

Proposed fix (not implemented): keep the 200-char floor for read-only/report
tasks; waive it when a non-read-only task ends with a non-empty trailing
`agent_message`.

## 3. Verify conflates infrastructure failure with verification failure — aid bug

Eight tasks on 08-08/08-09 (droid t-97ec35c9, t-4797b875, t-e73442ab,
t-90df71af, t-5b666460; cursor t-5ac39de2, t-d8d5d786; agy t-f4bc86f5) ended:

```
Failed during verification: cargo check
Output: sccache: encountered fatal error | sccache: error: failed to spawn Command { … }
```

In several of them the agent's own last milestone was "Full test suite passes —
2097/2097" or "All 2096 tests pass". The agent built and tested green; aid's
verify step then died on a toolchain spawn failure and the delivery was marked
FAILED.

`VerifyResult.success` is the raw exit code and nothing else
(`src/verify.rs:104`); `run_verify.rs:70-88` maps any non-success to
`record_verify_failed` with no infra/verification distinction. `verify.rs` and
`agent/env.rs` contain no `env_remove` / `env_clear` at all, so verify inherits
the operator's whole environment (including `RUSTC_WRAPPER=sccache`) while the
agent ran under an isolated `$HOME`.

Caveat, stated as ambiguous: the trace's proposed *trigger* (stale isolated-home
paths in the shared `CARGO_TARGET_DIR`) is not proven — the captured sccache
command shows a host `CARGO_HOME`, and `/tmp` was at 95% during this period, so
resource exhaustion is an equally live candidate. What **is** proven is the
classification defect: aid cannot distinguish the two, so it blames the change.

Proposed fix (not implemented): detect toolchain/spawn failures in verify output
and record them as an infrastructure error (retryable, distinct status) rather
than a verification failure; `cmd.env_remove("RUSTC_WRAPPER")` in
`run_verify_with_timeout` is the one-line mitigation.

## 4. `mimo/mimo-auto` is rejected upstream — aid bug (stale constant)

`t-275aa0a2` and `t-856cc6eb` failed in 9s and 3s with exit 0:

```
{"error":{"message":"Unsupported model mimo-auto","statusCode":400}}
```

`mimo/mimo-auto` is aid's hardcoded default and forced model
(`src/agent/mimocode.rs:19`, `src/agent/registry.rs:328`). Detection worked —
`model_health::is_model_unavailable_error` matches "unsupported model"
(`src/model_health.rs:17`) and aid logged "model unavailable → retry on default"
— but the retry landed on the same dead id. `mimocode` is currently
`disabled = true` in `~/.aid/agent_config.toml`, so this is latent, not live.

## 5. Dry-run milestone claims a dispatch that never happens — aid bug (cosmetic)

`t-402001d3` and `t-09bf6de1` show only:

```
milestone: Held route skipped: codex (until Aug 11th, 2026 2:23 PM) — dispatching to claude instead.
```

and end at `skipped` with nothing run. This looked like a broken substitution.
It is not: both rows carry `dispatch_args.dry_run = true` (verified), and
`run_dispatch.rs:44-45` returns before any spawn. The agy row with the same
milestone (`t-d93c7ccb`, `dry_run = false`) did execute. There is no
Claude-specific guard anywhere (`src/agent/claude.rs:24-53`,
`src/agent/binary.rs:75-92`, `src/cmd/run_delegation.rs:15-40`), and genuine
substitution failures do record `failed` (`run_dispatch_resolve_held.rs:53-60`,
`run_dispatch_prepare.rs:263-275`).

The bug is only that the milestone text asserts a dispatch during a dry run.
It cost real triage time — it is worth the words.

Proposed fix (not implemented): make the milestone dry-run-aware —
"dry-run: would dispatch to claude instead".

## 6. Buffered agents reaped as hung — known open bug, 8 new instances

- 180s first-token / idle kill: `t-b735fd88`, `t-73b69cde`, `t-24f12f38`,
  `t-acdfa80c`, `t-6a2d71eb`, `t-f6a7b826` (agy ×4, grok ×2), all with
  `duration_ms ≈ 183 000` and `hung_detected`.
- 600s idle kill: `t-54c4560a`, `t-7f6d42f5` (agy).

New evidence worth recording: `t-b735fd88` was dispatched *with* an explicit
keep-alive instruction ("print a one-line `[MILESTONE]` …") and was still reaped
at 183s. The keep-alive workaround does not work for agents that buffer to their
own log instead of the PTY. `t-acdfa80c`, `t-6a2d71eb` and `t-54c4560a` had
changed files and `verify_status = passed` at the moment they were killed.

## 7–9. Not aid's bugs

- **Provider refusals.** ~12 codex tasks on 08-08 (`t-823d86e0`, `t-5e969794`,
  `t-799622c9`, `t-f5b9a40e`, `t-469203e8`, `t-5307a8a6`, `t-d9b038b5`,
  `t-1b8a9c2d`, `t-9d187e2d`, `t-6f47705d`, `t-c3a50b61`, `t-82762618`) each
  ended on `turn.failed: You've hit your usage limit … try again at Aug 11th,
  2026 2:23 PM`. All were separate `caller_kind = claude-code` dispatches by me,
  all **before** v10.18.0 (tagged 2026-08-09 00:22) introduced hold enforcement —
  already fixed, not re-reported. `t-26bfb60d`: opencode "Insufficient balance".
- **Agent fault.** `t-a5c0c828` → `t-bc0c1396` → `t-152b375a` → `t-00daed8f`:
  four successive codex retries each left the same `E0308` (`info:
  &CompletionInfo` … "consider borrowing here"); `cargo check` failed for real.
  `t-152b375a` additionally claimed "focused static checks pass" while the tree
  did not compile.
- **Operator actions, not failures.** `t-bdab241c`, `t-d93c7ccb` — SIGTERM, I
  stopped them. `t-a0ccb121` — aid correctly refused to use the caller checkout
  as a task worktree. Eight `skipped` rows with prompts `goal` (×4), `noop`
  (×2) and `say ok` (×2) are my own probes. Three rows carry
  `dispatch_args.dry_run = true` — `t-402001d3`, `t-09bf6de1` (§5) and
  `t-88ec8149`, which has zero events at all because a dry run returns before
  the task is ever claimed.

## Supporting traces

- `docs/triage-codex-resume-and-delivery-guard.md` (cursor, t-cff64440)
- `docs/triage-verify-infra-failure.md` (agy, t-ec727cdc)
- `docs/triage-held-route-substitution.md` (codex, t-7b9fce8e)

Nothing here is fixed yet; all three traces were read-only by design.
