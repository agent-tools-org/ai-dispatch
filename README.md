# ai-dispatch (aid)

![Version](https://img.shields.io/badge/version-8.47.0-blue)
![Rust](https://img.shields.io/badge/rust-2024-orange)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

`aid` is a Multi-AI CLI Team Orchestrator written in Rust. It lets a human dispatcher or a primary AI such as Claude Code delegate work to multiple AI CLI tools, track progress, inspect artifacts, enforce methodology, and iterate through one consistent interface.

Licensed under the [MIT License](LICENSE).

## v8.47.0

- Codex CLI v0.116+ compatibility: auto-detect version and use native `-m` model flag, with fallback to `-c model="..."` for older versions.
- Parse `file_change` events so streamed file edits are captured correctly.
- Track `thread.started` events for more reliable Codex session detection.
- Handle inline error items without dropping surrounding streamed output.
- TUI: dim completed tasks in board tree view for better visual hierarchy.

## Why aid?

Without an orchestrator, a multi-agent CLI workflow breaks down fast:

- Managing multiple AI CLIs is chaotic because every tool has different flags, output formats, and calling conventions.
- No unified progress visibility means background work is mostly blind until a process exits.
- No cost tracking across tools makes token usage and spend hard to monitor over time.
- Manual worktree management for parallel code tasks adds friction to every implementation run.
- No methodology enforcement means prompt discipline, testing standards, and review habits drift between agents.

## Quick Start

### Prerequisites

Install Rust (1.85 or later, required for edition 2024) and whichever AI CLIs you want `aid` to orchestrate. `aid` auto-detects supported agents on your `PATH`: `gemini`, `codex`, `copilot`, `opencode`, `cursor`, `kilo`, `codebuff`, `droid`, `oz`, `claude`, and `auto`.

### Install

```bash
# From crates.io (recommended)
cargo install ai-dispatch

# Or one-liner
curl -fsSL https://aid.agent-tools.org/install.sh | sh
```

Then run the interactive setup wizard:

```bash
aid setup
```

This detects installed agents, configures your OpenRouter API key (for `aid query`), and shows your ready-to-use configuration.

### Install From Source

```bash
cargo install --path .
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

An agent is a **non-interactive CLI** that accepts a prompt, performs the task autonomously, and exits. `aid` normalizes command construction, logging, usage extraction, and completion handling behind one adapter trait.

Built-in agents: `gemini`, `codex`, `copilot`, `opencode`, `cursor`, `kilo`, `codebuff`, `droid`, `oz`, `claude`. Custom agents can be added via `aid agent add` for any compatible CLI (e.g. `aider`). `aid` supports non-interactive CLI modes such as `claude -p` and `copilot -p`; interactive chat sessions can still orchestrate `aid`, but they are not required.

Examples:

```bash
aid run gemini "Compare SQLite and Postgres for local task state" \
  -o /tmp/storage-notes.md

aid run codex "Implement retry-aware board filtering" \
  --dir . \
  --worktree feat/board-filter

aid run copilot "Trace the retry path and simplify the error handling" \
  --dir .

aid run opencode "Rename TaskRow to BoardRow in src/board.rs" \
  --dir .

aid run cursor "Refine the TUI layout for narrow terminals" \
  --dir .

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
  -c "Repo rules: English docs only, keep diffs minimal, prefer source-backed claims."

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

### Agent Memory

Project-scoped persistent knowledge that auto-injects into agent prompts. Four types:

- **Discovery** — bug patterns, API behaviors, gotchas
- **Convention** — code style, naming, architecture decisions
- **Lesson** — what worked/failed (30-day TTL)
- **Fact** — versions, configs, endpoints

```bash
aid memory add discovery "Auth module uses bcrypt not argon2"
aid memory list --type convention
aid memory search "auth"
aid memory update m-a3f1 "Auth now uses argon2 after migration"
aid memory forget m-a3f1
```

### Shared Findings

Workgroup-scoped ephemeral evidence for investigation collaboration. Agents emit `[FINDING]` tags in their output, which are auto-captured and injected into subsequent task prompts within the same workgroup.

```bash
# Manual posting
aid finding add wg-abc1 "gamma can be zero in tricrypto pool"

# Agent auto-capture: any agent output containing [FINDING] is saved
# Example agent output: "[FINDING] WBTC as input causes all outputs to panic"

# List findings
aid finding list wg-abc1

# Findings also appear in workgroup summaries
aid summary wg-abc1
```

### Fast Query (v5.8)

Instant LLM queries via OpenRouter — no agent subprocess startup. Two tiers:

```bash
# Free tier (default) — $0, uses openrouter/free
aid query "What does gamma=0 mean in CryptoSwap?"

# Auto tier — paid, OpenRouter selects best model
aid query --auto "Explain this error trace"

# Explicit model
aid query -m google/gemini-2.0-flash-001 "Summarize this"

# Save response as workgroup finding
aid query "Key insight about pool state" -g wg-abc1 --finding
```

Configure models and API key via `aid setup` or `~/.aid/config.toml`:

```toml
[query]
free_model = "openrouter/free"
auto_model = "openrouter/auto"
api_key = "sk-or-v1-..."
```

### Workspace Isolation (AID_GROUP)

Set `AID_GROUP` to automatically scope all commands to a workgroup without passing `--group` everywhere:

```bash
export AID_GROUP=$(aid group create my-feature -c "Feature implementation context")

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

### Container Sandbox (v8.27)

Run agents inside Apple Container micro-VMs for process isolation. The `--sandbox` flag wraps the agent command in a container with volume mounts for the project directory and agent config directories.

```bash
# Run a task in a sandboxed container
aid run codex "Implement feature" --dir . --sandbox

# Container-ready agents: codex, gemini, kilo, codebuff
# Falls back to host for unsandboxed/native agents: opencode, copilot, droid, oz, cursor, claude
```

The sandbox image (`aid-sandbox:latest`) comes with Node.js and all node-based agent CLIs pre-installed. Build it from the included `Containerfile`:

```bash
container build -t aid-sandbox:latest .
```

Agent authentication is handled by mounting config directories (`~/.codex`, `~/.gemini`, etc.) and forwarding API key env vars into the container.

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

Skills are auto-injected by default: coding agents (`codex`, `copilot`, `claude`, `opencode`, `kilo`, `codebuff`, `droid`, `oz`) get the `implementer` skill, `gemini` gets the `researcher` skill, and `cursor` keeps prompts unchanged unless you add skills explicitly. Use `--skill` to add extras or `--no-skill` to disable auto-injection.

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

### Teams

Teams provide knowledge context and soft agent preferences for different workflows. Each team has preferred agents (scoring boost in auto-selection), capability overrides, behavioral rules, and a knowledge directory.

```bash
# Create a team
aid team create dev

# Configure in ~/.aid/teams/dev.toml
```

```toml
[team]
id = "dev"
display_name = "Development Team"
preferred_agents = ["codex", "opencode", "cursor"]
default_agent = "codex"

# Always-injected constraints (no relevance filtering)
rules = [
    "Do NOT run cargo fmt or any auto-formatter",
    "Only git add files you explicitly modified",
]

# Override agent scoring for this team's tasks
[team.overrides.opencode]
simple_edit = 10
refactoring = 7
```

Team knowledge is stored in `~/.aid/teams/<id>/knowledge/` and auto-injected (relevance-filtered) when `--team` is used. Rules are always injected without filtering.

```bash
aid team list                              # list all teams
aid team show dev                          # show team config + knowledge + rules
aid run codex "implement feature" --team dev   # inject dev team context
aid batch tasks.toml --parallel            # batch with [defaults] team = "dev"
```

### Project Profiles

Project-level configuration via `.aid/project.toml` sets defaults for all tasks dispatched within a repository. Built-in profiles expand into sensible presets:

```bash
# Initialize in current repo
aid project init
# → creates .aid/project.toml + .aid/knowledge/KNOWLEDGE.md

aid project show
```

```toml
[project]
id = "my-app"
profile = "production"    # hobby | standard | production
team = "dev"
language = "rust"
rules = [
    "File size limit: 300 lines per file",
]
```

| Profile | Verify | Budget | Rules |
|---------|--------|--------|-------|
| `hobby` | - | $5/day, prefer_budget | - |
| `standard` | `auto` | $20/day | All new functions must have tests |
| `production` | `cargo test` / `npm test` | $50/day | Tests required, no unwrap(), cross-review |

Project defaults act as CLI fallbacks — explicit flags always win. Rules are always injected into agent prompts (no relevance filtering). Knowledge in `.aid/knowledge/` is relevance-filtered like team knowledge.

## Agent Store

`aid` includes a GitHub-backed community agent store for discovering and installing custom agent definitions.

```bash
# Browse all available agents
aid store browse

# Search for specific agents
aid store browse coding

# Preview an agent's configuration
aid store show community/aider

# Install an agent (with optional version pinning)
aid store install community/aider
aid store install community/aider@1.2.0

# Check for updates
aid store update
aid store update --apply
```

Packages can bundle agent configs, skills, and hooks together. Installing a package installs all components and records versions in `~/.aid/store.lock`.

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
| `aid merge` | Mark done task(s) as merged. Supports `--group` for bulk merge, `--approve` for interactive approval via hiboss. | `aid merge t-1234`, `aid merge --group wg-a3f1 --approve` |
| `aid clean` | Remove old tasks/events and orphaned worktrees/logs. Supports `--dry-run`. | `aid clean --older-than 7 --worktrees` |
| `aid config` | Inspect agent profiles, skills, pricing (with `--update` to fetch latest), and prompt token budget. | `aid config agents`, `aid config pricing --update` |
| `aid worktree` | Explicit worktree lifecycle management: create, list, remove. | `aid worktree create feat/x`, `aid worktree list`, `aid worktree remove feat/x` |
| `aid group` | Workgroup management: create, list, show, update, delete, summary, finding, broadcast. | `aid group create dispatch -c "Shared rollout notes"`, `aid group summary wg-a3f1` |
| `aid store` | Browse, install (with version pinning), update community agent/skill packages. | `aid store install community/aider@1.0.0`, `aid store update --apply` |
| `aid upgrade` | Upgrade aid to latest crates.io version (checks for running tasks). | `aid upgrade`, `aid upgrade --force` |
| `aid agent` | Manage custom agent definitions: list, show, add, remove, fork. | `aid agent list`, `aid agent fork codex --as codex-fast` |
| `aid export` | Export a task with full context (prompt, events, output, diff). Supports markdown and JSON. | `aid export t-1234`, `aid export t-1234 --format json --output task.json` |
| `aid memory` | Manage shared agent memory: add, list, search, update, forget. | `aid memory add discovery "Finding"`, `aid memory search "query"` |
| `aid tree` | Show retry chain as an ASCII tree with agent/status/cost per node. | `aid tree t-1234` |
| `aid query` | Fast LLM query via OpenRouter (no agent startup). Free and auto tiers. | `aid query "question"`, `aid query --auto "question"` |
| `aid setup` | Interactive setup wizard. Detects agents, sets API keys, initializes skills and templates. | `aid setup` |
| `aid team` | Manage teams: create, list, show, delete. Teams inject knowledge and rules into agent prompts. | `aid team list`, `aid team show dev`, `aid team create ops` |
| `aid project` | Initialize and show project configuration (`.aid/project.toml`). Profiles expand into verify/budget/rules defaults. | `aid project init`, `aid project show` |
| `aid stop` | Stop a running task. Graceful by default (SIGTERM + 5s + SIGKILL), `--force` for immediate SIGKILL. | `aid stop t-1234`, `aid stop t-1234 --force` |
| `aid steer` | Inject guidance into a running PTY task. | `aid steer t-1234 "focus on tests"` |

## Best Practices / Methodology

### The Orchestrator Pattern

The most effective `aid` workflow is:

1. Plan the work — decompose into 5–10 independent subtasks.
2. Dispatch agents in parallel via batch files.
3. Monitor with background watch (auto-notifies on completion).
4. Review artifacts with `aid show --diff`.
5. Iterate with retries, or re-dispatch with `--best-of` for critical tasks.

Think big — 6–10 parallel agents finish faster and often produce better results than one agent doing everything serially. Each agent stays focused on a small, well-defined task.

A practical sequence looks like this:

```bash
# Phase 1: Research (free)
aid run gemini "Analyze src/api/ architecture, list public types and extension points" \
  -o /tmp/research.md

# Phase 2: Parallel implementation (batch file, 4–6 tasks)
aid batch feature-tasks.toml --parallel

# Phase 3: Background watch (push notification, no polling)
aid watch --quiet --group wg-a3f1   # Bash run_in_background: true

# Phase 4: Review and iterate
aid show t-1234 --diff
aid retry t-1234 --feedback "Missing error handling in the timeout path"
```

**For AI orchestrators (Claude Code, etc.)**: Use `aid watch --quiet --group <wg-id>` as a background command to get automatic completion callbacks instead of polling `aid board`.

### Quality Tiers

Match effort to task importance:

| Tier | When | Pattern |
|------|------|---------|
| **Draft** | Exploration, prototyping | `aid run codex "..." --dir .` |
| **Standard** | Normal development | `aid run codex "..." --worktree feat/x --verify auto` |
| **Reviewed** | Important features | `aid run codex "..." --verify auto --peer-review gemini` |
| **Best-of** | Critical code paths | `aid run codex "..." --best-of 3 --metric "<cmd>" --verify auto` |

`--best-of N` dispatches the same task to N agents (or the same agent N times), runs an optional `--metric` command on each result, and keeps the best. Use it for bug fixes, core modules, and public APIs where quality matters more than speed.

`--peer-review <agent>` sends the completed diff to a second agent for scored critique (1–10). Cheap agents like gemini make excellent reviewers.

### Audit Report Mode

Review-style tasks now default to a structured Markdown report flow. If the prompt looks like an audit, cross-audit, or code review task, `aid` automatically:

- sets `--result-file result.md`
- tells the agent to write the final answer as a Markdown audit report
- makes `aid show --output`, the TUI Output tab, and the web UI prefer the saved report over raw logs

Examples:

```bash
aid run codex "Cross-audit the split routing fix. List findings by severity with evidence." --read-only
aid run codex "Review this diff for regressions and open questions." --read-only
```

Use `aid show <task-id> --result` to read the persisted report file directly.

### Agent Selection Guide

`auto` uses a capability matrix to match agents to task types. The scores below reflect relative strengths (higher = better fit):

| Agent | Research | Simple Edit | Complex Impl | Frontend | Debugging | Testing | Refactoring | Documentation |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `gemini` | **9** | 2 | 3 | 2 | 5 | 3 | 3 | 6 |
| `codex` | 1 | 4 | **9** | 4 | 7 | 7 | **8** | 3 |
| `copilot` | 4 | 6 | 8 | 6 | 7 | 7 | 7 | 5 |
| `opencode` | 1 | **8** | 3 | 2 | 4 | 4 | 4 | 5 |
| `kilo` | 1 | 7 | 2 | 2 | 3 | 3 | 3 | 4 |
| `cursor` | 2 | 4 | 7 | **9** | 5 | 5 | 6 | 4 |
| `codebuff` | 2 | 5 | 8 | 7 | 6 | 6 | 7 | 4 |
| `droid` | 3 | 5 | **9** | 5 | 7 | 7 | **8** | 4 |
| `oz` | 3 | 5 | 8 | 6 | 6 | 6 | 7 | 4 |
| `claude` | **9** | 5 | **10** | 7 | **10** | **10** | **10** | **9** |

Additional scoring adjustments: budget mode boosts cheap agents (+4) and penalizes expensive ones (-6); high-complexity tasks boost codex/copilot/cursor/droid/oz/claude (+2); rate-limited agents get -10; historical success rates apply about +4/-5.

Scores above are per-agent baselines. When `auto` selects, it also factors in model capability (1-10 scale): **Premium** (cap 9-10): gpt-5.4, gemini-pro, cursor opus-thinking. **Standard** (cap 6-8): gpt-4.1, gemini-flash, cursor-auto, opencode/glm-5. **Budget** (cap 3-5): gpt-4.1-nano, gemini-flash-lite, mimo-free. Final score = (agent_base × 0.4) + (model_capability × 0.6). Use `aid config agents` to see all model scores.

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

### Workgroup-Based Batch Collaboration

A workgroup lets several agents collaborate without repeating shared context. Batch files support DAG dependencies via `depends_on` — tasks dispatch as soon as their dependencies complete.

**Think in phases**: research (free) → parallel implementation (4–6 agents) → integration + validation. Each agent gets a focused, bounded task.

```toml
[defaults]
agent = "codex"
dir = "."
verify = "cargo check"
max_duration_mins = 30

# Phase 1: Research (free, fast)
[[task]]
name = "research"
agent = "gemini"
prompt = "Analyze src/api/ and src/types.rs. List all public types, relationships, and extension points for adding a webhook system."
output = "/tmp/research.md"
read_only = true

# Phase 2: Parallel implementation (all depend on research)
[[task]]
name = "types"
prompt = "Create src/webhook/types.rs with WebhookConfig, WebhookEvent, WebhookPayload structs. Use serde. Match patterns in src/types.rs. Include tests. Keep < 150 lines."
worktree = "feat/webhook-types"
depends_on = ["research"]

[[task]]
name = "handler"
prompt = "Create src/webhook/handler.rs that sends HTTP POST on task completion. Use reqwest with 10s timeout. Log + continue on error. Include tests. Keep < 200 lines."
worktree = "feat/webhook-handler"
depends_on = ["research"]

[[task]]
name = "config"
prompt = "Add [[webhook]] config parsing to src/config.rs with url, events, headers fields. Include tests for valid/invalid configs. Keep changes < 100 lines."
worktree = "feat/webhook-config"
depends_on = ["research"]

[[task]]
name = "cli"
prompt = "Add 'aid webhook test <url>' command in src/cli.rs and src/cmd/webhook.rs. Send test payload, print result. Keep < 80 lines."
worktree = "feat/webhook-cli"
depends_on = ["research"]

# Phase 3: Integration (depends on all implementations)
[[task]]
name = "integration"
prompt = "Wire webhook handler into task completion flow in src/watcher.rs. Import from src/webhook/. Call on Done/Failed. Add integration test. Keep < 50 lines."
worktree = "feat/webhook-integration"
depends_on = ["types", "handler", "config"]
verify = "cargo test"

# Phase 4: Docs (cheap agent, depends on CLI)
[[task]]
name = "docs"
agent = "opencode"
prompt = "Add webhook configuration section to README.md. Show config.toml example and 'aid webhook test' usage. Keep < 40 lines."
worktree = "feat/webhook-docs"
depends_on = ["cli"]
```

Dispatch and monitor:

```bash
aid batch webhook.toml --parallel
aid watch --quiet --group <wg-id>   # background, auto-notifies on completion
```

Batch dispatches with 2+ tasks auto-create a workgroup. The batch file is archived to `~/.aid/batches/`.
Use `max_duration_mins` for batch hard limits. The older `timeout` key is no longer accepted and now fails with a migration hint.

For critical tasks within a batch, use `best_of = 3` to dispatch to multiple agents and keep the best result:

```toml
[[task]]
name = "critical-fix"
agent = "codex"
prompt = "Fix the race condition in src/store.rs. Add a test that reproduces it."
worktree = "fix/store-race"
best_of = 3
metric = "cargo test 2>&1 | grep -c 'test.*ok'"
verify = "cargo test"
```

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
name = "gemini-daily"
agent = "gemini"
window = "24h"
task_limit = 50
cost_limit_usd = 5.0
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
- **Worktree sandbox guard**: All worktree cleanup operations validate paths must resolve to `/tmp/aid-wt-*` before any deletion. The sandbox guard prevents accidental removal of non-worktree directories even if task metadata is corrupted.
- **Worktree escape detection**: After each worktree task, `aid` checks if the agent accidentally modified the main repo and warns with a file list.
- **Auto merge on `aid merge`**: Merges the worktree branch into the current branch, runs pre-merge verification, and cleans up the worktree directory.
- **SQLite concurrency**: `busy_timeout=5000` prevents "database is locked" errors under parallel task access.
- **Fallback chain**: When an agent is rate-limited, `aid` suggests the next capable alternative from the current task class. The default coding chain is `codex → claude → copilot → cursor → droid → opencode → kilo`.
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
│  Gemini  Codex  Copilot  Claude     │
│  OpenCode  Cursor  Kilo  Codebuff   │
│  Droid  Oz  Custom                  │
└─────────────────────────────────────┘
```

How the pieces fit together:

- The CLI entrypoint parses commands and routes them to task-oriented handlers such as `run`, `watch`, `show`, `usage`, and `mcp`.
- The task classifier categorizes prompts into eight task types and estimates complexity, then the capability matrix scores each agent to pick the best fit.
- The agent registry selects and instantiates adapters for `gemini`, `codex`, `copilot`, `opencode`, `cursor`, `kilo`, `codebuff`, `droid`, `oz`, and `claude`.
- The watcher parses streamed or buffered output into milestones, tool activity, usage totals, and completion events.
- SQLite keeps task history, workgroups, and events queryable for `board`, `show`, `watch`, `usage`, and MCP clients.
- Artifact files under `~/.aid/` preserve the raw execution trail so the dispatcher can review what actually happened.

That combination is the core value of `aid`: one binary that turns a pile of incompatible AI CLIs into a trackable, reviewable, and methodology-aware team workflow.
