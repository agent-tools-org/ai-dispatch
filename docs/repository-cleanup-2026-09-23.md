# Repository cleanup — 2026-09-23

## Baseline and completed branch cleanup

The source snapshot is v10.47.1 (`b68046e2`). The working tree was clean, with one
local branch (`main`), one worktree, 2,352 reachable commits and 339 release tags.
Tracked files total about 6.8 MB, while `.git` occupies about 370 MiB.
The object pack is 365.66 MiB. Two accidentally committed build directories
account for 1,315,155,172 uncompressed blob bytes: `target-local/` and
`target-rate-limit/`, introduced in `3c28f201` and removed from the working tree
in `45922bb5`. Deleting them later did not remove their historical blobs.

Remote inspection found no open PRs. Of 20 non-main branches, 5 are ancestors of
`main` and 11 contain only patch-equivalent commits (`git cherry` has no `+`).
All 16 were deleted in one atomic, tip-leased push after bundle verification.
Four branches with unique changes remain; see [retained branches](kept-branches.md).

| Deleted branch | Original tip | Evidence |
| --- | --- | --- |
| `feat/byok-pattern` | `24bb620f8fbc4266e11b350b5f9e8d2c71a08c2b` | All unique commits patch-equivalent to main |
| `feat/team-toolbox` | `495f442e9ca999983209eb59b80a0f0a82888ecd` | Ancestor of main |
| `feat/tui-route-triple` | `d6a9b5c02a602fd8f56cb2e7d4b444f0c3095e28` | Ancestor of main |
| `fix/audit-detection-broaden` | `b208bd8b2dc907003795ce3d99e20ddc1c437b10` | All unique commits patch-equivalent to main |
| `fix/audit-routing-guard` | `b2a978d9da9888d5900c073beba5d5a290886969` | All unique commits patch-equivalent to main |
| `fix/codex-loop-and-reason` | `35ff90b37371ecfa3ee2bade9a36e8ec8c7f02ee` | All unique commits patch-equivalent to main |
| `fix/dirty-baseline-asymmetry` | `861280ac6feeae64e68ea08bdad5f252b882c5fe` | Ancestor of main |
| `fix/gemini-test-env-isolation` | `52b6d4f4d47028e97754fd9779d80eed04d81140` | All unique commits patch-equivalent to main |
| `fix/hollow-output-char-count` | `bf23918583e3536d84de9183c0b16fe640624b7d` | All unique commits patch-equivalent to main |
| `fix/hotfix-prune-tests` | `91063fda39f22a91a8e4101c84f3c4b72387f5de` | All unique commits patch-equivalent to main |
| `fix/issue-114-active-worktree-protection` | `57d2154cceea145a3f8b7b894ee8bfdcfce183f3` | All unique commits patch-equivalent to main |
| `fix/issue-115-codex-worktree-sandbox` | `6d9dcd4266a28cd06a380dce4b133fa48ff8d257` | All unique commits patch-equivalent to main |
| `fix/issue-116-stuck-loop` | `0388e6346e1796299d2e4699f6ea6d6f0bf7d49e` | All unique commits patch-equivalent to main |
| `fix/issue-122-commit-msg-sanitize` | `ee34953d0dc1a26d74e0d74da8c5f0f65876b0b2` | All unique commits patch-equivalent to main |
| `fix/recent-task-failure-causes` | `b367e20a619ab121641249a1c4aff9c48172415c` | Ancestor of main |
| `fix/steer-reaches-buffered-agents` | `0d9c0e9f7688778270c78dc30da7c8e4a3bad61e` | Ancestor of main |

## Working-tree cleanup and prevention

- Remove the tracked `mutants.out/` output and placeholder `output.txt`. The mutation
  run was a failed baseline, not coverage evidence; originals remain in v10.47.1
  and the external backup.
- Preserve both August agent audit reports under `docs/archive/reports/`.
- Archive the superseded August branch register rather than treating it as active.
- Ignore root build-output variants, mutation output and generated root reports.
- Add a CI gate inspecting Git index paths and blob sizes. It rejects generated
  root paths and blobs above 5 MiB, even if the working file has been changed
  after staging; ordinary source fixtures and archived reports remain allowed.

No Rust or Swift runtime code, Cargo dependency, release tag or published artifact
is changed by this working-tree cleanup. No new release is created.

## History rewrite — applied with explicit approval

An isolated mirror removes only `target-local/` and `target-rate-limit/` from
all history. It keeps empty commits and merge topology and preserves commit
messages, author and committer data. The pack drops from **365.66 MiB to 12.51 MiB**
(about **96.6%**); the mirror totals about 13 MiB.

After branch cleanup, all 2,342 retained commits were checked through the commit
map: parent mappings, metadata,
messages, and every root tree entry outside the two removed paths match. Unchanged
subtree object IDs prove their contents remain identical. All 339 tag mappings
were checked and `git fsck --full` passed.

The approved rewrite changes 1,509 commit IDs and 180 tag targets and strips the
signatures on 25 signed commits. Four branch tips and 180 tags were updated in
one atomic push with explicit old-tip leases. The first attempt hit a GitHub
`commit_refs` error and changed no refs; the identical retry succeeded. All 344
remote heads/tags were then verified against the mapping. Main moved from
`e42d641b1272dd8fed045f44b47d0e4273a4356b` to
`c39441ffb964944800da0b85dd98bd2544849d9e`, with an identical source tree.

Release and crates.io workflows were temporarily disabled and restored after
each attempt. All 237 Release records and 1,185 asset records (including asset
IDs, names, sizes and digests) match the pre-rewrite snapshot. Published
archives/crates were not rebuilt; old commit links and
tag provenance need the retained mapping. A fresh mirror clone still includes
27 GitHub-managed pull-request refs and a 366.13 MiB pack. These retained PR refs
are outside the rewritten heads/tags; server-side reclamation remains dependent
on GitHub. A fresh ordinary clone, which does not fetch those PR refs, has a
12.83 MiB pack and passes `git fsck --full`.

Collaborators should preserve any unpushed work, clone the repository again,
and reapply only their unpublished changes. Do not merge or push old branch/tag
history into the rewritten repository: it would restore the discarded blobs.
The operator's local repository was backed up, synchronized and garbage-collected;
its object pack is now 12.32 MiB and `git fsck --full` passes.

## Backup and recovery

Operator-local backup directory: `~/.local/share/ai-dispatch-backups/cleanup-20260923/`.

- `before-cleanup.bundle`: full original refs/objects; `git bundle verify` passed.
- `refs-before.txt`, `branches-before.json`: original tips and classification.
- `branch-delete-candidates.json`, `branch-delete-{dry-run,result}.txt`: deletion manifest and results.
- `rewrite-preview.git/`: isolated rewritten repository, with filter-repo commit/ref maps.
- `verify-rewrite.py`, `rewrite-verification.json`: reproducible structural verification.
- `cleanup-delta.bundle`: cleanup commit on top of the original verified bundle.
- `rewrite-ready.git/filter-repo/{commit-map,ref-map}`: applied commit/ref mappings.
- `rewrite-ready-verification.json`: verification of the final 2,342-commit graph.
- `rewrite-push-plan.json`, `rewrite-push-retry.log`: exact leased updates and successful push.
- `refs-after-rewrite.txt`, `releases-{before,after}-apply.json`: remote verification evidence.
- `workflow-states-after-apply.json`: restored publishing workflow states.
- `git-before-local-gc.tar`, `git-before-local-gc.sha256`: complete local Git metadata
  backup, including reflogs and objects, before local object pruning.

Keep the bundle off the repository and do not expire it during this maintenance.
A deleted branch can be recovered from its recorded original tip in the bundle.
After a history rewrite, review recovery carefully so restoring an old ref does
not reintroduce the discarded build blobs.

## Validation

Run `python3 scripts/check-repo-hygiene-test.py` and
`python3 scripts/check-repo-hygiene.py`; the latter checks the staged index.
Also check the archive moves byte-for-byte, changed-document links, workflow YAML
and `git diff --check`. Rust compilation is not needed for this docs/housekeeping
change; future runtime changes retain the remote build requirement.
