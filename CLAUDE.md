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
