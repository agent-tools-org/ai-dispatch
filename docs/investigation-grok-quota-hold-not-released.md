# Grok quota recovered but dispatch still refuses

**Date:** 2026-08-17
**Status:** root cause confirmed; no code change in this report
**Surface:** `aid run grok` / auto-selection / `aid agent quota`

## Problem

Grok Build usage recovered (aidbar shows 0% used), but aid still will not assign
grok, and an explicit `aid run grok` is refused or substituted. Question: is
there a scheduled probe that should have released the hold?

## Symptoms (live, this machine)

- `~/.aid/rate-limit-grok` written 2026-08-14 13:30:

  ```
  recovery_at:
  hold: manual
  provider: unknown
  message: API error (status 402 Payment Required): Grok Build usage balance exhausted
  ```

- `aid agent quota`: `grok LIMITED held until cleared with aid config clear-limit grok`
- `aid agent list`: grok `LIMITED`
- `~/.cache/aidbar/grok.json` fetched 2026-08-17 12:24, newer than the marker:

  ```
  ok: true
  used_percent: 0.0
  label: "Aug 11 – Aug 18"
  resets_at: 2026-08-18T00:55:28Z
  ```

## Competing hypotheses

| # | Hypothesis | Verdict |
|---|---|---|
| H1 | aid has no probe, so a recovered window can never reopen | Partial. aid itself never probes; aidbar does. The snapshot is present and would be enough for a clock hold. |
| H2 | aidbar snapshot missing, stale, or `ok:false`, so override cannot fire | Refuted. Snapshot is fresh, successful, 0% used, newer than the marker. |
| H3 | grok's 402 is classified `NeedsHuman`; live quota is forbidden to release it | Confirmed. |
| H4 | a model-group marker is holding a family while the agent looks recovered | Refuted. grok has no model groups; the hold is the whole-agent file. |

## Evidence

### 1. aid does not schedule probes

Quota knowledge in aid is after-the-fact text from a refusal, plus a read of
aidbar's disk cache at dispatch time.

- Write path: `src/rate_limit.rs` `write_marker` / `classify_hold`
- Read path: `src/rate_limit.rs` `dispatch_blocking_hold_at_path` → `live_quota::overrides_marker`
- `src/live_quota.rs` only *reads* `~/.cache/aidbar/{provider}.json`. It never
  fetches, never refreshes, never writes.

No launchd/cron in aid probes quota. `is_rate_limited` (`src/rate_limit.rs:460`)
does not consult live quota at all — only `marker_is_active`.

### 2. aidbar does probe on a timer

`/Users/mingsun/Develop/ai/aidbar/src/tray.rs:19`:

```
const REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);
```

Provider TTL defaults to 300s. Grok probe hits
`https://grok.com/grok_api_v2.GrokBuildBilling/GetGrokCreditsConfig` and maps
`used_percent` + billing period (`aidbar/src/providers/grok.rs:107-120`).
Process `application.dev.aidbar.menu.*` is running; cache mtimes are minutes
old.

v10.19 wired this cache into dispatch: a marker is released only when the
snapshot is newer than the marker **and** every window has headroom
(`used_percent ∈ [0, 100)`). That path works for clock holds (codex/agy/qwen).

### 3. grok's 402 is a permanent human hold

`src/rate_limit_signatures.rs:127-132`:

```
// "API error (status 402 Payment Required): Grok Build usage balance exhausted"
// A spent balance does not come back on a clock.
QuotaSignature { agent: Grok, needle: "usage balance exhausted", recovery: NeedsHuman }
```

Write-time: `hold: manual`, empty `recovery_at`.
Read-time: `StoredHold::NeedsHuman` stays active forever
(`src/rate_limit.rs:282`).
Override gate: `live_quota_can_override` is false for NeedsHuman
(`src/rate_limit.rs:250-252`). Tests pin this
(`src/rate_limit_hold_tests.rs:45-63`, `:339-348`;
`src/live_quota.rs` opencode NeedsHuman test).

The v10.19 changelog states the policy: a percentage never releases a hold only
a person can end. That rule was written for opencode `$19.37 / $20`, where 100%
is not the wall. Grok was lumped into the same class.

### 4. Dispatch refuses or substitutes before spawn

`src/cmd/run_dispatch_resolve.rs:147-165`: if
`dispatch_blocking_hold` returns `Some`, aid walks `--cascade` then
`coding_fallback_for_prompt`. If nothing is free:

```
grok is held (until cleared with `aid config clear-limit grok`).
Use --cascade <agent> or `aid config clear-limit grok` to clear.
```

That is v10.18: a held route is not spawned (no phantom FAIL). Only
`--declared-urgency background` keeps the requested agent.

Auto-selection (`src/agent/selection_scoring.rs:117`) subtracts 10 when
`is_rate_limited`. Fallback (`src/agent/selection_fallback.rs:92`) skips
rate-limited agents. Both use `is_rate_limited`, which ignores aidbar.

## Root cause

Two stacked facts:

1. **No aid-side scheduled release.** The only periodic probe is aidbar's
   5-minute tray refresh. aid consumes that cache only at dispatch, and only
   for non-`NeedsHuman` holds.

2. **Grok 402 is misclassified as a prepaid balance.** The message does not
   state a reset time, so v10.18 made it `NeedsHuman` to stop the old 5-minute
   transient from reopening a dead route. Grok Build credits *are* a billing
   window (aidbar already reports `period_start`/`period_end`). Today's
   snapshot is 0% used with `resets_at` 2026-08-18. The override conditions
   are all true except the NeedsHuman veto.

Trigger: 402 on 2026-08-14. Fragility: treating "no reset time in the refusal
text" as "only a person can end this", when an independent live probe already
knows the window.

## Immediate recovery

```
aid config clear-limit grok
```

Do not dispatch to grok until that file is gone. The marker will not age out
and aidbar cannot lift it.

## Fix options (for 老张)

1. **Reclassify grok `usage balance exhausted` as clock-ended.** Use
   `After(N)` as a floor, or better: let a newer grok aidbar snapshot with
   headroom release the hold (same rule as codex). Risk: if 402 ever means a
   true prepaid zero, a 0% snapshot would reopen too early. Need to confirm
   Grok Build 402 is always the window, never a top-up.

2. **Keep NeedsHuman, but allow live-quota override when the snapshot names a
   `resets_at` and has headroom.** Narrower than (1): still blocks opencode
   `insufficient balance` (windows have `resets_at: null` on this machine).
   Fits the v10.19 sentence "a percentage never releases a hold only a person
   can end" if we require a dated window, not just a percent.

3. **Leave classification; document the escape hatch.** Zero code risk. grok
   stays dark after every 402 until a human runs `clear-limit`.

Recommend measuring one more fact before choosing: capture the next grok 402
and the aidbar snapshot from the same minute. If `used_percent` is 100 and
`resets_at` is the period end, (1) or (2) is correct. If aidbar still shows
headroom while grok 402s, the probe and the CLI are not the same meter — do
not auto-release.

## What this report did not do

- Did not run `clear-limit`.
- Did not change signatures or override policy.
- Did not reproduce a live grok 402 (account currently has headroom).
