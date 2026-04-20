# aid UX Debt

Systemic UX issues observed via dogfooding. Sorted by severity within category. Fixed items link to the commit that shipped them. Open items belong to the **v9.0 UX overhaul** milestone.

---

## Fixed in v8.94.0 (latest)

### GitButler batch merge-back (issue #105)

- **Aid worktrees leaked after batch completion, blocking `but apply`** — successful tasks now auto-prune their aid-owned worktree. Failed and shared worktrees preserved. Opt-out: `.aid/project.toml` → `keep_worktrees_after_done = true`.
- **`aid merge --lanes` was undiscoverable on GitButler repos** — `aid batch` completion + `aid watch --quiet --group` now print the lane merge-back hint when GitButler integration is active.
- **First `aid batch` on a GitButler repo without project.toml config required manual wiring** — batch now offers a one-time enable prompt. `--yes` / `--no-prompt` skip silently; declining writes a `suppress_gitbutler_prompt = true` marker.
- **No end-to-end docs for `aid` + GitButler workflow** — new `docs/gitbutler.md` covers modes, batch→review→merge pipeline, `AID_GITBUTLER=0` escape hatch, troubleshooting, `keep_worktrees_after_done` knob.

### A+B steer / reply / unstick (port completion)

- **`aid steer` was fire-and-forget** — now delegates to persisted `aid reply` path: new `task_messages` table, delivery tracking, ack on first output-after-delivery.
- **No way to detect or recover hung tasks** — new `aid unstick <task-id>` command + `TaskStatus::Stalled` variant + `IdleDetector` policy with auto-nudge-then-escalate thresholds wired into the PTY monitor.

### CI / release reliability

- **Flaky `workspace_dir` test isolation** — `/tmp/aid-wg-{id}` was hardcoded, so parallel tests sharing workgroup IDs raced on shared filesystem paths. Now test-isolated via `AidHomeGuard` (production behavior unchanged).
- **Fallback tests failed in CI where no agent binaries are on PATH** — new `DetectAgentsGuard` pins `detect_agents()` return value per-thread under `cfg(test)`.
- **28 pre-existing clippy `-D warnings` lints** (rust-1.93 + rust-1.95 strictness) blocked CI for 5+ releases — all mechanical rewrites, no behavior change. CI's build job is green again.

---

## Fixed in v8.85 (this release cycle)

### Batch / dispatch

- **`dir = "."` silently resolves to `/tmp/.`** — batch TOML loader resolved relative to runtime's inherited cwd, not the TOML file. First-wave tasks failed with cryptic `"Not a git repository: /tmp/."`. Now resolves relative to TOML file's parent, with a clear error if unresolvable.
- **Stale `.aid-lock` blocks retries** — a crashed task left its lock behind; the next attempt hit `Worktree is locked by task <id>`. Lock acquisition now checks whether the recorded PID is alive and clears dead locks automatically.
- **Zombie RUNNING tasks** — when a dispatched agent process died without updating status, the task stayed in `running` forever. `aid board` and `aid watch` now reap zombies (PID-dead tasks older than 60s) before rendering.
- **`aid clean --worktrees` blowing away other repos' worktrees** — treated every `/tmp/aid-wt-*` as an orphan regardless of which repo owned it, at risk of destroying active work on smart-router, uniswapx-filler, etc. Now reads each worktree's `.git` gitdir and skips non-current-repo worktrees; also skips worktrees with active file handles.
- **`aid merge` rejects FAIL tasks** — had to hand-merge via raw `git` when verify failed but code was good. Added `--force` flag.
- **`aid group delete` orphans member tasks** — only removed group metadata. Added `--cascade` flag to delete member tasks transactionally.
- **`aid board` anti-poll eats output** — repeated calls within the throttle window replaced the entire board with a hint. Now prepends a one-line hint and still renders the board. `--json` output unaffected.

### Release hygiene

- **`scripts/release.sh` ships without orphan-branch / orphan-worktree checks** — releases could go out with dozens of merged-but-undeleted local branches and stale worktrees. Added `check_orphans()` pre-flight with `--skip-hygiene` escape hatch.

### Agent dispatch reliability

- **GH#89: missing agent binary → cryptic spawn failure on background path** — foreground had a preflight, background didn't. Extracted shared helper in `src/agent/mod.rs`, preflight both paths.

---

## Open — v9.0 UX overhaul

### High severity — state recovery / safety

- **`aid batch` `depends_on` serializes execution but doesn't rebase child branches onto parent's output.** Every task starts from the same workspace base, so parallel tasks that touch the same file produce semantically-overlapping commits that git treats as cross-merge conflicts (this release had 3 conflicts for that reason alone). Dependent tasks should start from the merge-base of their declared dependencies, not from the shared workspace commit.
- **Cross-branch semantic coupling isn't caught.** Task A marks a helper `#[cfg(test)]`; task B uses that helper in production code; both compile in isolation; merging both produces a compile-time regression git's 3-way merge cannot detect. No tooling flagged this in batch review. `--analyze` today is lexical (file overlap) — needs semantic overlap (symbol usage).
- **`aid merge` auto-stash traps conflicts in the stash itself.** When a merge conflicts while a stash is active, the error is `"Your stashed local changes conflict with the merge. Resolve with: git stash pop"` — misleading because the conflict is between branches, not between stash and worktree. The auto-stash layer should detect the merge-level conflict and surface the real cause.
- **GitButler hook vs `aid merge` asymmetry.** `git merge` with auto-merge commit bypasses `pre-commit` (no `pre-merge-commit` hook exists), so `aid merge` works. But a manual `git commit` after resolving a conflict fires `pre-commit` and GitButler blocks it. Either: teach `aid merge` to drive conflict resolution through a non-workspace branch, or coordinate with GitButler to recognize aid-created merge commits.
- **Batch failures don't deduplicate on retry.** Every retry of a failing batch TOML creates a fresh workgroup and 7 new tasks; the failed-state predecessors stay in the DB (this release made 7 duplicate workgroups × 7 tasks = 49 stale rows). `aid batch <file>` should detect an existing workgroup by content hash and offer "resume" instead of "re-dispatch".
- **No shared resource lifecycle.** Worktrees, locks, group metadata, task rows, log files — each has its own ad-hoc cleanup path, some auto, most manual. A `Resource` trait with `acquire` / `release` / `gc` contracts would let `aid clean` be declarative and guarantee no leaks after crashes.

### Medium severity — error messages / UX

- **Errors surface at OS layer, not config layer.** `Not a git repository: /tmp/.` came from git; the user's actual problem was `dir = "."` in batch TOML. Every shell error and git error should be wrapped at the nearest configuration boundary before bubbling up.
- **`aid batch --analyze` warns but doesn't enforce.** This release had a lexical file-overlap warning ignored; conflicts came anyway. Consider `--strict-analyze` that aborts on overlaps above a threshold.
- **`aid group delete` without `--cascade` prints `"Historical tasks still tagged: N"`** — only informative after v8.85. Needs prompt language: `"— use --cascade to also delete them"` (partially done, verify in smoke test).
- **Update-check banner overrides real command output** in some code paths. `aid board --json` shows the banner — scripts that parse JSON will break.
- **Agent-dispatched tasks auto-add `implementer` skill even on research-only tasks** — skill injection runs before prompt-type inference. Waste of tokens and occasional behavior pollution (e.g. audit tasks getting told "don't run cargo fmt" which they never would anyway).

### Low severity — polish

- **`aid board` limit default is 50 but silently truncates** without hint on repeated queries. The new hint fix now shows total, which helps; could still propose `--limit N` or `--all` earlier.
- **`aid show --diff` against a merged task shows huge diffs** because the diff base is the merged workspace, which has advanced. Fix: record merge-base per task so `--diff` shows only the task's unique changes.
- **TaskStatus variants aren't fully consistent** across display code. Adding `Stalled` in this release required grepping `match status` and adding arms by hand; the compiler found some but not all (several `_ =>` fallthroughs silently accepted it as "unknown").

---

## Cross-cutting principles (non-negotiable for v9.0)

1. **Every write operation must have a documented recovery path.** lock, worktree, workgroup, task row — if aid creates it, aid must be able to clean it without manual SQL.
2. **Paths default to relative to the declaring file, not pwd.** Batch TOML's `dir`, `context`, `scope`, etc. all resolve against the TOML file's parent; absolute paths stay absolute; clear error if neither works.
3. **Cross-repo safety is default.** Any command that might affect another repo's state (clean, worktree prune, lock release) skips foreign worktrees unless explicitly opted in.
4. **Errors translate to the user's configuration layer.** The user's model is the TOML they wrote and the command they typed, not `git rev-parse --show-toplevel`.
5. **The board does not lie.** If a task's process is dead, its status is FAIL. Not RUN, not indefinite "please wait".
