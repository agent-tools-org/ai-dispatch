I'll audit the merge-stash design read-only: first the provided diff, then the live stash/merge code paths against the three risk questions.Context is in; next I'll read the live stash modules and probe Git's locale-dependent stash messages.[MILESTONE] Loaded production stash modules and English probe baseline

Locale probe stayed English on this host — checking whether Git translates that string and how capture failures surface handles.[MILESTONE] Locale gettext confirmed; strip-failure leaves work in stash without handle  
[MILESTONE] Safety and positional checks complete; writing final audit report

# Merge-Stash Scope — Final Design Audit

**Scope:** live production path in `src/cmd/merge_stash_identity.rs`, `src/cmd/merge_stash.rs`, `src/cmd/merge_git.rs`, `src/cmd/merge.rs` (plus v7 diff).  
**Mode:** read-only design audit. No source changes.  
**Evidence:** code inspection, local git 2.50.1 probes, and upstream `builtin/stash.c` (git master).

---

## 1. Locale / human-readable stdout identity — **FAIL**

### Confirm or refute

**Confirmed: identity depends on a translatable English prose prefix.**

`push_stash` after a successful `git stash push` does:

```31:35:src/cmd/merge_stash_identity.rs
        let prefix = "Saved working directory and index state ";
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(|line| line.strip_prefix(prefix).map(str::to_string))
            .ok_or_else(|| "git stash did not report its exact identity".to_string())
```

Upstream git prints that line via gettext:

```c
printf_ln(_("Saved working directory and index state %s"),
          stash_msg_buf.buf);
```

(from `builtin/stash.c` / git master). The format string is NLS-marked. On a system with git message catalogs and a non-English `LANG`/`LC_ALL`/`LC_MESSAGES`, the prefix is not English and `strip_prefix` returns no match.

Local probe on Apple Git 2.50.1 kept English under `de_DE`/`fr_FR`/etc. (no catalogs in that binary). That does **not** refute the bug — it only shows one environment’s catalogs are absent. The code contract is English prose.

Synthetic strip check:

| stdout line | strip result |
|---|---|
| `Saved working directory and index state On main: aid merge-local 1-2` | `On main: aid merge-local 1-2` |
| `Arbeitsverzeichnis und Index-Status gespeichert On main: aid merge-local 1-2` | **None** |
| French-style translated prefix | **None** |

Same failure class if a future git rewords the English string.

### What the user experiences when strip fails

Sequence:

1. `has_local_changes` → dirty; continue.
2. `git stash push --include-untracked --message "aid merge-local {pid}-{nanos}"` **succeeds**.
3. Git has already moved tracked + untracked local work into the stash and cleaned the worktree (`reset --hard` / clean inside stash push).
4. Strip fails → `push_stash` returns  
   `"git stash did not report its exact identity"`.
5. `capture_local_changes` propagates that with `?` **before** `format_capture_error`.
6. `git_merge_branch` returns `MergeResult::StashRestoreFailed` **before any merge** (`merge_git.rs:180–183`).

So:

| Question | Answer |
|---|---|
| Merge refused before anything is destroyed? | **Yes** — merge never starts. |
| Work destroyed? | **No** — it is in the normal stash list. |
| Work removed from the worktree? | **Yes** — immediately after successful push. |
| Handle printed? | **No.** Error is only `git stash did not report its exact identity`. No SHA, no `aid merge-local …` token, no `format_capture_error` wrap. |

`format_capture_error` (which *would* print `stash message {message} (search git stash list)`) only wraps `find_stash` failures, not `push_stash` strip failures:

```59:62:src/cmd/merge_stash.rs
    let subject = push_stash(repo_dir, &message)?;
    before_identify(&message);
    let stash_ref = find_stash(repo_dir, &subject)
        .map_err(|error| format_capture_error(None, &message, &error))?;
```

Recovery still exists for a careful user: `git stash list` shows something like  
`On main: aid merge-local <pid>-<nanos>`, and `git stash apply` restores it. That is not what the error tells them.

### Identity without parsing prose — is it strictly better?

**Yes.** Prefer matching the unique message token aid already generates:

- Message: `aid merge-local {pid}-{nanos}` (`unique_stash_message`).
- List: `git stash list --format=%H%x09%gs`.
- Match: `%gs` contains / ends with that exact message; fail closed on 0 or >1 hits.
- Restore: `git stash apply --index <sha>` (already correct).

Why strictly better:

1. **No dependency on translated UI stdout.**
2. **No dependency on future reword of the “Saved…” line.**
3. **Still uses git’s real `%gs` list**, not a reconstructed subject.
4. **Uniqueness is already guaranteed by pid+nanos**; fail-closed multi-match remains.
5. **`%gs` subject body is not gettext’d** — git builds it as `On %s: ` + message (`strbuf_insertf(stash_msg_buf, 0, "On %s: ", branch_name)`), so token match is locale-stable even when the push *banner* is translated.

Optional hardening (not required if token match is used): force `LC_ALL=C` on git invocations is a band-aid; token match removes the prose dependency entirely.

---

## 2. Can uncommitted work be destroyed or become unfindable? — **PASS** (with residual)

| Path | Outcome | Evidence |
|---|---|---|
| Happy path capture + restore | Work restored via `git stash apply --index <sha>`; entry **not** dropped | `apply_stash`; no `stash drop`/`pop` in production merge stack (`rg` over `src/cmd` for `stash drop\|pop\|stash@\{` → no production hits) |
| Concurrent competing stash between push and identify | Own entry identified by exact subject/SHA; competing left intact | Tests: `stash_capture_keeps_identity_when_a_competing_stash_appears`, `git_merge_branch_restores_its_own_stash_when_newer_stash_appears`; identity is SHA from list match, not `stash@{0}` |
| Concurrent merges (two aid processes) | Distinct `pid-nanos` messages; each resolves own SHA or fails closed on multi-match | `unique_stash_message`; `find_stash` 0/1/many |
| Process death between push and apply | Entry remains in user’s normal stash list forever (nothing drops) | No drop path; durable `refs/stash` reflog entry |
| Conflicted merge handed to human | Merge not aborted away from conflict; untracked restored from `stash^3` when free; tracked left out of conflicted index; recovery text includes **stash commit SHA** | `preserve_changes_after_failed_merge`, `restore_untracked_after_failed_merge`, `format_stash_restore_error` |
| Partial / failed restore after successful merge | `StashRestoreFailed` with SHA; stash retained | `restore_local_changes` → `apply_stash` error path |
| `git status` failure | Fail closed **before** stash | `has_local_changes` returns `Err`; no push |
| Aggressive `git gc` after capture | SHA still reachable via stash reflog (tested intent) | test `tracked_merge_backup_survives_aggressive_git_gc` |
| Locale / strip failure after successful push | Work **not destroyed**, still in stash list under distinctive message; worktree cleared; **handle omitted from error** | Q1 evidence |

**Residual (does not flip to FAIL under “destroyed/unfindable”):** on the strip-failure path, work is findable only if the user inspects `git stash list` without guidance. That is a recovery-UX defect (Q1), not silent deletion.

**Not destruction:** successful merges leave `aid merge-local …` entries in the stash list permanently (by design: never drop). Clutter, not loss.

---

## 3. Positional `stash@{n}` or aid-constructed subject? — **PASS**

### Positional selectors

Production path uses only:

| Step | Command / API |
|---|---|
| Capture | `git stash push --include-untracked --message <unique>` |
| Identify | `git stash list --format=%H%x09%gs` + exact subject match → **SHA** |
| Restore | `git stash apply --index <sha>` |
| Conflict untracked | `git restore --source <tree> --worktree` from `stash^3` |

No `stash@{n}`, no `stash pop`, no `stash drop` in production merge code.  
Verified: SHA apply of a non-top entry (`stash@{1}`) works while neighbors remain.

### Subject construction

- Aid constructs only the **message token** (`aid merge-local pid-nanos`) and passes it to `-m`.
- Identity matching uses the **full subject string parsed from git’s push stdout** (intended to equal `%gs`), not an aid-predicted `On <branch>: …` string.
- Detached HEAD probe: git prints / lists `On (no branch): aid merge-local …` — reading git’s subject avoids the earlier detached-HEAD misprediction class.

**Caveat (does not fail Q3 as stated):** obtaining that subject still goes through English prose strip (Q1). That is prose parsing of UI text, not aid reconstructing the branch subject. Q3’s historical failure modes (drop wrong `stash@{n}`, mispredict subject) are gone.

---

## What I might have missed (open)

1. **Permanent stash accumulation** — every successful merge-with-dirty-tree leaves an entry. Fine for safety; noisy for heavy users.
2. **`git stash apply --index`** is stricter than plain `apply`; index conflicts fail restore while leaving the SHA in the list (correct, but can surprise).
3. **Collision check `path_has_existing_parent`** blocks restore when untracked paths (or intermediate non-dir parents) already exist — good fail-closed; human must move files, then apply by SHA.
4. **No test for non-English `LC_MESSAGES`** — suite can stay green forever on English-only CI while Q1 is latent.
5. **Did not re-run the 2331 suite** — green status is user-reported, not re-observed here; this audit does not treat “tests green” as safety proof (prior green versions destroyed data).
6. **Did not exercise a fully translated git install** — confirmation is from gettext source + strip logic, not a live `de.mo` git binary.
7. **`Command::new("git")` inherits ambient locale** — no `LC_ALL=C` anywhere in this path.
8. **Manual message forgery** — a human could create a second stash with the same full subject; fail-closed multi-match is correct.
9. **In-place merge path** (`merge.rs` non-worktree branch) does not use this stash machinery — out of scope but worth knowing.

---

## Verdict table

| # | Question | Result |
|---|---|---|
| 1 | Locale / prose stdout identity | **FAIL** — breaks under translated git UI; merge refused; work in stash but **handle not printed** on strip failure; message-token match is strictly better |
| 2 | Destroy / unfindable work | **PASS** — no drop; SHA retention; conflict/partial/concurrent/death paths keep data; residual: strip-failure UX |
| 3 | Positional `stash@{n}` / aid-built subject | **PASS** — SHA apply only; subject read from git (not predicted); no drop |

# Overall: **FIX**

Not **SHIP**: Q1 is a real identity bug class (translatable porcelain + post-push error that omits the recovery handle). Non-English (or reworded) git can clear the worktree, refuse the merge, and print a useless identity error.

Not **BLOCK**: this revision removed the prior data-destruction mechanisms (positional drop / wrong-entry pop). Work remains in the normal stash list even on the bad path.

**Minimum fix before ship:** stop identifying via the “Saved working directory…” line. Resolve SHA by exact unique message token against `git stash list --format=%H%x09%gs` (fail closed on 0/many), and on any post-push failure always print the message token (and SHA if known).