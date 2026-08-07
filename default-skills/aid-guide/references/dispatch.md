# Dispatch and Execution

## Tools and skills are the caller's choice

aid does not choose either on the caller's behalf.

- **Skills**: `--skill <name>` declares them, `--no-skill` declares none, and a project sets a
  default once with `skills = ["implementer"]` in `.aid/project.toml`. Omitting all three means no
  skill. aid previously picked one from the **agent kind alone**, never looking at the task, so
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
- `--budget` declares the eligible `free`, `cheap`, `standard`, or `premium` model tier.
- `--urgency` declares `background`, `normal`, or `urgent` rate-limit handling.
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
- `--verify [COMMAND]` verifies completion; without a value it uses project
  configuration or supported defaults.
- `--retry N` permits new attempts after failure.
- `--bg` returns the task ID immediately.
- `--read-only` forbids modifying the repository under test; the task result
  file and audit report remain writable.
- `--sandbox` requests sandboxed execution.
- `--timeout SECS` is a hard wall-clock cap in seconds.
- `--idle-timeout SECS` stops a task whose stream goes quiet. Meaningful raw
  output refreshes the clock even when aid cannot parse it into an event; aid's
  own idle nudges, PTY echoes of those nudges, reply/ack bookkeeping, and pure
  terminal-control noise do not reset the idle clock.
- `--audit` runs the configured cross-audit.
- `--result-file` requires a durable result artifact.
- `--output` selects a task output path.

Run `aid run --help` for iteration, evaluation, judging, peer review, best-of,
model, budget, context, scope, checklist, skill, template, hook, container, and
cascade options.

Missing task-profile dimensions produce one warning and persist as null. Projects
with `require_task_profile = true` reject incomplete runs; the production profile
enables this requirement.

## Task Execution Isolation

When dispatches are executed, `aid` isolates the agent process's `HOME` directory to prevent identity and instruction leaks from the orchestrator (e.g. `~/.claude/CLAUDE.md`, `~/.claude/settings.json`):

- **Isolated Per-Task HOME**: At dispatch time, `HOME` is set to an isolated directory created under the task directory (`<task_dir>/home`).
- **Default-Allow Symlink Policy**: Every top-level entry in the host `$HOME` (e.g. `.cargo`, `.rustup`, `.gitconfig`, `.ssh`, `.gemini`, `.grok`, `.cursor`, `.codex`, etc.) is symlinked into the isolated `HOME` so development toolchains and CLI auth directories function without interruption.
- **Orchestrator Surface Denylist**: Orchestrator-scoped instruction files and permission configurations (such as `.claude` and `.claude.json`) are denylisted and excluded from the symlinked environment.
- **Automatic Lifecycle Cleanup**: The isolated `HOME` directory is created per task and automatically cleaned up upon task execution completion.

## Preview routing without dispatch

```bash
aid advise "Refactor the scheduler" \
  --difficulty complex --budget premium --urgency urgent --rigor critical \
  --kind refactoring --top 5 --json
```

`aid advise` requires all four declared dimensions. It reads the live inventory,
rate-limit markers, team preferences, and task history, then runs the production
selector without launching an agent or writing the task store. Use `--top 0` for
all candidates, `--team` for team preferences, and omit `--json` for a concise
human-readable breakdown. Missing the declared capability floor or budget is a
ranking penalty, not a hard gate: alternatives still appear with an exclusion
reason such as `base 6 < floor 8 for complex`. Custom agents are reported
separately because their configured capability values are not on the built-in
score scale. Inferred kind is advisory; pass `--kind` when the caller knows the
task kind. Advice exits successfully even when every agent is rate-limited.

`aid run auto` and batch `agent = "auto"` (or an empty agent) are hard errors.
There is no silent routing shim: declare a task profile, run `aid advise`, then
dispatch an explicit agent.

## Context and instructions

Use `--context <path>...` for source material, not extra positional arguments.
Use `--context-from <task>...` to inject prior task output. Use `--scope` to
state intended files. Use `--checklist` or `--checklist-file` for explicit
acceptance criteria.

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

Unsupported agent and flag combinations (for example `qwen` with `--read-only`)
are refused before a task row is created, with an error that names what to do
instead. Unknown models that the agent CLI does not report as invalid are passed
through.

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
cannot read or pollute the developer's `~/.aid/`. It is opt-in, not the default.

Inherited `CARGO_TARGET_DIR` wins for the first attempt. If cargo cannot write
that directory (common under agent OS sandboxes that only allow the worktree
and temp dirs), `aid build` / `aid test` retries once under the system temp
directory (`aid-build-target/<project-key>/`) and records the fallback paths in
the digest. Do not preflight with a generic write probe — the fallback is keyed
off cargo's real permission error.

## Retry and fallback

```bash
aid retry <task-id> --feedback "Address the failed invariant"
aid run codex "Task" --cascade opencode,cursor
```

A retry is a new attempt linked to its parent. It does not erase or rewrite the
failed attempt. Inspect the tree with `aid tree <task-id>`.

When `--cascade` is omitted and the primary agent is rate-limited or hits a
quota/auth dead path, aid auto-cascades to the best installed peer for the
task category (capability matrix), skipping rate-limited, disabled, not
installed, and known-unhealthy agents (for example gemini when `agy` is
present). A frontend task falling off codex prefers cursor; a research task
prefers agy — not gemini.

## Verify before review

```bash
aid wait <task-id>
aid show <task-id> --summary
aid show <task-id> --diff
aid show <task-id> --result
```

Agent success and verification are evidence for review, not acceptance.
