# Caller-owned execution protocol (`aid.execution/1`)

Status: frozen interface. Implementations and tests follow this file; a change to any name, value,
exit code or rule below is a new protocol version.

A caller-owned task is one execution the caller fully specifies: task ID, agent and model. aid launches
exactly that execution and nothing else on its behalf. Default mode (no `--policy`) keeps all existing
behaviour.

## 1. Capability discovery

```
aid capabilities --json
```

Read-only: opens no task, writes nothing. Exit 0. Output:

```json
{
  "protocol": "aid.execution/1",
  "aid_version": "11.8.0",
  "capabilities": ["caller_owned_policy", "settlement_receipt", "worker_stop", "idempotent_submit"],
  "policies": {
    "caller-owned": {
      "version": 1,
      "requires": ["id", "model"],
      "refused_flags": ["retry", "cascade", "best_of", "iterate", "judge", "peer_review", "audit", "budget"],
      "disabled": ["retry", "cascade", "model_substitution", "route_substitution", "model_self_heal",
                   "hang_retry", "delivery_recovery", "dirty_rescue", "quota_continuation",
                   "verify_retry", "checklist_retry", "iterate", "best_of", "judge", "peer_review",
                   "audit", "nested_delegation", "transport_fallback", "explicit_retry"]
    }
  }
}
```

A caller that does not find `aid.execution/1` and `caller-owned` version 1 must not submit.
Unknown keys may be added within version 1; existing keys and values never change meaning.

## 2. Submission

```
aid run <agent> <prompt> --policy caller-owned --id <ID> --model <MODEL> --json [other flags]
```

Refused before any row is written or changed, exit 3, nothing launched:

- `--policy` names an unknown policy (`policy_unsupported`).
- `--id` missing or invalid (`id_required`), `--model` missing (`model_required`).
- The model is not served by the agent (`model_not_served`).
- Any refused flag is given explicitly: `--retry N` with N > 0, `--cascade`, `--best-of`, `--iterate`,
  `--judge`, `--peer-review`, `--audit`, `--budget` (`flag_refused`, `detail` names the flag).

Project and team defaults that would enable a refused flag are ignored under this policy.
`--verify`, `--checklist`, `--worktree`, `--dir`, `--timeout`, `--bg`, `--context`, `--skill`,
`--sandbox`, `--container` and `--remote-build` keep their behaviour; they never start another agent.

On acceptance aid writes the task row and the policy record in one transaction before launch, then
launches the exact agent/model. No route hold, rate-limit group or model resolution substitutes either.

Exit 0 with a receipt (section 3) plus `"submission": "launched"`.

### Same ID again

- Existing row with the same agent, model and policy: nothing is launched, exit 0, the current
  receipt with `"submission": "existing"`. This covers lost responses and concurrent duplicates: of two
  racing identical submits exactly one launches.
- Existing row with a different agent, model or policy (including a default-mode row): exit 4,
  `id_conflict`, nothing launched, the existing row unchanged.
- The default-mode auto-suffix (`<id>-2`) never applies to caller-owned submissions.

### Refusal body (exit 3 or 4)

```json
{"protocol": "aid.execution/1", "error": {"code": "flag_refused", "message": "...", "detail": "cascade"}}
```

Codes: `policy_unsupported`, `id_required`, `model_required`, `model_not_served`, `flag_refused`,
`id_conflict`, `not_found` (observation/stop of an unknown ID, exit 1).

## 3. Execution receipt

Printed by `aid run --json` (caller-owned), `aid wait <ID> --json`, `aid stop <ID> --json`, and
embedded as `"execution"` in `aid show <ID> --json`. Every task has a receipt; `policy` is `null` in
default mode.

```json
{
  "protocol": "aid.execution/1",
  "task_id": "t-aich-0001",
  "revision": 6,
  "policy": {"name": "caller-owned", "version": 1, "agent": "codex", "model": "gpt-5.5",
             "accepted_at": "2026-10-29T10:00:00Z"},
  "task": {"status": "done", "terminal": true, "agent": "codex", "requested_model": "gpt-5.5",
           "observed_model": "gpt-5.5", "attribution_source": "...", "exit_code": 0,
           "completed_at": "2026-10-29T10:12:00Z"},
  "worker": {"state": "settled", "outcome": "completed", "pid": null, "settled_at": "2026-10-29T10:12:03Z",
             "detail": null},
  "settled": true,
  "observed_at": "2026-10-29T10:13:00Z"
}
```

- `task.status` is the task row; `terminal` is true for done, failed, stopped, merged, skipped.
  A terminal task does not mean the execution has ended.
- `worker.state`: `pending` (row exists, worker not yet running), `running` (worker process alive),
  `settled` (persisted settlement record), `unconfirmed` (evidence missing or unverifiable, e.g. a row
  from before this protocol, an unreadable job spec, a PID whose identity cannot be checked).
- `worker.outcome` (only when settled): `completed` (worker finished its lifecycle), `stopped` (stop
  confirmed the worker gone), `lost` (worker confirmed dead before finishing), `launch_failed`.
- `settled` is true exactly when `worker.state == "settled"`. Settled means the worker closed its
  lifecycle and, under caller-owned policy, no further execution can be created for this task.
- `revision` is persisted with the task row, starts at 1, and strictly increases on every change of
  status, worker settlement or policy record. It survives restarts and never decreases. Equal revision
  means equal lifecycle state; a consumer keeps the receipt with the highest revision.
- Model and usage fields come from this task's own row only, never from another task.

## 4. Settlement wait

```
aid wait <ID> --json [--timeout SECS]
```

Exactly one task ID with `--json`. Uses the existing worker barrier: it returns when the worker is
settled (or unconfirmed with no live process), not when the status turns terminal. `--timeout 0` is a
single observation. Exit 0 when the printed receipt has `settled: true`; 124 when the timeout passed
first (the receipt is still printed, `settled: false`); 1 for `not_found` or a read error.

## 5. Stop

```
aid stop <ID> --json [--force]
```

Targets the worker of exactly that ID, in any task status. A terminal row whose worker is alive is a
valid target: aid signals the verified worker and agent processes, waits for exit, then persists
settlement `stopped`. A row with no live process for it is settled as `stopped` (or keeps its existing
settlement). Output is the receipt plus:

```json
"stop": {"result": "stopped" | "already_settled" | "unconfirmed", "detail": "..."}
```

Exit 0 for `stopped` and `already_settled`; exit 5 for `unconfirmed` (signal failed, process still
alive, or identity unverifiable). A failed stop never claims settlement; `aid wait <ID> --json` still
observes the same ID afterwards.

## 6. Enforcement

- The policy record is read from the store, not from process memory, so a restarted worker or a later
  command applies it.
- No task row may be created with a caller-owned task as parent, for any origin (retry, cascade,
  self-heal, nested `aid run` from the agent, `aid retry`). This store-level guard backs the per-path
  gates.
- The Codex app-server to `codex exec` transport fallback is disabled; an app-server failure is the
  attempt's failure.
- Model unavailability ends the attempt as `failed` with its error event. Choosing another model is a
  new caller submission with a new ID.
