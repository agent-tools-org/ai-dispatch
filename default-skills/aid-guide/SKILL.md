---
name: aid-guide
description: Official comprehensive operating guide for the AID multi-agent CLI. Use whenever a user asks how to install, configure, use, troubleshoot, automate, or safely operate AID; asks which AID command or workflow to choose; works with tasks, agents, worktrees, batches, groups, reviews, acceptance, custody GC, skills, templates, memory, knowledge graphs, GitButler, containers, MCP, BYOK, costs, or upgrades; or asks for an AID tutorial, quickstart, command example, or best practice.
---

# AID Official Guide

Use this skill as the authoritative operating guide for the installed AID
release. Prefer the installed binary's `aid <command> --help` when exact flags
may differ from the reference.

## What the dispatcher owns

AID's premise is that the caller dispatching work is the best-informed
component in the system. AID therefore declines to guess what that caller
already knows, which puts these responsibilities on the caller:

1. **Declare the task profile.** `--difficulty --budget --urgency --rigor`, and
   `--kind` when the category matters. Undeclared values are stored as null
   rather than inferred, and a null tells every downstream decision that nobody
   knows. Data locality is a separate declaration: `--egress any|local|private-network`
   (default `any`), decided by the provider endpoint — not by CLI identity.
   `local` admits loopback only (`localhost`, `127.0.0.0/8`, `::1`); `private-network`
   admits loopback or RFC1918/link-local IPs plus `.local` / `.home.arpa` hostnames
   and does not widen `local`.
2. **Declare tools and skills.** AID applies no skill unless one is declared
   (`--skill`, or a project default). Omitting `--kind` describes every
   resolved toolbox tool rather than hiding some behind a guessed category.
3. **Route by provider, not by agent name.** A route is
   `<cli>/<provider>/<model>`. One exhausted route says nothing about another
   provider that reaches a model of the same class. Never dispatch to a weaker
   model on the provider pool the caller is already running on: a different
   provider is delegation, the same pool for a worse model is waste.
4. **Verify by running.** An agent's own report of success is not evidence.
   `--rigor` states the proof owed — `draft` compiles, `standard` runs the
   changed path and captures real output, `critical` adds an independent audit.
5. **Read `unknown` as unknown.** A model, provider, or cost AID could not
   establish is reported as unknown rather than filled in with a plausible
   value. Several CLIs never name the model they ran, so unknown is the honest
   and expected answer for many tasks.
6. **Keep briefs short.** State the goal and the red lines; the implementation
   path is the thing being delegated.
7. **Do not edit a directory while an agent is working in it.** AID snapshots
   the dirty paths once, at dispatch, and excludes them when it rescues an
   agent's uncommitted output — so edits made *before* dispatch are safe. Edits
   made *during* the run are not in that snapshot and are indistinguishable
   from the agent's, so they are swept into the same rescue commit. Dispatch
   with `--worktree <branch>` and the agent works somewhere else entirely.
   Every rescue prints the files it staged; read that line rather than
   discovering it later.

## Operating method

1. Identify the user's goal and current task state.
2. Read only the references required for that workflow.
3. Inspect live state before proposing a mutating command:
   - `aid --version`
   - `aid board`
   - `aid show <task>`
   - `aid project state`
4. Explain the next safe command and its expected state transition.
5. Never equate agent completion, `Done`, or `Merged` with principal acceptance.
6. Never recommend raw `git worktree prune`, direct removal of AID task
   worktrees, or deletion of task branches as cleanup.
7. For exact syntax not shown here, run `aid <command> --help`.

## Reference routing

- For choosing among all public commands, read
  [references/command-index.md](references/command-index.md).
- For setup, project configuration, agents, skills, templates, containers,
  credentials, MCP, hooks, and upgrades, read
  [references/configuration.md](references/configuration.md).
- For `run`, `query`, `ask`, `build`, benchmarks, experiments, worktrees,
  verification, retries, and model selection, read
  [references/dispatch.md](references/dispatch.md).
- For batches, workgroups, teams, shared context, findings, memory, and the
  knowledge graph, read
  [references/collaboration.md](references/collaboration.md).
- For watching, inspecting, responding, steering, stopping, merging, exporting,
  usage, cost, statistics, notifications, and recovery, read
  [references/task-operations.md](references/task-operations.md).
- For principal review, `accept`, `reject`, custody guarantees, and `gc`, always
  read [references/task-lifecycle.md](references/task-lifecycle.md).

## Default workflow

```bash
aid setup
aid project init
aid run codex "Implement the change" --worktree feat/change --dir . --verify --bg
aid watch <task-id>
aid show <task-id> --summary
aid show <task-id> --diff
aid accept <task-id>
aid gc --task <task-id>
```

Stop before `accept` if the result has not been reviewed. Stop before `gc` if
the principal has not explicitly accepted the delivered artifact.

## Maintenance contract

Treat this directory as release-managed source, not user-authored state.

When changing an AID command, flag, lifecycle transition, safety invariant, or
default workflow:

1. Update `SKILL.md` routing if the feature category changes.
2. Update the relevant reference and the command index in the same change.
3. Update examples that expose removed or renamed behavior.
4. Run the bundled-guide coverage test and `tests/init_e2e.rs`.
5. Run the skill validator before release.

`aid init` refreshes this official skill from the running release. Personal
customizations belong in a separately named skill.
