# Caller-owned execution (`aid.execution/1`)

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
transport fallback, and explicit `aid retry`. The store refuses every child of a caller-owned task.
Project/team defaults cannot enable these features. Verify and checklist still run without retries.
Preservation commits, including dirty-worktree rescue and failure salvage, still run.
Other execution flags, including worktrees, sandbox and remote builds, retain their behavior.

Admission requires valid `--id` and nonempty `--model`. Unknown policies, known unavailable models, and
explicit positive `--retry`, `--cascade`, `--best-of`, `--iterate`, `--judge`, `--peer-review`,
`--audit`, `--budget`, `--dry-run` or `--container` are refused before task insertion (exit 3, structured JSON error).
An agent without a known served-model list receives the explicit model unchanged.
A same-ID submission with the same agent (after alias/case normalization), model and policy returns `submission: existing`,
launches nothing and exits 0; a different tuple returns `id_conflict` (exit 4). Concurrent identical
submissions claim the ID once. Caller-owned IDs never receive an automatic suffix.

Receipts carry persisted monotonic `revision`, `policy`, `task` and `worker` fields. Task terminal
status and worker settlement are separate: a terminal task can still have a live worker.
`worker.state` is `pending`, `running`, `settled`, or `unconfirmed`; only `settled` makes
`settled: true`. Settlement outcomes are `completed`, `stopped`, `lost`, or `launch_failed`.
`policy` is null for default-mode tasks. Revisions come from a store-wide sequence and never
restart when an ID is deleted and reused; changing the agent also advances the revision.
Keep the receipt with the highest revision. Settlement is derived only by observation: the job
spec is absent (or its verified worker is dead) and the recorded agent process group is gone.
The agent PID, group and launch identity are persisted in the task row before monitoring starts,
so evidence survives spec deletion. Missing group/identity evidence remains `unconfirmed`.
A readable identity mismatch counts as gone. Processes that leave the agent group are outside
settlement; aid does not track detached tool sessions or close shared project containers.
The first derived settlement wins: `stopped` for a recorded stop, `launch_failed` for a failed
submission that never spawned a worker or agent, `lost` for a dead worker's remaining spec,
otherwise `completed`. Worker, stop and reaper cleanup never write settlement directly.
`--container` is refused because its shared project container cannot be attributed to one task.
`--sandbox` retains its task-specific container behavior.

`aid wait --json` accepts exactly one ID and uses the worker barrier. It exits 0 for settlement,
124 for an unsettled timeout (still printing the receipt), and 1 for unknown IDs or read errors.
`unconfirmed` keeps the wait open until timeout. `--timeout 0` observes once. `aid stop --json` targets exactly that worker, including one behind
a terminal row; it exits 0 for `stopped`/`already_settled` and 5 for `unconfirmed`.
Stop checks liveness without reaping recorded processes, verifies identity before every signal,
and confirms exit before recording settlement. A readable identity mismatch means the original
process is gone: the reused PID is never signalled and does not block settlement. A live process
with missing or unreadable identity returns `unconfirmed`; non-terminal task rows still become
`stopped`, but no settlement is written. Caller-owned text mode returns an error.
A terminal row with no live recorded task process keeps its status, artifacts, locks and final-state
metadata untouched. Text mode retains `task already <status>: ...`; JSON acknowledges settlement
with exit 0. Existing settlement is preserved. Worktree preservation runs only after signalling a
verified live process. Text mode on a default-mode task keeps its earlier behavior: a terminal
row returns `task already <status>: ...` before touching its worker or artifacts. Caller-owned text
mode and all JSON stops can signal a worker behind a terminal row. If another closer writes the
settlement first, JSON reports `already_settled`.
