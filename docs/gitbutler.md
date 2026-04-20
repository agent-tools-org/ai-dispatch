# Using aid with GitButler

`aid` can integrate with GitButler so parallel task branches are easier to commit, review, and merge back into the workspace.

## Configuration

Set the mode in `.aid/project.toml`:

```toml
[project]
id = "my-repo"
gitbutler = "auto"
keep_worktrees_after_done = false
```

Supported values:

- `gitbutler = "off"` disables GitButler-specific hooks and merge hints.
- `gitbutler = "auto"` enables GitButler features when the `but` CLI is installed.
- `gitbutler = "always"` forces GitButler behavior and expects `but` to be available.

`keep_worktrees_after_done = false` is the default. When a task finishes successfully and its branch has commits, `aid` removes the task worktree automatically so GitButler can apply the branch cleanly. Set it to `true` if you want to inspect completed worktrees before removing them manually.

If `aid batch` detects a GitButler repo and you have no `gitbutler = ...` setting yet, it will prompt once to enable `gitbutler = "auto"`. Declining writes `suppress_gitbutler_prompt = true` so the prompt does not repeat.

## Recommended Flow

1. Enable project integration with `gitbutler = "auto"` or `gitbutler = "always"`.
2. Dispatch parallel worktree tasks with `aid batch tasks.toml --parallel`.
3. Review task output with `aid watch --quiet --group <wg-id>` or `aid show <task-id> --diff`.
4. Apply the finished branches into the GitButler workspace with `aid merge --lanes --group <wg-id>`.
5. If you want a normal git merge instead, use `aid merge --group <wg-id>`.

Typical batch flow:

```bash
aid batch tasks.toml --parallel
aid watch --quiet --group wg-abc123
aid merge --lanes --group wg-abc123
```

`aid merge --lanes --group <wg-id>` is the GitButler-first path. It applies each done task branch as a lane in the current workspace instead of merging them directly into the current branch.

## Escape Hatch

Set `AID_GITBUTLER=0` to disable GitButler integration for a single command or shell session:

```bash
AID_GITBUTLER=0 aid batch tasks.toml --parallel
```

This disables dispatch-time GitButler hooks and suppresses the lane-merge hints.

## Troubleshooting

### `but apply <branch>` fails because the branch is held by a worktree

Symptom:

```text
Failed to apply branch. Worktree changes would be overwritten by checkout
```

Cause: the branch is still checked out in `/tmp/aid-wt-<branch>/`.

What to do:

- Leave `keep_worktrees_after_done = false` so finished task worktrees are pruned automatically.
- If you intentionally kept worktrees, remove the specific one with `aid worktree remove <branch>` or `git worktree remove --force /tmp/aid-wt-<branch>`.
- Re-run `but apply <branch>` after the worktree is gone.

### `but apply` says the workspace commit is not at the top

Symptom:

```text
Refusing to work on workspace whose workspace commit isn't at the top
```

Cause: GitButler workspace state is out of sync for sequential manual applies.

What to do:

- Prefer `aid merge --lanes --group <wg-id>` over manual `but apply` loops after a parallel batch.
- If you are already mid-recovery, check `but status -fv`, apply only the branches you still need, and avoid mixing manual `but apply` with stale external worktrees.
- If necessary, disable GitButler temporarily with `AID_GITBUTLER=0` and fall back to a normal `aid merge --group <wg-id>` or your explicit git flow.

### The GitButler enable prompt never appears

Check all of these:

- `but` is installed and on `PATH`.
- The repo has GitButler markers (`.git/gitbutler/` or `.git/virtual_branches.toml`).
- `.aid/project.toml` does not already set `gitbutler = "off" | "auto" | "always"`.
- `.aid/project.toml` does not contain `suppress_gitbutler_prompt = true`.
- You are running interactively and did not pass `--yes` or `--no-prompt`.

### I want to inspect the completed worktree before merge-back

Set:

```toml
[project]
keep_worktrees_after_done = true
```

Then prune manually later with:

```bash
aid worktree prune
```
