# ai-dispatch (aid)

## Install

- NEVER cp binary to `/opt/homebrew/bin/` — macOS provenance xattr blocks execution
- `/opt/homebrew/bin/aid` is a symlink to `~/.cargo/bin/aid`
- Install command (MUST re-sign after copy — sandbox provenance blocks execution):
  ```bash
  cp "$CARGO_TARGET_DIR/release/aid" ~/.cargo/bin/aid && codesign --force --sign - ~/.cargo/bin/aid
  ```

## Release

Release must go through `scripts/release.sh`. Do not manually bump `Cargo.toml`, edit the top release entry in `CHANGELOG.md`, create the release commit, create the release tag, or push the release branch/tag by hand.

Release flow requirements:
- Start from a clean git worktree. Commit or stash local edits before running the release script.
- Prepare a Markdown notes file with one `- ` bullet per shipped change.
- Run `scripts/release.sh --dry-run <version> <notes-file>` first and review the planned commit/tag/push.
- Run `scripts/release.sh <version> <notes-file>` for the actual release.
- Treat any direct `git tag`, manual version bump, or manual changelog-only release edit as an invalid release flow.

```bash
cat > /tmp/aid-release-notes.md <<'EOF'
- Short release summary
- Additional shipped change
EOF

scripts/release.sh --dry-run 8.75.0 /tmp/aid-release-notes.md
scripts/release.sh 8.75.0 /tmp/aid-release-notes.md
```

## Run

Dispatch a task to an AI agent. Core command — most other features build on this.

```bash
aid run codex "Add unit tests" --verify              # with auto-verify
aid run gemini "Research topic" -o notes.md           # research with output file
aid run codex "Refactor" -w feat/refactor --bg        # background + worktree
aid run auto "implement feature" --team dev           # auto-select agent with team context
```

### Key flags

| Flag | Purpose |
|------|---------|
| `-w, --worktree <branch>` | Run in isolated git worktree |
| `--verify [<cmd>]` | Auto-verify on completion (default: project verify cmd) |
| `--judge [<agent>]` | AI judge evaluates output quality |
| `--peer-review <agent>` | Dispatch peer review after completion |
| `--cascade <agents>` | Comma-separated fallback agents on failure |
| `--context <file>...` | Inject files as context into the prompt |
| `--context-from <task-id>...` | Inject output from previous tasks as context |
| `--scope <path>...` | Restrict agent file access to specific paths |
| `--skill <name>...` | Inject methodology skills into the prompt |
| `--template <name>` | Wrap prompt with a template |
| `--on-done <cmd>` | Shell command to run on task completion |
| `--hook <spec>...` | Hook specs for the dispatched task |
| `--bg` | Run in background (non-blocking) |
| `--sandbox` | Run agent in sandboxed mode |
| `--container <image>` | Run agent inside a container |
| `--best-of <N>` | Run N copies, pick best result |
| `--metric <cmd>` | Custom metric command for best-of selection |
| `--budget` | Use budget-optimized model |
| `--read-only` | Agent cannot modify files |
| `--idle-timeout <secs>` | Kill agent if idle for N seconds |
| `--retry <N>` | Auto-retry on failure (default: 0) |
| `-g, --group <wg-id>` | Assign to workgroup |

## Batch

Dispatch multiple tasks from a TOML file.

```bash
aid batch tasks.toml --parallel                     # parallel dispatch
aid batch tasks.toml --parallel --max-concurrent 3  # limit concurrency
aid batch tasks.toml --analyze                      # warn about file overlaps
aid batch tasks.toml --wait                         # block until all complete
aid batch tasks.toml --var key=value                # template variables
aid batch init                                      # generate template TOML
aid batch retry --group wg-abc1                     # re-dispatch failed tasks
```

### Batch TOML format

```toml
[defaults]
dir = "."
agent = "codex"
team = "dev"
verify = "cargo check"
fallback = "cursor"
model = "o3"
context = ["src/types.rs"]
skills = ["implementer"]
worktree_prefix = "feat/my-feature"    # auto-generates worktree per task
analyze = true                          # warn about overlapping file edits

[[task]]
name = "parser"                         # REQUIRED if sharing worktree with other tasks
prompt = "Implement parser"
worktree = "feat/my-feature/parser"
depends_on = ["types"]                  # wait for named task to complete
fallback = "oz,opencode"
on_success = "tests"                    # trigger conditional task on success
on_fail = "cleanup"                     # trigger conditional task on failure
conditional = true                      # only runs when triggered by on_success/on_fail
idle_timeout = 120
```

- `context`, `skills`, `scope` accept both string and array: `context = "file.md"` or `context = ["a.md", "b.md"]`
- `fallback` supports comma-separated agents: `fallback = "oz,opencode,codex"`
- `worktree_prefix` auto-generates `{prefix}/{task-name}` (or `{prefix}/task-{index}` for unnamed tasks)

## Watch & Board

```bash
aid watch t-1234                         # live TUI for one task
aid watch --quiet t-1234                 # block until done (for scripts)
aid watch --quiet --group wg-abc1        # block until group finishes
aid watch --tui                          # full dashboard TUI
aid watch --exit-on-await t-1234         # exit when task awaits input
aid watch --timeout 600 t-1234           # timeout after 10 minutes
aid board                                # recent tasks (default: 50)
aid board --running                      # only active tasks
aid board --today                        # today's tasks
aid board --group wg-abc1                # tasks in workgroup
aid board -l 10                          # limit to 10 tasks
aid board --stream                       # live-updating stream
aid board --json                         # machine-readable JSON
```

## Task Lifecycle

```bash
aid retry t-1234 -f "Fix the compilation error in parser.rs"
aid retry t-1234 -f "Use HashMap instead" --agent opencode   # switch agent
aid retry t-1234 -f "Start fresh" --reset                    # reset worktree
aid stop t-1234                          # graceful stop
aid stop t-1234 --force                  # force kill
aid steer t-1234 "Focus on the error handling, skip the logging changes"
aid respond t-1234 "Yes, use the async version"
aid respond t-1234 -f response.md        # respond with file contents
```

## Show

`aid show <task-id>` auto-detects task type and adjusts output:

- **Code tasks**: events + diff stat (default), `--diff` for full diff
- **Research tasks** (no worktree/changes): events + **Findings** section with agent conclusions

```bash
aid show <task-id>                   # smart default: diff stat OR findings
aid show <task-id> --diff            # full diff (code tasks)
aid show <task-id> --output          # agent messages
aid show <task-id> --output --full   # complete untruncated output
aid show <task-id> --summary         # one-line status + conclusion
aid show <task-id> --context         # original + resolved prompt
aid show <task-id> --explain         # AI-generated explanation of changes
aid show <task-id> --json            # machine-readable JSON
```

## Merge

```bash
aid merge <task-id>                      # merge into current branch
aid merge <task-id> --target release     # merge into specific branch
aid merge --group <wg-id>               # merge all tasks in group
aid merge --group <wg-id> --check       # dry-run conflict check
```

## Workgroups

```bash
aid group create --name "v9 release"           # create workgroup
aid group list                                  # list workgroups
aid group show wg-abc1                          # show group + member tasks
aid group update wg-abc1 --name "v9.1 release" # rename
aid group summary wg-abc1                       # milestones, findings, costs
aid group finding wg-abc1 "Key discovery"       # post a finding
aid group broadcast wg-abc1 "Update: ..."       # message all group members
aid group delete wg-abc1                        # delete group definition
```

## Worktree Management

```bash
aid worktree create feat/my-feature      # create worktree
aid worktree list                        # list aid-managed worktrees
aid worktree prune                       # clean up stale worktrees (>24h old)
aid worktree remove feat/my-feature      # remove specific worktree
```

Context files specified via `--context` are automatically synced into worktrees if they don't exist there (e.g., files created by earlier batch waves).

### Worktree Safety Rules

- **Shared worktree naming**: When 2+ batch tasks share the same worktree, ALL must have `name = "..."` so aid can auto-sequence access via `depends_on`. Unnamed tasks sharing a worktree will be rejected at validation time.
- **Lock mechanism**: Each worktree gets an `.aid-lock` file during task execution. If another task tries to use a locked worktree, it fails with a clear error. Stale locks (dead PID) are auto-cleared.
- **Failure preservation**: When a task fails in a shared worktree, the worktree is preserved if sibling tasks are still active.
- **`worktree_prefix`**: Auto-generates unique worktree paths per task (`{prefix}/{name}` or `{prefix}/task-{idx}`). Preferred over manually assigning the same worktree to multiple tasks.

## Utilities

```bash
aid ask "What is the latest Rust edition?"               # one-shot question
aid query "key insight" -g wg-abc1 --finding             # search task history
aid tree t-1234                          # show task tree (parent + children)
aid output t-1234                        # raw agent output (--full for complete)
aid export t-1234                        # export as markdown (default)
aid export t-1234 --format json -o out.json  # export as JSON
aid memory add discovery "fact"          # store agent memory
aid memory list | search "keyword"       # recall memories
aid tool list | show <name> | add <name> | test <name>   # manage tools
aid container build | list | stop        # manage containers
aid experiment run experiment.toml       # automated experiment loops
aid clean                                # remove tasks older than 7 days
aid clean --older-than 30 --worktrees    # custom retention + prune worktrees
aid upgrade                              # upgrade aid to latest version
aid stats                                # agent performance (--window today/30d)
aid config agents | pricing | skills | templates         # configuration
aid config clear-limit codex             # clear rate-limit marker
```

Audit and review prompts now default to an audit-report flow: `aid run codex "Cross-audit ..."` auto-writes `result.md`, asks the agent for a Markdown report, and makes `aid show --output` / TUI / web prefer the saved report over raw logs.

## Teams

Teams provide **knowledge context and soft agent preferences** — not hard restrictions. All agents remain available; `--team` boosts preferred agents in auto-selection and injects team knowledge into prompts.

```toml
[team]
id = "dev"
display_name = "Development Team"
preferred_agents = ["codex", "opencode", "cursor"]  # soft boost, not hard filter
default_agent = "codex"
rules = ["Do NOT run cargo fmt or any auto-formatter"]

[team.overrides.opencode]
simple_edit = 10
```

Knowledge: `~/.aid/teams/<id>/KNOWLEDGE.md` — auto-injected into prompts with `--team`.

```bash
aid team list | show <name> | create <name> | delete <name>
```

## Agent Config

Per-agent default model stored in `~/.aid/agent_config.toml`. CLI `--model` flag overrides.

```bash
aid agent config cursor --model composer-2   # set default model
aid agent config codex --model gpt-5.4       # set codex default
```

## Project Profiles

Per-repo configuration via `.aid/project.toml`. Profiles expand into defaults for verify, budget, and rules.

| Profile | Verify | Budget | Rules |
|---------|--------|--------|-------|
| `hobby` | - | $5/day, prefer_budget | - |
| `standard` | `auto` | $20/day | Tests for new functions |
| `production` | `cargo test`/`npm test` | $50/day | Tests, no unwrap(), cross-review |

CLI: `aid project init`, `aid project show`, `aid project sync`. Project defaults are fallbacks — CLI flags always win.

## MCP & Hooks

```bash
aid mcp                                  # start MCP server mode
aid hook session-start                   # print session-start hook text
```

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
