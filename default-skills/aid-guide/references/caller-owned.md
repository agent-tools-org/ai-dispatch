# Caller-owned execution (`aid.execution/1`)

`aid run --json` requires an explicit `--policy`. JSON wait/stop against an older schema
return `policy_unsupported` with `schema_upgrade_required`; open a normal command to migrate first.

Discover support with `aid capabilities --json`; require protocol `aid.execution/1` and
`policies.caller-owned.version` equal to 1 before submitting:

```bash
aid run codex "Implement the change" --policy caller-owned --id t-my-task --model gpt-5.5 --json --bg
aid wait t-my-task --json --timeout 60
aid stop t-my-task --json --force
aid show t-my-task --json
```

The policy pins the exact agent and explicit model. It disables retry, cascade, route/model
substitution, self-heal, hang retry, delivery recovery, dirty-worktree rescue agents, quota continuation,
verify/checklist retries, iterate, best-of, judge, peer review, audit, nested delegation,
transport fallback, and explicit `aid retry`. The store refuses every child of a caller-owned task. Batch retry counts an owned task's refusal
as a per-task failure and continues retrying the other tasks.
Project/team defaults, including completion hooks, cannot enable these features. Verify and checklist still run without retries.
Preservation commits, including dirty-worktree rescue and failure salvage, still run.
Other execution flags, including worktrees, sandbox and remote builds, retain their behavior.

Admission requires valid `--id` and nonempty `--model`. Unknown policies, known unavailable models, and
explicit positive `--retry`, `--cascade`, `--best-of`, `--iterate`, `--judge`, `--peer-review`,
`--audit`, `--budget`, `--dry-run`, `--container`, `--on-done` or `--hook` are refused before task insertion (exit 3, structured JSON error).
An agent without a known served-model list receives the explicit model unchanged.
A same-ID submission with the same agent (after alias/case normalization), model and policy returns `submission: existing`,
launches nothing and exits 0; a different tuple returns `id_conflict` (exit 4). Concurrent identical
submissions claim the ID once. Caller-owned IDs never receive an automatic suffix.

Receipts carry persisted monotonic `revision`, `policy`, `task` and `worker` fields. Task terminal
status and worker settlement are separate: a terminal task can still have a live worker.
`worker.state` is `pending`, `running`, `settled`, or `unconfirmed`; only `settled` makes
`settled: true`. Settlement outcomes are `completed`, `stopped`, `lost`, or `launch_failed`.
Default-mode receipts have `policy: null`, `worker.state: unconfirmed` and `settled: false`;
they are never derived or persisted by observation. JSON wait/stop refuse default tasks with
exit 3 (`policy_unsupported`) before startup reaping, observation, signalling or writes. Revisions come from a store-wide sequence and never
restart when an ID is deleted and reused; changing the agent also advances the revision.
Explicit policy writes advance revision even when the policy value stays the same.
Keep the receipt with the highest revision. Settlement is derived only by observation: the job
spec is absent (or its verified worker is dead) and the recorded agent process group is gone.
Caller-owned tasks refuse a second agent spawn at the common launch transaction. The agent PID, group and launch identity are persisted in the task row before monitoring starts,
so evidence survives spec deletion. Missing group/identity evidence remains `unconfirmed`.
A worker crash between spawning the agent and committing its evidence remains unconfirmed.
Evidence publication errors abort the owned launch, kill its verified agent group and reset evidence
after confirming exit. A known worker PID survives job-spec publication failure in the task row.
Lifecycle gates use the worker's saved dispatch policy; default launches add no evidence writes.
The group is always probed, even when the root launch identity differs. A mismatch never authorizes a signal. Processes that leave the agent group are outside
settlement; stop also signals captured descendants, including tools in separate sessions.
`aid show --json` embeds the last persisted receipt without probing processes. Its normal startup
still opens and migrates the store and reaps crashed workers, like other commands.
Run/wait/stop JSON observations persist the first settlement.
Transient wait observation errors are retried until timeout; the last valid receipt is retained.
A stop marker is recorded before signalling and cleared if stop cannot confirm completion. The first derived settlement wins: `stopped` for a recorded stop, `launch_failed` for a failed
submission that never spawned a worker or agent, `lost` for a dead worker's remaining spec,
otherwise `completed`. Worker, stop and reaper cleanup never write settlement directly.
`--container` is refused because its shared project container cannot be attributed to one task.
`--sandbox` retains its task-specific container behavior.

`aid wait --json` accepts exactly one caller-owned ID and uses the worker barrier. It exits 0 for settlement,
124 for an unsettled timeout (still printing the receipt), and 1 for unknown IDs or read errors.
`unconfirmed` keeps the wait open until timeout. `--timeout 0` observes once. `aid stop --json` targets exactly that worker, including one behind
a terminal row; it exits 0 for `stopped`/`already_settled` and 5 for `unconfirmed`.
Stop checks liveness without reaping recorded processes, verifies identity before every signal,
and confirms exit before recording settlement. A readable identity mismatch means the original
process is not a signal target: the reused PID is never signalled, and the recorded group must still be gone. A live process
with missing or unreadable identity returns `unconfirmed`; non-terminal task rows still become
`stopped`, but no settlement is written. Caller-owned text mode returns an error.
A terminal row with no live recorded task process keeps its status, artifacts, locks and final-state
metadata untouched. Text mode retains `task already <status>: ...`; JSON acknowledges settlement
with exit 0. Existing settlement is preserved. Worktree preservation runs only after signalling a
verified live process. Text mode on a default-mode task keeps its earlier behavior: a terminal
row returns `task already <status>: ...` before touching its worker or artifacts. Caller-owned text
mode and caller-owned JSON stops can signal a worker behind a terminal row. If another closer writes the
settlement first, JSON reports `already_settled`.
