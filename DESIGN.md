# ai-dispatch — Multi-AI CLI Team Orchestrator

## Current Status

Implemented in the current release:

- `aid run` with background workers, worktrees, context injection, and `--retry`
- `aid watch --tui` plus the original text watch mode
- `aid wait` and `batch --wait` for blocking orchestration flows
- `aid board --mine` for caller-session filtering
- `aid audit`, `aid review`, and `aid output` for artifact inspection
- deterministic usage extraction from streaming agent events
- `aid usage` for task-history cost reporting and configured budget windows

State is stored under `~/.aid` by default, or `AID_HOME` when overridden.

## Roadmap

### v0.6 delivered

- complete deterministic token, model, and cost extraction for each supported CLI
- tighten `board`, `audit`, and `usage` fidelity around non-worktree and retried tasks
- keep `wait` and `batch --wait` as the blocking orchestration primitives

### v0.7 next

- add optional AI-assisted log explanation and failure summarization as a cached layer
- introduce task dependency DAGs for explicit scheduling beyond retry chains
- surface resource telemetry in the TUI, starting with CPU and memory
- support configurable prompt registry references instead of hard-coded prompt text only
- expand provider quota reporting beyond task history into plan-window awareness

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

### `aid watch` — Live progress dashboard

```bash
aid watch            # Text mode for running tasks
aid watch t-3a7f     # Follow a specific task
aid watch --tui      # Interactive ratatui dashboard
aid wait             # Block until current running tasks finish
aid wait t-3a7f      # Block until one task finishes
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

### `aid audit` — Review completed task

```bash
aid audit t-3a7f
```

Output:
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

### `aid output` — Print task artifacts

```bash
aid output t-3a7f
```

Reads the task's recorded `output_path` and prints it to stdout.

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

## Future (v0.2+)

- TUI dashboard with ratatui
- OpenCode adapter (once installed)
- Cursor Agent adapter
- Auto-retry on failure with exponential backoff
- Cost tracking with configurable per-model pricing
- MCP server mode (expose dispatch as MCP tools for Claude)
- Parallel dispatch: `aid run-parallel tasks.toml` (batch file)
- Smart agent selection: analyze task → pick best agent automatically
