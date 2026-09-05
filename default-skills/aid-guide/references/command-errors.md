# Command errors and rejected requests

Use `aid errors` when a request failed before receiving a task ID:

```bash
aid errors
aid errors --limit 5
aid errors --limit 50 --json
```

Records are newest first. The limit defaults to 20 and accepts 1 through 1000.
Reading history does not initialize the task database, launch agents, or run
startup maintenance. Missing history produces an empty result.

History is stored at `$AID_HOME/logs/command-errors.jsonl`, or
`~/.aid/logs/command-errors.jsonl` when `AID_HOME` is unset. Each record contains
UTC timestamp, process ID, working directory, redacted arguments, stage, exit
code, and issues with codes, messages, and correction hints. New log files use
owner-only permissions on Unix. Concurrent process appends are serialized.

Stages:

- `parse`: clap rejected arguments, before task/database initialization. The
  record includes the parser error kind, an argument identifier when available,
  and a hint; raw invalid values are omitted. The complete error remains on stderr.
- `validation`: deterministic run-option checks failed after project defaults
  were applied, before task creation. All issues found by these checks are
  returned together, with stable issue codes and correction hints.
- `command`: another command returned an error. This records a generic failure
  marker; use the original stderr or task events for the detailed cause. Raw
  unstructured errors can contain credentials or configuration values and are
  not copied into this history.

Argument values, including prompts, paths supplied as arguments, and shell
commands are redacted. The working directory is retained as diagnostic context.
Help and version exits are not errors. Failure to write history produces a
warning and preserves the original error/exit code. The history does not replace
task logs, backfill historical refusals, capture external shell failures, or
record every unsuccessful task outcome. It is append-only; no automatic retention
or rotation is currently applied.

## Correcting an audit dispatch

`audit` is not a task kind. For a bug investigation, use:

```bash
aid run grok "Audit the scan lifecycle and report findings" \
  --kind debugging --read-only --dir /absolute/path/to/checkout \
  --difficulty complex --budget standard --urgency normal --rigor standard
```

Use the actual checkout directory as `--dir`, rather than mentioning it only in
the prompt. `--worktree` creates/reuses a writable task branch and is currently
incompatible with `--read-only`.

`--audit` schedules an additional post-task cross-audit using `aic`. It does not
describe the task's purpose. `--rigor critical` requires enabled verification and
post-task audit, supplied explicitly or through project defaults. Empty, `none`,
`false`, and `skip` verification values do not satisfy this requirement. Do not
silently lower rigor to make a rejected request pass.

The current read-only lifecycle skips verification even when it was configured.
Thus passing critical preflight is not proof that a read-only task executed
verification. Inspect the recorded verification and audit outcomes.

## Scope of combined validation

Combined validation covers read-only/worktree, sandbox/container (including
injected defaults), audit/no-audit, critical proof configuration, and iteration
count/evaluation requirements. It runs before agent command preflight and task
insertion. Clap still reports its own first parsing/dependency error; invalid
syntax cannot be fully evaluated as a run request. Contextual routing, nested
delegation, and filesystem checks retain their own validation paths.
