# aid Roadmap

Snapshot of where the project is heading. Maintained by hand; authoritative backlog lives in `ai-board` (`ai-board item list --project ai-dispatch`).

## Current state

- Released on origin: **v8.94.0** (2026-04-20) — A+B reply/unstick full port, `aid merge --force`, `aid group delete --cascade`, batch `dir = "."` resolve, GH#89 bg preflight, issue #105 GitButler batch UX (auto-prune worktrees, merge-back hint, detect-and-prompt, `docs/gitbutler.md`), 28-lint clippy cleanup that unblocked CI after 5+ red releases.
- No stale unreleased work — `gitbutler/workspace` is fully caught up with origin via port PRs #106, #107, #108, #109, #110. `ai-board` item `wi-273e` is done.

## Near-term (next release cycle)

**Goals**:

1. **Tidy up commit-message hygiene.** Several v8.94.0 port PRs landed with auto-generated "task N" commit messages from a Claude Code hook. Investigate the post-Stop / PostToolUse hook that generates those and either write meaningful messages or disable it during release flows.
2. **Promote `DetectAgentsGuard` and `workspace_dir` override to production.** Both are test-only today; the same pattern (thread-local guards for "expensive discovery" logic) could give dispatch code a similar fast-path for `--sandbox` / CI usage.
3. **Clippy gate in `release.sh`.** Now that CI runs clippy green, make `scripts/release.sh` also run `cargo clippy --all-targets -- -D warnings` as a pre-release check so we don't regress quietly between rust minor-version bumps.

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

- **C: non-PTY agent message support** — original A+B+C plan had C for API/background agents (not just PTY). A and B shipped in v8.94.0; C requires agent-side polling hook or message-aware tick. Reopen after v9.0 lands.
- **Batch resume by content hash** — `aid batch foo.toml` detects existing workgroup with matching content, offers resume. Kills the "49 zombie rows from 7 failed dispatches" class of UX bug.
- **Session-preflight** — already shipped as a per-repo script + Claude Code SessionStart hook (`scripts/session-preflight.sh`, `.claude/settings.json`). Not yet promoted to user-level (`~/.claude/settings.json`) for all repos. Candidate for v9.x once the per-repo version proves stable.

## Process

- Every task opens on ai-board before dispatch; link commits by item ID (`wi-<id>`).
- Release notes source of truth: `scripts/release.sh` + a notes file at release time; CHANGELOG.md is generated.
- Session-start preflight is enforced via Claude Code hook for this repo; run `bash scripts/session-preflight.sh` manually when opening a fresh shell.
