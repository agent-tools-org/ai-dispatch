# Retained branches

Audited 2026-09-23 against `main` at `b68046e2` (v10.47.1).
The remote has four retained topic branches and `main`; there are no open PRs
at the audit snapshot. These topics still contain unique patches and were not deleted.
Tips below reflect the approved September 23 history rewrite; patch contents are unchanged.

| Branch | Tip | Unique commits vs main | Next review |
| --- | --- | ---: | --- |
| `feat/evermemos-plugin` | `205de388b970` | 6 | Decide whether the optional memory plugin is still wanted; 4 non-merge patches, 6 total commits. |
| `fix/issue-127-sandbox-gitdir` | `1d68568d9ab2` | 2 | Compare sandbox mount regressions with the current worktree implementation. |
| `fix/issue-134-id-collision` | `49f26ba6a1e4` | 3 | Compare dispatch collision and 8-hex display changes with current behavior. |
| `fix/worktree-reuse-orphan-137` | `1a9efec467b7` | 2 | Review orphan protection and reset-base races against current reconciliation. |

The 16 integrated branch tips and their deletion evidence are listed in the
[cleanup report](repository-cleanup-2026-09-23.md). Deletion used explicit tip
leases and an atomic push after a full verified bundle backup.

The [August kept-branch register](archive/kept-branches-20260812.md) is historical;
its five `keep/*` names are absent from the current remote and are not active work.

Before deleting another branch: refresh refs, check open PRs and worktree custody,
prove ancestry or patch equivalence, retain its tip in a verified external bundle,
and use an exact ref lease. Unique patches require an explicit retain/drop decision.
Do not infer that an old branch is disposable from age alone.
