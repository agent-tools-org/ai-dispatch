# Dispatch and Execution

## Tools and skills are the caller's choice

aid does not choose either on the caller's behalf.

- **Skills**: `--skill <name>` declares them, `--no-skill` declares none, and a project sets a
  default once with `skills = ["implementer"]` in `.aid/project.toml`. Omitting all three means no
  skill. A `--read-only` run skips the project default; an explicit `--skill` still applies. aid previously picked one from the **agent kind alone**, never looking at the task, so
  every implementation CLI was handed `implementer` and gemini and agy were handed `researcher`
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
  (`cli_config`: codex `config.toml`, qwen settings), else `agent_default` with
  no model. Only `explicit`, `sticky`, `smart_route`, and `budget_route` are
  pinned: aid passes the model to the CLI. `cli_config` and `custom_forced` are
  reported but not passed by aid.
  Every dispatch reports its effective model and source (`--model`, agent config,
  budget route, `CLI config (no -m)`, or `CLI default (no -m)`). Quota/budget
  routing overrides and existing adapter defaults (Cursor, Qwen, MiMoCode) are
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
- `--verify [COMMAND]` verifies completion; without a value it uses project
  configuration or supported defaults. A task with verification configured is
  written with `verify_status = pending` at dispatch, so `pending` means that a
  result is in flight. Verification is skipped (not failed) when the task is
  `--read-only` or has no working directory. An empty diff is not a skip —
  delivery assessment records `empty_diff`, and a configured verify still runs
  against the tree. A verify timeout is recorded as `timed_out`, and a verify
  tooling failure without a compiler or test diagnostic is recorded as
  `infrastructure_failure`; both are inconclusive. A finished verify command
  with a non-zero diagnostic result is `failed`.
- `--retry N` permits new attempts after failure.
- `--bg` returns the task ID immediately.
- `--read-only` compares the Git run directory with its dispatch snapshot after
  the agent exits, including content changes to already dirty files, new untracked
  files, deletions, and nested repository HEAD changes. Gitlinks also compare their
  recorded index commit. Without `--dir`, aid captures and uses its current working
  directory. Any change outside the designated result file, `-o/--output` file,
  and aid-owned paths makes the task Failed with an error listing paths. Files
  stay in place; aid does not commit, stash, revert, or automatically retry the
  violation. Absolute artifact paths work with relative run directories; a
  symlink artifact does not exempt its destination. Enforcement covers Git run
  directories only: a non-Git directory proceeds with a warning event naming
  the reason. Git errors or unavailable Git snapshots fail enforcement. Ignored
  untracked files and paths outside the run directory are outside this check;
  from a repository subdirectory, only that subtree is compared. The run
  repository's own HEAD, branches, and tags are not compared, so a commit of
  already staged changes is not detected. An embedded repository without
  commits fails the snapshot.
  Nested repositories compare commits, not their uncommitted files.
  Claude retains Bash for audit commands, alongside Read, Glob, and Grep. Without
  a result file it uses plan mode and denies Write, Edit, MultiEdit, and
  NotebookEdit. With a result file it uses `dontAsk`, allows Write, and denies
  Edit, MultiEdit, and NotebookEdit. Bash and Write are not restricted to specific
  paths by this adapter; the post-run guard enforces the artifact exceptions.
  Read-only runs never bypass Claude permissions.
- `--remote-build [BOX]` routes Cargo build/check/test/clippy/bench/doc through a PATH shim to rbox while the agent, PTY, isolated HOME, steer/respond, and idle detection stay local. Bare flag or `auto` runs `rbox pick --role rust-build --repo <main-working-tree>` once before launch; an explicit name is used as-is. Missing rbox or no available box fails before launch, preserving selection stderr. CLI overrides project `remote_build`; batch defaults and tasks accept the same key. The chosen box is saved with the task, shown by `aid show`/`--json` and events, reused by `aid retry`, and exported as `AID_BUILD_BOX` to agent and verify, overriding ambient values. Verify receives the same PATH shim, so plain `cargo test` runs remotely too. If the pinned box refuses admission on disk space (exit 69), `aid` re-picks once excluding the old box, persists the new box, and re-runs verify once. A failed re-pick or a second refusal results in an `InfrastructureFailure`. Remote tasks also get a private `.cargo/bin/cargo` shim so login-shell profiles cannot bypass routing by prepending `$HOME/.cargo/bin`; the other Cargo entries link to the real home and `CARGO_HOME`/`RUSTUP_HOME` stay unchanged. Tasks without a box retain the plain `.cargo` symlink. Tasks without remote build omit `remote_build` from `aid show --json`. Expect sync/lock latency; do not bypass the shim. `cargo fmt` and other Cargo commands stay local. This flag conflicts with `--sandbox` and `--container`.
- `--sandbox` requests sandboxed execution. Before native agent launch, aid creates and probes the Rust target (`_base` for tasks without `-w`) and private temporary directory under isolated HOME, exported as `TMPDIR`; failure aborts with an error naming the directory. Codex roots and Copilot's allowed directories include both scratch paths and writable Git metadata for regular repositories and linked worktrees. An unwritable Git directory is omitted with a task event naming the directory and reason. Capability preflight and `--dry-run` do not create or probe these directories.
- `--timeout SECS` is a hard wall-clock cap in seconds.
- `--idle-timeout SECS` stops a task whose stream goes quiet. Meaningful raw
  output refreshes the clock even when aid cannot parse it into an event; aid's
  own idle nudges, PTY echoes of those nudges, reply/ack bookkeeping, and pure
  terminal-control noise do not reset the idle clock.
- `--audit` runs the configured cross-audit.
- `--result-file` requires a durable result artifact.
- `--output` selects a task output path.

For remote Cargo work, use `aid run codex "Implement the change" --remote-build --worktree feat/change --dir . --bg`. Rbox sync includes untracked files and uses the same sanitized repo/branch checkout and remote target as `scripts/remote-test.sh`; the local `CARGO_TARGET_DIR` is never forwarded. Cargo runs from the caller's same relative subdirectory within the synced checkout. Shim defaults are `AID_BUILD_JOBS` or 4 jobs, 3600 seconds timeout and 900 seconds lock timeout. A stderr heartbeat every 60 seconds keeps silent builds visible to the idle watcher. Exit 75 means the box lock was not acquired; exit 124 means the job is still running on the box; exit 69 means the box refused admission due to disk space constraints.

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

Run `aid run --help` for iteration, evaluation, judging, peer review, best-of,
model, budget, context, scope, checklist, skill, template, hook, container, and cascade options.

`--best-of N` races the top N launchable builtin `aid advise` candidates for the
declared profile, kind, team and caller pool. Unavailable, disabled, held (including
the selected model's group), auth-failed, superseded, weaker-caller-pool and
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

## Completion judgment and exit status

`TaskStatus` answers lifecycle and integration: whether a task is running, has
delivered artifacts, or has been merged. `VerifyStatus` answers what happened
to verification. Delivery assessment records empty-diff / hollow / missing-final
observations. `TaskOutcome` is derived from those facts and is the only axis
that answers whether the task succeeded.

The terminal outcomes are:

- `Verified`: delivered and verification passed.
- `Delivered`: delivered without required verification.
- `Broken`: delivered but verification failed.
- `Unverified`: delivered, but verification was inconclusive because it timed
  out, failed as infrastructure, produced no result, or the agent exited
  unobserved because no completion event survived the agent exit (kill and
  success cannot be told apart).
- `Failed`, `Stopped`, and `Skipped`: the task did not produce a successful
  delivery; `InProgress` is non-terminal.

Only `Verified` and `Delivered` are success outcomes. Hollow or missing-final
delivery assessments demote an otherwise successful outcome to `Failed` so
empty runs do not inflate `aid stats` / `aid advise`. `empty_diff` alone does
not demote. A foreground `aid run` exits 0 only for success outcomes; all other
outcomes use a non-zero exit. Do not read `Done` or `Merged` as success without
checking the outcome.

When `aid wait` observes `verify_status = pending`, it
continues waiting for verification, bounded by the verification timeout. Once
the task settles, either command returns non-zero if the outcome is not a
success outcome.

## Task Execution Isolation

When dispatches are executed, `aid` isolates the agent process's `HOME` directory to prevent identity and instruction leaks from the orchestrator (e.g. `~/.claude/CLAUDE.md`, `~/.claude/settings.json`):

- **Isolated Per-Task HOME**: At dispatch time, `HOME` is set to an isolated directory created under the task directory (`<task_dir>/home`).
- **Default-Allow Symlink Policy**: Every top-level entry in the host `$HOME` (e.g. `.cargo`, `.rustup`, `.gitconfig`, `.ssh`, `.gemini`, `.grok`, `.cursor`, `.codex`, etc.) is symlinked into the isolated `HOME` so development toolchains and CLI auth directories function without interruption.
- **Orchestrator Surface Masking**: `.claude` is rebuilt without its instruction and permission entries (`CLAUDE.md`, `settings.json`, `settings.local.json`, `skills`, `agents`, `commands`, `plugins`, `hooks`, `memory`, `agent-memory` and the rest of that list in `src/agent/home_isolation.rs`); the names in `DEFAULT_DENYLIST` (`src/agent/home_isolation.rs`) are not linked. Other entries, including `.claude.json`, are linked.
- **Config Redirect Variables Removed**: Inherited per-CLI variables that move an agent's config or state away from `$HOME` (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `GEMINI_CLI_HOME`, `OPENCODE_CONFIG_DIR`, `GROK_HOME`, `FACTORY_HOME_OVERRIDE` and the rest of `CONFIG_REDIRECT_VARS` in `src/agent/env_redirects.rs`) are removed from the host agent environment, including PTY launches. A value set by batch `env` or `env_forward` is kept. Exception: a non-sandbox, non-container Codex launch uses the durable Codex home, which is the operator's `CODEX_HOME` when set, else `~/.codex`. Sandbox and container wrappers do not unset variables inside the container, and container agents run with `HOME=/root`. CLI probes such as `--help` and `models` are not task launches and keep the caller's environment.
- **Forwarded Values Resolved at Launch**: batch `env_forward` names are resolved when the agent launches, so aid saves only the names. Background job specs, which hold inline batch `env` values, are written owner-only (`0600`). Anything the agent itself prints, including an environment value, still reaches its task log.
- **Automatic Lifecycle Cleanup**: The isolated `HOME` directory is created per task and automatically cleaned up upon task execution completion.

## Preview routing without dispatch

```bash
aid advise "Refactor the scheduler" \
  --difficulty complex --budget premium --urgency urgent --rigor critical \
  --kind refactoring --top 5 --json
```

`aid advise` requires all four declared dimensions. It reads the live inventory,
rate-limit markers, aidbar disk snapshots, team preferences, and task history,
then runs the production selector without launching an agent or writing the task store.
Fresh live used-percent ranks remaining headroom (a penalty as the window
fills; unused quota never boosts). Held routes still take today's −10 when
urgency is not `background`. Quota penalties and candidate quota use the resolved
model’s metered group; a hold on another group does not penalize that route. Use `--top 0` for all candidates, `--team` for
team preferences, and omit `--json` for a concise human-readable breakdown
(including a headroom term). JSON candidates add a `quota` object (`status`,
`wall`, `used_percent`, `resets_at`, `freshness_secs`, `stale`, `source`)
without renaming existing keys. `quota.status` is `unknown` when no successful
probe observed the quota (`source: "none"`, a failed probe, or an expired
marker); it is never reported as dispatchable-by-evidence without one.

Eligibility uses the same "can this route run" predicate as `aid agent list`
and the `aid run` preflight: a candidate whose binary is missing from `PATH` is
`installed: false, eligible: false` with reason
`not installed: binary '<bin>' missing from PATH`. The binary is the one the
adapter would spawn: Cursor counts an `agent` on `PATH` only when it identifies
as Cursor, else it needs `cursor-agent`. Every ineligible candidate
carries `exclusion_reason` (human text, `; `-joined) and `exclusion_codes`
(one stable code per reason):

| Code | Reason text |
|---|---|
| `not_installed` | `not installed: binary '<bin>' missing from PATH` |
| `below_floor` | `base 6 < floor 8 for complex` (team override or rated model capability below the floor) |
| `no_budget_model` | `no model for budget <budget>` |
| `auth_failed` | `auth failed (observed <time>)` |
| `weaker_on_caller_pool` | `weaker model on caller's pool` |

Candidates rank eligible first, then eligible-but-demoted, then ineligible;
ineligible alternatives still appear with their reasons. The recommendation is
the first eligible installed route permitted for recommendation; fallback routes
must also be installed, enabled, and outside weaker-caller-pool exclusions. When
none remain, `recommended` is `null` and the run hint stays silent. Claude stays listed and requires team preference for advice. Advice has no
declared-agent field: explicit `aid run claude` selects Claude for execution
without changing the hint’s advice profile. Each candidate also carries `auth`: `state` is
`failed` (with `observed_at` and `message`) when a run of that agent ended on a
recognised not-signed-in refusal within the last hour (grok `Not signed in`,
claude `Please run /login` / `Not logged in`, oz `credentials are invalid`),
otherwise `unknown`. A successful run clears it; aid never reports `ok` and
never marks auth failed from a run or probe that could not start.

Leaderboard evidence: `aid advise` (text and JSON) shows each candidate's
`capability_evidence`: `canonical_id`, all raw `scores`, the `selected` score,
`capability` (0–10 or `null`), and `harness` (`measured` or `harness unmeasured`).
Each score preserves `source`, `board`, `cli`, `effort`, `value`, `unit`, `rank`,
`of`, `ci_low`, `ci_high`, and `date`; missing effort/rank/CI stays `null`.
The report's `sources` block preserves each source's `ok`, `count`, `updated_at`,
`url`, `licence`, `attribution`, and optional `error` (`null` when absent).
Custom candidates also expose raw evidence; their configured scoring scale stays separate.

Scoring chooses one source, never a blend: simple-edit/complex-impl/debugging/
testing/refactoring use Terminal-Bench 4.0 accuracy for the exact canonical model,
CLI, and configured effort, falling back to Epoch ECI. Frontend uses LMArena
webdev; research/documentation use Epoch ECI. Price-feed exact IDs and aliases
provide the canonical mapping; there is no fuzzy model matching. Terminal-Bench
results from another CLI or effort do not rate this harness: fallback evidence
is labelled `harness unmeasured`. Codex effort comes from `model_reasoning_effort`
in its configured home; Claude uses `CLAUDE_CODE_EFFORT_LEVEL` then `effortLevel`
in its user settings. Unobserved effort matches only an effort-less benchmark row.
No applicable data (including a failed source) is unknown, never a fabricated zero.

The sole linear rescale is `10 * (value - min) / (max - min)`: Terminal-Bench
accuracy uses its 0–100 percent (or 0–1 rate) range; ECI and webdev use the
observed model-level min/max of their own source, board, and unit in the snapshot.
A degenerate range is unknown. A measured minimum may legitimately score zero.
Free/cheap budget selection keeps the lowest-price rule and catalog order for
price ties; standard/premium
compare model-level ECI within each preferred tier, with unknown ties keeping catalog order.
`breakdown.complexity_bonus` was removed; no CLI gets an automatic +2 for complexity.
`scores.json` shares the price cache directory and 24-hour TTL. Refresh is
out of band; failed, malformed, empty, or server-stale fetches preserve the old
cache, which remains usable offline. Without a valid cache, evidence is unknown.

Caller pool: advise reads the calling session (`AID_CALLER_KIND`, Claude Code,
Codex; see `aid board --mine`) and the caller's own model from
`--caller-model <model>` (wins) or `AID_CALLER_MODEL`. Claude Code maps to the
`anthropic` pool and Codex to `openai-chatgpt-plan`. The JSON report adds
`caller` (`session`, `agent`, `provider`, `model`, `capability`). A candidate on
the same pool whose recommended model has a lower leaderboard capability than the
caller's model is excluded as `weaker model on caller's pool`. When either
capability is unknown, same-pool candidates stay eligible but carry
`demotion_reason` and rank below every eligible other-pool candidate. This is
advice only: an explicit `aid run <agent>` is never blocked by it. Custom agents are
reported separately because their configured capability values are not on the
built-in score scale. A candidate's `model`, `pinned`, and `source` come from
the same resolver `aid run` uses for that declared profile, so `model` is what
`aid run` would launch. For a declared `free` or `cheap` budget it is the
catalog budget model (`source: "budget_route"`, `pinned: true`). A codex
`model = "gpt-6-sol"` in its config makes the candidate `gpt-6-sol` with
`pinned: false` and `source: "cli_config"`. With no readable CLI default,
`model` is `null`, `source` is `agent_default`, and the human output says
`agent default (unknown)`; advise never names a catalog model it will not
launch. A built-in candidate's base is its team override, else its rated model
catalog capability, else a neutral `6.0`. Model capability is the base, not an
averaged adjustment. Without either source of evidence, the floor does not
exclude it and report `notes` includes `unrated: no measured or model capability
for <category>`. Custom agents keep their configured capabilities and floor.
The caller-pool comparison treats unknown model capability as unknown.
When a candidate's model is older
than a served-only model of the same family (for example catalog `gpt-5.6-sol`,
served `gpt-6-sol`), the candidate carries `unrated_served_models` (omitted
when empty) and the human output adds one `note:` line. Those models stay
unrated and are not selected. Inferred kind is advisory; pass `--kind` when the
caller knows the task kind. Advice exits successfully even when every agent
is rate-limited. Advise does not spawn `aidbar`.

For `free` and `cheap` profiles, `breakdown.budget_penalty` is `-3.0` only
when the candidate's resolved model price has a positive input or output rate.
Scoring uses the same price resolution as cost estimation: exact agent/model
override (case-insensitive), subscription inclusion, catalog figure, then an
exact vendor-CLI feed entry. Unknown prices and included subscription routes
have no paid-model penalty, even when the catalog lists API-looking rates.
A zero override removes the penalty from a paid route; a positive override
adds it to a free, subscription, or previously unknown route. Vendor feed
rates do not price reseller routes. `standard` and `premium` profiles have no
paid-model penalty. These price rules do not change model eligibility or the
other score terms.

`aid run` hints use this ranker with the declared profile and task kind. Missing
values default to `moderate` / `standard` / `normal` / `standard` and inferred kind;
keywords never choose budget. Hints stay silent for prompts under 20 characters,
`--no-hint`, or a recommendation matching the chosen agent.

`aid run auto` and batch `agent = "auto"` (or an empty agent) are hard errors.
There is no silent routing shim: declare a task profile, run `aid advise`, then
dispatch an explicit agent.

## Context and instructions

Use `--context <path>...` for source material, not extra positional arguments.
Use `--context-from <task>...` to inject prior task output. If that task
declared `-o` and the owned file is missing, aid reports the absence and does
not substitute the task log. Use `--scope` to state intended files. Use
`--checklist` or `--checklist-file` for explicit acceptance criteria.

Prefer a project verify command for consistent results:

```toml
[project]
id = "example"
verify = "cargo test --bin app"
```

## Result delivery

Prompts expressing `read-only … audit`, including `read-only audit` and
modifiers such as `read-only comparative audit`, `read-only cross-audit`, or
`read-only re-audit`, are dispatched as report tasks from the prompt alone.
`--read-only` still permits writing the task result file and audit report; it
forbids modifying the repository under test. AID auto-selects a task-specific
result file and omits implementation methodology and Git staging instructions.
This prompt formatting decision is independent of dirty-worktree enforcement.
Implementation noun phrases such as `add an audit log` and write requests such as
`add tests for the read-only audit module` or
`make changes to the read-only audit logic` remain normal writable tasks.
Write verbs after the audit phrase also keep implementation scaffolding unless
they are negated, as in `do not modify` or `without modifying`.
An explicit `--result-file` controls report formatting and delivery; it does not
by itself remove implementation methodology or Git staging instructions.
A declared writable kind (`--kind simple-edit|complex-impl|frontend|testing|refactoring`
without `--read-only`) is never turned into a report task by prompt wording.
The kind carries over to `aid retry`, so a brief that discusses audits,
reviews, or findings stays an implementation task on every attempt. To get a
report, declare `--kind research|debugging|documentation` or pass `--read-only`.

Unsupported agent and flag combinations are refused before a task row is created,
with an error that names what to do instead. The same preflight resolves the agent
command (built-in or custom) and refuses when the binary is missing from `PATH`,
naming the missing binary. Qwen supports `--read-only` through its native plan
approval mode.
Unknown models that the agent CLI does not report as invalid are passed
through.

If the agent process still fails to start after preflight, the task ends in
`failed` with an agent-spawn error — it is never left `running` with no worker.

When `--result-file` is set (audit and review prompts set it automatically) and
the agent never writes that file, AID salvages the captured agent output into
the task's `result.md` so evidence is not lost. If that output is pre-tool
narration rather than a report, AID records a `missing_final_delivery`
assessment and an error event. `aid show` then prints the missing-result banner
instead of presenting the tool log as findings. Treat that banner as "no audit
happened" and re-dispatch.

## Worktree safety

```bash
aid worktree create feat/change
aid worktree list
```

AID task worktrees are custody containers, not disposable scratch directories.
Completion, failure, stop, retry, and merge preserve them. Do not use raw
`git worktree prune` or direct `git worktree remove` on them.

If AID reports missing worktree registration or conflicting metadata, stop and
identify the owning task. Automatic pruning is intentionally forbidden because
linked-worktree Git metadata may contain unique submodule objects.

Each worktree holds a `.aid-lock` lease for the active task. Unrelated tasks
still collide and are refused. A nested child whose `parent_task_id` chain
reaches the lease holder may re-enter the same worktree so edits stay on the
parent branch.

Creating a worktree also adds AID's own runtime files — `.aid-*`, `aid-batch-*`,
`result-t-*.md` — to the repository's local `.git/info/exclude`. That file is never
committed and the repository's `.gitignore` is left alone; it exists so an agent
running `git add .` cannot commit AID's lease file. Dirty-worktree enforcement
ignores those paths as well, so AID clearing its own lease at task end is not read
as the agent leaving work uncommitted.

## Recursive delegation

Agents already receive `AID_TASK_ID` (and now `AID_TASK_DEPTH`) and can run
`aid run` from inside a task. When `AID_TASK_ID` is set:

- `aid run` fills `parent_task_id` from it so `aid tree` shows the child.
- Depth is parent depth + 1 and dispatch beyond depth `2` is refused.
- `--bg` is refused; the child must finish before the parent releases the lease.
- Child `--difficulty` / `--budget` may not exceed the parent's declared values.

```bash
# inside an agent process (AID_TASK_ID already set):
aid run opencode "Extract the parser helper" \
  --dir . \
  --worktree feat/request-validation \
  --difficulty simple --budget cheap --urgency normal --rigor draft
```

## Build and test

```bash
aid build check
aid build clippy -- --all-targets
aid test --bin aid
aid test --bin aid my_module::my_test -- --exact
aid test -- my_filter
aid test -- my_filter --exact
aid test --isolated --bin aid
```

Use `aid build` for compile checks. It emits compact, deduplicated diagnostics
and integrates progress into task events. `aid build` no longer accepts `test`
as a command — use `aid test` so agents cannot mistake a zero-match filter or
empty target set for a green suite.

`aid build` guarantees:

- A run that matched no build targets never looks like a pass (for example
  `cargo check --lib` on a binary-only crate).
- A cached no-op build (cargo exit 0, everything already fresh) is still success.
- Task events say `succeeded` or `failed`, not an ambiguous `finished` line.

`aid test` reuses the same cargo process supervision and diagnostic pipeline as
`aid build`, then parses libtest stdout. Guarantees:

- A filter that matches zero tests exits non-zero and names the filter.
  Filters may be positional (`aid test name`) or free args after `--`
  (`aid test -- name`), matching cargo muscle memory.
- Target selectors are aid flags only: `--lib`, `--bin NAME`, `--test NAME`
  (integration-test *target*, not a name filter). Do not put them after `--`.
- A run with no test targets never looks like a pass
- The digest lists which tests ran (not only a pass count)
- Failure output stays compact (panics and assertion diffs)

`--isolated` gives the cargo test process a temporary `AID_HOME` so the run
cannot read or pollute the developer's `~/.aid/`. It also clears nested
`AID_TASK_ID` / `AID_TASK_DEPTH` in that child so unit tests that call
`prepare_dispatch` are not refused as over-depth when `aid test` itself runs
inside a task. It is opt-in, not the default.

Inherited `CARGO_TARGET_DIR` wins for the first attempt. If cargo cannot write
that directory (common under agent OS sandboxes that only allow the worktree
and temp dirs), `aid build` / `aid test` retries once under the system temp
directory (`aid-build-target/<project-key>/`) and records the fallback paths in
the digest. Do not preflight with a generic write probe — the fallback is keyed
off cargo's real permission error.

## Retry and fallback

```bash
aid retry <task-id> --feedback "Address the failed invariant"
aid retry <task-id> --feedback-file notes.md --model gpt-5.4 --idle-timeout 900
aid run codex "Task" --cascade opencode,cursor
```

A retry is a new attempt linked to its parent. It does not erase or rewrite the
failed attempt. Inspect the tree with `aid tree <task-id>`. Unspecified
`--model` / `--idle-timeout` keep the original task values; `--feedback` and
`--feedback-file` cannot be combined.

Automatic cascade takes the first launchable builtin candidate in `aid advise`'s
ranked list, using the task's difficulty, budget, urgency, rigor, kind, team,
history and detected caller. It excludes the exhausted agent and always excludes
Claude, even when team-preferred. Advice gates exclude disabled, missing,
auth-failed, below-floor, weaker same-pool and superseded Gemini routes, plus
holds on the candidate's selected model group even under background urgency.
An advisory `recommended` route never rescues an empty launchable set.

The candidate's model is retained through dispatch: known CLI defaults are
pinned, and unknown defaults cannot be replaced by later budget routing. A known
unservable advice model fails before launch rather than running another default.
When served-model evidence is unknown, the exact requested advice pin is allowed.
Advice provenance is persisted for both model pins and unknown defaults.
Explicit cascade/fallback lists keep their order, custom-name resolution,
remaining entries and unknown-name errors. Failed-task batch `auto_fallback`
never replaces an exhausted explicit list with advice. Explicit `aid batch retry`
can use advice when the saved list is empty and the agent is held. Explicit Gemini
and Claude remain allowed when
agy is installed. Cross-agent switches clear the source model and session before
applying the selected route.

A hold diverts dispatch whenever it is still live, whether it ends on a stated
time, a dated aidbar window, or only when a person runs
`aid config clear-limit <agent>` (see `references/configuration.md` for the
four hold classes). A marker whose stated
time has already passed does not divert anything, and neither does the short
cooldown left by an unrecognised refusal — that window is shorter than the cost
of moving off the agent you asked for. Where a tiered agent has only one tier
held — cursor's premium pool, or droid's standard or Core pool — dispatch stays on the
agent and switches to a tier that still serves, reporting the swap rather than
making it silently. A droid refusal that names neither `standard usage` nor
`weekly Droid Core usage limit` holds the whole agent.

Cursor's `ActionRequiredError: You've hit your usage limit` monthly refusal
holds the dispatched model group, including Auto when Auto refused. Its
`M/D/YYYY` cycle-end date holds through that entire UTC day (release at the
following midnight UTC); a missing or invalid date uses a 30-day hold.
Quoted prose and assistant/tool envelopes do not qualify as this error line.

A hold is scoped to what actually refused. When a CLI serves several providers,
a refusal is attributed to the provider of the route aid dispatched, so one
provider running out of credit leaves its siblings dispatchable — an
`opencode/` balance failure does not hold `opencode-go/`. When the refusing
provider cannot be identified, the hold covers the whole agent rather than
guessing a provider.

Model validation distinguishes who chose the model. A model you named with
`--model` that the CLI does not serve is a hard error listing the served
models. A model aid resolved for you — from the catalog, the declared budget,
or a stored per-agent default — is dropped with a warning and the agent's own
default runs instead, because a stale catalog entry is aid's problem to absorb,
not a reason to refuse your dispatch. Where the CLI cannot be asked what it
serves, dispatch proceeds unvalidated and says so.

For explicit fallback substitution — where a held agent was replaced by a
caller-listed fallback before dispatch — an aid-resolved model survives a served-list miss
rather than being dropped. The substitution already proved the requested
family spent, and the fallback's own default can re-enter that exhausted
family (agy's default is a gemini model, the group the hold just escaped), so
the aid-resolved pin is retained through the cache lag. This covers any
aid-resolved effective model on the substituted route, not only the healthy
family pin: `switch_agent` clears `--model`, so a fallback's per-agent default
or a budget-picked model arrives as aid-resolved too, and the same trade
applies. A served-list miss is cache lag, not proof the CLI will reject the
model.

The served list is cached on disk for 24 hours, so a slow CLI is asked once
rather than on every dispatch. When the model you asked for is absent from the
cached list, aid re-probes once before rejecting it, so a model the CLI gained
since the last probe is accepted rather than refused for a day. Codex cache
entries also track the modification time and size of `$CODEX_HOME/models_cache.json`;
when Codex refreshes that file, aid refreshes its disk and in-process model
lists on the next lookup, including in a long-running `aid mcp` process.

`aid agent list --json`, `aid config pricing`, and `aid advise` merge every
model in the 24-hour served-model cache that has no catalog row into the
catalog views, for each agent with a served-model probe: codex, grok, cursor,
qwen, agy, and opencode (so providers such as `opencode-go` appear beside the
built-in `opencode/*` rows). For Codex, a changed source-file stamp refreshes
the cache from its local model list. Other agents read the cache only and never probe;
an absent or expired cache adds nothing. Each `models.available` row carries
`rated`, `capability` (the complex-impl rule, or `null`), `capability_evidence`
(the same raw/selected/harness structure as advice), and `source` (`catalog`,
`served`, or `pricing_override`). The agent-list response has the same `sources`
metadata block as advice. A served-only
row keeps `source: "served"`; `rated` reflects applicable leaderboard evidence, and
`capability` is `null` when unknown. Every
displayed price (`input_per_m`/`output_per_m` in the JSON, `aid config pricing`,
the `aid config agents` model lines) is the price cost estimation resolves below;
with none known the JSON carries `null` and the text prints `unknown`. aid never
invents ratings for models without leaderboard evidence, and routing never auto-selects one.
Cost estimation prices a model only from an exact match, in this order: an
explicit `pricing.json` override for the agent and model, subscription
inclusion, its static catalog row, or an exact price-feed id or alias.
An explicit override also replaces a listed row's tier and description while
preserving its origin; ratings resolve independently from the leaderboard feed.
The price feed carries each vendor's own per-token API rate, so it prices only
the vendor's own CLI (codex, claude, gemini, grok); on a reseller route (droid,
oz, opencode, and the others) a model without a catalog figure or override
costs `unknown`. It never uses a substring, vendor-prefix, family, or
`-free`-suffix match, and never a fixed fallback model: a served-only model
gets its price from an exact price-feed entry on its vendor's CLI, never from a
similar-name rate. A task with neither a pinned nor an observed model (for
example unpinned gemini or codex) costs `unknown`, stored as NULL. Subscription
agents (Cursor, Copilot) cost 0.0 as included.

A catalog row storing 0.0/0.0 is a price only when its tier is `free`; on any
other tier (droid, oz, grok rows) it means "no figure on record" and costs
`unknown`, never free, so a `max_task_cost` ceiling on that route warns that it
cannot be enforced. An override outranks every other source, including
subscription inclusion and the catalog row.

For providers represented by aidbar, a successful cached snapshot can release a
time-based, transient, or Windowed older marker for this dispatch decision only
when its `fetched_at` is newer than the marker file's modification time and
every relevant usage window has headroom. A Windowed hold also requires a
dated `resets_at` on at least one of those windows. Cursor premium's relevant
window is the one labelled `Plan`; On-demand is ignored. A `NeedsHuman` hold
— prepaid, a plan change, or invalid credentials — is never released by
percentages: used-percent readings say nothing about a spend or balance hold
(opencode refused at $19.37 of a $20 window; oz logged-out stderr is
`credentials are invalid`). The marker is not deleted, and aidbar errors, missing
probes, and unrecognized providers do not release it. `aid advise` continues
to score and report the marker state rather than this one-round dispatch view.

The held route is **not spawned**. Substitution happens before dispatch, so no
task row is recorded for the agent that was never run, and the fallback carries
none of the held route's model: a model name means something only inside one
CLI. The substitution is announced on stderr and recorded as an event on the
dispatched task, both naming `aid config clear-limit <agent>`. When no usable
aidbar snapshot can release the marker, that escape hatch is how a topped-up
account or other changed provider state releases a stale hold.

`--urgency background` keeps the agent you asked for when the hold is a clock
or Windowed quota — those can clear before a background task runs. A NeedsHuman
hold (logged-out credentials, prepaid, plan-change) still blocks and substitutes
or errors exactly as under normal urgency. The wait itself blocks only on a
clock or a dated mapped snapshot; prepaid, plan-change, and unmapped holds
return immediately and tell you to `aid config clear-limit` or pick another
agent.

A dry-run substitution milestone says `would dispatch` and records JSON
metadata naming both routes and whether the model class was preserved. It does
not mean the substitute ran.

A `--cascade` entry aid cannot resolve is an error, not a skipped entry. Custom
agents are valid cascade targets and are checked against their own hold, not a
shared one.

## Verify before review

```bash
aid wait <task-id>
aid show <task-id> --summary
aid show <task-id> --diff
aid show <task-id> --result
```

Agent success and verification are evidence for review, not acceptance.
