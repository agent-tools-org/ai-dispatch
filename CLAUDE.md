# ai-dispatch (aid)

## Install

- NEVER cp binary to `/opt/homebrew/bin/` — macOS provenance xattr blocks execution
- `/opt/homebrew/bin/aid` is a symlink to `~/.cargo/bin/aid`
- Install command (MUST re-sign after copy — sandbox provenance blocks execution):
  ```bash
  cp "$CARGO_TARGET_DIR/release/aid" ~/.cargo/bin/aid && codesign --force --sign - ~/.cargo/bin/aid
  ```

## Teams

Teams provide **knowledge context and soft agent preferences** — not hard restrictions. All agents remain available; `--team` boosts preferred agents in auto-selection and injects team knowledge into prompts.

### TOML format

```toml
[team]
id = "dev"
display_name = "Development Team"
description = "Feature development and code quality"
preferred_agents = ["codex", "opencode", "cursor"]  # soft boost, not hard filter
default_agent = "codex"

# Always-injected behavioral constraints — no relevance filtering
rules = [
    "Do NOT run cargo fmt or any auto-formatter",
    "Only git add files you explicitly modified",
]

[team.overrides.opencode]
simple_edit = 10
debugging = 6
```

### Knowledge

Each team has a knowledge directory auto-created on `aid team create`:

```
~/.aid/teams/<id>/
  KNOWLEDGE.md          # index — auto-injected into prompts with --team
  knowledge/            # individual knowledge files
```

When `--team dev` is used, `KNOWLEDGE.md` content is prepended to the agent's prompt.

### CLI

```
aid team list                    # list all teams
aid team show <name>             # show team details + knowledge
aid team create <name>           # scaffold team TOML + knowledge dir
aid team delete <name>           # remove team TOML
```

## Agent Config

Per-agent default model stored in `~/.aid/agent_config.toml`. CLI `--model` flag overrides.

```bash
aid agent config cursor --model composer-2   # set default model
aid agent config cursor --model ""           # clear default
aid agent config codex --model gpt-5.4       # set codex default
```

Default cursor model is `composer-2` (Cursor's frontier coding model, $0.50/$2.50 per M tokens).

## Stats

Agent performance dashboard with failure causes, model usage, and success rates.

```bash
aid stats                    # last 7 days (default)
aid stats --window today     # today only
aid stats --window 30d       # last 30 days
aid stats --agent codex      # filter to one agent
```

## Show

`aid show <task-id>` auto-detects task type and adjusts output:

- **Code tasks**: events + diff stat (default), `--diff` for full diff
- **Research tasks** (no worktree/changes): events + **Findings** section with agent conclusions
- Auto-saved output: research task output is auto-extracted to `~/.aid/tasks/<id>/output.md` on completion

```bash
aid show <task-id>              # smart default: diff stat OR findings
aid show <task-id> --diff       # full diff (code tasks)
aid show <task-id> --output     # agent messages (research-aware: relaxed limits)
aid show <task-id> --output --full  # complete untruncated output
aid show <task-id> --summary    # one-line status + conclusion
aid show <task-id> --context    # original + resolved prompt
aid show <task-id> --explain    # AI-generated explanation of changes
aid show <task-id> --json       # machine-readable JSON
```

## Merge

```bash
aid merge <task-id>                      # merge into current branch
aid merge <task-id> --target release     # merge into specific branch
aid merge --group <wg-id>               # merge all tasks in group
aid merge --group <wg-id> --check       # dry-run conflict check
```

## Worktree Management

```bash
aid worktree create feat/my-feature      # create worktree
aid worktree list                        # list aid-managed worktrees
aid worktree prune                       # clean up stale worktrees (>24h old)
aid worktree remove feat/my-feature      # remove specific worktree
```

Context files specified via `--context` are automatically synced into worktrees if they don't exist there (e.g., files created by earlier batch waves).

## Batch TOML Tips

- `context`, `skills`, `scope` accept both string and array: `context = "file.md"` or `context = ["a.md", "b.md"]`
- `fallback` supports comma-separated agents: `fallback = "oz,opencode,codex"`
- `aid tool show <name> --team dev` / `aid tool test <name> --team dev` to find team-scoped tools

### Usage with run and batch

```bash
# Boost dev team agents + inject dev knowledge
aid run auto "implement feature" --team dev

# Any agent can still be used explicitly with team knowledge
aid run gemini "research API design" --team dev

# Batch with team-level defaults
# [defaults]
# team = "dev"

# Usage filtered to preferred agents
aid usage --team dev
```

## Project Profiles

Per-repo configuration via `.aid/project.toml`. Profiles expand into defaults for verify, budget, and rules.

```toml
[project]
id = "my-app"
profile = "production"   # hobby | standard | production
team = "dev"
language = "rust"
budget = "$1000/day"     # shorthand syntax
verify = "cargo test"
rules = ["File size limit: 300 lines"]
```

| Profile | Verify | Budget | Rules |
|---------|--------|--------|-------|
| `hobby` | - | $5/day, prefer_budget | - |
| `standard` | `auto` | $20/day | Tests for new functions |
| `production` | `cargo test`/`npm test` | $50/day | Tests, no unwrap(), cross-review |

Budget shorthand: `budget = "$1000/day"` or `budget = "$500/month"` (also accepts struct syntax).

CLI: `aid project init` (interactive setup + CLAUDE.md sync), `aid project show`, `aid project sync` (re-sync to CLAUDE.md + global config). Project defaults are fallbacks — CLI flags always win.


<!-- aid:start -->
## aid orchestration

This project uses [aid](https://github.com/agent-tools-org/ai-dispatch) as the primary development method.
Use `aid run` to dispatch coding tasks to AI agents instead of writing code directly.

- **Project**: ai-dispatch
- **Profile**: production
- **Language**: rust
- **Budget**: $50
- **Verify**: cargo test
- **Team**: dev

### Rules
- File size limit: 300 lines per file
- Never use features = ["full"] for dependencies
- All public functions must have tests
- All changes must have tests
- No unwrap() in production code
- Changes require cross-review

### Usage
- Dispatch work: `aid run <agent> "<prompt>" --dir .`
- Review output: `aid show <id> --diff`
- Batch dispatch: `aid batch <file> --parallel`
- Project config: `.aid/project.toml`

<!-- aid:end -->
