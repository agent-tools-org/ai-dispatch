# aid Roadmap

Snapshot of where the project is heading. Maintained by hand; authoritative backlog lives in `ai-board` (`ai-board item list --project ai-dispatch`).

## Current state

- Released on origin: **v8.92.0** — fix(verify): detect declared-but-unadded new files.
- Unreleased on `gitbutler/workspace`: 14 merge commits (A+B reply/unstick, GH#89 bg preflight, release orphan-check, 6 UX hotfixes). **Not on origin/main**. See `ai-board` item `wi-273e` for port plan.

## Near-term (next release, v8.93-ish)

**Goal**: port today's work onto origin/main and cut a release.

- Port A+B reply/unstick (message_queries, IdleDetector, pty consumption, TaskStatus::Stalled).
- Port GH#89 background-path agent binary preflight.
- Port `scripts/release.sh` orphan-branch + orphan-worktree hygiene check.
- Port `aid merge --force`, `aid group delete --cascade`, `aid batch dir="."` resolve, docs/ux-debt.md.
- **Skip** (origin has different direction already): board anti-poll relaxation, lock PID check (origin's `aid doctor` / `aid_gc auto` handles same class), zombie reaper, clean --worktrees repo-scope.

## v9.0 — UX overhaul (semver major)

Systemic cleanup of the 14 debt items in [`docs/ux-debt.md`](./ux-debt.md). Tracked as ai-board epic `wi-5b7e`.

**5 non-negotiable principles**:
1. Every write operation has a documented recovery path (lock, worktree, group, task row — no manual SQL required after crash).
2. Paths in batch TOML default to relative to the declaring file, not pwd.
3. Any command affecting another repo's state skips foreign worktrees unless opted in.
4. Errors translate to the user's configuration layer (batch TOML, CLI flag, project.toml) not raw git/fs errors.
5. The board does not lie — dead-PID tasks are FAIL, not RUN-forever.

**Breaking changes**:
- `aid batch` child tasks with `depends_on` now start from parent's output branch, not the shared workspace base (existing TOMLs still work; child branches just become narrower).
- `aid clean --worktrees` default becomes repo-scoped (not global); opt-in `--all-repos` for the old behavior.
- `aid merge` requires DONE or explicit `--force` (already true; tightens the error message).

## v9.x — deferred ideas

- **C: non-PTY agent message support** — original A+B+C plan had C for API/background agents (not just PTY). Requires agent-side polling hook or message-aware tick. Reopen after v9.0 lands.
- **Batch resume by content hash** — `aid batch foo.toml` detects existing workgroup with matching content, offers resume. Kills the "49 zombie rows from 7 failed dispatches" class of UX bug.
- **Session-preflight** — already shipped as a per-repo script + Claude Code SessionStart hook (`scripts/session-preflight.sh`, `.claude/settings.json`). Not yet promoted to user-level (`~/.claude/settings.json`) for all repos. Candidate for v9.x once the per-repo version proves stable.

## Process

- Every task opens on ai-board before dispatch; link commits by item ID (`wi-<id>`).
- Release notes source of truth: `scripts/release.sh` + a notes file at release time; CHANGELOG.md is generated.
- Session-start preflight is enforced via Claude Code hook for this repo; run `bash scripts/session-preflight.sh` manually when opening a fresh shell.
