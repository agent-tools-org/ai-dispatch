# Kept branches

Branches live ≤ 7 days. These carry unmerged work that is still wanted but does not
apply cleanly to current `main`, so each is renamed under `keep/` with a reason here.
`scripts/release.sh` exempts `keep/*` from the orphan check.

Triaged 2026-08-12. All five conflict with `main` and need a rebase before landing —
`git merge-tree` reports conflicts for each. None is a release blocker.

| Branch | Why it is kept | What it needs |
|---|---|---|
| `keep/agy-liveness` | Judges a buffered agent alive by the log it writes rather than stdout it will not write until done. This is the open mirror half of the idle-watchdog blind spot: the idle path can reap a live agent that writes to its log and not to the PTY. 3 commits, ~200 insertions with tests. | Rebase onto current `main`, then re-verify against a real buffered agent (agy or grok), not only unit tests. |
| `keep/verify-infra-vs-change-failure` | Separates a verification-infrastructure failure from a failure of the change under test. Directly relevant: on 2026-08-12 a poisoned sccache server made six unrelated deliveries report FAIL, and `aid merge` then refused them. 2 commits, one of them a partial-work salvage. | Rebase; the salvage commit needs review before it is trusted. |
| `keep/merge-guard-before-derivation` | Closes two holes an audit found in the worktree-isolation guard. ~268 insertions, carries its own tests. | Rebase; re-run the audit questions that produced it. |
| `keep/persist-result-stdout-fallback` | Audit-report persistence: when an agent writes its report to stdout instead of the result file, the report is lost. ~193 insertions. | Rebase. Check overlap with the envelope-unwrapping work that landed 2026-08-12, which touched the same persistence path. |
| `keep/aid-artifacts-not-committed` | Keeps aid's own worktree artifacts out of the agent's commits. ~462 insertions including an e2e test. | Rebase. |

Dropped in the same pass, recorded so the decision is not re-litigated: `feat/retry-bg`
(superseded — `aid retry --bg` exists), `fix/dispatched-verify-toolchain` (superseded by
the host-toolchain-path fix that landed 2026-08-12 with the same root cause),
`fix/hint-budget` and `fix/reclaim-liveness` (both superseded by later work the same day),
`audit/codex-attribution` (a stray `diff.patch`).
