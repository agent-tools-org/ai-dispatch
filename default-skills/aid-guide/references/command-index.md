# Public Command Index

Use this index to select a command. Run `aid <command> --help` for its complete
current arguments.

## Dispatch and execution

| Command | Purpose |
|---|---|
| `aid run` | Dispatch one agent task with optional worktree, verification, retry, audit, context, skills, or background execution; only a successful `TaskOutcome` exits 0 in the foreground. |
| `aid advise` | Preview declared-profile agent/model routing without dispatching or writing task state. JSON candidates include an additive `quota` object (status, wall, used percent, freshness) and `breakdown.headroom_penalty`. |
| `aid batch` | Dispatch a dependency-aware TOML task graph. |
| `aid benchmark` | Run the same task through multiple agents and compare results. |
| `aid ask` | Run a focused research or exploration request with optional files. |
| `aid query` | Query an LLM directly, optionally using automatic routing. |
| `aid build` | Run supported Cargo checks (check/clippy) with compact diagnostics; zero-unit no-target runs fail clearly. |
| `aid test` | Run Cargo tests with trusted guarantees: zero-match filters fail, executed tests are named, failures stay compact. |
| `aid experiment` | Run and inspect metric-driven iterative experiments. |

## Observe and control tasks

| Command | Purpose |
|---|---|
| `aid board` | Show the current task board (default: current project only; `--all` shows every project). Includes verification tags when verification has something to report. |
| `aid watch` | Stream task or group progress; `--wait` waits for verification to settle and exits non-zero when a task did not succeed. |
| `aid wait` | Block until selected tasks or a group reach a stopping state, including verification completion; returns non-zero when any task did not succeed. |
| `aid show` | Inspect task state, outcome, verification, events, context, output, result, transcript, summary, audit, or diff; `--diff --branch` widens the diff from the task's own commits to the whole branch. |
| `aid output` | Print task output directly. |
| `aid tree` | Show task ancestry and retries. |
| `aid respond` | Supply an answer to a task awaiting input. |
| `aid reply` | Send a message to a running task and optionally wait for acknowledgement. |
| `aid steer` | Inject updated direction into a running task. |
| `aid unstick` | Request recovery or escalation for a stalled task. |
| `aid stop` | Stop one task or its retry tree while preserving artifacts. |
| `aid retry` | Start a new attempt using prior task context and artifacts; supersedes a non-terminal task by stopping its live worker first. Optional `--model`, `--idle-timeout`, and `--feedback-file` (`-F`) override those fields; unspecified model/idle-timeout inherit the original task. |
| `aid merge` | Merge delivered code only when its outcome is successful by default; `--force` overrides a failed or inconclusive verification and records the reason. This is not principal acceptance. |

## Review and artifact custody

| Command | Purpose |
|---|---|
| `aid accept` | Record the principal's explicit acceptance of a terminal task artifact. |
| `aid reject` | Record rejection while preserving every artifact. |
| `aid gc` | Delete an accepted task worktree only after recursive durability proof. |
| `aid worktree` | Create or list AID-managed worktrees; it does not destroy them. |

## Organize knowledge and collaboration

| Command | Purpose |
|---|---|
| `aid group` | Create and manage workgroups, findings, summaries, and broadcasts. |
| `aid team` | Manage reusable team definitions. |
| `aid memory` | Add, search, update, version, or forget project memory. |
| `aid kg` | Add, query, invalidate, search, or inspect temporal knowledge-graph facts. |
| `aid notifications` | Print recent task notifications. |
| `aid export` | Export a task in a supported format. |

## Configure and administer

| Command | Purpose |
|---|---|
| `aid setup` | Configure AID and install bundled resources when needed. |
| `aid project` | Initialize, inspect, or synchronize project configuration. |
| `aid agent` | Inspect built-in agent availability and related state. `aid agent config <name> --model` sets a sticky default for `aid run` and `aid batch`. `aid agent quota` shows live used percent and freshness when an aidbar snapshot exists; `STALE` is display-only. |
| `aid config` | Inspect agents, pricing, installed skills, templates, and prompt budgets. |
| `aid store` | Browse, install, inspect, and update community packages. |
| `aid tool` | Manage reusable tool definitions. |
| `aid credential` | Manage credential-pool entries. |
| `aid byok` | Manage custom OpenAI-compatible providers through opencode. |
| `aid container` | Build, list, or stop development containers. |
| `aid hook` | Install or invoke supported AID hooks; task hook payloads expose additive `outcome` and `verify_status` fields. |
| `aid mcp` | Start AID's stdio MCP server; task payloads expose additive `outcome` and `verify_status` fields. |
| `aid doctor` | Report repository/worktree hygiene without bypassing custody. |
| `aid clean` | Remove disposable logs and caches while retaining custody evidence; reclaims a task's fallback cargo target only once the directory it was keyed from is gone, and reports how many it held back. |
| `aid web` | Serve the embedded dashboard and client API; `--host` selects the bind address and non-loopback binds require `--token`. |
| `aid upgrade` | Upgrade AID after checking active-task safety. |
| `aid changelog` | Read release notes. |

## Reporting

| Command | Purpose |
|---|---|
| `aid usage` | Report token and usage totals. |
| `aid cost` | Report estimated costs by group, agent, or period. |
| `aid stats` | Report outcome-based task success, declared difficulty versus outcomes, models, failures, and usage concentration. |

Global options include `--quiet`, `--help`, and `--version`.
