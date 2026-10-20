# Dispatch and Execution

## Tools and skills are the caller's choice

aid does not choose either on the caller's behalf.

- **Skills**: `--skill <name>` declares them, `--no-skill` declares none, and a project sets a
  default once with `skills = ["implementer"]` in `.aid/project.toml`. Omitting all three means no
  skill. A `--read-only` run skips the project default; an explicit `--skill` still applies. aid previously picked one from the **agent kind alone**, never looking at the task, so
  every implementation CLI was handed `implementer` and agy was handed `researcher`
  whatever the work was — a large block of methodology text and a persona nobody had asked for.
- **Tools**: omitting `--kind` describes every resolved toolbox tool. Narrowing is opt-in because
  omission is not a decision: a guessed category once cut a multi-file refactor down to 2 of 24
  tools, with nothing to tell the caller what had been hidden.

`aid show --context` and `aid export` report the skills a task was actually dispatched with, read
from its stored args rather than re-derived from its agent. Tasks dispatched before skills became
declared report none, which is what their record says.

## Routes: CLI x provider x model

An execution route is three independent things, not one opaque agent id:

```text
opencode / opencode-zen / glm-5.2
└ CLI      └ provider     └ model
```

| Dimension | Owns |
|---|---|
| CLI | invocation: flags, output shape, session resume, sandboxing |
| provider | metering and billing: the quota pool and its reset semantics |
| model | capability per category, context window, per-token price |

Before dispatching, `aid` validates requested `--model` parameters against the target CLI's served model list (e.g. `grok models`, `agy models`, `cursor-agent models`, or local CLI config). Only models positively reported as absent by the CLI are rejected before execution.

For `agy`, host preflight also rejects a requested model when `agy --help` succeeds
but does not define `--model`. Omit `--model` or upgrade agy. If help inspection
fails or times out, `aid` warns and skips this check. Container, sandbox, and
dry-run dispatch skip host flag validation; command construction still warns
and drops the requested model when agy has no model flag.

`aid advise` names the recommended route in this form. `aid agent list --json`
carries `provider` and `metering` per agent. Agent names keep working unchanged:
`aid run codex` resolves to a route.

Some CLIs are themselves the provider. For example, `aid run commandcode`
routes through the `commandcode` CLI and the `commandcode.ai` provider even
when the observed model belongs to Anthropic, OpenAI, Google, xAI, or another
upstream vendor served by that account.

`metering` says how a provider meters, which decides what one outage implies:

| Value | Meaning |
|---|---|
| `account_pool` | one pool for the whole account, shared by every model it serves |
| `per_model_family` | separate pools per family; one exhausted family says nothing about the others |
| `spend_budget` | a currency budget that does not refill with time — only a top-up clears it |
| `subscription` | not metered per task, though model tiers cost the plan differently |
| `none` | no pool: billed per token against your own key |
| `unknown` | not established — aid has never observed this provider refuse |

`unknown` is a real answer rather than a gap to be filled with a plausible
guess, and it appears for every provider whose metering aid has not seen.

## Choose the entry point

- Use `aid run` for one accountable task with a stored lifecycle.
- Use `aid advise` to inspect routing without dispatching or changing the store.
- Use `aid batch` for multiple dependent or parallel tasks.
- Use `aid ask` for quick research with file context.
- Use `aid query` for a direct model query that does not need a task worktree.
- Use `aid benchmark` to compare agents on the same prompt.
- Use `aid experiment` for repeated metric-driven improvement.
- Use `aid build` for compact Rust compile diagnostics (check/clippy).
- Use `aid test` for trusted Cargo test runs (zero-match is an error; digests name executed tests).

## Run one task

```bash
aid run codex "Implement request validation" \
  --dir . \
  --difficulty moderate --budget standard --urgency normal --rigor standard \
  --worktree feat/request-validation \
  --verify \
  --retry 1 \
  --bg
```

Important controls:

- `--difficulty` declares `trivial`, `simple`, `moderate`, or `complex` capability needs.
- `--budget` declares a preferred `free`, `cheap`, `standard`, or `premium` model
  tier. For an explicit agent, model selection is `--model`, then the per-agent
  default in `~/.aid/agent_config.toml` (`aid agent config <agent> --model`), then
  the catalog pick only for a declared `free` or `cheap` budget. `standard` and
  `premium` leave the model unset for the CLI's own default (no `-m`).
  Simple-task smart routing applies only when no budget is declared.
  A configured default is sticky like
  `--model`: it outranks the catalog budget pick. `aid run` and `aid batch` share
  that order. When the configured default outranks a `free` or `cheap` declaration
  whose catalog still has a preferred-tier row, aid warns once on stderr that the
  configured default overrode the declared budget. When no catalog model sits on a
  preferred tier, aid warns on stderr (agent, declared budget, model actually
  chosen) and still dispatches. Catalog tier `unknown` means unpriced, not
  ineligible — it is selectable as a last resort after the known preferred tiers.
  `aid run`, `aid batch`, and `aid advise` resolve the model with one function,
  in this precedence: `--model` (`explicit`), a self-heal retry (`forced_default`,
  passes nothing and also drops `--model`), the sticky default (`sticky`), a custom
  agent's `forced_model` (`custom_forced`), simple-task smart routing
  (`smart_route`), the budget route (`budget_route`: a declared `free`/`cheap`
  budget, `--budget`, or `selection.budget_mode`), the CLI's own readable default
  (`cli_config`: codex `config.toml`), else `agent_default` with
  no model. Only `explicit`, `sticky`, `smart_route`, and `budget_route` are
  pinned: aid passes the model to the CLI. `cli_config` and `custom_forced` are
  reported but not passed by aid.
  Every dispatch reports its effective model and source (`--model`, agent config,
  budget route, `CLI config (no -m)`, or `CLI default (no -m)`). Quota/budget
  routing overrides and existing adapter defaults (Cursor, MiMoCode) are
  labeled separately. A healthy default quota group keeps the model
  unset; a held default group pins the first healthy alternative family.
- `--urgency` declares `background`, `normal`, or `urgent` rate-limit handling. `background` may wait out a clock or Windowed hold; a NeedsHuman hold still blocks.
- `--rigor` declares `draft`, `standard`, or `critical` proof level (compiles / path exercised /
  cross-audit). `critical` forces `--verify` and `--audit`; it does **not** restrict which agent
  may run.
- `--egress` declares `any` (default), `local`, or `private-network`. `local` admits only a provider whose established
  endpoint is loopback (`localhost`, `127.0.0.0/8`, or `::1`). `private-network` admits loopback, RFC1918/link-local
  IPs, or private DNS suffixes (`.local`, `.home.arpa`) but does not widen `local`. Every current built-in agent is third-party or
  unknown and therefore ineligible for either gate. Egress is decided by the provider (or a custom agent's
  `base_url`), not by CLI identity or a hand-set `trust_tier`. Custom BYOK agents declare `provider` and optional
  `metering` in the manifest (copied into the generated agent TOML); aid never infers provider identity from the host.
- `--kind` overrides the inferred task kind while difficulty remains caller-declared. On `aid run`
  it is also how a caller narrows the injected toolbox: declare it and tools are filtered to that
  category, omit it and every resolved tool is described.
- `--dir` sets the task working directory.
- `--repo` or `--repo-root` supplies the repository anchor.
- `--worktree` creates or reuses an isolated task branch.
- For a read-only audit of an existing checkout, use `--kind debugging --read-only --dir <checkout-path>`; `--read-only` cannot be combined with `--worktree`. `--audit` schedules an additional post-task cross-audit. Rejected requests can be inspected with `aid errors`; see [command-errors.md](command-errors.md).
- `--verify [COMMAND]` and project/batch `verify` send eligible plain `cargo check|test|clippy`
  commands without shell syntax through aid's build runner (adding `--message-format=json`)
  outside remote-build/container runs, otherwise through `sh -c` in the task directory; bare
  `--verify` auto-detects, and an absent flag uses the project default.
  Missing under `sh -c`, a program fails verification; missing `cargo` on the runner is `InfrastructureFailure`.
  A task with verification configured is written with `verify_status = pending` at dispatch; read-only tasks
  or tasks without a working directory skip verification.
  An empty diff still runs configured verification; delivery assessment records `empty_diff`.
  Timeout is `timed_out`; errors launching verification or Cargo infrastructure failures can be
  `infrastructure_failure`; both are inconclusive; other nonzero verification exits are `failed`.
- `--retry N` permits new attempts after failure.
  A decoded provider-capacity refusal ends the task `failed`, naming the error
  and selected model, and creates a short hold on that model. Capacity does not
  create an automatic continuation or waiting child. Recovery uses the ordinary
  `--retry N` budget or a manual `aid retry`; ordinary retries consume that budget.
  Explicit `--cascade` retains its configured policy. Held-route dispatch never
  automatically substitutes an advised agent; a capacity hold fails dispatch
  with its reason, preserving agent, model and session.
  Only decoded provider `error` and `turn.failed` envelopes persisted as events
  count as capacity evidence. Raw text, assistant messages and tool envelopes do
  not create a hold. Over the PTY transport stdout and stderr are merged, so a
  provider-shaped capacity error printed by a tool can be classified as capacity.
  Events and `aid show` identify provider capacity. Verification of the interrupted
  attempt is skipped; an ordinary retry keeps the configured verification command.
- `--bg` returns the task ID immediately.
- `--read-only` compares the entire Git repository with its dispatch snapshot after
  the agent exits, including content changes to already dirty files, new untracked
  files, deletions, repository HEAD and its current branch, and nested repository HEAD changes.
  Unrelated ref updates are recorded as diagnostics. Gitlinks compare their
  recorded index commit. Without `--dir`, aid captures and uses its current working
  directory. Changes outside artifact and aid-owned paths follow the
  [read-only settlement rule](dispatch-delivery.md#native-codex-permissions). Files
  stay in place; aid does not commit, stash, revert, or automatically retry the
  violation. Absolute artifact paths work with relative run directories; a
  symlink artifact does not exempt its destination. Enforcement covers Git run
  directories only: a non-Git directory proceeds with a warning event naming
  the reason. Git errors or unavailable Git snapshots fail enforcement. Ignored
  untracked files and paths outside the repository are outside this check.
  Runs from a subdirectory still compare the repository top level. Repositories
  without commits record an unborn HEAD; tracked paths replaced with special
  files record metadata without opening them. A violation records one failure
  reason and marks verification skipped. With `--audit`, a preserved report
  still receives cross-audit post-processing and its verdict is recorded.
  Nested repositories compare commits, not their uncommitted files.
  Codex uses one native permission profile for fresh and resumed sessions. Read-only grants
  are canonicalized and rejected when they overlap the repository, worktree, or Git common directory.
  See [native Codex permissions and artifact delivery](dispatch-delivery.md#native-codex-permissions).
  Claude retains Bash for audit commands, alongside Read, Glob, and Grep. Without
  a result file it uses plan mode and denies Write, Edit, MultiEdit, and
  NotebookEdit. With a result file it uses `dontAsk`, allows Write, and denies
  Edit, MultiEdit, and NotebookEdit. Bash and Write are not restricted to specific
  paths by this adapter; the post-run guard enforces the artifact exceptions.
  Read-only runs never bypass Claude permissions.
- `--remote-build [BOX]` routes Cargo build/check/test/clippy/bench/doc through a PATH shim to rbox while the agent, PTY, isolated HOME, steer/respond, and idle detection stay local. Bare flag or `auto` runs `rbox pick --tool rust --repo <main-working-tree>` once before launch; an explicit name is used as-is. Missing rbox or no available box fails before launch, preserving selection stderr. CLI overrides project `remote_build`; batch defaults and tasks accept the same key. The chosen box is saved with the task, shown by `aid show`/`--json` and events, reused by `aid retry`, and exported as `AID_BUILD_BOX` to agent and verify, overriding ambient values. Verify receives the same PATH shim, so plain `cargo test` runs remotely too. For verification routed through a saved remote box, confirmed pre-command rbox failures (offline, stamp/ensure refusal, sync failure, lock refusal, or disk admission) record `infrastructure_failure`, preserving Done with an unverified infrastructure outcome and the untruncated diagnostic in `aid show` and board. Local shell verification retains ordinary failure handling even if its output contains rbox diagnostics. Remote classification requires an anchored diagnostic and no completion marker; numeric exits alone and submodule warnings do not qualify. Only a saved auto-picked box may re-pick once with `--exclude <failed-box>` and persist the replacement within the original verify deadline. Named and legacy boxes stay pinned. Sync failures and jobs reported still running or lost mid-job are never replayed on another host. Completed compiler/test failures remain failed verification and retain agent auto-retry. A failed re-pick or second pre-command refusal remains infrastructure failure; verification never falls back to local compilation. Remote tasks also get a private `.cargo/bin/cargo` shim so login-shell profiles cannot bypass routing by prepending `$HOME/.cargo/bin`; the other Cargo entries link to the real home and `CARGO_HOME`/`RUSTUP_HOME` stay unchanged. Tasks without a box retain the plain `.cargo` symlink. Tasks without remote build omit `remote_build` from `aid show --json`. Expect sync/lock latency; do not bypass the shim. `cargo fmt` and other Cargo commands stay local. This flag conflicts with `--sandbox` and `--container`.
- `--sandbox` requests sandboxed execution. Before native agent launch, aid creates and probes the Rust target (`_base` for tasks without `-w`) and private temporary directory under isolated HOME, exported as `TMPDIR`; failure aborts with an error naming the directory. Codex roots and Copilot's allowed directories include both scratch paths and writable Git metadata for regular repositories and linked worktrees. An unwritable Git directory is omitted with a task event naming the directory and reason. Capability preflight and `--dry-run` do not create or probe these directories.
- `--timeout SECS` is a hard wall-clock cap in seconds.
  Values below 300 seconds warn about the unit without rejecting the run:
  `--timeout 90` suggests `--timeout 5400` if 90 minutes was intended.
  For caps of at least 600 seconds, the first hard-cap reconciliation poll at
  90% queues one steer asking the agent to commit permitted work in progress
  and write its report. A milestone records the request. Agents without steer
  support and read-only tasks receive nothing; a pending operator steer is preserved.
  Failed signal publication releases the request for retry. The cap stays unchanged.
- `--idle-timeout SECS` stops a task whose stream goes quiet. Meaningful raw
  output refreshes the clock even when aid cannot parse it into an event.
  After first output, growing descendant CPU time and live `ssh`/`rbox` waits
  in the agent's process group also count. Sleeping children without CPU growth
  remain idle; remote waits remain bounded by the absolute task deadline. Aid's
  own idle nudges, PTY echoes of those nudges, reply/ack bookkeeping, and pure
  terminal-control noise do not reset the idle clock.
- `--audit` runs the configured cross-audit.
- `--result-file` requires a durable result artifact.
- `--output` selects a task output path.

For remote Cargo work, use `aid run codex "Implement the change" --remote-build --worktree feat/change --dir . --bg`. Rbox sync includes untracked files and uses the same sanitized repo/branch checkout and remote target as `scripts/remote-test.sh`; the local `CARGO_TARGET_DIR` is never forwarded. Cargo runs from the caller's same relative subdirectory within the synced checkout. Shim defaults are `AID_BUILD_JOBS` or 4 jobs, 3600 seconds timeout and 900 seconds lock timeout. A stderr heartbeat every 60 seconds keeps silent builds visible to the idle watcher. For pre-command failures, rbox diagnostics distinguish lock refusal (75), a job still running on the box (124), and disk admission refusal (69). A completed command can return any of these numbers; use the completion marker and diagnostic rather than the number alone.

For a task with a saved remote box, the verification wrapper uses the remaining
resolved task duration: saved `TimeoutPolicy.max_duration` bounded by `hard_cap`,
minus wall time since `started_at` (or `created_at` if not started). Future
timestamps are clamped to the first verification attempt's current time. Each
attempt, including command preparation, the verifier lock and a disk-admission
re-pick, consumes the same deadline. The wrapper rechecks it before launch and
uses only the remaining allowance when waiting for the command. An
exhausted deadline records `timed_out` without launching a verification command
or remote job. This wrapper deadline is separate from rbox's own wait timeout;
a wrapper timeout does not establish the remote job's test result. Legacy tasks
without a saved remote box and local verification retain the 120-second cap.
Duration-based verification excludes recognized rbox lock notices until the
sync/job-start marker, bounded by the lock notice's timeout. This exclusion does
not extend the absolute deadline for tasks with a saved remote box.

Run `aid run --help` for iteration, evaluation, judging, peer review, best-of,
model, budget, context, scope, checklist, skill, template, hook, container, and cascade options.

`--best-of N` races the top N launchable builtin `aid advise` candidates for the
declared profile, kind, team and caller pool. Unavailable, disabled, held (including
the selected model's group), auth-failed, weaker-caller-pool and
below-floor routes do not race; Claude requires team preference. Custom advice
uses a separate scale and is excluded. Fewer launchable candidates cycle to N;
none produces an error before creating racer tasks.

Each racer uses its advised model, overriding even a same-agent parent's
`--model`. Known CLI defaults are pinned to retain model agreement; unavailable
selected models fail that launch instead of using another default. Unknown
defaults pass no model and suppress later budget routing. Saved model provenance
is AID-resolved. Agent switches clear the parent's session; same-agent sessions
remain. Output/result paths, candidate task IDs and winner finalization retain
their usual best-of behavior. Missing profile dimensions use the advice defaults
(moderate / standard / normal / standard) and are recorded on every racer.

Outside best-of, missing task-profile dimensions produce one warning and persist as null. Projects
with `require_task_profile = true` reject incomplete runs; the production profile
enables this requirement.

## Task Execution Isolation

When dispatches are executed, `aid` isolates the agent process's `HOME` directory to prevent identity and instruction leaks from the orchestrator (e.g. `~/.claude/CLAUDE.md`, `~/.claude/settings.json`):

- **Isolated Per-Task HOME**: At dispatch time, `HOME` is set to an isolated directory created under the task directory (`<task_dir>/home`).
- **Default-Allow Symlink Policy**: Every top-level entry in the host `$HOME` (e.g. `.cargo`, `.rustup`, `.gitconfig`, `.ssh`, `.grok`, `.cursor`, `.codex`, etc.) is symlinked into the isolated `HOME` so development toolchains and CLI auth directories function without interruption.
- **Orchestrator Surface Masking**: `.claude` is rebuilt without its instruction and permission entries (`CLAUDE.md`, `settings.json`, `settings.local.json`, `skills`, `agents`, `commands`, `plugins`, `hooks`, `memory`, `agent-memory` and the rest of that list in `src/agent/home_isolation.rs`); the names in `DEFAULT_DENYLIST` (`src/agent/home_isolation.rs`) are not linked. Other entries, including `.claude.json`, are linked.
- **Config Redirect Variables Removed**: Inherited per-CLI variables that move an agent's config or state away from `$HOME` (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `OPENCODE_CONFIG_DIR`, `GROK_HOME`, `FACTORY_HOME_OVERRIDE` and the rest of `CONFIG_REDIRECT_VARS` in `src/agent/env_redirects.rs`) are removed from the host agent environment, including PTY launches. A value set by batch `env` or `env_forward` is kept. Exception: a non-sandbox, non-container Codex launch uses the durable Codex home, which is the operator's `CODEX_HOME` when set, else `~/.codex`. Sandbox and container wrappers do not unset variables inside the container, and container agents run with `HOME=/root`. CLI probes such as `--help` and `models` are not task launches and keep the caller's environment.
- **Forwarded Values Resolved at Launch**: batch `env_forward` names are resolved when the agent launches, so aid saves only the names. Background job specs, which hold inline batch `env` values, are written owner-only (`0600`). Anything the agent itself prints, including an environment value, still reaches its task log.
- **Automatic Lifecycle Cleanup**: The isolated `HOME` directory is created per task and automatically cleaned up upon task execution completion.

## Further execution references

- [Routing previews and context](dispatch-routing.md)
- [Result delivery, worktree safety, delegation, and builds](dispatch-delivery.md)
- [Retries, fallback, and verification](dispatch-retry.md)

### Removed agent names

`gemini` was removed in this release. Naming it as an agent in
CLI dispatch, batch TOML, team preferences/defaults/overrides, explicit agent
configuration commands, or cascade/fallback lists fails with migration guidance: use `agy`, a Google
provider through `opencode`/`kilo`, or a custom agent with a different name.
Google and Qwen model IDs served by supported agents remain available.
Stored tasks, events, quota markers, and statistics keep their original agent names.
`aid ask`, `aid show --explain`, and bare `--judge` default to `agy`.
Container launches retain `.gemini` for agy authentication and mount `.qwen` for Qwen authentication.
`GEMINI_CLI_HOME` is no longer managed as a built-in CLI config redirect.
Stale `[gemini]` tables in `~/.aid/agent_config.toml` are ignored
without modifying the file. A warning names the file and key once per key per
process; supported agents keep their configured defaults.

Qwen Code is a built-in agent (`aid run qwen "Review the module"`). Its own
ModelStudio token-plan refusal signatures apply only to Qwen. Provider-attributed
429/rate-limit errors create transient holds; Google-specific refusal texts do
not hold Qwen. Unsplit terminal refusals cannot establish long or manual holds.
