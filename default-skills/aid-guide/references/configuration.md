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
rate-limit condition has cleared. Quota exhaustion is detected from per-CLI
refusal templates (stderr, structured error events, or provider-specific exit
text). Generic tokens like `429` or `rate limit` count in agent-authored prose
only when the whole line is essentially the refusal (for example `429 Too Many
Requests`), not when they appear inside discussion.

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
scripts. `aid mcp` exposes AID operations over stdio JSON-RPC for an MCP host.

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
