# Architecture Overview

Source baseline: v10.47.0 (`ffd07e8e`), reviewed 2026-09-22. See the
[project inventory](../../docs/project-status-2026-09-22.md) for evidence and limits,
and the [roadmap](../../docs/roadmap.md) for remaining work.

## Module layout

| Boundary | Source | Responsibility |
| --- | --- | --- |
| Entry and commands | `src/main.rs`, `src/cli/`, `src/cli_actions.rs`, `src/cmd_dispatch/` | Parse, diagnose, and dispatch commands |
| Run and batch | `src/cmd/run/`, `src/cmd/batch/`, `src/batch/` | Resolve target project, route, prompt, task and DAG execution |
| Agent abstraction | `src/agent/mod.rs`, `src/agent/registry.rs`, per-agent modules | `Agent` trait, `RunOpts`, adapters and custom definitions |
| Routing | `src/agent/selection*`, `src/route_availability*`, `src/live_quota*`, `src/rate_limit*` | Advice, capabilities, quota groups, hold/degraded state |
| Worker and transport | `src/background*`, `src/pty_*`, `src/watcher/` | Detached worker, interaction, output and liveness |
| Lifecycle | `src/cmd/run/lifecycle.rs`, adjacent phase modules, `src/task_lifecycle.rs` | Settlement, verification, delivery, persistence and side effects |
| Domain facts | `src/types/`, `src/task_view.rs` | Status, outcome, delivery assessment and shared presentation facts |
| Persistence | `src/store/` | SQLite schema/migrations, queries, guarded mutations, custody records |
| Git and custody | `src/worktree/`, `src/commit/`, `src/artifact_custody/`, `src/cmd/merge*` | Locks, snapshots, rescue, merge, explicit acceptance and durable GC |
| Configuration | `src/project/`, `src/project.rs`, `src/config.rs`, `src/team.rs` | Target identity/discovery, project/global/team defaults |
| Interfaces | `src/cmd/show/`, `src/board/`, `src/tui/`, `src/web/` | CLI, terminal UI and optional authenticated API/SSE |
| Native client | `client/AIDCommand/` | Shared SwiftUI macOS/iPadOS client, live/demo data sources, Keychain |
| Operations | `src/remote_build/`, `src/backup/`, `scripts/` | Remote Cargo through rbox, artifact backup, validation and release |

## Execution and ownership

1. CLI parsing and `cmd_dispatch` route to command handlers. `cmd/run` resolves the
   dispatch target, route and prompt; batch schedules ready tasks from its dependency graph.
2. Foreground runs also dispatch a detached worker and attach a watcher. Worker launch
   uses a double fork before starting the worker's Tokio runtime; caller death must not
   destroy the agent's output transport.
3. Agent adapters build commands and interpret agent-specific events. Watcher/PTY paths
   capture output, usage, session and liveness evidence. Terminal errors must be interpreted
   according to the adapter's actual protocol, not exit code alone.
4. Run lifecycle coordinates settlement, verification, delivery assessment and postprocessing.
   `task_lifecycle` owns status intents and failure side effects; Store guards transitions.
5. Task views expose persisted facts to CLI/TUI/Web. Completion, verification, delivery,
   principal acceptance, and permission to delete artifacts are separate decisions.

The default state root is `~/.aid`, overridden by `AID_HOME`. Project configuration
lives in `.aid/project.toml`; linked worktrees can inherit the main checkout's config.
The budget gate receives the dispatch target identity explicitly. Budget SQL and
usage/TUI filtering use persisted `project_id`; legacy rows without it remain
unattributed. Project budget synchronization through init/sync is still separate.

## Key types and feature boundaries

- `TaskId`: string newtype; `Task`: persisted task facts and dispatch metadata.
- `AgentKind`: 14 built-in kinds plus Custom; consult `src/types/agent.rs` for the complete list.
- `TaskStatus`: Waiting, Pending, Running, AwaitingInput, Stalled, Done, Merged, Failed,
  Skipped, Stopped. Legal transitions live in `src/types/status.rs`.
- `TaskOutcome`: derived judgment; a Done task can still be Unverified. See
  `src/types/outcome.rs`, `verify_status.rs` and `delivery.rs`.
- `Store`: SQLite access; `RunArgs`: runtime dispatch arguments, with persisted state
  rehydrated for retry/background execution.
- Cargo is one package with an `aid` binary; `web` is optional and disabled by default.
  Swift builds are separate, generated from `client/project.yml`.

This is a code map, not proof that every lifecycle edge has been tested. Preserve the
regression boundaries in the roadmap when extracting modules or changing state semantics.
