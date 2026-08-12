Root cause: merge restoration used positional stash refs, allowing competing stashes to be restored.

Fix committed in `a31746f0`:

- Uses `git stash create` with immutable commit IDs.
- Backs up untracked files explicitly.
- Preserves merge failures and restores untracked files without touching conflicted indexes.
- Reports recovery IDs/paths loudly.

Pre-fix regression:

```text
left: "task-b rescue\n"
right: "local change\n"
```

Verification:

- 48 merge tests passed
- `aid build` passed
- clippy passed
- File-size, headers, unwrap, and staging checks passed
- Working tree clean