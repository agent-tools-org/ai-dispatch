Root cause: recovery errors named only the tracked SHA, hiding the untracked backup directory.

Fix committed as `8ca542a0`:

- Recovery messages now name every handle: tracked SHA, reachable ref, and untracked backup path.
- `stash create` commits are anchored under `refs/aid/merge-local/<sha>` to survive `git gc --prune=now`; the ref is removed after successful restoration.
- Chose a dedicated ref over `git stash store` to avoid the shared stash stack and positional cleanup race.

Regression failed before the fix because `aid-merge-local-*` was absent from the error. Afterward:

- 49 merge tests passed
- `aid build` and clippy passed
- File checks passed
- Working tree clean