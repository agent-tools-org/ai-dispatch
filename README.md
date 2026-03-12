# aid

`aid` is a local multi-agent CLI orchestrator for Gemini, Codex, OpenCode, and Cursor.
It wraps task dispatch, logging, worktrees, retries, and usage tracking behind one binary.

## Install

```bash
cargo install --path .
```

For isolated testing or multiple local sandboxes, set `AID_HOME`:

```bash
export AID_HOME=/tmp/aid-dev
```

All runtime state lives under `$AID_HOME` or `~/.aid`:

- `aid.db` for task metadata and events
- `logs/` for task stdout and stderr
- `jobs/` for detached background worker specs

## Core Commands

```bash
aid run auto "research ratatui table selection"
aid run codex "implement retry logic" --worktree feat/retry --verify auto --retry 2
aid watch --tui
aid wait
aid group create dispatch --context "Shared repo constraints and API contract"
aid group update wg-a3f1 --context "Updated rollout notes"
aid group delete wg-a3f1
aid watch --group wg-a3f1
aid watch --tui --group wg-a3f1
aid board --today
aid board --group wg-a3f1
aid board --mine --running
aid audit t-1234
aid review t-1234
aid output t-1234
aid usage
aid batch work.toml --parallel --wait
```

## Current Features

- Detached background execution for `--bg` tasks
- Worktree-aware task dispatch for parallel code changes
- Parent retry chains with exponential backoff via `--retry`
- Session-aware task attribution and `aid board --mine`
- `aid explore` with prompt-based file auto-detection
- Workgroups with caller-injected shared context and full lifecycle commands via `aid group`
- Workgroup-aware filtering in `aid board --group` and `aid watch --group`
- `aid watch --tui` dashboard built with `ratatui`, scoped by task or workgroup
- `aid wait` and `aid batch --wait` for blocking orchestration flows
- Deterministic usage extraction from streaming agent JSONL events
- Usage and budget reporting through `aid usage`

## Budget Configuration

Create `~/.aid/config.toml`:

```toml
[[usage.budget]]
name = "codex-dev"
agent = "codex"
window = "24h"
task_limit = 20
token_limit = 1000000
cost_limit_usd = 15.0

[[usage.budget]]
name = "claude-code"
plan = "max"
window = "5h"
request_limit = 200
external_requests = 120
resets_at = "2026-03-13T02:00:00+07:00"
notes = "Track Claude Code separately from aid task history."
```

`aid usage` combines local task history with these external counters.

## Notes

- `aid review` falls back to output files or raw logs when a task has no worktree.
- `aid output` prints the recorded output artifact for research-style tasks.
- Batch tasks can opt into a shared workgroup context with `group = "wg-..."`.
- Deleting a workgroup removes the shared-context definition but keeps historical task tags.
- Raw logs remain the source of truth; AI-based log explanation is planned as an optional layer.
- The project design and architecture notes are in [DESIGN.md](DESIGN.md).
