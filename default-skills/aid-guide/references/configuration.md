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

Configuration discovery uses the checkout's own `.aid/project.toml` first; if absent in a linked worktree, it uses the main working tree's `.aid/project.toml`.

`aid run --dir PATH` resolves the target Git root and project config once per dispatch.
Project rules, default skills, state, knowledge, memories, and toolbox lookup use that
resolved context. Relative paths are resolved from the caller's working directory;
omitting `--dir` uses the caller's project. Outside Git, no project context or memories
are injected. A Git repository without `.aid/project.toml` still uses its scoped
memories and toolbox. State and knowledge come from the resolved checkout's `.aid/`.
Explicit `--skill` or `--no-skill` overrides that project's default skills.

`[project].id` is the **stable project identity** recorded on every dispatched
task. Main checkout and linked worktrees resolve to the same id. When no
`project.toml` exists, aid falls back to a path-based id of the main working
tree. Outside any git repository, tasks are stored with `project_id` unset
(the unattributed bucket). Historical tasks without a recorded identity stay
unattributed — aid does not invent one after the fact.

Common project controls include:

- default team and verification command;
- setup command and container image;
- `remote_build = "auto"` or `remote_build = "<box>"` in `.aid/project.toml` (`[project]`), overridden by `aid run --remote-build [BOX]`; batch `[defaults]` and per-task `remote_build` use the same values. A project `auto` default is ignored with a warning when `rbox` is not on `PATH` (the build stays local); an explicit `--remote-build` or a project default naming a box still fails without `rbox`;
- agent and model preferences;
- budget and duration limits;
- GitButler mode;
- worktree naming prefix;
- audit and idle-recovery policy;
- artifact backup (`[backup]`, below).

### Project budget identity

A name-only `[[usage.budget]]` entry in the global configuration is a project
budget: its `name` must match the target task's `project_id`. Dispatch checks the
identity resolved for `--dir`, including relative directories and linked worktrees,
instead of the caller's working directory. Batch tasks and retries pass through the
same gate. With no `--dir`, normal caller-project discovery still applies. Outside
Git there is no project budget; configured per-agent budgets still apply.

Usage aggregation, `aid usage` budget rows, and TUI budget gauges use persisted
`project_id`, not the repository directory's basename. A project without declared
configuration uses its stable path-based identity, shared with its linked worktrees.
Historical rows with no `project_id` remain unattributed and are not guessed into a
project budget. Per-agent budgets still include those rows. Explicit external usage
counters continue to contribute to the configured budget.

Exhausted cost, token, or task caps reject dispatch before creating the task claim;
near-limit warnings and automatic budget mode use the same target scope. This is
a check against recorded usage, not a reservation of future concurrent spend.
`aid project init` / `aid project sync` still synchronize project budget settings
into the global configuration; this identity fix does not change synchronization
or configuration precedence. The model preference `--budget` is separate from
these enforced usage caps.

### Artifact backup

A `[backup]` table in `.aid/project.toml` uploads a bundle of a task's artifacts
to an off-machine target when the task reaches a terminal state. The first target
is `gdrive`, which shells out to the Google Workspace CLI (`gws`, install with
`npm install -g @googleworkspace/cli`, sign in with `gws auth login`); aid never
holds Drive credentials itself.

```toml
[backup]
target = "gdrive"
folder = "aid-backups/{project}"        # templates: {project} {date} {task_id} {branch}
include = ["export", "diff", "transcript"]   # default: all three
on = ["complete", "fail"]               # subset of complete, fail
```

`[backup.gdrive] folder = "..."` in `~/.aid/config.toml` supplies the default
folder when the project sets none; the built-in default is `aid-backups/{project}`.
`[backup.gdrive] binary = "/path/to/gws"` names the `gws` executable when it is
not on the PATH of the `aid` process that performs the upload (a detached worker
often lacks the shell's `nvm` directories).
Folder segments are created under My Drive when missing. `aid run --backup
TARGET[:FOLDER]` enables or redirects the backup for one task without project
config, `--no-backup` disables it, and `aid retry` inherits whichever was set.
The bundle is `<task_id>-<short_sha>.tar.gz` containing `export.md` (the
Markdown export), `diff.patch` (`aid show --diff`), and `transcript.jsonl` (the
raw task log) as selected by `include`. The upload runs synchronously and is
attempted once, after the post-run lifecycle of a task that ran, once the final
status, verify status and result file are persisted. Tasks ended by `aid stop`, by the background reaper (dead
worker, idle, timeout, pending or waiting timeout), or by a failure before the
agent started are not backed up. A missing `gws`, missing sign-in, API failure,
or invalid `[backup]` value is recorded as a milestone event on the task and
printed to stderr, counts as the task's one attempt, and never changes the
task's status, `latest_error`, or exit code.

### Remote tests for ai-dispatch

Tests and their compilation run on the build box through `rbox`, never on this Mac
(boss rule 2026-09-11). Set `AID_RELEASE_TEST_CMD='scripts/remote-test.sh'` for releases;
project verify is `scripts/remote-test.sh -- --bin aid`. AID starts verify outside the
agent sandbox, inheriting the operator's `AID_BUILD_BOX`, `RBOX_CONFIG`, and `PATH` with rbox.
When remote build is active, verify receives the task's resolved `AID_BUILD_BOX`, overriding the operator's value so both use the same checkout. Otherwise, export `AID_BUILD_BOX` as the configured rbox name; never commit the name. An unset
value fails clearly. Rbox requires a Git repository root with a committed HEAD and
ships uncommitted tracked edits; commit or stage new test files before remote verification.

The wrapper uses one `rbox exec` and runs the full workspace suite as the configured
non-root user, with no test skips. Checkouts live at `~/.rbox/work/<repo-name>/<checkout-id>`:
the Git common directory supplies the repo name, and the branch (short HEAD SHA when
detached) is sanitized to letters, digits, `_`, and `-`. All worktrees share
`$HOME/.rbox/target/<repo-name>`. Rbox owns sync, transport, streaming, and the box lock.
Use `--jobs N` (default 4, or `AID_BUILD_JOBS`), `--timeout S` (5400),
`--lock-timeout S` (3600), `-- <extra cargo test args>`, or `--dry-run` to print the command.
Exit 124 means the wait timed out and the job continues; 75 means the box lock was not
acquired. Diagnostics name the job ID when assigned (sync lock failure has no job yet).
Completed test exits retain their status and are identified by rbox's completion marker.
If the agent sandbox cannot reach Tailscale, stop at dry-run/fake-rbox checks; the
operator runs the live release-test path. Never fall back to local compilation.

### Task profiles

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
uses the catalog model only for a declared `free` or `cheap` budget. `standard`
and `premium` leave the model unset so the CLI uses its own default (no `-m`).
Simple-task smart routing applies only when no budget is declared.
A healthy default quota group preserves that unset model; a held default group
pins a model from the first healthy alternative group. Every dispatch reports
the effective model and source: `--model`, agent config, catalog (declared budget),
or `CLI default (no -m)`; quota/budget routing overrides and existing adapter
defaults (Cursor, Qwen, MiMoCode) are labeled separately.
Register a local custom agent
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
| `rate-limited (needs manual clear: aid config clear-limit <agent>)` | a person acts | spent opencode balance, copilot monthly/premium, gemini `IneligibleTier`, oz `credentials are invalid`; grok 402 with no aidbar probe |

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
`aid agent list --json` reports `degraded` with `used_percent`, `resets_at`, and
`source`; the session-start hook prints `DEGRADED`; `aid run` warns and still
dispatches. Dispatch is not diverted. An on-disk
`hold: manual` marker is re-read against the current signature table, so a
Windowed needle written before this class existed still classifies as
Windowed rather than as a person hold.

An agent whose plan splits one allowance into tiers is marked per tier. Cursor
meters a single premium pool that every model except `auto` draws on, so a
premium refusal holds those models while `auto` stays dispatchable;
`aid config clear-limit cursor` clears both. Droid's Factory plan meters a
weekly or 5-hour `standard` pool separately from Droid Core, so a standard 402
holds those models while Core stays dispatchable, and a Core 402 holds Core
while standard stays dispatchable. A droid 402 that names neither pool —
including `reload your tokens` — holds the whole agent,
even when a model was on the dispatched route.
`aid config clear-limit droid` clears both. A group hold is not an agent hold:
`aid agent list` and `aid agent quota` report it as `PARTIAL` (still dispatchable
on clear tiers), not `LIMITED` or `OK`. STATUS now matches dispatch: a snapshot
that releases a route for `aid run` also clears LIMITED / PARTIAL. A hold only
a person ends is shown as `needs human: <first line of the stored message> —
fix, then aid config clear-limit <agent>` on `aid agent list`, the session-start
hook, and `aid doctor` — never a bare `LIMITED`. Clock and windowed holds still
print `LIMITED` / `LIMITED (resets HH:MM)`. aid never invents a reset time it
did not observe.

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

`doctor` is diagnostic. It lists NeedsHuman agent holds with the stored first
line and unlock command, and must not prune unaccepted artifacts. Run cleanup in
dry-run mode first. `clean` retains task records and events as custody evidence
and does not replace `aid gc --task`.
