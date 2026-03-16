# ai-dispatch — Multi-AI CLI Team Orchestrator

## Current Status (v7.9.1)

**Foundation (v1.x–v3.x):** Task dispatch, batch parallel, worktree isolation, retry chains, webhook notifications, prompt templates, TUI multipane with charts, modular architecture, agent benchmark, zombie cleanup, UTF-8 safety, task classifier with capability matrix.

**Agent Ecosystem (v4.x–v5.x):** Custom agent TOML format + registry, agent store with versioning, per-agent analytics, hook system, prompt compaction, agent memory (blackboard), task tree, workgroup summary, investigation lead, shared findings, broadcast, fast query, setup wizard, skill packages, merge safety.

**Intelligent Routing (v7.0–v7.5):**
- `--json` for show/board, trust tiers, strengths scoring, knowledge relevance filtering
- `--context-from` result forwarding, shared workspace, knowledge compaction
- Empty diff guard, foreground task timeout, `--cascade` model cascade
- Conditional batch chains (`on_success`/`on_fail`/`conditional` in TOML)
- Episodic memory with append-only versioning, success-weighted injection, multi-query search
- Budget-aware cost-efficiency routing, `--judge` flag for automatic AI review with auto-retry

**Collective Intelligence (v7.6–v7.7):**
- `--parent` for thread composition, completion summary generation, sibling context injection
- `--peer-review` scored critique, `--metric` for best-of quality measurement

**Autonomous Experiments + TUI (v7.8):**
- `aid experiment run` metric-driven dispatch loop, TUI tree view with workgroup grouping
- Rolling context compression, batch milestone SQL, throttled metrics

**Code Health + Performance (v7.9):**
- File-split refactoring (run_bestof.rs, selection_scoring.rs, prompt_context.rs extracted)
- Milestone tag stripping from agent output
- Binary size: 9.5MB → 3.1MB (strip + LTO + ureq→curl)
- SQLite indexes on hot query paths (tasks, events), opt-level="z"

State is stored under `~/.aid` by default, or `AID_HOME` when overridden.

## Roadmap

### v0.8 delivered

- add caller-injected workgroups with `aid group` and `aid run --group`
- wire workgroup filtering into `board`, `watch`, and batch task dispatch
- preserve shared context across retries, background runs, and artifact inspection
- add workgroup lifecycle commands while keeping historical task tags on old tasks

### v0.9 delivered

- TUI scoped by task ID and workgroup (`aid watch --tui t-xxxx`, `aid watch --tui --group wg-xxxx`)
- `aid explain <task-id>` — dispatch task logs to cheap AI for failure summarization
- task dependency DAGs in batch files (`depends_on` + topological sort + level-based parallel dispatch)
- clippy-clean codebase, 129 tests passing

### v1.0 delivered

- fix worktree branch reuse bug (stale branches checked out at old commit)
- fix zombie background tasks (dead processes shown as Running forever)
- fix UTF-8 boundary panic in codex/opencode adapters (multi-byte chars)
- shared CARGO_TARGET_DIR for worktree tasks (build cache reuse)
- CLI consolidation: 17 commands → 11 (`show`, `ask`, `watch --quiet`, `config agents`)

## Problem

When using a primary AI (Claude Code) as a dispatcher to coordinate multiple AI CLI tools (Gemini, Codex, OpenCode, Cursor Agent), the workflow suffers from:

1. **No output standardization** — each tool has different stdout formats, stderr mixing, and exit conventions
2. **No progress visibility** — background tasks run blind until completion
3. **Boilerplate errors** — forgetting `-o json` for gemini, missing no-op guards for codex, piping through `tail` that destroys logs
4. **No audit trail** — task inputs, outputs, timing, and token costs scatter across temp files
5. **Manual worktree management** — creating/cleaning git worktrees for parallel code tasks is tedious

## Solution

A single CLI binary that wraps all AI CLI tools behind a unified dispatch/watch/audit interface. The dispatcher (human or AI) uses `aid` commands instead of raw `gemini`/`codex`/`opencode` calls.

## Core Design Principles

- **Zero config** — auto-detects installed AI CLIs, works immediately
- **Artifact-based** — every task produces inspectable files, never ephemeral stdout
- **Git-native** — automatic worktree creation/cleanup for code tasks
- **Cost-aware** — tracks token usage per task and per agent, reports totals
- **Dispatcher-friendly** — output designed for both human reading and AI parsing

## Architecture

```
┌─────────────────────────────────────┐
│           aid (CLI binary)          │
├──────┬──────┬──────┬───────┬────────┤
│ run  │ watch│ audit│ board │ usage  │  ← subcommands
├──────┴──────┴──────┴───────┴────────┤
│           Task Manager              │
│  ┌────────┐ ┌────────┐ ┌────────┐  │
│  │ Agent  │ │ Watch  │ │ Store  │  │
│  │Registry│ │ Engine │ │(SQLite)│  │
│  └────┬───┘ └────┬───┘ └────┬───┘  │
│       │          │          │       │
├───────┴──────────┴──────────┴───────┤
│         Agent Adapters              │
│  ┌──────┐ ┌─────┐ ┌────────┐       │
│  │Gemini│ │Codex│ │OpenCode│ ...   │
│  └──────┘ └─────┘ └────────┘       │
└─────────────────────────────────────┘
```

## Subcommands

### `aid run` — Dispatch a task

```bash
# Research task (gemini)
aid run gemini "What is the DODO V2 PMM mechanism?" -o /tmp/dodo_research.md

# Code task with auto-worktree (codex)
aid run codex "Implement DODO calldata encoding" --worktree feat/dodo-calldata --dir ./

# Code task with free model (opencode)
aid run opencode "Add type annotations to src/lib.rs" --model mimo-v2-flash-free --dir ./

# Retry failed runs with exponential backoff
aid run codex "Fix the retry path" --dir ./ --verify auto --retry 2

# Reuse shared context from a workgroup
aid run codex "Implement the TUI filter row" --group wg-a3f1 --dir ./

# Background dispatch (returns task ID immediately)
aid run codex "Add tests for quote handler" --bg --worktree feat/quote-tests --dir ./
# => Task t-3a7f started in background
```

**What `aid run` does under the hood:**
1. Creates worktree (if `--worktree`)
2. Injects agent-specific best practices into the prompt:
   - Codex: no-op guard, commit message format
   - Gemini: nothing (prompt passthrough)
   - OpenCode: model selection
3. Launches the agent process, capturing full stdout+stderr to `~/.aid/logs/<task_id>.jsonl`
4. If `--bg` is set, persists a detached worker spec under `~/.aid/jobs/`
5. Records task metadata in SQLite

### `aid group` — Reuse shared context

```bash
aid group create dispatch --context "Shared repo rules, API constraints, and rollout notes"
aid group list
aid group show wg-a3f1
```

Each workgroup stores caller-injected context once. Tasks launched with `aid run --group <id>`
inherit that shared context before any per-task file context is injected. Batch tasks can also
set `group = "wg-a3f1"` in TOML, and both `aid board` and `aid watch` can filter to that group.

### `aid watch` — Live progress dashboard

```bash
aid watch            # Text mode for running tasks
aid watch t-3a7f     # Follow a specific task
aid watch --group wg-a3f1
aid watch --tui      # Interactive ratatui dashboard
aid watch --quiet    # Block until current running tasks finish
aid watch --quiet t-3a7f  # Block until one task finishes
```

```
┌ ai-dispatch board ──────────────────────────────┐
│                                                  │
│ [t-3a7f] codex: Add tests for quote handler      │
│   Status: RUNNING (2m 13s)                       │
│   Progress: 8 shell calls, 12 steps              │
│   Last: cargo test -p sr-service (5s ago)        │
│                                                  │
│ [t-b201] gemini: Research Balancer V3 hooks      │
│   Status: DONE (47s, 3,201 tokens)               │
│   Output: /tmp/balancer_research.md              │
│                                                  │
│ [t-c8e9] opencode: Type annotations              │
│   Status: DONE (1m 02s, FREE)                    │
│   Worktree: /tmp/wt-c8e9 (feat/type-annotations)│
│   Changes: 3 files, +42 -8                       │
│                                                  │
└──────────────────────────────────────────────────┘
```

### `aid show` — Inspect task artifacts

```bash
aid show t-3a7f             # Default: events + stderr + diff stat
aid show t-3a7f --diff      # Full worktree diff
aid show t-3a7f --output    # Print output file
aid show t-3a7f --log       # Print raw log file
aid show t-3a7f --explain   # Dispatch AI summary (creates child task)
```

Default output:
```
Task: t-3a7f — codex: Add tests for quote handler
Duration: 3m 47s
Tokens: 45,210 (codex/gpt-5.4)
Worktree: /tmp/wt-3a7f (feat/quote-tests)

Changes:
  M crates/sr-service/src/api_quote.rs  (+12 -3)
  A crates/sr-service/tests/quote_test.rs  (+87)

Tests: cargo test -p sr-service — 42 passed, 0 failed
Commit: a1b2c3d "feat: add quote handler tests"

Watcher Events:
  14:30:01  Started
  14:30:15  ... 3 shell calls, 5 steps
  14:31:02  BUILD: cargo check passed
  14:32:30  TEST: 42 passed; 0 failed
  14:33:48  COMMIT: [feat/quote-tests a1b2c3d] feat: add quote handler tests
  14:33:48  COMPLETED — 12 shell calls, 18 steps, 45210 tokens
```

### `aid board` — Summary of all tasks

```bash
aid board                    # All tasks
aid board --today            # Today's tasks
aid board --running          # Only running
aid board --group wg-a3f1    # Only tasks in one workgroup
aid board --mine             # Only tasks from the current caller session
```

```
Today: 5 tasks | 3 done | 1 running | 1 failed
Total tokens: 231,408 | Est. cost: $0.34

ID       Agent    Status  Duration  Tokens   Branch
t-3a7f   codex    DONE    3m 47s    45,210   feat/quote-tests
t-b201   gemini   DONE    47s       3,201    —
t-c8e9   opencode DONE    1m 02s    FREE     feat/type-annotations
t-d4f1   codex    RUN     2m 13s    ~28,000  feat/dodo-calldata
t-e5g2   codex    FAIL    1m 30s    22,105   fix/parse-error
```

### `aid usage` — Cost and budget visibility

```bash
aid usage
```

Shows:

- task-history usage by agent
- configured budget windows from `~/.aid/config.toml`
- external counters for tools such as Claude Code

### `aid config` — Manage agent registry

```bash
aid config agents            # List detected agents
aid config agents add foo    # Register custom agent
aid config prompts           # Show prompt templates
```

## Agent Adapters

Each agent adapter encapsulates the CLI-specific quirks:

### Gemini Adapter
```
Command: gemini -o json -p "{prompt}" 2>/dev/null
Extract: jq -r '.response'
Tokens:  jq '.stats.models[].tokens.total'
Quirks:  stderr "Loaded cached credentials" must be suppressed
```

### Codex Adapter
```
Command: codex exec --json "{prompt}" -C {dir}
Stream:
  - item.started / item.completed for tool calls and agent messages
  - turn.completed for usage totals
Extract:
  - command_execution.command / aggregated_output
  - usage.input_tokens / usage.cached_input_tokens / usage.output_tokens
Prompt injection:
  - Append: "\nIMPORTANT: If no changes are needed, do NOT commit. Print 'NO_CHANGES_NEEDED: <reason>'."
  - Append: "\nCommit with message: '{conventional_commit_msg}'"
Quirks: Must capture the full JSONL stream; usage only arrives at turn completion
```

### OpenCode Adapter
```
Command: opencode run -m "{model}" --dir {dir} "{prompt}"
Models: opencode/mimo-v2-flash-free (free), opencode-go/glm-5 (cheap)
Quirks: --dir must point to git repo for file writes
```

## Watcher Engine

The watcher runs as a background thread per task, monitoring the agent's log file:

```
Log file → JSON / text classifier → Event stream → Board writer
                                             → SQLite store
```

**Patterns detected:**
| Signal | Event | Agent |
|--------|-------|-------|
| `item.started` + `command_execution.command` | Tool, build, test, commit | codex |
| `item.completed` + `agent_message.text` | Reasoning | codex |
| `turn.completed.usage.*_tokens` | Completion usage | codex |
| `test result:` / `Finished` / `error[` | Build, test, error | codex, opencode |
| JSON `.stats.models[].tokens.total` | Completion usage | gemini |
| `NO_CHANGES_NEEDED` | No-op detected | codex |

## Storage

SQLite at `~/.aid/aid.db`:

```sql
CREATE TABLE tasks (
    id TEXT PRIMARY KEY,          -- t-xxxx
    agent TEXT NOT NULL,           -- gemini, codex, opencode
    prompt TEXT NOT NULL,
    status TEXT DEFAULT 'pending', -- pending, running, done, failed
    parent_task_id TEXT,
    caller_kind TEXT,
    caller_session_id TEXT,
    worktree_path TEXT,
    worktree_branch TEXT,
    log_path TEXT,
    output_path TEXT,
    tokens INTEGER,
    duration_ms INTEGER,
    model TEXT,
    cost_usd REAL,
    created_at DATETIME,
    completed_at DATETIME
);

CREATE TABLE events (
    task_id TEXT REFERENCES tasks(id),
    timestamp DATETIME,
    event_type TEXT,  -- tool_call, reasoning, build, test, commit, completion, error
    detail TEXT,
    metadata TEXT
);
```

## Budget Config

`aid usage` reads `~/.aid/config.toml`:

```toml
[[usage.budget]]
name = "codex-dev"
agent = "codex"
window = "24h"
token_limit = 1000000
cost_limit_usd = 15.0

[[usage.budget]]
name = "claude-code"
plan = "max"
window = "5h"
request_limit = 200
external_requests = 120
```

## Prompt Templates

Built-in templates that get injected per agent:

```toml
[templates.codex.guard]
append = """
IMPORTANT: If no changes are needed, do NOT create an empty commit.
Instead, print 'NO_CHANGES_NEEDED: <reason>' and exit."""

[templates.codex.commit]
append = "Commit with message: '{msg}'"

[templates.codex.verify]
append = "After changes, run: {cmd}. Fix any errors before committing."
```

## Technology Choice

**Rust + tokio + clap** recommended because:
- Single static binary, no runtime deps
- tokio for async process management + file watching
- clap for CLI parsing (derive mode)
- rusqlite for embedded storage
- ratatui for optional TUI dashboard (`aid watch`)
- Same ecosystem as the user's main projects

**Alternative**: Go would also work well (goroutines, cobra CLI, single binary).

## File Structure

```
ai-dispatch/
├── Cargo.toml
├── DESIGN.md              ← this file
├── src/
│   ├── main.rs            ← clap CLI entry point
│   ├── agent/
│   │   ├── mod.rs         ← Agent trait
│   │   ├── gemini.rs      ← Gemini adapter
│   │   ├── codex.rs       ← Codex adapter
│   │   └── opencode.rs    ← OpenCode adapter
│   ├── watcher.rs         ← Log watcher engine
│   ├── store.rs           ← SQLite task/event store
│   ├── board.rs           ← Board rendering (text + TUI)
│   └── templates.rs       ← Prompt template engine
├── templates/
│   └── default.toml       ← Default prompt templates
└── tests/
    ├── gemini_adapter.rs
    ├── codex_adapter.rs
    └── watcher_test.rs
```

## MVP Scope (v0.1)

1. `aid run gemini/codex` — dispatch with correct flags + log capture
2. `aid watch` — text-mode progress board (no TUI yet)
3. `aid board` — list all tasks with status
4. `aid audit <id>` — show task details + git diff
5. Watcher: codex log patterns only
6. SQLite storage
7. Built-in codex prompt templates (no-op guard, commit format)

## Roadmap

### v8.0 — Programmable Orchestration (next)
- Task steering: `aid steer <task-id> "message"` — mid-flight course correction via PTY
- Pre-dispatch plan validation — lightweight prompt quality check before dispatch
- `aid merge --group` bulk merge — cherry-pick all completed tasks in a workgroup
- Structured state deltas — parse agent output into structured file-change summaries

### v8.1 — Ecosystem Maturity
- `aid store publish` — publish local agent/skill packages to community store
- Daemon mode — `aid daemon` as persistent service via Unix socket
- Agent protocol v2 — unified structured event protocol for all agents
