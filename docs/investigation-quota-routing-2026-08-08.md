# Investigation: a held route is still dispatched, and quota never ranks live routes

Date: 2026-08-08 · Status: root-caused, not fixed · aid 10.17.2 (e642d4f7)

## Problem

Two questions from the operator:

1. codex is known unavailable (usage limit, resets Aug 11) — why does aid keep dispatching to it?
2. grok has quota left and is almost never used — is our usage information opaque?

## Evidence

### 1. codex was dispatched 9 times today, every one refused in ~4s

`~/.aid/aid.db`, `agent='codex' and created_at > '2026-08-08'`:

| task | time | status |
|---|---|---|
| t-c3a50b61 | 02:39 | failed |
| t-e8bd25a3 | 09:34 | failed |
| t-9d187e2d | 09:44 | failed |
| t-6f47705d | 13:38 | failed |
| t-1b8a9c2d | 17:15 | failed |
| t-d9b038b5 | 21:06 | failed |
| t-5307a8a6 | 21:14 | failed |
| t-469203e8 | 21:19 | failed |
| t-f5b9a40e | 21:27 | failed |

Every one carries the same error event:

```
You've hit your usage limit. Visit https://chatgpt.com/codex/settings/usage
to purchase more credits or try again at Aug 11th, 2026 2:23 PM.
```

So from **02:39** onward aid held a marker (`~/.aid/rate-limit-codex`,
`recovery_at: Aug 11th, 2026 2:23 PM`) stating a three-day outage, and dispatched
into it eight more times.

The four evening tasks each spawned a cascade child that succeeded
(`t-7ef5ded7`, `t-b3af1ae9`, `t-22c28562`, `t-97c72328`, all `agy`, all `done`).
The reviews were delivered; the codex rows are pure waste and pure board noise.

### 2. Reproduced live

```
$ aid run codex "Correctness review of one commit on branch fix/net-loss-breaker …" \
    --difficulty moderate --budget standard --urgency normal --rigor standard --dry-run
[aid] codex is rate-limited (until Aug 11th, 2026 2:23 PM), auto-cascading to opencode
[dry-run] Agent: codex
```

aid states the hold, states the fallback, and then dispatches **codex**.

(The auto-cascade here picks `opencode`; the evening runs carried
`dispatch_args.cascade = ["agy"]`, i.e. the caller passed `--cascade agy`. Both
branches behave the same — see below.)

## Root cause

`src/cmd/run_dispatch_resolve.rs:140-168`

```rust
} else if let Some(hold) = rate_limit::dispatch_blocking_hold(&agent_kind) {
    if let Some(next_agent) = args.cascade.first() {
        aid_warn!("… will cascade to {}", next_agent);          // logs only
    } else if let Some(fallback) = coding_fallback_for_prompt(…) {
        aid_warn!("… auto-cascading to {}", fallback);
        args.cascade = vec![fallback];                           // arms only
    } else {
        anyhow::bail!(…);
    }
}
```

Detection is correct. The **response** is not: neither branch changes
`agent_kind` and neither skips the dispatch. `args.cascade` is consumed only
after the primary fails (`src/cmd/run_post.rs:99`, inside the transient-failure
handler). A live hold therefore costs one real dispatch, one refusal, one `failed`
task row, and only then routes to the agent aid had already chosen.

`aid batch` does not have this defect — `src/cmd/batch_dispatch_support.rs:238`
picks the first non-rate-limited candidate *before* dispatch. `aid run` is the
inconsistent path.

## Why grok is never chosen

Not a quota decision. `src/agent/selection_fallback.rs:90-94` (`is_usable_fallback`)
uses rate-limit state only to **exclude** known-dead routes; it never **ranks**
live ones. Ranking is `base_score` over the static matrix in
`src/agent/selection_capabilities.rs:25-30`:

```rust
(AgentKind::Grok, &[
    Research 4, Documentation 4, Debugging 4, SimpleEdit 4,
    ComplexImpl 4, Frontend 3, Testing 3, Refactoring 4,
]),
```

A uniform row — the shape of a placeholder nobody measured. For every category
grok loses outright: ComplexImpl codex 9 / droid 9 / copilot 8 / oz 8 /
commandcode 8 / cursor 7 vs grok 4; Research agy 9 / gemini 9 / qwen 8 vs grok 4.
`pick_fallback` can only reach grok when essentially every other installed agent
is disabled or held. Having quota is irrelevant to that outcome.

Caveat against simply raising it: grok has a recorded defect — it silently
cancels its own edits without `--always-approve`, and has reported DONE on a
cancelled run. That argues for raising it on **read-only review/audit** work,
where the defect cannot bite, rather than across the board.

## Transparency gaps

1. `aid agent list` — the natural "who can I use" command — prints name, egress,
   description and **no status**. codex, qwen and copilot all show identically to
   agy (`src/cmd/agent_display.rs:96`).
2. `aid agent quota` does show status, but is a separate, undiscoverable command.
3. `show_quota` / `rate_limited_agents` (`src/cmd/agent_display.rs:74`,
   `src/rate_limit.rs:330`) iterate agent-level markers only. Group markers are
   invisible: `~/.aid/rate-limit-cursor--premium` carries `hold: manual` — a hold
   that never expires without human action — and `aid agent quota` still prints
   `cursor  OK`.
4. Nothing reads *remaining* quota. aid only learns a route is dead by dying on
   it. `aidbar` already probes provider quota live; aid does not consume it.

## Fix options

**F1 — a held route is not dispatched** (`run_dispatch_resolve.rs:140`)
When `dispatch_blocking_hold` returns `Some` and a fallback exists (caller-passed
or auto), switch `agent_kind` to the fallback instead of arming a post-failure
cascade. Record the held route as a divert event on the dispatched task; do not
create a `failed` row for it (`status='skipped'` already exists — `--dry-run`
uses it). Keep unchanged: the `--declared-urgency background` exemption
(`:133`), and the transient-cooldown class deliberately not gating (`:224`).

Trade-off to encode: the burned dispatch is currently the only self-correction
for a stale marker (operator tops up before Aug 11 → probe succeeds → marker
cleared on success). Skipping the probe means a topped-up account stays diverted,
so the divert message must name the escape hatch
(`aid config clear-limit codex`). `NeedsHuman` holds never self-clear anyway.

**F2 — make state visible where the choice is made**
STATUS column in `aid agent list`; include group markers in
`rate_limited_agents()` / `show_quota` so `cursor` cannot read OK while its
premium group is manually held.

**F3 — rank live routes on evidence**
Re-derive grok's row (and check the others) from `aid stats` outcome data rather
than hand-tuning, and scope any raise to read-only review/audit categories until
the edit-cancellation defect is confirmed fixed.

## Two further aid defects, surfaced while fixing the above

Both were found by checking the worktree rather than trusting the status line.

### D1 — an empty run recorded as a delivery, crediting the parent's commit

`t-e74132e8` (cursor, retry of `t-20fa3a13`) recorded `status=done`, `exit_code=0`,
and a completion summary reading *"done: 5 files changed (…)"*. The worktree had
no new commit and no dirty file. The agent's own result envelope was:

```json
{"type":"result","subtype":"success","duration_ms":6705,
 "result":"","usage":{"outputTokens":0,…}}
```

Zero output tokens, empty result — the model produced nothing. The five files
named are the **parent task's** commit (`3f5d954e`), i.e. the branch diff against
`main`, not this run's work. So a retry that did nothing inherited credit for the
attempt before it.

Two separable bugs: (a) an exit-0 run with an empty result and zero output tokens
must not classify as `done` — `DeliveryAssessment` / `HollowOutput` machinery
exists and did not fire; (b) "files changed" must be measured from the task's own
start SHA, not from the branch's divergence from `main`.

### D2 — sccache spawn failure reported as the agent failing verification

`t-97ec35c9` and `t-4797b875` both recorded `verify_status=failed` with:

```
sccache: encountered fatal error
sccache: error: failed to spawn Command { … unicode-ident-1.0.24 … }
```

Re-running `cargo check --bin aid` and `aid test --isolated --bin aid` on the same
branch: clean, **2099 passed, 0 failed**. The agent's code was correct both times;
aid's own verify step died in the build toolchain and attributed it to the agent.
A verify failure whose output is a wrapper/toolchain spawn error is an
infrastructure fault and must be reported as one — retrying the agent cannot fix
it, and it wasted a full re-dispatch here.

### D3 — the 180s first-token budget cannot be raised by anything (root cause found)

Both cross-audit tasks dispatched for the fixes above died at **exactly 183s**:

```
t-24f12f38 (agy)  22:24:32 hung_detected · Agent hung: no output for 180 seconds
t-73b69cde (grok) 22:24:43 hung_detected · Agent hung: no output for 180 seconds
```

This is the long-standing "killed at 183s despite `--idle-timeout 900`" symptom.
The cause is one line — `src/timeout_policy.rs:81`:

```rust
pub(crate) fn resolve(agent_name, cli_idle_secs, cli_max_duration_mins, project) -> Self {
    …
    idle:        Duration::from_secs(idle_secs),   // CLI → agent cfg → project → default
    first_token: defaults.first_token,             // ← always 180, nothing can reach it
```

Every other field resolves through the CLI/agent/project precedence chain;
`first_token` is pinned to the default. `--idle-timeout` only ever feeds `idle`,
which is why raising it never helped. And `env_with_policy` (`:150`) then writes
that same 180 into the child environment, so an operator who exports
`AID_FIRST_TOKEN_TIMEOUT_SECS` has it silently overwritten by aid — `from_env`
honours the variable, but `resolve` has already discarded any chance of setting it.

`src/pty_watch.rs:449-461` then kills the task with `transient: true`.

Impact is concentrated on exactly the work aid is asked to do most carefully: a
read-only audit reads several files before it says anything, and 180 seconds of
quiet reading is normal. The default is not wrong; being unable to change it is.

## Note on orchestration habit

Half of "why does it keep dispatching to codex" is caller-side: the dispatching
session chose codex four times in 21 minutes without consulting
`aid agent quota` or `aid advise`. F1 and F2 make that safe and discoverable;
they do not make it correct.
