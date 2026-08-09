# Setup and Configuration

## First-time setup

```bash
aid setup
aid config agents
aid project init
aid project show
aid project state
```

`aid setup` creates user configuration and installs bundled resources when the
skills directory is absent or empty. `aid init` is an internal compatibility
entry that can reinstall defaults and always refreshes the official guide.

## Project configuration

Store project defaults in `.aid/project.toml`. Prefer `aid project init` over
writing it from memory. Inspect the effective result with `aid project show`
and `aid project state`. Use `aid project sync` to synchronize supported
project instructions and budgets.

Common project controls include:

- default team and verification command;
- setup command and container image;
- agent and model preferences;
- budget and duration limits;
- GitButler mode;
- worktree naming prefix;
- audit and idle-recovery policy.

Set `require_task_profile = true` to reject `aid run` calls that omit any of
`--difficulty`, `--budget`, `--urgency`, or `--rigor`. The built-in production
profile enables this automatically.

The removed `aid_gc` and `keep_worktrees_after_done` settings are invalid.
Artifact deletion is controlled only by explicit acceptance and custody GC.

## Agents and providers

```bash
aid agent
aid config agents
aid config add-agent local-agent ./run-agent --streaming
aid config clear-limit codex
aid byok --help
aid credential --help
```

Use `aid config agents` to see configured and detected agents. Built-in dispatch
probes binaries by their real CLI names, for example `grok` and `commandcode`
(not the generic `agent` alias used by cursor). Register a local custom agent
with `config add-agent`. Use `clear-limit` only after confirming a provider's
rate-limit condition has cleared. Each custom agent has its own marker keyed on
its id (`rate-limit-<id>`), so one custom hitting quota does not hold the
others; `aid config clear-limit <custom-id>` clears that agent alone.
Built-in markers (`rate-limit-codex`, …) are unchanged.

For providers that aidbar probes, dispatch may temporarily treat a
time-based or transient older marker as released when a successful cache
snapshot is newer than the marker and every reported usage window has headroom.
A `NeedsHuman` hold — the explicit `hold: manual` marker or a refusal whose
text requires rereading — is never released by percentages: used-percent
readings say nothing about a spend or balance hold (opencode refused at $19.37
of a $20 window). The marker remains on disk, so stale, failed, missing, or
unsupported provider readings do not release it and the normal marker state
returns on the next dispatch decision.

Quota exhaustion is read from two named channels and nowhere else: the CLI's
stderr, and the raw lines of its output stream. Within the stream, a refusal is
admitted only from an envelope the CLI itself opened — a structured error event —
never from an assistant message, a tool call, a tool result, or the event text
aid renders for the task board. Those are the model's words or aid's own, and
matching them wrote real holds on providers that were serving.

What may match depends on how strongly the text is attributed. A string inside a
CLI error envelope may match a generic token like `429`, `402` or `rate limit`,
because only the CLI could have put it there. A line with no envelope around it —
plain-text CLIs such as `agy`, and anything running under a PTY, where the
captured buffer is the rendered answer — must match that agent's own captured
refusal template. An agent whose refusal wording has never been captured stays
undetectable, which is the honest answer rather than a guess.

A hold ends in one of three ways, and `aid config agents` names which:

| Status | Ends when | Example |
|---|---|---|
| `rate-limited (try again at <time>)` | that time passes | codex usage limit, qwen token-plan window |
| `rate-limited (needs manual clear: aid config clear-limit <agent>)` | a person acts | spent balance, billing cycle, retired plan tier |
| `rate-limited (cooling down)` | a short cooldown elapses | a bare `429`/`402` with no recognised template |

The middle class covers refusals that never state a reset time and do not return
on a clock — a spent opencode balance, a copilot monthly quota, a cursor premium
pool, grok's exhausted Build balance. These are held until `aid config
clear-limit <agent>` rather than given an invented expiry, because a guessed
cooldown sends work back to a provider that is still refusing it. The last class
is the opposite guard: an unrecognised refusal must not take a route out
permanently, so it expires by itself.

An agent whose plan splits one allowance into tiers is marked per tier. Cursor
meters a single premium pool that every model except `auto` draws on, so a
premium refusal holds those models while `auto` stays dispatchable;
`aid config clear-limit cursor` clears both. A group hold is not an agent hold:
`aid agent list` and `aid agent quota` report it as `PARTIAL` (still dispatchable
on clear tiers), not `LIMITED` or `OK`. A hold only a person ends names
`aid config clear-limit <agent>`; aid never invents a reset time it did not
observe.

Use `aid byok` for custom OpenAI-compatible endpoints. Use `aid credential` to
manage named credential-pool entries; never place secret values in prompts,
task output, committed project configuration, or skills.

## Skills and templates

```bash
aid config skills
aid config prompt-budget
aid config templates
aid run codex "Implement parser" --skill implementer
aid run codex "Fix parser" --template bug-fix
```

Bundled methodology skills are installed under `~/.aid/skills`. The official
`aid-guide` directory is release-managed and refreshed by `aid init`. Put
personal instructions in a different skill name.

Long task prompts receive full skill methodology; short prompts may receive
only compact references to control prompt cost. Use `--no-skill` to suppress
automatic methodology injection.

## Store, tools, and teams

```bash
aid store browse
aid store show publisher/package
aid store install publisher/package
aid store update
aid tool --help
aid team --help
```

Inspect a store package before installing it. Treat installed skills, agents,
and scripts as executable supply-chain inputs.

## Containers, hooks, and MCP

```bash
aid container build aid-dev --file Containerfile
aid container list
aid hook --help
aid mcp
```

Use containers for isolated execution when a task needs reproducible
dependencies. Hooks run commands and therefore require the same trust review as
scripts. Completion hook payloads include additive `outcome` and
`verify_status` fields alongside the existing lifecycle `status`. `aid mcp`
exposes AID operations over stdio JSON-RPC for an MCP host; its task views
likewise include `outcome` and `verify_status`.

## Maintenance

```bash
aid doctor
aid clean --dry-run
aid changelog
aid upgrade
```

`doctor` is diagnostic. It must not prune unaccepted artifacts. Run cleanup in
dry-run mode first. `clean` retains task records and events as custody evidence
and does not replace `aid gc --task`.
