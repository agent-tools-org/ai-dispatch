---
name: aid-guide
description: Official comprehensive operating guide for the AID multi-agent CLI. Use whenever a user asks how to install, configure, use, troubleshoot, automate, or safely operate AID; asks which AID command or workflow to choose; works with tasks, agents, worktrees, batches, groups, reviews, acceptance, custody GC, skills, templates, memory, knowledge graphs, GitButler, containers, MCP, BYOK, costs, or upgrades; or asks for an AID tutorial, quickstart, command example, or best practice.
---

# AID Official Guide

Use this skill as the authoritative operating guide for the installed AID
release. Prefer the installed binary's `aid <command> --help` when exact flags
may differ from the reference.

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
