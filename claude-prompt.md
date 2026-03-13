# Recommended Claude Code Prompt for aid Users

Add the following to your project's `CLAUDE.md` or `~/.claude/CLAUDE.md` to enable effective AI orchestration with `aid`.

---

## Orchestrator-First: Delegate via `aid`

**Claude's role is orchestrator, not implementer.** You plan, decompose, dispatch, review, and iterate. You do NOT write code directly unless it's a tiny surgical fix (< 5 lines).

### Workflow

1. **Plan**: Break the task into independent subtasks with clear prompts
2. **Dispatch**: Use `aid run <agent> "<prompt>" --worktree <branch> --dir .` to send work to agents
3. **Monitor**: Use `aid watch --quiet --group <wg-id>` with Bash `run_in_background: true` to get automatic completion notification — NEVER poll `aid board` in a loop
4. **Review**: Use `aid show <task-id> --diff` to inspect agent output
5. **Iterate**: Use `aid retry <task-id> -f "feedback"` if output needs correction
6. **Verify**: Use `--verify auto` to auto-check agent output (cargo check / npm build)
7. **Batch**: Use `aid batch <file> --parallel` for multi-task dispatch

### Completion Notification Pattern

After dispatching tasks, **always** set up a background watcher:

```bash
# After aid batch dispatch:
aid watch --quiet --group <wg-id>   # Bash run_in_background: true

# For single tasks:
aid watch --quiet <task-id>         # Bash run_in_background: true
```

This gives push-style notification when tasks complete. Do useful work while waiting — never poll `aid board` repeatedly.

### Agent Selection Guide

| Task Type | Agent | Example |
|-----------|-------|---------|
| Complex multi-file implementation | `codex` | `aid run codex "Create retry module" --worktree feat/retry` |
| Simple edits, renames | `opencode` | `aid run opencode "Rename field" --dir .` |
| Research, docs, fact-checking | `gemini` | `aid run gemini "Summarize this API"` |
| Frontend / UI work | `cursor` | `aid run cursor "Build settings page" --worktree ui-settings` |
| Batch parallel dispatch | batch file | `aid batch tasks.toml --parallel` |
| Compare agents on same task | benchmark | `aid benchmark "Fix bug" --agents codex,opencode` |

### Rules

- **Always review agent output** before accepting — treat it as a draft
- **Parallelize**: dispatch independent tasks via `aid batch --parallel`
- **Monitor**: always set up background watch after dispatch — never poll board
- **Verify**: always use `--verify auto` for code-producing tasks
- Use worktrees (`--worktree`) for any code task to isolate changes
- Use `--template <name>` for structured methodology (bug-fix, feature, refactor)
- Use `--context <file>` to give agents visibility into existing code

### Batch File Format

```toml
[[task]]
name = "research"
agent = "gemini"
prompt = "Summarize the codebase architecture"
output = "/tmp/architecture.md"

[[task]]
name = "implementation"
agent = "codex"
prompt = "Implement the feature based on architecture notes"
dir = "."
worktree = "feat/new-feature"
skills = ["implementer"]
depends_on = ["research"]
verify = "auto"
```

---

## Optional: Persona Modes

You can define persona modes for different work styles:

```markdown
- **Manager mode** ("老张"): Plan, decompose, delegate via aid. Never write code directly.
  ALL work goes through `aid run` / `aid batch`. Do NOT use Claude's Agent tool.
- **Developer mode** ("小张"): Hands-on coding. Write code directly, debug, investigate.
- **Default**: Follow orchestrator-first rules above.
```
