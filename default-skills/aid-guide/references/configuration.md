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

`[project].id` is the **stable project identity** recorded on every dispatched
task. Main checkout and linked worktrees resolve to the same id. When no
`project.toml` exists, aid falls back to a path-based id of the main working
tree. Outside any git repository, tasks are stored with `project_id` unset
(the unattributed bucket). Historical tasks without a recorded identity stay
unattributed — aid does not invent one after the fact.

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
(not the generic `agent` alias used by cursor). `aid agent config <agent> --model <id>`
writes the per-agent default to `~/.aid/agent_config.toml`. That default is sticky:
`aid run` and `aid batch` use it whenever `--model` / `model =` is omitted, including
when a budget is declared. `--model` always wins. With no configured default, aid
uses the catalog model for the declared budget. Register a local custom agent
with `config add-agent`. Use `clear-limit` only after confirming a provider's
rate-limit condition has cleared. Each custom agent has its own marker keyed on
its id (`rate-limit-<id>`), so one custom hitting quota does not hold the
others; `aid config clear-limit <custom-id>` clears that agent alone.
Built-in markers (`rate-limit-codex`, …) are unchanged.

Custom agent TOML may set `interactive_input = false` when its CLI does not
consume PTY stdin. The field defaults to `true` so existing custom agents keep
their historical steering, reply, respond, and idle-nudge behavior; it is
independent of the `streaming` output setting.

For providers that aidbar probes, dispatch may temporarily treat a
time-based, transient, or **Windowed** older marker as released when a
successful cache snapshot is newer than the marker and every **relevant**
usage window has headroom. A Windowed hold also requires at least one of
those windows to carry a dated `resets_at` — a bare percentage cannot end
it. A `NeedsHuman` hold (prepaid or plan-change) is never released by a
snapshot: used-percent readings say nothing about a spend or balance hold
(opencode refused at $19.37 of a $20 window). The marker remains on disk,
so stale, failed, missing, or unsupported provider readings do not release
it and the normal marker state returns on the next dispatch decision.

`aid advise` and `aid agent quota` may best-effort spawn `aidbar` when a mapped
cache is already stale. They do not run one `aidbar --no-cache` against the
whole provider set (that refresh is sequential and grok's HTTP timeout is 10s).
Until aidbar grows a per-id refresh flag, those commands stay on the disk
cache and do not promise current percents. `AID_QUOTA_REFRESH=0` disables the
spawn. `aid run` never spawns. A snapshot older than 15 minutes is tagged
`STALE` on quota display and is not treated as Held.

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
| `rate-limited (until a dated <provider> snapshot with headroom …)` | a newer dated aidbar window shows headroom, or `clear-limit` | cursor premium `you're out of usage`; grok 402 `usage balance exhausted` (when aidbar probes grok) |
| `rate-limited (needs manual clear: aid config clear-limit <agent>)` | a person acts | spent opencode balance, copilot monthly/premium, gemini `IneligibleTier`; grok 402 with no aidbar probe |

The Windowed class covers refusals that never state a reset time, but whose
wall is a dated billing window aidbar already probes. A percentage alone
cannot end them; `resets_at` must be present. A Windowed hold also requires
aidbar to actually probe the route: when no live snapshot source exists, the
recovery condition is unobservable, so the hold is human-cleared and `aid agent
quota` / `aid advise` name `aid config clear-limit` rather than promising a
dated snapshot that will never arrive. Cursor premium matches the
`Plan` window only — `On-demand` is never relevant for that group, even at
115%. The person-only class is prepaid or a plan change: a guessed cooldown
or a dated spend window would send work back to an account that still cannot
pay. A bare `429`/`402` with no recognised template is Degraded, not a hold:
`aid agent quota` prints OK and dispatch is not diverted. An on-disk
`hold: manual` marker is re-read against the current signature table, so a
Windowed needle written before this class existed still classifies as
Windowed rather than as a person hold.

An agent whose plan splits one allowance into tiers is marked per tier. Cursor
meters a single premium pool that every model except `auto` draws on, so a
premium refusal holds those models while `auto` stays dispatchable;
`aid config clear-limit cursor` clears both. A group hold is not an agent hold:
`aid agent list` and `aid agent quota` report it as `PARTIAL` (still dispatchable
on clear tiers), not `LIMITED` or `OK`. STATUS now matches dispatch: a snapshot
that releases a route for `aid run` also clears LIMITED / PARTIAL. A hold only
a person ends names `aid config clear-limit <agent>`; aid never invents a reset
time it did not observe.

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
