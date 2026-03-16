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

This project uses [aid](https://github.com/agent-tools-org/ai-dispatch) for AI task orchestration.

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
