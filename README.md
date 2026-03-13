# ai-dispatch (aid)

![Version](https://img.shields.io/badge/version-2.0.0-blue)
![Rust](https://img.shields.io/badge/rust-2024-orange)
![License](https://img.shields.io/badge/license-not%20specified-lightgrey)

`aid` is a Multi-AI CLI Team Orchestrator written in Rust. It lets a human dispatcher or a primary AI such as Claude Code delegate work to multiple AI CLI tools, track progress, inspect artifacts, enforce methodology, and iterate through one consistent interface.

The current repository snapshot does not declare a license file or license metadata in `Cargo.toml`, so the badge above intentionally reports `not specified`.

## Why aid?

Without an orchestrator, a multi-agent CLI workflow breaks down fast:

- Managing multiple AI CLIs is chaotic because every tool has different flags, output formats, and calling conventions.
- No unified progress visibility means background work is mostly blind until a process exits.
- No cost tracking across tools makes token usage and spend hard to monitor over time.
- Manual worktree management for parallel code tasks adds friction to every implementation run.
- No methodology enforcement means prompt discipline, testing standards, and review habits drift between agents.

## Quick Start

### Prerequisites

Install Rust and whichever AI CLIs you want `aid` to orchestrate. `aid` auto-detects supported agents on your `PATH`: `gemini`, `codex`, `opencode`, `cursor`, and `auto`.

### Install From Source

```bash
cargo install --path .
aid config agents
aid config skills
```

### Setup for Claude Code

`aid` ships with a recommended Claude Code prompt that enables orchestrator-first workflows. Copy it into your project or global CLAUDE.md:

```bash
# Project-level (recommended)
cat claude-prompt.md >> CLAUDE.md

# Or global (applies to all projects)
cat claude-prompt.md >> ~/.claude/CLAUDE.md
```

See [claude-prompt.md](claude-prompt.md) for the full recommended prompt with agent selection guide, batch file format, and completion notification pattern.

If you want an isolated state directory while testing:

```bash
export AID_HOME=/tmp/aid-dev
```

### First Research Task

Run a lightweight research task and write the answer to a file:

```bash
aid run gemini "Summarize the design principles in DESIGN.md" \
  -o /tmp/aid-design-summary.md
```

For quick ad hoc exploration, use `aid ask`:

```bash
aid ask "How does the retry flow work in this repo?"
```

### First Coding Task

Dispatch a coding task into its own git worktree and ask `aid` to verify automatically:

```bash
aid run codex "Document the MCP server workflow in README.md" \
  --dir . \
  --worktree docs/mcp-readme \
  --verify auto
```

### Watch, Inspect, Iterate

Track progress while the agent runs, then inspect the artifacts:

```bash
aid watch --tui
aid board --today
aid show t-1234
aid show t-1234 --diff
aid retry t-1234 --feedback "Tighten the configuration example and keep it source-accurate."
```

### Run With Auto Agent Selection

Let `aid` choose the best available agent from the prompt shape:

```bash
aid run auto "Create a responsive settings UI for the usage dashboard" --dir .
```

`auto` currently prefers:

- `gemini` for research and question-heavy prompts
- `opencode` for simple edits
- `cursor` for frontend or UI work
- `codex` for complex or multi-file implementation tasks

## Core Concepts

### Agents

An agent is the CLI backend that actually performs work. `aid` normalizes command construction, logging, usage extraction, and completion handling behind one adapter trait.

Examples:

```bash
aid run gemini "Compare SQLite and Postgres for local task state" \
  -o /tmp/storage-notes.md

aid run codex "Implement retry-aware board filtering" \
  --dir . \
  --worktree feat/board-filter

aid run opencode "Rename TaskRow to BoardRow in src/board.rs" \
  --dir .

aid run cursor "Refine the TUI layout for narrow terminals" \
  --dir .

aid run auto "Explain the best agent for a multi-file refactor in this repo" \
  --dir .
```

### Tasks

Every dispatch becomes a tracked task with a stable ID like `t-1234`. Tasks can run in the foreground, in the background, or as retries with feedback.

Examples:

```bash
aid run codex "Add MCP schema regression tests" \
  --dir . \
  --bg \
  --worktree feat/mcp-tests

aid watch t-1234

aid retry t-1234 \
  --feedback "The parser still fails on framed messages. Reproduce first and add coverage."

aid run codex "Harden transient subprocess handling" \
  --dir . \
  --retry 2 \
  --verify auto
```

### Workgroups

Workgroups are shared context containers. Create a workgroup once, then dispatch multiple tasks that inherit the same background constraints and notes.

Examples:

```bash
aid group create dispatch \
  --context "Repo rules: English docs only, keep diffs minimal, prefer source-backed claims."

aid run gemini "Summarize open design questions in DESIGN.md" \
  --group wg-a3f1 \
  -o /tmp/open-questions.md

aid run codex "Update README.md with the orchestrator workflow" \
  --group wg-a3f1 \
  --dir . \
  --worktree docs/readme

aid watch --group wg-a3f1
aid board --group wg-a3f1
aid group show wg-a3f1
```

### Skills

Skills are methodology files loaded from `~/.aid/skills/` and appended to the effective prompt under a `--- Methodology ---` section. They make agent behavior more consistent across runs.

Skills are auto-injected by default: coding agents (`codex`, `opencode`, `cursor`) get the `implementer` skill, and `gemini` gets the `researcher` skill. Use `--skill` to add extras or `--no-skill` to disable auto-injection.

Examples:

```bash
aid config skills

aid run codex "Refactor the retry code path" \
  --dir . \
  --skill code-scout \
  --skill test-writer \
  --verify auto

aid run opencode "Rename TaskRow to BoardRow" --dir . --no-skill
```

### Milestones

`aid` injects milestone guidance into prompts so agents emit progress markers that the watcher can parse and surface in `aid watch`, `aid board`, and the TUI.

Expected milestone format:

```text
[MILESTONE] mapped the failing code path
[MILESTONE] implemented the fix
[MILESTONE] verified tests and summarized the diff
```

## Command Reference

| Command | Purpose | Typical use |
| --- | --- | --- |
| `aid run` | Dispatch one task to an agent. Supports `--bg`, `--verify`, `--worktree`, `--on-done`, `--no-skill`, `--retry`, `--context`, and `--skill`. | `aid run codex "Implement retry logic" --dir . --worktree feat/retry --verify auto` |
| `aid batch` | Dispatch a TOML batch file with DAG dependency scheduling. Auto-creates a workgroup and archives the file to `~/.aid/batches/`. | `aid batch tasks.toml --parallel --wait` |
| `aid watch` | Follow live progress in text mode, quiet wait mode, or the TUI. | `aid watch --tui`, `aid watch t-1234`, `aid watch --quiet --group wg-a3f1` |
| `aid board` | List tracked tasks with filters. Auto-detects zombie tasks. | `aid board --today`, `aid board --mine`, `aid board --group wg-a3f1` |
| `aid show` | Inspect one task's summary, diff, output, raw log, or AI-generated explanation. Diffs show changes vs main branch. | `aid show t-1234 --diff`, `aid show t-1234 --output`, `aid show t-1234 --explain` |
| `aid usage` | Render task-history usage plus configured budget windows. Use `--session` for current session only. | `aid usage`, `aid usage --session` |
| `aid retry` | Re-dispatch a failed task with explicit feedback. | `aid retry t-1234 --feedback "Reproduce the failure before editing."` |
| `aid respond` | Send interactive input to a running background task. | `aid respond t-1234 "yes"` |
| `aid benchmark` | Dispatch the same task to multiple agents and compare results. | `aid benchmark "Fix the bug" --agents codex,opencode --dir .` |
| `aid output` | Show task output directly. | `aid output t-1234` |
| `aid ask` | Run a quick research or exploration task, optionally with file context. | `aid ask "What changed in src/main.rs?" --files src/main.rs` |
| `aid mcp` | Start the stdio MCP server so another tool can call `aid` natively. | `aid mcp` |
| `aid config` | Inspect agent capability profiles, available skills, and model pricing. | `aid config agents`, `aid config skills`, `aid config pricing` |
| `aid group` | Create, list, show, update, and delete shared-context workgroups. | `aid group create dispatch --context "Shared rollout notes"` |

## Best Practices / Methodology

### The Orchestrator Pattern

The most effective `aid` workflow is:

1. Plan the work.
2. Dispatch specialized agents.
3. Monitor with background watch (auto-notifies on completion).
4. Review artifacts and milestones.
5. Iterate with retries or follow-up tasks.

A practical sequence looks like this:

```bash
aid ask "Break this feature into research, implementation, and validation steps."

aid group create release-docs \
  --context "Keep the README user-facing, source-accurate, and aligned with current commands."

aid run gemini "Summarize DESIGN.md and call out the user-visible features" \
  --group wg-a3f1 \
  -o /tmp/design-notes.md

aid run codex "Rewrite README.md around the current CLI surface" \
  --group wg-a3f1 \
  --dir . \
  --worktree docs/readme \
  --skill implementer \
  --verify auto

aid watch --quiet --group wg-a3f1   # blocks until all tasks complete
aid show t-1234 --diff
aid retry t-1234 --feedback "Trim unsupported claims and improve MCP setup guidance."
```

**For AI orchestrators (Claude Code, etc.)**: Use `aid watch --quiet --group <wg-id>` as a background command to get automatic completion callbacks instead of polling `aid board`.

This pattern keeps planning cheap, execution specialized, and review artifact-driven.

### Agent Selection Guide

Use the agent that matches the shape of the work:

| Agent | Best for | Why |
| --- | --- | --- |
| `gemini` | research, questions, comparison, documentation discovery | Low-friction prompt/answer loop for exploration-heavy tasks |
| `codex` | complex implementation, multi-file changes, deep repo work | Best default for substantial code modifications and iterative verification |
| `opencode` | simple edits, rename passes, light cleanup | Good fit for smaller coding tasks where a cheaper tool is enough |
| `cursor` | frontend, UI, layout, visual polish | Best when the prompt clearly targets UI structure or responsiveness |
| `auto` | mixed or uncertain tasks | Scores the prompt and picks the best installed agent automatically |

If you are unsure, start with `aid ask` or `aid run auto`, then escalate to a more expensive agent only when the task scope is clear.

### Use Skills To Enforce Quality

Skills give you repeatable task methodology instead of relying on ad hoc prompt wording. In practice: use `code-scout` for unfamiliar code, `researcher` for fact-heavy work, `implementer` for minimal diffs, `test-writer` for regression coverage, and `debugger` when the task starts with a failure.

Example:

```bash
aid run codex "Fix the MCP framing bug and add regression coverage" \
  --dir . \
  --skill code-scout \
  --skill debugger \
  --skill implementer \
  --skill test-writer \
  --verify auto
```

### Workgroup-Based Collaborative Investigation

A workgroup lets several agents collaborate without repeating the same shared context in every prompt.

For larger investigations, pair a workgroup with a batch file. Batch files support DAG dependencies via `depends_on` — tasks dispatch as soon as their individual dependencies complete, not when an entire level finishes:

```toml
[[task]]
name = "research"
agent = "gemini"
prompt = "Summarize DESIGN.md and note MCP constraints"
output = "/tmp/mcp-notes.md"

[[task]]
name = "implementation"
agent = "codex"
prompt = "Update README.md with MCP setup guidance"
dir = "."
worktree = "docs/mcp-guide"
skills = ["implementer"]
depends_on = ["research"]
verify = "cargo test"
```

Dispatch it like this:

```bash
aid batch tasks.toml --parallel --wait
aid board
aid show t-1234
```

Batch dispatches with 2+ tasks auto-create a workgroup. The batch file is archived to `~/.aid/batches/` after dispatch.

This works well for incident response, release prep, and cross-cutting refactors where one agent researches while another edits.

### Cost Optimization Tips

- Use `aid ask` or `gemini` first when the task is still exploratory.
- Prefer `opencode` for straightforward single-file edits or rename work.
- Use `auto` when you want a reasonable default without thinking about the agent first.
- Set `[[usage.budget]]` entries and check `aid usage` before long coding sessions.
- Reuse workgroups so shared context is stored once instead of repeated in every prompt.
- Use `--model` only when you need a specific backend behavior or cost profile.
- Use `--on-done "command"` to get notified when a background task completes (sets `AID_TASK_ID` and `AID_TASK_STATUS` env vars).
- Use `--template <name>` to wrap prompts with structured methodology (bug-fix, feature, refactor).
- Use `--repo /path/to/other-project` to dispatch tasks to a different git repository.
- Use `aid benchmark` to compare agent quality/speed/cost on the same task.
- Configure webhooks in `config.toml` for Slack/Discord notifications on task completion.

## Built-in Skills

The default skill directory is `~/.aid/skills/`.

| Skill | Description |
| --- | --- |
| `test-writer` | Writes tests that target real failure modes, boundary cases, and integration seams instead of mirroring the implementation. |
| `code-scout` | Maps the entry point, call chain, relevant files, patterns, and risks before a change is made. |
| `researcher` | Collects verified information from primary sources, records confidence, and extracts facts that are safe to use downstream. |
| `implementer` | Makes the requested change with a minimal diff, matches the local style, and verifies the result. |
| `debugger` | Reproduces issues, traces execution, isolates the root cause, and validates the fix with evidence. |

## MCP Integration

`aid` can run as a stdio MCP server so Claude Code or another MCP client can call it without shell parsing.

Start the server directly:

```bash
aid mcp
```

The server exposes these tools:

- `aid_run`
- `aid_board`
- `aid_show`
- `aid_retry`
- `aid_usage`
- `aid_ask`

To connect from Claude Code, register `aid` as a stdio MCP server in your Claude Code MCP configuration. The exact config file location depends on your Claude Code setup, but the server definition itself looks like this:

```json
{
  "mcpServers": {
    "aid": {
      "command": "aid",
      "args": ["mcp"]
    }
  }
}
```

If you are developing from source instead of using an installed binary, point Claude Code at `cargo run`:

```json
{
  "mcpServers": {
    "aid-dev": {
      "command": "cargo",
      "args": [
        "run",
        "--manifest-path",
        "/absolute/path/to/aid/Cargo.toml",
        "--",
        "mcp"
      ]
    }
  }
}
```

Once connected, Claude Code can call `aid_board` to list tasks, `aid_show` to inspect artifacts, `aid_run` to dispatch new work, and `aid_retry` to iterate on failures.

## Configuration

By default, `aid` stores state in `~/.aid/`. Override it with `AID_HOME` when you want a disposable sandbox or separate environment.

Example:

```bash
export AID_HOME=/tmp/aid-dev
```

Typical directory layout:

```text
~/.aid/
├── aid.db
├── config.toml
├── logs/
│   ├── t-1234.jsonl
│   └── t-1234.stderr
├── jobs/
│   └── t-1234.json
├── batches/
│   └── 20260313-112850-v15-fixes.toml
├── skills/
│   ├── code-scout.md
│   ├── debugger.md
│   ├── implementer.md
│   ├── researcher.md
│   └── test-writer.md
└── cargo-target/
```

What lives there:

- `aid.db`: SQLite task, workgroup, and event store
- `logs/`: raw agent output plus stderr capture
- `jobs/`: detached background worker specs
- `batches/`: archived batch TOML files (auto-saved after dispatch)
- `skills/`: methodology files loaded by `--skill` (auto-injected by default)
- `templates/`: prompt templates loaded by `--template` (see default-templates/ for examples)
- `cargo-target/`: shared Rust build cache for worktree-based tasks

Configure budgets and webhooks in `~/.aid/config.toml`:

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

Then inspect usage and budget status:

```bash
aid usage
```

`aid usage` combines tracked task history with any external counters you record in `config.toml`.

### Webhooks

Configure webhooks to receive notifications when tasks complete:

```toml
[[webhook]]
name = "slack-notify"
url = "https://hooks.slack.com/services/..."
on_done = true
on_failed = true
```

Webhooks fire automatically when background tasks reach a terminal state. Custom headers can be added via `headers`.

## Reliability

`aid` includes several mechanisms to keep long-running multi-agent workflows healthy:

- **Zombie detection**: `aid board` automatically detects dead worker processes (including defunct/zombie state) and marks their tasks as FAILED.
- **Max task duration**: Tasks running longer than 60 minutes are automatically killed and marked FAILED.
- **Auto-commit enforcement**: Background worktree tasks auto-commit uncommitted changes after completion, preventing lost work.
- **Zombie recovery**: When a zombie task is detected in a worktree with uncommitted changes, those changes are preserved via auto-commit before marking the task as failed.
- **Shared build cache**: Rust worktree tasks share `CARGO_TARGET_DIR` to avoid redundant recompilation across parallel dispatches.

## Architecture

At a high level, `aid` is a CLI front end over a task manager, a watcher pipeline, persistent storage, and agent-specific adapters.

The diagram below is adapted from `DESIGN.md` to reflect the current `show` command name:

```text
┌─────────────────────────────────────┐
│           aid (CLI binary)          │
├──────┬──────┬──────┬───────┬────────┬───────────┤
│ run  │ watch│ show │ board │ usage  │ benchmark │  ← user-facing commands
├──────┴──────┴──────┴───────┴────────┤
│           Task Manager              │
│  ┌────────┐ ┌────────┐ ┌────────┐  │
│  │ Agent  │ │ Watch  │ │ Store  │  │
│  │Registry│ │ Engine │ │(SQLite)│  │
│  └────┬───┘ └────┬───┘ └────┬───┘  │
│       │          │          │       │
├───────┴──────────┴──────────┴───────┤
│         Agent Adapters              │
│  ┌──────┐ ┌─────┐ ┌────────┐ ┌──────┐
│  │Gemini│ │Codex│ │OpenCode│ │Cursor│
│  └──────┘ └─────┘ └────────┘ └──────┘
└─────────────────────────────────────┘
```

How the pieces fit together:

- The CLI entrypoint parses commands and routes them to task-oriented handlers such as `run`, `watch`, `show`, `usage`, and `mcp`.
- The agent registry selects and instantiates adapters for `gemini`, `codex`, `opencode`, and `cursor`.
- The watcher parses streamed or buffered output into milestones, tool activity, usage totals, and completion events.
- SQLite keeps task history, workgroups, and events queryable for `board`, `show`, `watch`, `usage`, and MCP clients.
- Artifact files under `~/.aid/` preserve the raw execution trail so the dispatcher can review what actually happened.

That combination is the core value of `aid`: one binary that turns a pile of incompatible AI CLIs into a trackable, reviewable, and methodology-aware team workflow.
