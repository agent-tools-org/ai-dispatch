# Public Command Index

Use this index to select a command. Run `aid <command> --help` for its complete
current arguments.

## Dispatch and execution

| Command | Purpose |
|---|---|
| `aid run` | `--remote-build [BOX]` runs Cargo builds on rbox (bare/auto picks once; retry reuses box, verify re-picks on disk admission refusal). `--backup TARGET[:FOLDER]` / `--no-backup` override the project's `[backup]` artifact upload (`gdrive` via `gws`); `aid retry` inherits the setting. Dispatch one agent task with optional worktree, verification, retry, audit, context, skills, or background execution; only a successful `TaskOutcome` exits 0 in the foreground. A Degraded route prints a warning at dispatch and is not diverted. |
| `aid advise` | Preview declared-profile agent/model routing without dispatching or writing task state. JSON candidates include an additive `quota` object (status incl. `unknown` without evidence, wall, used percent, freshness), `auth` (`failed` with `observed_at`, or `unknown`), `exclusion_codes`, `demotion_reason`, and `breakdown.headroom_penalty`; the report adds `caller`. Not-installed candidates are ineligible. `--caller-model <model>` (else `AID_CALLER_MODEL`) excludes weaker models on the caller's provider pool. Each candidate and the recommendation carry `model`, `pinned`, and `source` from the resolver `aid run` uses (`explicit` / `forced_default` / `sticky` / `custom_forced` / `smart_route` / `budget_route` / `cli_config` / `agent_default`); `model: null` prints as `agent default (unknown)`. `unrated_served_models` lists served, unrated models newer than the candidate's model (omitted when empty). |
| `aid batch` | Dispatch a dependency-aware TOML task graph. |
| `aid benchmark` | Run the same task through multiple agents and compare results. |
| `aid ask` | Run a focused research or exploration request with optional files. |
| `aid query` | Query an LLM directly, optionally using automatic routing. |
| `aid classify` | Ask TypeSafe Jev typed `noul`/`choice`/`score` questions about a text or JSON state (stdin by default) and print a small validated JSON answer; also the MCP `classify` tool. Exits 2 no key, 3 API error, 4 invalid questions, 5 state refused. Answers over agent output are hints, never a verdict; see [classify.md](classify.md). |
| `aid build` | Run supported Cargo checks (check/clippy) with compact diagnostics; zero-unit no-target runs fail clearly. |
| `aid test` | Run Cargo tests with trusted guarantees: zero-match filters fail, executed tests are named, failures stay compact. |
| `aid experiment` | Run and inspect metric-driven iterative experiments. |

## Observe and control tasks

| Command | Purpose |
|---|---|
| `aid errors` | Inspect recent CLI parse errors and pre-task rejections, with correction hints; see [command-errors.md](command-errors.md). |
| `aid board` | Show the current task board (default: current project only; `--all` shows every project). Includes verification tags when verification has something to report. |
| `aid watch` | Stream task or group progress; `--wait` waits for worker delivery and verification checks to settle and exits non-zero when a task did not succeed. |
| `aid wait` | Block until selected tasks or a group finish worker settlement, including delivery and verification checks; returns non-zero when any task did not succeed. |
| `aid show` | Inspect task state, outcome, verification, events, context, output, result, transcript, summary, audit, or diff; prints `Backup: <url>` (also `backup_url` in `--json`) once an artifact backup was uploaded; `--diff --branch` widens the diff from the task's own commits to the whole branch. |
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
| `aid agent` | Inspect built-in agent availability and related state. `aid agent config <name> --model` sets a sticky default for `aid run` and `aid batch`. `aid agent quota` shows live used percent and freshness when an aidbar snapshot exists; `STALE` is display-only. `aid agent list` includes `claude`. `aid agent list --json` quota objects carry `ok` (a successful probe observed it), `unknown` (no evidence), `degraded`, `partial`, or `limited` state plus `used_percent`, `resets_at`, and `source` (`probe` / `marker` / `none`); each agent carries `auth` (`failed` with `observed_at` after a not-signed-in run within the last hour, else `unknown`). Its `models` object carries `default_source` (`sticky` / `custom_forced` / `budget_route` / `cli_config`; `null` with `default` when unknown), and each `models.available` row carries `rated` and `source` (`catalog` / `served` / `pricing_override`); served-only rows are `rated: false` with `null` capability and prices. `aid agent quota` prints `UNKNOWN` for a route no probe observed. A NeedsHuman hold on `aid agent list` prints `needs human: <stored first line> — fix, then aid config clear-limit <agent>` rather than a bare `LIMITED`. |
| `aid config` | Inspect agents, pricing, installed skills, templates, and prompt budgets. |
| `aid store` | Browse, install, inspect, and update community packages. |
| `aid tool` | Manage reusable tool definitions. |
| `aid credential` | Manage credential-pool entries. |
| `aid byok` | Manage custom OpenAI-compatible providers through opencode. |
| `aid container` | Build, list, or stop development containers. |
| `aid hook` | Install or invoke supported AID hooks; task hook payloads expose additive `outcome` and `verify_status` fields. |
| `aid mcp` | Start AID's stdio MCP server; task payloads expose additive `outcome` and `verify_status` fields. The `classify` tool takes `state`, `state_json`, `questions`, and `model` and returns the same JSON as `aid classify`. |
| `aid doctor` | Report repository/worktree hygiene, leaked operator symlinks, and NeedsHuman agent holds; `--apply` repairs only those symlinks without bypassing custody. |
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
