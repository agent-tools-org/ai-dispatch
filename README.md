# aid

![Version](https://img.shields.io/badge/version-11.8.0-blue)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

`aid` delegates execution to other agent contexts so a human or primary agent can
keep attention for decisions. Code tasks run in their own sessions and Git
worktrees; saved reports, summary lines, verdicts, and diffs bring back what the
next decision needs. The full transcript stays with the task, available on demand.
The dispatcher chooses the route, verifies by running checks, and explicitly
accepts the result.

[Website](https://aid.agent-tools.org) ·
[Operating guide](default-skills/aid-guide/SKILL.md)

## Install

Install and authenticate at least one supported agent CLI, then install `aid`:

```sh
curl -fsSL https://aid.agent-tools.org/install.sh | sh
```

You can [read the installer](https://aid.agent-tools.org/install.sh) first.
Alternatively, install from crates.io with a Rust toolchain:

```sh
cargo install ai-dispatch --locked
```

Or build and install from source:

```sh
git clone https://github.com/agent-tools-org/ai-dispatch.git
cd ai-dispatch
cargo install --path . --locked
```

Run the setup wizard:

```sh
aid --version
aid setup
```

Setup detects installed agents and configures bundled resources. See the
[configuration guide](default-skills/aid-guide/references/configuration.md)
for project defaults, credentials, and agent configuration.

## Quick start

### Dispatch a code task

Start in your project's Git repository. This example uses Codex; choose an
installed and authenticated agent for your task.

```sh
aid run codex "Add request validation and regression tests" \
  --dir . --worktree feat/request-validation \
  --difficulty moderate --budget standard \
  --urgency normal --rigor standard --verify --bg
```

The task gets a separate worktree, and background dispatch returns a task ID.
Configure a real project verification command first: bare `--verify` uses project
or supported defaults. For the full verification and remote-build behavior, see
[dispatch](default-skills/aid-guide/references/dispatch.md).

Replace `t-1234` below with the returned ID. Watch progress, wait for completion,
then inspect the outcome, diff, and saved output:

```sh
aid watch t-1234
aid wait t-1234
aid show t-1234 --summary
aid show t-1234 --diff
aid show t-1234 --output
```

A completed task is not proof of success. `Verified` and `Delivered` are successful
outcomes; `Unverified` is inconclusive, and `Broken` means verification failed.
Read the evidence before deciding whether to integrate and accept the artifact.

After review, merge into your intended current branch, record acceptance, and
collect the worktree when its contents are durable:

```sh
aid merge t-1234
aid accept t-1234
aid gc --task t-1234
```

Merge integrates code; acceptance records your decision. Collection requires a
matching accepted artifact, a clean worktree, and proof that commits are durably
reachable, including recursive submodules. An inconclusive proof keeps the work.
See the [lifecycle guide](default-skills/aid-guide/references/task-lifecycle.md).

### Ask for a report

Research can return a saved answer without creating a writable code worktree:

```sh
aid run agy "Summarize the design in README.md and cite relevant sections" \
  --dir . --read-only -o /tmp/aid-summary.md \
  --difficulty simple --budget standard --urgency normal --rigor draft
```

Use your chosen research-capable agent. The answer is saved to the declared output
file. Read-only enforcement compares the Git repository before and after the run;
it detects changes rather than preventing every write.

## Independent cross-audit

Keep the change's HEAD fixed and choose agents configured for different providers.
Different CLI names alone do not establish independence: check the provider and
model behind each route. Run these from a terminal outside an active aid task,
replacing `/path/to/change` with the checkout to inspect.

```sh
audit="Read-only cross-audit of HEAD. Report SHIP/FIX/BLOCK with evidence. Do not modify files."
aid run codex "$audit" --read-only --dir /path/to/change --bg \
  --kind debugging --difficulty moderate --budget standard --urgency normal --rigor standard
aid run claude "$audit" --read-only --dir /path/to/change --bg \
  --kind debugging --difficulty moderate --budget standard --urgency normal --rigor standard
```

Each task writes its own saved audit report. Replace the IDs below with the two
returned IDs, then compare the reports and their cited evidence:

```sh
aid wait t-audit-1 t-audit-2
aid show t-audit-1 --output --full
aid show t-audit-2 --output --full
```

`SHIP`, `FIX`, and `BLOCK` express each auditor's conclusion. Disagreement stays
visible; the dispatcher decides whether to accept the change. A missing report
is not an audit verdict. See the
[task operations guide](default-skills/aid-guide/references/task-operations.md)
for saved output and transcript inspection.

## How it works

- **Tasks carry execution context.** A brief, route, events, output, and transcript
  belong to the task. Inspect a compact result first and open the full evidence
  when the decision needs it.
- **Worktrees keep changes separate and in custody.** Failed work retains its
  artifacts. Acceptance and collection are explicit steps; rejection preserves
  the work for inspection or another attempt.
- **Verification runs checks.** Agent claims and lifecycle status do not replace
  command output. Verification and artifact delivery contribute to the outcome.
- **Profiles guide routing.** Declare difficulty, budget, urgency, and rigor.
  Routing advice compares candidates; the dispatcher selects the agent and model.
  Stronger models can be assigned where needed. Critical rigor requires enabled
  verification and a post-task audit; the latter uses the separate `aic` tool.
- **Groups and batches coordinate related tasks.** Groups collect findings and
  summaries. TOML batches support parallel work and dependency ordering. See
  [collaboration](default-skills/aid-guide/references/collaboration.md).

Preview routing without dispatching:

```sh
aid advise "Add request validation and regression tests" \
  --difficulty moderate --budget standard --urgency normal --rigor standard
```

Supported built-in agents: `agy`, `qwen`, `codex`, `copilot`, `opencode`,
`commandcode`, `cursor`, `kilo`, `mimocode`, `droid`, `oz`, `claude`, and `grok`.
Availability depends on installed CLIs and their authentication. The
[configuration guide](default-skills/aid-guide/references/configuration.md)
also covers custom agents.

The `gemini` agent was removed. Use `agy`, a Google provider through
`opencode`/`kilo`, or a custom agent. Historical tasks keep their original names.

## Where to go next

- [Official operating guide](default-skills/aid-guide/SKILL.md): authoritative
  workflows and reference material.
- [Command index](default-skills/aid-guide/references/command-index.md): find the
  command for an operation without loading the entire reference.
- [Documentation map](docs/README.md): navigate the repository documentation.
- [Changelog](CHANGELOG.md): release history.
- [Website quick start](https://aid.agent-tools.org/quick-start/): installation
  and examples.

## Optional components

The `web` Cargo feature adds an authenticated HTTP API, SSE updates, and a web UI.
It is off by default. See the [server source](src/web/mod.rs).

The AID Command client in `client/` connects to the server from macOS or iPadOS.
Its targets are defined in [the client project specification](client/project.yml).

## License

[MIT](LICENSE).
