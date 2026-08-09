# Grok silent failures since v10.10.0

Query window: `agent='grok'`, `created_at > 2026-08-06 08:38`, `status='failed'`, `exit_code IS NULL` → 5 rows.

## Verdict (all five)

**An aid watchdog/reaper killed a live grok process.** None of the five died on their own, and none failed to start.

Grok is a buffered agent (`streaming=false`): it writes nothing useful to the PTY until exit. At the time of these runs, the adapter did **not** pass `--debug-file` to `~/.aid/tasks/<id>/agent.log` (that landed later in `cad6e02d`, 2026-08-08 22:39 +07, v10.18.0). So aid had no byte proof-of-life and reaped healthy sessions.

Chose the simpler option when the session and aid log disagreed with later code comments: the kill line in aid's log is decisive; session/memtrace prove grok was mid-work.

---

## Per-task

### 1. `t-764b2a1d` — aid idle/orphan reaper (1200s)

| | |
|---|---|
| Created | 2026-08-07T16:46:39+07 |
| Worktree | `…/poolstrade-compounder-a279c7ca/test/executor-fork` |
| Aid log / stderr | `hung detected (monitor wedged): no events for 1200s (idle timeout 600s, margin 2x)` |
| `agent.log` (`--debug-file`) | **missing** |
| Grok session | `019fdb9e-2091-70a0-a4f4-a2e422f66ef8` (`grok_home` = this task) |
| Memtrace | `~/.grok/memtrace/1786095999-26238.jsonl` — `start` pid 26238 at task start through ~20 min of samples |

**Evidence:** Session last event at `2026-08-07T10:06:26.003Z` is `phase_changed` → `tool_execution` after `tool_started` `run_terminal_command` — grok was mid-turn when aid reaped at 17:06:39+07. 475 messages / 85 tool starts. Aid only had a setup event (`Cargo target seeded…`), so the orphan path used **2× idle (1200s)** rather than first-token.

Note: later commit `7ffb87cf` names this id for a hiboss Stop-hook theory. This session has no hiboss/Stop-hook stall at kill time; the observed killer is the idle reaper against a silent buffered stream.

### 2. `t-a4d41b83` — aid first-token dead-stream detector (180s)

| | |
|---|---|
| Created | 2026-08-07T20:06:15+07 |
| Aid log / stderr | `hung detected (monitor wedged): no agent output since spawn for 180s (first-token timeout)` |
| `agent.log` | **missing** |
| Grok session | `019fdc54-e11f-72e3-95e4-41e2ca546b82` in uniswapx-filler cwd |
| Memtrace | `1786107975-93790.jsonl` — alive across the 180s window |

**Evidence:** Session active until `13:09:14Z` (~180s): 23 tools completed, last phase `streaming_reasoning`. Aid events: `event_count: 0` then first-token hang.

### 3. `t-de706ea8` — aid first-token dead-stream detector (180s)

| | |
|---|---|
| Created | 2026-08-07T21:42:48+07 |
| Aid log / stderr | `hung detected (monitor wedged): no agent output since spawn for 180s (first-token timeout)` |
| `agent.log` | **missing** |
| Grok session | `019fdcad-42f2-73f2-a89a-ecb1204bdfd1` |
| Memtrace | `1786113768-16490.jsonl` |

**Evidence:** Session last event `waiting_for_model` at `14:45:47Z`; 18 tools completed. Same first-token reap pattern as (2).

### 4. `t-f0492930` — aid first-token dead-stream detector (180s)

| | |
|---|---|
| Created | 2026-08-08T01:06:18+07 |
| Worktree | `…/morpho-liquidator/preheat-value-admission` |
| Aid log / stderr | `hung detected (monitor wedged): no agent output since spawn for 180s (first-token timeout)` |
| `agent.log` | **missing** |
| Grok session | `019fdd67-98f1-7693-98ae-83643bd39004` |
| Memtrace | `1786125979-44587.jsonl` |

**Evidence:** Session last events at `18:09:20Z` are successful tool completions then `waiting_for_model` (28 tools). Aid had a setup event but hung metadata still reports first-token / `event_count: 0` for agent output since spawn.

### 5. `t-f6a7b826` — aid first-token dead-stream detector (180s)

| | |
|---|---|
| Created | 2026-08-08T22:24:46+07 |
| Aid log / stderr | `hung detected (monitor wedged): no agent output since spawn for 180s (first-token timeout)` |
| `agent.log` | **missing** |
| Grok session | `019fe1fa-0c05-7ee0-9fd5-fd55d98b12c4` (prompt includes “Previous attempt hung after 180 seconds…”) |
| Memtrace | `1786202687-52606.jsonl` |

**Evidence:** Session last phase `streaming_reasoning` at `15:27:32Z`; 24 tools completed. Killed ~15 minutes before `cad6e02d` shipped `--debug-file` + buffered-liveness for both reapers.

---

## Shared pattern

| Signal | All five |
|---|---|
| Aid capture (`*.jsonl` / `*.stderr`) | Only the hung line — no agent JSON |
| `~/.aid/tasks/<id>/agent.log` | Absent (no `--debug-file` yet) |
| Grok process | Started (memtrace `kind:start`) |
| Grok work | Session files growing; tools mid-flight at reap |
| `exit_code` in DB | NULL — hung path, not a natural process exit |

**Root cause:** aid misclassified buffered silence as death. Not grok crashes, not spawn failures.

**Fixes already landed after these incidents (not re-verified here):**
- `7ffb87cf` — `--deny Bash(hiboss:*)` on grok (2026-08-07 17:56)
- `cad6e02d` — pass `--debug-file` + check agent-log growth in first-token / orphan reapers (2026-08-08 22:39)
