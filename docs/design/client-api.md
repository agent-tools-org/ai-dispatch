# aid client API — contract

The HTTP surface `aid web` exposes for the AID Command desktop client (macOS + iPadOS).
This document is the contract between the server (`src/web/`) and the client
(`client/`). Both sides are built against it; neither invents a field.

Status: design. Server work lands on `feat/client-api`, client work on `feat/fleet-client`.

## 1. Principles

- **One namespace.** The client API extends the existing `/api/` surface additively.
  No `/api/v1/` fork, no parallel API, nothing deprecated. Existing endpoints keep
  their current response shape so the embedded web dashboard keeps working.
- **One round trip for a screen.** The client should never issue N+1 requests to
  paint a view. `/api/fleet` returns everything the main window needs.
- **Unknown stays unknown.** A field aid does not know is `null`, never a plausible
  substitute. No `0.0` for an unmeasured cost, no `"auto"` for an unobserved model,
  no `0` for an unknown progress. The client renders `null` as `—`.
- **The server does not invent progress.** aid has no percentage for a running task.
  It reports the facts it has (`started_at`, `duration_ms`, `latest_milestone`) and
  the client derives any bar it wants. Do not add a `progress` field.

## 2. Transport, binding and auth

Today `aid web` binds `127.0.0.1` with no auth. An iPad on the LAN cannot reach that,
so:

```
aid web [--port 8080] [--host 127.0.0.1] [--token <token>]
```

- `--host` defaults to `127.0.0.1` (unchanged). `--host 0.0.0.0` (or any non-loopback
  address) binds for LAN access.
- **Auth is mandatory whenever the bind address is not loopback.** Starting with a
  non-loopback host and no token is an error, not a warning — refuse to start rather
  than exposing the dispatch surface unauthenticated. `--token` may be supplied, or
  aid generates one (32 bytes, base64url) and persists it at `~/.aid/web_token`
  (mode 0600), reusing it across restarts.
- On a loopback bind the token is optional; if one is configured it is still accepted.
- Credentials travel as `Authorization: Bearer <token>`. For SSE, `?token=<token>` is
  also accepted, because not every client can set headers on an event stream.
- A failed or missing token returns `401` with `{"error":"unauthorized"}`. Never
  reveal whether the token merely expired vs. was wrong.
- On startup with a non-loopback bind, print the reachable URL and the token once:

```
[aid] web listening on http://192.168.1.24:8080
[aid] client token: 8sJd…  (also at ~/.aid/web_token)
```

- Rate-limit failed auth attempts (e.g. 10/min per peer) so the token cannot be
  brute-forced over a LAN.

## 3. Endpoints

### 3.1 Existing (unchanged response shape)

| Method | Path | Notes |
|---|---|---|
| GET | `/api/tasks?filter=` | `[Task]` |
| GET | `/api/tasks/{id}` | `Task` |
| GET | `/api/tasks/{id}/events` | `[Event]` |
| GET | `/api/tasks/{id}/output` | `{output}` |
| GET | `/api/tasks/{id}/diff` | `{diff}` |
| POST | `/api/tasks/{id}/stop` | `{ok, error?}` |
| POST | `/api/tasks/{id}/retry` | body `{feedback?}` → `{ok, new_task_id?, error?}` |
| POST | `/api/tasks/{id}/merge` | `{ok, error?}` |
| GET | `/api/usage` | `{agents:[…]}` |
| GET | `/api/events` | SSE |

`Task` gains fields (see §4); it loses none.

### 3.2 New

| Method | Path | Purpose |
|---|---|---|
| GET | `/api/fleet` | one snapshot: server info, summary, sectors with their tasks, agent roster |
| GET | `/api/agents` | agent roster + quota, standalone |
| GET | `/api/tasks/{id}/result` | the saved `result.md` report if the task wrote one |
| POST | `/api/tasks/{id}/steer` | body `{message}` — mid-flight course correction |
| POST | `/api/tasks/{id}/respond` | body `{message}` — answer an `awaiting_input` task |
| POST | `/api/tasks/{id}/accept` | principal accepts the delivery |
| POST | `/api/tasks/{id}/reject` | principal rejects; artifacts preserved |

Every action endpoint returns `{ok: bool, error: string?}` and is **idempotent in
effect**: stopping a stopped task, accepting an accepted one, is `ok: true` with no
side effect, not a 500. An action that is illegal for the task's current state
returns `409` with `{ok:false, error:"<why>"}` — the client shows the reason and
does not retry.

`steer` and `respond` must reuse the same code paths as `aid steer` / `aid respond`,
including their guards. Note the known hazard recorded in this repo: steering a
buffered agent (grok, agy) can kill it — if that guard exists in the CLI path, it
must apply here too. Do not build a second, weaker implementation.

## 4. Payloads

### 4.1 `GET /api/fleet`

```jsonc
{
  "server":  { "version": "10.37.0", "host": "192.168.1.24", "port": 8080,
                "started_at": "2026-08-20T07:00:00Z", "aid_home": "/Users/…/.aid" },
  "summary": { "running": 3, "done": 12, "failed": 4, "stopped": 2,
               "spend_usd": 4.31, "tokens": 28400000, "memory_mb": 411,
               "window": "today" },
  "sectors": [
    { "id": "uniswapx-filler",            // project_id, or last path component of repo_path
      "name": "uniswapx-filler",
      "repo_path": "/Users/…/uniswapx-filler",
      "workgroup_id": "wg-8937e74c",
      "tasks": [ /* Task, see 4.2 */ ] }
  ],
  "agents": [ /* Agent, see 4.3 */ ]
}
```

`summary.window` states what the counts cover (`today` by default; `?window=7d`
accepts `today|24h|7d|30d|all`). A count with no stated window is a lie the client
would render as a total.

Tasks in `/api/fleet` are the same `Task` object as `/api/tasks/{id}`, minus
`prompt`/`resolved_prompt` (they can be large) — the detail view fetches those.
Include `prompt_excerpt`: the first 160 characters, so a list row has something to show.

### 4.2 `Task`

Current fields are kept verbatim. Added:

| Field | Type | Meaning |
|---|---|---|
| `started_at` | `string?` | when the agent actually started, distinct from `created_at` |
| `prompt_excerpt` | `string` | first 160 chars of the prompt, for list rows |
| `sector_id` | `string?` | the grouping key used by `/api/fleet` |
| `difficulty` | `string?` | declared profile: `trivial\|simple\|moderate\|complex` |
| `rigor` | `string?` | declared profile: `draft\|standard\|critical` |
| `budget_class` | `string?` | declared profile: `free\|cheap\|standard\|premium` |
| `urgency` | `string?` | declared profile: `background\|normal\|urgent` |
| `memory_mb` | `i64?` | resident memory of the running agent, if measured |
| `has_result` | `bool` | whether `/result` will return a report |
| `has_diff` | `bool` | whether the task has a non-empty diff |
| `awaiting_reason` | `string?` | why the task is `awaiting_input` |
| `latest_events` | `[Event]` | the last 3 events, so a list row can show live activity |

The four profile fields are **`null` when undeclared** — aid stores them as null and
does not infer them. The client must render an undeclared profile as blank, never as
a default value.

### 4.3 `Agent`

Assembled from the same source as `aid agent list --json` — do not write a second
agent-inspection path.

```jsonc
{ "name": "codex", "kind": "builtin", "installed": true, "disabled": false,
  "provider": "openai", "metering": "subscription",
  "quota": { "state": "ok", "recovery_at": null, "message": null, "source": "marker" },
  "default_model": "gpt-5.6", "observed_model": "gpt-5.6",
  "busy": true, "running_task_ids": ["t-0c41aa19"],
  "success_rate": 0.82, "task_count": 41, "avg_cost_usd": 0.29 }
```

`quota.state` is aid's own hold state (`ok | limited | held | unknown`). The client
renders `unknown` as unknown — it never guesses `ok`.

### 4.4 SSE `/api/events`

Existing events keep their names. `task_update` gains `outcome`, `verify_status`,
`sector_id`, `latest_error`. Two new events:

- `agent_update` — `{name, quota, busy, running_task_ids}` when a hold is taken or
  released, so the client's crew roster is live.
- `fleet_summary` — the `summary` block, at most once per 2s, so gauges move without
  polling.

`heartbeat` stays as is; the client uses it to detect a dead link and show the
LINK lamp as red.

## 5. What is explicitly out of scope

- Dispatching new tasks from the client (`aid run`). Read, monitor and act on
  existing tasks only. Dispatch is the orchestrator's job and needs the full profile
  declaration; a phone-sized form would produce undeclared profiles, which this repo
  treats as a defect.
- Editing files or viewing the worktree tree.
- Multi-user accounts. One token, one commander.

## 6. Server acceptance

- `aid web --host 0.0.0.0` without a token refuses to start, with a message naming
  `--token`.
- With a token, every endpoint rejects a missing/incorrect bearer with `401`,
  proven by a test that asserts a wrong token fails and the right one succeeds.
- `/api/fleet` is one query round; assert it does not issue a per-task query in a
  loop (the store already has `*_batch` helpers — use them).
- A task with no cost serialises `"cost_usd": null`, not `0.0`. Assert this.
- `steer`/`respond`/`accept`/`reject` reuse the CLI code paths; a test asserts the
  same guard rejects the same illegal transition through both entry points.
