# Investigation: `aid retry` on a dirty worktree fails — the rescue `git add -A` exits 1 on an ignored `.aid` path

Date: 2026-09-06 · aid 10.41.0 (4b9909d5) · git 2.50.1 (Apple Git-155) · investigator: 老李

KB consulted: `kb aid retry rescue git add ignored worktree` and `kb git add -A ignored path` — no match
on the symptom (top hits were worktree/target isolation and dispatch hygiene, not the rescue commit).

## Problem

Task t-bb789748 (codex, agentswap, worktree `feat/v6-readers`) hit the 60-minute cap at 21:18:29 with
32 modified + 2 untracked files. The failure-time salvage produced `partial-work.md` but no commit, and
both `aid retry t-bb789748 -f ... --bg` attempts (21:19:34, 21:19:54) exited 1. The operator recovered
the work by hand (commit 7d474fb at 21:20:13, merged to main as 70f04a8 at 21:49:47), then removed the
worktree, so the live worktree no longer exists.

## Evidence

1. **Minimal reproduction** (scratch repo, git 2.50.1): `.gitignore` contains `.aid/`, one file under
   `.aid/` is force-tracked (agentswap's exact shape: `.aid/seo-phase1.toml` is tracked while
   `.gitignore:7` ignores `.aid/`). Running the rescue command
   `git add -A -- . <AID_ADD_EXCLUDES>` prints

   ```
   The following paths are ignored by one of your .gitignore files:
   .aid
   hint: Use -f if you really want to add them.
   ```

   and **exits 1 — but the staging has already happened** (`M a.txt`, `A new.txt` are in the index).
   Plain `git add -A -- .` in the same state exits 0.
2. **Which pathspec triggers it.** Each entry of `AID_ADD_EXCLUDES` (`src/worktree/snapshot.rs:169`)
   tested alone: `:(exclude).aid/state.toml` → exit 1, `:(exclude).aid/batches/**` → exit 1; the
   top-level globs `:(exclude).aid-*`, `**/.aid-*`, `result-*.md` → exit 0. An exclude pathspec that
   names a path inside an ignored directory makes git's directory walk collect that directory as
   "ignored but matched", which is the exit-1 advice path. `advice.addIgnoredFile=false`,
   `--ignore-errors`, `:(exclude,literal)` and `:(exclude).aid/**` do not change the exit code.
3. **Trigger conditions** (all verified): the worktree has a `.aid/` directory AND either (a) the
   directory itself is ignored by `.gitignore` — even when it holds only a tracked file — or (b) any
   ignored entry exists under it (e.g. `.aid/state.toml` ignored only through aid's own
   `info/exclude` entry, which `ensure_aid_paths_excluded` writes on every worktree creation). A repo
   with no `.aid/` directory, or with only tracked files under a non-ignored `.aid/`, is unaffected.
   An `.aid-lock` ignored via `info/exclude` does **not** trigger it.
4. **Why the caller fails.** `save_partial_work` (`src/cmd/retry.rs:264`) and `commit_partial_work`
   (`src/failure_salvage.rs:105`) both run the add through a `run_git` that `ensure!`s exit 0, so the
   commit never runs even though the index is fully staged. The bug report's "34 staged files" is
   consistent with exactly this: `partial-work.md` (written before the add) records `staged: 0`; the
   add then staged all 34 and aborted before `git commit`; each retry saw a dirty worktree and repeated
   the same add. This is inference from the timestamps and the repro, not observed directly.
5. **Regression window.** The two nested excludes were introduced by 1f3a689d and ede6d189 on
   2026-08-07, first released in v10.15.0. Salvage commits still appear in ai-dispatch through
   2026-08-17 because aid's own worktrees carry only tracked `.aid/project.toml` and `knowledge/`
   (checked on the two live worktrees) — condition 3 is not met there.
6. **Observability gaps.** `~/.aid/logs/command-errors.jsonl` recorded both retries as
   `CommandFailed` with no git stderr. The failure-time salvage error is emitted only through
   `aid_warn!`; no record of it was found in the events table, the task log or the job file.
7. **The existing test cannot catch this.** `aid_add_excludes_covers_nested_and_untyped_bookkeeping_paths`
   (`src/worktree/snapshot_tests.rs:71`) runs the same command through a helper that asserts exit 0,
   but its fixture has no ignore rules, so the ignored-directory input never occurs.

## Call sites of `AID_ADD_EXCLUDES`

| Site | Add form | Exit code handling | Effect today |
|---|---|---|---|
| `src/cmd/retry.rs:264` `save_partial_work` | `add -A` | `ensure!` | retry aborts — the reported symptom |
| `src/failure_salvage.rs:105` `commit_partial_work` | `add -A` | `ensure!` | partial work never committed; warning only |
| `src/cmd/merge_git.rs:86` `auto_commit_uncommitted` | `add -A` + `target/`, `node_modules/`… | discarded, then checks staged | works by accident; also exits 1 whenever an ignored `target/` or `node_modules/` exists |
| `src/cmd/experiment.rs:136` `git_commit` | `add -A` | discarded | works by accident |
| `src/commit.rs:52` `auto_commit` | `add -u` | `ensure!` | unaffected (`-u` does no directory walk) |

## Root cause

`AID_ADD_EXCLUDES` names two paths inside `.aid/` (`.aid/state.toml`, `.aid/batches/**`). Whenever the
worktree's `.aid/` directory is ignored or contains an ignored entry, git 2.50 reports the exclude
pathspec as "matched only ignored paths" and exits 1 after staging. Two of the five callers treat that
exit as a failed add and abort before committing, so the rescue commit that `aid retry` and the
failure salvage depend on is never made in repos shaped like agentswap.

## Fix options

1. **Shipped:** `src/worktree/staging.rs` collects `.aid/state.toml`, `.aid/batches`, and any
   caller-specific generated directories, then pipes them to `git check-ignore --stdin`. It passes
   `:(exclude)<candidate>` only for candidates Git does not report as ignored, along with the
   top-level aid exclusions, before running `git add -A` or `git add -u`. Ignored candidates are
   omitted because Git already excludes them and their nested pathspecs can make `git add` exit 1.
   There is no post-add reset, so files already staged by an agent remain staged.
2. Drop the two nested excludes and rely on `ensure_aid_paths_excluded`'s `info/exclude` entries.
   Simpler, but changes behaviour in a repo where that exclude file could not be written, and a
   tracked `.aid/state.toml` would then be committed on every rescue.
3. Rejected: matching the "ignored by one of your .gitignore files" text on stderr.

Regression test: extend the existing snapshot test fixture with `.gitignore` = `.aid/` and one
force-tracked file under `.aid/` (agentswap's shape); the helper's exit-0 assertion then fails at HEAD
and must pass with the fix. Add a second case with `.aid/state.toml` ignored only via `info/exclude`.

Also worth fixing: record the git stderr in `command-errors.jsonl` for `CommandFailed`, and emit an
event when failure salvage fails, so the next instance is visible without a live worktree.
