# Architecture Overview

## Module Layout

```
src/
├── main.rs            — CLI dispatch, wires Commands to cmd/* handlers
├── cli.rs             — clap derive definitions (Commands enum, Args structs)
├── cli_actions.rs     — Subcommand enums (GroupAction, TeamAction, ProjectAction, etc.)
├── types.rs           — Core types: TaskId, TaskStatus, AgentKind, MemoryType, etc.
├── store/             — SQLite persistence (tasks, events, workgroups, memory)
│   ├── mod.rs         — Store struct, migrations, CRUD
│   └── queries.rs     — Complex queries (search, analytics, history)
├── agent/             — Agent adapters + selection
│   ├── mod.rs         — AgentAdapter trait, RunOpts, select_agent_with_reason
│   ├── classifier.rs  — Prompt → TaskCategory + Complexity classification
│   ├── selection.rs   — Capability matrix scoring, team/history adjustments
│   ├── selection_scoring.rs — Score calculation helpers
│   ├── registry.rs    — Custom agent TOML loading from ~/.aid/agents/
│   ├── codex.rs, gemini.rs, opencode.rs, cursor.rs, kilo.rs, codebuff.rs — Per-agent adapters
│   └── custom.rs      — Generic adapter for user-defined agents
├── cmd/               — Command handlers (one file per command)
│   ├── run.rs         — Main dispatch: run() → spawn agent → watcher
│   ├── run_prompt.rs  — Prompt assembly: skills + context + team/project knowledge + rules
│   ├── run_agent.rs   — Agent process spawning and PTY bridge
│   ├── batch.rs       — TOML batch parsing + DAG scheduler
│   ├── merge.rs       — Worktree merge with auto-commit + conflict detection
│   ├── show.rs        — Task inspection (diff, output, context, explain)
│   └── ...            — Other commands follow same pattern
├── project.rs         — .aid/project.toml parsing, profile expansion, knowledge reading
├── team.rs            — Team TOML parsing, knowledge entries, rules
├── watcher.rs         — Agent output parser: milestones, findings, usage, completion
├── worktree.rs        — Git worktree lifecycle (create, reuse, prune, changed_files)
├── tui/               — Terminal UI (ratatui-based)
└── background.rs      — Background task runner (detached process management)
```

## Key Data Flow

1. `aid run` → `cmd::run::run()` → build prompt (`run_prompt.rs`) → spawn agent (`run_agent.rs`) → watch output (`watcher.rs`) → store completion (`store`)
2. `aid batch` → parse TOML → DAG sort → dispatch ready tasks → watch group
3. Prompt assembly order: base prompt → skills → team rules → team knowledge → project rules → project knowledge → context files → context-from → sibling summaries

## Key Types

- `TaskId` — wrapped String, format `t-NNNN`
- `AgentKind` — enum: Gemini, Codex, OpenCode, Cursor, Kilo, Codebuff, Custom
- `TaskStatus` — Running, Done, Failed, Merged, Stopped, Skipped
- `Store` — SQLite wrapper, thread-safe via `Arc<Store>`
- `RunArgs` — all `aid run` parameters (40+ fields, `Default` impl)
