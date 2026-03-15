# ai-dispatch (aid)

![Version](https://img.shields.io/badge/version-5.4.0-blue)
![Rust](https://img.shields.io/badge/rust-2024-orange)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

`aid` is a Multi-AI CLI Team Orchestrator written in Rust. It lets a human dispatcher or a primary AI such as Claude Code delegate work to multiple AI CLI tools, track progress, inspect artifacts, enforce methodology, and iterate through one consistent interface.

Licensed under the [MIT License](LICENSE).

## Why aid?

Without an orchestrator, a multi-agent CLI workflow breaks down fast:

- Managing multiple AI CLIs is chaotic because every tool has different flags, output formats, and calling conventions.
- No unified progress visibility means background work is mostly blind until a process exits.
- No cost tracking across tools makes token usage and spend hard to monitor over time.
- Manual worktree management for parallel code tasks adds friction to every implementation run.
- No methodology enforcement means prompt discipline, testing standards, and review habits drift between agents.

## Quick Start

### Prerequisites

Install Rust (1.85 or later, required for edition 2024) and whichever AI CLIs you want `aid` to orchestrate. `aid` auto-detects supported agents on your `PATH`: `gemini`, `codex`, `opencode`, `cursor`, `kilo`, `ob1`, `codebuff`, and `auto`.

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

Let `aid` choose the best available agent using its task classifier and capability matrix:

```bash
aid run auto "Create a responsive settings UI for the usage dashboard" --dir .
# [aid] Auto-selected agent: cursor (reason: frontend task (medium) → cursor (score: 9))
# [aid] Auto-selected model: auto (complexity: medium)
```

`auto` classifies each prompt into one of eight task categories — research, simple-edit, complex-impl, frontend, debugging, testing, refactoring, documentation — estimates complexity (low/medium/high), then scores every installed agent against a capability matrix. The best-scoring agent wins, with adjustments for budget mode, rate limits, and historical success rates.

The model tier is auto-selected based on complexity: low → cheap/free models, medium → standard, high → premium.

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

aid run ob1 "Analyze the cost structure of this codebase" \
  --dir . --read-only

aid run codebuff "Refactor the auth module into separate files" \
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

### Workspace Isolation (AID_GROUP)

Set `AID_GROUP` to automatically scope all commands to a workgroup without passing `--group` everywhere:

```bash
export AID_GROUP=$(aid group create my-feature --context "Feature implementation context")

aid run codex "Implement the parser" --dir . --worktree feat/parser
aid run codex "Add parser tests" --dir . --worktree feat/parser-tests
aid board          # only shows tasks in this group
aid watch --quiet  # only watches tasks in this group
aid merge --group  # merges all done tasks in this group
```

### Worktree Management

`aid` manages git worktrees for parallel conflict-free task execution. Worktrees can be created automatically with `--worktree` or managed explicitly:

```bash
# Explicit worktree lifecycle
WT=$(aid worktree create feat/my-feature)
aid run codex "Implement feature" --dir $WT
aid run codex "Add tests" --dir $WT
aid worktree list
aid worktree remove feat/my-feature

# Automatic worktree (created per-task)
aid run codex "Implement feature" --worktree feat/my-feature --dir .
```

`aid merge` auto-merges the worktree branch into the current branch and cleans up the worktree directory. Failed tasks auto-cleanup their worktrees. Worktree escape detection warns if an agent accidentally modifies the main repo.

### Codebuff Plugin (Optional)

`codebuff` is an optional agent that requires separate installation. It bridges the [Codebuff SDK](https://www.codebuff.com/docs/advanced/sdk) to `aid`'s event protocol via a Node.js wrapper.

```bash
# 1. Install the plugin
cd plugins/codebuff && npm install && npm install -g .

# 2. Get an API key at https://www.codebuff.com/api-keys
export CODEBUFF_API_KEY=cb-pat-...

# 3. Add to your shell profile to persist across sessions
echo 'export CODEBUFF_API_KEY=cb-pat-...' >> ~/.zshrc

# 4. Run a task
aid run codebuff "Refactor the auth module" --dir .
```

The plugin outputs codex-compatible JSONL events, so `aid show`, `aid watch`, and the TUI work seamlessly. If `CODEBUFF_API_KEY` is not set, `aid` will show setup instructions instead of failing silently.

**Cost note**: Codebuff SDK v0.10 runs sub-agents (Context Pruner, Nit Pick Nick) automatically, which can make even simple tasks expensive. Use `--mode free` via `--budget` flag for cost-sensitive work.

### TUI Stats View

Press `s` in the TUI to toggle the stats/charts view, which shows:

- **Cost by Agent** — horizontal bar chart of spend per agent
- **Success Rate** — bar chart of done/merged percentage per agent
- **Budget Usage** — gauge bars for configured budget windows
- **Summary** — task counts, total/today cost, token totals, and a cost sparkline

Press `a` to toggle between today-only and all-time task views.

### Skills

Skills are methodology files loaded from `~/.aid/skills/` and appended to the effective prompt under a `--- Methodology ---` section. They make agent behavior more consistent across runs.

Skills are auto-injected by default: coding agents (`codex`, `opencode`, `cursor`, `kilo`, `ob1`, `codebuff`) get the `implementer` skill, and `gemini` gets the `researcher` skill. Use `--skill` to add extras or `--no-skill` to disable auto-injection.

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

## Agent Store

`aid` includes a GitHub-backed community agent store for discovering and installing custom agent definitions.

```bash
# Browse all available agents
aid store browse

# Search for specific agents
aid store browse coding

# Preview an agent's configuration
aid store show community/aider

# Install an agent
aid store install community/aider
```

Installed agents appear in `aid config agents` and participate in auto-selection via their capability scores. The store is backed by [agent-tools-org/aid-agents](https://github.com/agent-tools-org/aid-agents) — community contributions welcome.

### Task Lifecycle Hooks

Define shell hooks that run at key points in the task lifecycle. Hooks receive task JSON on stdin.

Configure hooks in `~/.aid/hooks.toml`:

```toml
[[hook]]
event = "before_run"
command = "~/.aid/hooks/validate.sh"

[[hook]]
event = "after_complete"
command = "~/.aid/hooks/notify.sh"
agent = "codex"

[[hook]]
event = "on_fail"
command = "~/.aid/hooks/alert.sh"
```

Or pass hooks per-task via CLI:

```bash
aid run codex "Implement feature" --hook before_run:./validate.sh --hook on_fail:./alert.sh
```

Batch files support hooks in `[defaults]` and per-task:

```toml
[defaults]
hooks = ["before_run:./validate.sh"]

[[task]]
agent = "codex"
prompt = "Implement feature"
hooks = ["after_complete:./notify.sh"]
```

Hook events:
- `before_run` — runs after task creation, before agent starts. Fails the task if hook exits non-zero.
- `after_complete` — runs after task completes (after verify). Best-effort.
- `on_fail` — runs when task fails. Best-effort.

### Prompt Budget

Check skill token overhead with `aid config prompt-budget`:

```bash
$ aid config prompt-budget
Skill Token Budget:
  code-scout     ~195 tokens
  debugger       ~219 tokens
  implementer    ~323 tokens
  researcher     ~209 tokens
  test-writer    ~199 tokens
  ─────────────────────
  Total:         ~1145 tokens
```

Token usage is also logged during dispatch:

```
[aid] Skills loaded: 1 skills, ~323 tokens
[aid] Context injected: 2 files, ~450 tokens
```

### Milestones

`aid` injects milestone guidance into prompts so agents emit progress markers that the watcher can parse and surface in `aid watch`, `aid board`, and the TUI.

Expected milestone format:

```text
[MILESTONE] mapped the failing code path
[MILESTONE] implemented the fix
[MILESTONE] verified tests and summarized the diff
```

### Agent Memory (Blackboard)

`aid` includes a shared memory system that lets agents build up knowledge across tasks. Memories are stored in SQLite alongside tasks and automatically injected into agent prompts when relevant.

Memory types:
- **Discovery** — new findings about the codebase or environment
- **Convention** — patterns, naming rules, or style decisions
- **Lesson** — mistakes to avoid (auto-expires after 30 days)
- **Fact** — verified constants (addresses, versions, config values)

Agents can emit memories by including `[MEMORY: type] content` tags in their output. These are automatically extracted and deduplicated after task completion.

```bash
# Manual memory management
aid memory add discovery "The retry module uses exponential backoff capped at 45s"
aid memory add convention "All SQL migrations use ALTER TABLE with DEFAULT values"
aid memory list
aid memory list --type lesson
aid memory search "retry"
aid memory forget mem-a1b2
```

Memories are auto-injected into prompts during dispatch. Agents see a `--- Relevant Memories ---` section with matching memories from previous tasks.

### Verify Status

Tasks now track verification outcome separately from execution status via `verify_status`:

- **Skipped** — no `--verify` was set
- **Pending** — verify hasn't run yet
- **Passed** — verify command succeeded
- **Failed** — verify command failed (task may still be marked Done)

The board displays `[VFAIL]` next to tasks that completed but failed verification, making it easy to distinguish "agent crashed" from "code doesn't pass checks".

## Command Reference

| Command | Purpose | Typical use |
| --- | --- | --- |
| `aid run` | Dispatch one task to an agent. Supports `--bg`, `--verify`, `--worktree`, `--on-done`, `--no-skill`, `--retry`, `--context`, and `--skill`. | `aid run codex "Implement retry logic" --dir . --worktree feat/retry --verify auto` |
| `aid batch` | Dispatch a TOML batch file with DAG dependency scheduling. Auto-creates a workgroup and archives the file to `~/.aid/batches/`. | `aid batch tasks.toml --parallel --wait` |
| `aid watch` | Follow live progress in text mode, quiet wait mode, or the TUI. | `aid watch --tui`, `aid watch t-1234`, `aid watch --quiet --group wg-a3f1` |
| `aid board` | List tracked tasks with filters. Auto-detects zombie tasks. Use `--stream` for scrollback-preserving output. | `aid board --today`, `aid board --stream --group wg-a3f1` |
| `aid show` | Inspect one task's summary, diff, output, raw log, or AI-generated explanation. Diffs show changes vs main branch. | `aid show t-1234 --diff`, `aid show t-1234 --output`, `aid show t-1234 --explain` |
| `aid usage` | Render task-history usage plus configured budget windows. Supports `--agent`, `--period`, and `--json`. | `aid usage`, `aid usage --agent codex --period 7d --json` |
| `aid retry` | Re-dispatch a failed task with explicit feedback. | `aid retry t-1234 --feedback "Reproduce the failure before editing."` |
| `aid respond` | Send interactive input to a running background task. | `aid respond t-1234 "yes"` |
| `aid benchmark` | Dispatch the same task to multiple agents and compare results. | `aid benchmark "Fix the bug" --agents codex,opencode --dir .` |
| `aid output` | Show task output directly. | `aid output t-1234` |
| `aid ask` | Run a quick research or exploration task, optionally with file context. | `aid ask "What changed in src/main.rs?" --files src/main.rs` |
| `aid mcp` | Start the stdio MCP server so another tool can call `aid` natively. | `aid mcp` |
| `aid merge` | Mark done task(s) as merged. Supports `--group` for bulk workgroup merge with worktree cleanup. | `aid merge t-1234`, `aid merge --group wg-a3f1` |
| `aid clean` | Remove old tasks/events and orphaned worktrees/logs. Supports `--dry-run`. | `aid clean --older-than 7 --worktrees` |
| `aid config` | Inspect agent profiles, skills, pricing, and prompt token budget. | `aid config agents`, `aid config prompt-budget`, `aid config pricing` |
| `aid worktree` | Explicit worktree lifecycle management: create, list, remove. | `aid worktree create feat/x`, `aid worktree list`, `aid worktree remove feat/x` |
| `aid group` | Create, list, show, update, and delete shared-context workgroups. | `aid group create dispatch --context "Shared rollout notes"` |
| `aid store` | Browse, search, preview, and install community agent definitions. | `aid store browse`, `aid store install community/aider` |
| `aid agent` | Manage custom agent definitions: list, show, add, remove, fork. | `aid agent list`, `aid agent fork codex --as codex-fast` |
| `aid memory` | Manage shared agent memory: add, list, search, forget. | `aid memory add discovery "Finding"`, `aid memory search "query"` |
| `aid init` | Initialize default skills and templates. | `aid init` |

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

`auto` uses a capability matrix to match agents to task types. The scores below reflect relative strengths (higher = better fit):

| Agent | Research | Simple Edit | Complex Impl | Frontend | Debugging | Testing | Refactoring | Documentation |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `gemini` | **9** | 2 | 3 | 2 | 5 | 3 | 3 | 6 |
| `codex` | 1 | 4 | **9** | 4 | 7 | 7 | **8** | 3 |
| `opencode` | 1 | **8** | 3 | 2 | 4 | 4 | 4 | 5 |
| `kilo` | 1 | 7 | 2 | 2 | 3 | 3 | 3 | 4 |
| `cursor` | 2 | 4 | 7 | **9** | 5 | 5 | 6 | 4 |
| `ob1` | 5 | 3 | 5 | 3 | 4 | 4 | 4 | 3 |
| `codebuff` | 2 | 5 | 8 | 7 | 6 | 6 | 7 | 4 |

Additional scoring adjustments: budget mode boosts cheap agents (+4) and penalizes expensive ones (-6); high-complexity tasks boost codex/cursor (+2); rate-limited agents get -10; historical success rates apply ±2-3.

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
read_only = true

[[task]]
name = "implementation"
agent = "codex"
prompt = "Update README.md with MCP setup guidance"
dir = "."
worktree = "docs/mcp-guide"
skills = ["implementer"]
depends_on = ["research"]
verify = "cargo test"

[[task]]
name = "formatting"
agent = "opencode"
prompt = "Run cargo fmt and fix any clippy warnings"
dir = "."
budget = true
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
- Use `--budget` to force cheaper agent/model selection for low-priority tasks.
- Low-value tasks (tests, formatting, linting, docs) auto-detect as budget mode.
- Use `read_only = true` in batch tasks for research/review that should not modify files.
- Use `aid benchmark` to compare agent quality/speed/cost on the same task.
- Use `codebuff` with `--budget` for cost-sensitive tasks — the SDK's sub-agents can make simple tasks expensive in normal mode.
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
├── hooks.toml
├── skills/
│   ├── code-scout.md
│   ├── debugger.md
│   ├── implementer.md
│   ├── researcher.md
│   └── test-writer.md
├── agents/
│   └── *.toml
└── cargo-target/
```

What lives there:

- `aid.db`: SQLite task, workgroup, event, and memory store
- `logs/`: raw agent output plus stderr capture
- `jobs/`: detached background worker specs
- `batches/`: archived batch TOML files (auto-saved after dispatch)
- `hooks.toml`: task lifecycle hooks (before_run, after_complete, on_fail)
- `skills/`: methodology files loaded by `--skill` (auto-injected by default)
- `templates/`: prompt templates loaded by `--template` (see default-templates/ for examples)
- `agents/`: custom agent TOML definitions
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
- **Worktree escape detection**: After each worktree task, `aid` checks if the agent accidentally modified the main repo and warns with a file list.
- **Auto merge on `aid merge`**: Merges the worktree branch into the current branch, runs pre-merge verification, and cleans up the worktree directory.
- **SQLite concurrency**: `busy_timeout=5000` prevents "database is locked" errors under parallel task access.
- **Fallback chain**: When an agent is rate-limited, `aid` suggests the next capable alternative (codex → cursor → opencode → kilo).
- **Retry worktree preservation**: When a failed task's worktree is auto-cleaned, retries recreate a fresh worktree on the same branch instead of falling back to the main repo.

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
│  │Classif.│ │ Watch  │ │ Store  │  │
│  │+ Agent │ │ Engine │ │(SQLite)│  │
│  │Registry│ │        │ │        │  │
│  └────┬───┘ └────┬───┘ └────┬───┘  │
│       │          │          │       │
├───────┴──────────┴──────────┴───────┤
│         Agent Adapters              │
│  ┌──────┐ ┌─────┐ ┌────────┐ ┌──────┐ ┌────┐ ┌───┐ ┌────────┐ ┌──────┐
│  │Gemini│ │Codex│ │OpenCode│ │Cursor│ │Kilo│ │OB1│ │Codebuff│ │Custom│
│  └──────┘ └─────┘ └────────┘ └──────┘ └────┘ └───┘ └────────┘ └──────┘
└─────────────────────────────────────┘
```

How the pieces fit together:

- The CLI entrypoint parses commands and routes them to task-oriented handlers such as `run`, `watch`, `show`, `usage`, and `mcp`.
- The task classifier categorizes prompts into eight task types and estimates complexity, then the capability matrix scores each agent to pick the best fit.
- The agent registry selects and instantiates adapters for `gemini`, `codex`, `opencode`, `cursor`, `kilo`, `ob1`, and `codebuff`.
- The watcher parses streamed or buffered output into milestones, tool activity, usage totals, and completion events.
- SQLite keeps task history, workgroups, and events queryable for `board`, `show`, `watch`, `usage`, and MCP clients.
- Artifact files under `~/.aid/` preserve the raw execution trail so the dispatcher can review what actually happened.

That combination is the core value of `aid`: one binary that turns a pile of incompatible AI CLIs into a trackable, reviewable, and methodology-aware team workflow.
