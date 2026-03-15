# ai-dispatch (aid)

## Install

- NEVER cp binary to `/opt/homebrew/bin/` — macOS provenance xattr blocks execution
- `/opt/homebrew/bin/aid` is a symlink to `~/.cargo/bin/aid`
- Install command (MUST re-sign after copy — sandbox provenance blocks execution):
  ```bash
  cp "$CARGO_TARGET_DIR/release/aid" ~/.cargo/bin/aid && codesign --force --sign - ~/.cargo/bin/aid
  ```

## Teams

Teams group agents into role-specific sets for constrained auto-selection. Team definitions live in `~/.aid/teams/*.toml`.

### TOML format

```toml
[team]
id = "dev"
display_name = "Development Team"
description = "Feature development and code quality"
agents = ["codex", "opencode", "cursor", "claude-code", "codebuff", "ob1", "kilo"]
default_agent = "codex"

[team.overrides.opencode]
simple_edit = 10
debugging = 6
```

### CLI

```
aid team list                    # list all teams
aid team show <name>             # show team details + members
aid team create <name>           # scaffold team TOML
aid team delete <name>           # remove team TOML
```

### Usage with run and batch

```bash
# Auto-select only from dev team members
aid run auto "implement feature" --team dev

# Batch with team-level defaults
# In batch TOML:
# [defaults]
# team = "dev"

# Usage filtered to a team
aid usage --team dev
```
