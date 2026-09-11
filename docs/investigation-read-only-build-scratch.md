# Investigation: read-only / sandboxed tasks cannot build or test

KB consulted: `rust/sccache-daemon-inherits-the-first-callers-sandbox.md`,
`ai-coding/read-only-agent-is-not-git-read-only.md` (both relevant; see mechanisms 3 and D1).

Date: 2026-09-11. Trigger: two independent audits of aid deliveries (t-33151b50, t-70585194)
reported "local verification was stopped after the requested shared target proved unwritable and
`aid` automatically selected a fallback"; both auditors abandoned the Mac and ran the tests on a
remote box by hand. The question raised: why does a review task lose the ability to compile, and
why is there no scratch-directory exemption.

## 1. Premise correction

Neither audit was dispatched with `--read-only` (deliberately: the flag is known to disable the
auditor's shell on several CLIs). What they hit is a **different** mechanism from read-only mode:
codex's own `workspace-write` sandbox refused writes to the shared cargo target directory, and aid's
`build`/`test` wrapper silently fell back to a cold, per-checkout target under `$TMPDIR`
(`cargo check` took 288 s; the full test suite would have been a 30+ minute cold build). The
auditors gave up on the Mac. Both the underlying point and the fix request remain valid, but there
are three separate mechanisms, and a fix that addresses only "read-only" leaves the one that bit
today untouched.

## 2. Evidence

### 2.1 Today's audits (proven, from the task transcripts)

- `aid build check` inside t-33151b50 printed:
  `note: CARGO_TARGET_DIR unwritable; fell back from ~/.cargo-target/ai-dispatch/_base to
  $TMPDIR/aid-build-target/cursor-usage-limit-hold-<hash>`, then `elapsed: 288.0s` for a check.
- The same task's `ps` calls returned `zsh:1: operation not permitted: ps` — the codex seatbelt.
- Both tasks were dispatched `-d <linked worktree>` with no `-w`, so aid chose the non-branch
  target `_base`. `_base` was created at 20:13; the audits started at 20:02.

### 2.2 Sandbox canary (proven, run 2026-09-11 20:40–20:48)

A one-file crate built inside `codex exec -s workspace-write` with
`sandbox_workspace_write.writable_roots=[<dir>]`:

| Case | Target dir | RUSTC_WRAPPER | Result |
|---|---|---|---|
| A | granted, existing | sccache | EXIT=0, compiled |
| C | granted, existing | unset | EXIT=0 |
| D | **not** granted (sibling) | unset | `Operation not permitted (os error 1)` at the target path |

So the grant mechanism works when the directory is listed **and exists**; what fails in the field is
the listing or the pre-existence, or something outside the sandboxed process (mechanism 3).

### 2.3 Field frequency of the fallback (measured over ~/.aid task artifacts)

Tasks whose artifacts carry the fallback note, per day. Zero for 20 consecutive days, then daily:

| Aug 12–31 | Sep 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 0 / 2,960 | 2 | 6 | 9 | 7 | 5 | 7 | 5 | 5 | 3 | 1 | 4 |

Fourteen-day breakdown (55 hits; a handful are aid's own test-suite artifacts that contain the
note string, e.g. `/shared` and `/tmp/aid-wg-*/blocked`, so the true count is ~50):

| Agent | Mode | Checkout kind | Hits |
|---|---|---|---|
| codex | rw | own `-w` worktree | 25 |
| codex | ro | `-d` linked worktree | 7 |
| codex | rw | `-d` linked worktree | 5 |
| codex | ro | `-d` main checkout | 1 |
| cursor | ro | `-d` main checkout | 5 |
| cursor | ro | `-d` linked worktree | 3 |
| cursor | rw | own `-w` worktree | 5 |
| qwen / kilo | rw | mixed | 3 |

Every codex `-w` hit names the per-branch leaf (`~/.cargo-target/<project>/<branch>`) as the
directory that was unwritable — the directory aid intends to grant.

### 2.4 True `--read-only` tasks, last 30 days (measured, ~1,500 tasks)

Status per agent: agy 192 done / 46 failed / 19 stopped; claude 24 / 2 / 3; codex 431 / 35 / 5;
commandcode 28 / 5; cursor 269 / 55 / 3; droid 45 / 26 / 1; grok 169 / 31 / 2; kilo 28 / 10;
**oz 0 done / 74 failed**; qwen 8 / 2.

Sampled failures are not write failures: oz fails every run on `Your credentials are invalid.
Please log in again with oz login` (74/74, and oz has stayed dispatchable the whole window);
cursor and droid samples are quota refusals; agy/grok samples are "Background worker died
unexpectedly"; the codex sample is a network timeout.

The write-related signal is inside tasks that *finished*: verified snippets from read-only task
outputs —

- cursor t-fc9e3d36: "Tests not executed on this branch — audit was read-only".
- commandcode t-bb599347: "the sandbox denied every shell command (including `npm run build`)".
- kilo t-b5296fa3: "I can't create files (read-only".
- qwen t-fefd4179: "test... but I'm in READ-ONLY".
- codex t-e815ad6a: "Cargo target directory with `Operation not permitted`".

A loose regex over all read-only artifacts finds mentions of "could not build/test because of the
mode" in 141 codex, 73 cursor, 21 grok, 12 commandcode, 10 droid, 7 qwen, 4 kilo, 4 agy tasks. The
counts are indicative only (one kilo hit is a Solidity `Permissions(` match); the snippets above are
the verified ones. Sixteen read-only tasks in 14 days also carry the target fallback note.

## 3. Mechanisms

### M1 — `--read-only` is a hard plan mode on most CLIs (proven from `src/agent/*.rs`)

| CLI | What `--read-only` does today | Shell available |
|---|---|---|
| cursor | `--mode plan` unless a result file is declared | no |
| gemini | `--approval-mode plan` unless result file | no |
| grok | `--permission-mode plan` unless result file | no |
| commandcode | `--permission-mode plan` (always) | no |
| droid | `--use-spec` unless result file | no |
| claude | `--allowedTools Read,Glob,Grep,LS[,Write]` | no Bash |
| copilot | allowed dirs shrink to none | limited |
| codex, opencode, custom, agy (no plan flag) | prompt prefix only | yes, but sandboxed |

With a result file declared, cursor/gemini/grok/droid/agy drop to the prompt-level mode (the
result-file exception from `src/agent/read_only.rs`), so those audits *can* run tests; without a
result file, or on commandcode and claude, a read-only audit cannot execute anything. That is the
"verdict without test output" class.

### M2 — the codex writable-roots grant does not always land (proven in part)

- `writable_roots_config()` is only built when `resolve_worktree_gitdir(dir)` returns a linked
  worktree; for a task dispatched into a regular checkout (`-d <main checkout>`, `-d .`) **no**
  writable roots are passed at all, so the shared target is never granted (test
  `build_command_skips_writable_roots_for_regular_repo` documents this).
- For non-`-w` tasks the chosen target is `<root>/_base`. The `-w` path pre-creates its leaf
  (`ensure_branch_target_dir`); the `_base` path has no equivalent. A granted-but-absent directory
  is unwritable (canary D; memory of the 2026-08-12 fix says the same), and the sandbox cannot
  `mkdir` it.
- Settled for today's audit t-33151b50: its codex command line granted exactly
  `[<worktree gitdir>, <.git>, ~/.cargo-target/ai-dispatch/_base]` — the target **was** listed — and
  `_base` did not exist until 20:13, eleven minutes after launch. The first `aid build check` fell
  back (288 s cold); a later one in the same task built straight into `_base` once an unsandboxed
  process had created it. Granting a path that does not exist is the same as not granting it.
- The 25 codex `-w` hits name the leaf; their artifacts do not record the command line, so whether
  the leaf existed at launch is not recoverable from the data (D6's probe makes it observable).

### M3 — the sccache server inherits the first caller's sandbox (POSSIBLE; documented, not reproduced here)

`RUSTC_WRAPPER=sccache` is in the operator environment (added around 2026-09-01; the KB entry is
dated 2026-09-02, and the shell config carries a note about incidents on 2026-08-31 and
2026-09-04 where "a server spawned from an aid task HOME … EPERMs every build on the box"). A server
started inside a sandboxed task is confined to that task's roots; every later build through it, from
any process, fails with `Operation not permitted` on a path the caller can `touch`. The fallback
timeline (zero before Sep 1, daily after) is consistent with this and with M2; it is not proof.
Today's server (started 19:35, reparented to launchd) was unconfined: a local build through it
into a fresh `~/.cargo-target/...` directory succeeded at 20:48.

### M4 — the fallback is silent and unowned (proven)

`aid build`/`aid test` retry into `$TMPDIR/aid-build-target/<cwd>-<hash>` on a permission block,
keyed per checkout and never seeded from the warm base, so each task pays a cold build. `aid clean`
does know that root, but nothing runs it: 3.7 GB sat there today, the two audits alone added 765 MB,
and the Data volume reached 100 % (325 MiB free) during this investigation, which then broke an
unrelated command with ENOSPC. Top-level usage measured at that moment: `~/Library` 42 G,
`~/Develop` 25 G, `~/.cargo-target` 16 G, `~/.codex` 10 G, `~/.aid` 8 G, `~/.gemini` 7 G.

## 4. Design

Decisions (mine), each a property the code can enforce:

**D1. Read-only is a contract on the repository under audit, verified by outcome, not a capability
restriction.** Every `--read-only` task runs in its own disposable linked worktree, detached at the
requested commit (`aid` creates it; the current `--read-only cannot be used with --worktree`
exclusivity is replaced by "read-only always gets a private checkout"). At completion aid checks
`git status --porcelain` is empty and HEAD is unchanged; a violation marks the task
`read_only_violated` (a FAILED outcome with the diff preserved in the task artifacts). This also
closes the KB incident where a read-only auditor ran `git checkout` in the caller's checkout.

**D2. A task-scratch set exists for every task, created by aid before launch, and is the only
place a read-only task may write.** Scratch = the result file, the task HOME, a task-private
`TMPDIR`, and the task's cargo target directory (the per-branch leaf for `-w` tasks, `_base`
otherwise — both created and, for leaves, seeded from `_base`). aid passes the whole set to every
CLI that has a grant knob (codex `writable_roots`, copilot allowed dirs) and exports
`TMPDIR`/`CARGO_TARGET_DIR` for the rest. Cursor has no knob: its fallback stays, but is seeded from
the warm base and owned by `aid clean`.

**D3. Hard plan modes become opt-in.** With D1 verifying the outcome, the default for
`--read-only` on every CLI is the prompt-level mode plus D2 scratch; `--read-only=strict` keeps
today's plan/spec/allowedTools modes for callers that want no shell at all (secrets-adjacent repos).
commandcode and claude lose their always-hard behaviour under the default.

**D4. The build fallback is never silent.** When the granted target is unwritable, `aid build`/
`aid test` either seed the fallback from `_base` (warm) or fail with the note as an error; the note
is also recorded as a task event so `aid show` and the board expose it. `aid clean` runs the
fallback-root sweep on every dispatch when the root exceeds a size cap.

**D5. sccache guard.** If `RUSTC_WRAPPER=sccache` is in the environment, aid runs
`sccache --start-server` from its own unsandboxed process before launching any sandboxed task, so
the server is never born inside a sandbox. Independent of whether M3 is confirmed; cheap.

**D6. Grant correctness for codex.** `writable_roots_config` is built for every checkout kind (a
regular repo gets its `.git` dir and the scratch set), and every granted path is created before
launch. A dispatch-time self-check writes a probe file into each granted directory and refuses to
launch with a clear error if any is unwritable.

## 5. Sequencing

1. D6 + D2 (grant correctness, scratch set, pre-creation) — removes the mechanism that bit today.
2. D4 + D5 (loud fallback, clean sweep, sccache guard) — stops the disk and cold-build bleed.
3. D1 + D3 (private worktree, outcome verification, plan modes opt-in) — the read-only redesign.
Each step is a separate dispatch with its own audit; D1/D3 change public flags and need the guide
references updated in the same commit.

## 6. Open

- For the 25 codex `-w` fallbacks the launch command line is not in the artifacts; the
  existence of the leaf at launch cannot be reconstructed after the fact.
- Whether the sccache server on this Mac was confined at the fallback timestamps (no history kept;
  D5 makes the question moot).
- oz: 74 consecutive read-only failures on invalid credentials without a hold — separate item.

## 7. Scoped implementation and verification (2026-09-11)

Implemented D6 and the cargo-target/private-TMPDIR portion of D2. Branch seeding and
dispatch share directory creation; dispatch refuses failed scratch write/delete probes.
Canonical paths align native environment values and Codex roots.
Read-only modes, the build fallback, and sccache are unchanged.

Initial implementation (`697b4720`) evidence, superseded where noted below:

- `aid test --isolated --bin aid agent::scratch::tests`: 11 passed, 0 failed.
  Includes non-worktree `_base` creation, regular-checkout Git/target/temp roots,
  read-only probe refusal, Copilot grants, resume, and symlinked container paths.
- Removing the `_base` creation call made
  `non_worktree_rust_task_creates_base_target` fail: 0 passed, 1 failed,
  `cannot write to granted directory '…/cache/_base'`, caused by
  `No such file or directory (os error 2)`. Restoring creation passed.
- Broader agent regression suite: 471 passed, 0 failed, 7 ignored.
- Final `aid build check -p ai-dispatch`: 0 errors, 2 existing warnings.
  Final `aid build clippy -p ai-dispatch -- --all-targets`: 0 errors,
  27 existing warnings. No formatters were run or dependencies added.
- Remote guide E2E on `grok-bot-twitter`: 14 passed, 0 failed; the new guide
  contract test also passed after the final scratch-path normalization.
- Independent cross-review accepted the final normalization and grant wiring.

Real Codex 0.154.0 runs used a plain Git checkout at
`/tmp/aid-sandbox-scratch-20260911/plain-repo` on `grok-bot-twitter`, with:

```sh
aid run codex "run aid build check and paste its digest line" -d /tmp/aid-sandbox-scratch-20260911/plain-repo --verify true --idle-timeout 300
```

The fixture's target was `/root/.cargo-target/sandbox-scratch-probe-20260911/_base`,
outside the sandbox's default `/tmp` allowance. Session records confirm
`workspace-write`, with the checkout `.git`, that target, and each task's private
TMPDIR in `writable_roots`. Both tasks finished `done` with verification `passed`:

```text
t-1eecb409: succeeded: 0 errors, 0 warnings; command: cargo check; elapsed: 124ms
t-23842033: succeeded: 0 errors, 0 warnings; command: cargo check; elapsed: 140ms
```

Before the second dispatch, `_base` was deleted and confirmed absent. It was
recreated, and Cargo's dependency artifact names that exact target. Neither raw
tool digest contains a fallback note. Evidence: remote
`/tmp/aid-sandbox-scratch-proof.txt` and `/tmp/aid-sandbox-scratch-run{1,2}.log`.
The remote harness isolated nested `aid build` state with a separate `AID_HOME`:
earlier builds succeeded but the nested worker reaper marked their dispatch failed.
That separate lifecycle issue was not changed. Actual container execution was not
tested. The local warm target was preserved.

The FIX round moves preparation out of command construction to the real worker
launch boundary. Git metadata is not aid-owned scratch: failed probes omit its
grant and record a task event with the directory and reason. Owned target and
TMPDIR failures remain fatal. Command builders only serialize supplied roots.
The untested container scratch mounting and guest probing were removed; container
wrappers retain their main behavior. Guest scratch paths, symlinked cache verification,
permissions, and temporary-directory cleanup are follow-up work. The earlier
container unit results above describe the superseded implementation, not guest proof.

FIX validation: 13 scratch unit tests passed; the broader agent suite passed
474 tests with 7 ignored. Removing `_base` creation again failed its regression
test with `No such file or directory`; restoring it passed. Remote CLI coverage
passed 5 sandbox/preflight, 14 guide, 3 foreground, and 2 batch tests. The same
read-only caller fixture exits 1 with `697b4720` and 0 with the fix for `--dry-run`.
Before/after artifacts are on `grok-bot-twitter` at
`/tmp/aid-preflight-before-after-8515-wub978in/`; CLI tests use fake agents.
The earlier real-provider build proofs remain separate from these regression tests.
Container and sandbox launches skip host scratch preparation and export, retaining main's guest command behavior.
