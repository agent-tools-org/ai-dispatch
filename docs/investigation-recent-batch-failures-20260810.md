# Recent failed diagnosis batch: findings and repair

Date: 2026-08-10

Status: implemented and verified with focused regression tests.
Scope: diagnosis tasks created around 05:37 Asia/Bangkok and their automatic fallback.

## Verdict

| Task | Route | Immediate result | Classification |
|---|---|---|---|
| `t-73cfe9db` | oz | quota limit reached | Provider refusal |
| `t-aa80d2df` | codex | rejected `--full-auto` | aid defect |
| `t-8b8d0c09` | codex | rejected `--full-auto` | aid defect |
| `t-e57df500` | agy fallback | refused caller checkout | aid recovery defect |

Two of the three primary failures were caused by aid. Including the generated fallback, three of
four failed rows involved an aid adapter or recovery defect. `t-ddc541b6` was an unrelated
SIGTERM interruption that began before this batch.

Evidence came from the task database, task JSONL logs, installed Codex CLI help, and source.

## Root causes

### 1. Codex CLI compatibility

The installed Codex 0.147.0 rejected aid's unconditional `--full-auto` argument before starting
a thread. Its `codex exec --help` defines `--approve-for-me`. Aid's existing version probe only
controlled native model selection and did not validate the approval flag contract.

Repair:

- read `codex --version` before constructing fresh-session arguments;
- use `--full-auto` before 0.147.0 and `--approve-for-me` from 0.147.0 onward;
- validate that `codex exec --help` exposes the selected flag before claiming a task; and
- keep resume arguments unchanged.

Host validation is intentionally skipped for container and sandbox dispatches because their CLI
surface belongs to the guest environment.

### 2. Cascade repository anchor

Oz correctly reported a quota refusal and aid created agy fallback `t-e57df500`. The fallback
copied the live worktree and branch but lost `task.repo_path`, so the linked worktree became the
apparent repository root. The worktree safety check then correctly refused that caller checkout.

Repair: cascade target inheritance now reuses `apply_retry_target`, the authoritative resolver
that restores the main repository anchor before applying worktree metadata.

### 3. False changed-file summaries

The failed tasks had no agent events and identical start and final commits, yet their summaries
listed a file from the repository's previous commit. `gather_diff` used `HEAD~1..HEAD` instead of
the task's recorded `start_sha`.

Repair: diff the current worktree against `start_sha`. This includes committed, staged, and
unstaged tracked task changes without attributing an earlier commit to a no-op task. If no start
SHA exists, only the current uncommitted diff is considered.

## Regression coverage

- E2E fake Codex 0.146 and 0.147 CLIs accept only their respective approval flags; both fresh
  `aid run codex` flows must finish as `done`.
- Capability tests reject help output that lacks the version-selected flag.
- Cascade integration preserves the main repository while reusing a linked worktree.
- Diff tests cover a no-op task and combined committed plus unstaged changes after `start_sha`.

No failed production task is automatically retried by this patch.
